import * as THREE from "three";

/**
 * Inspection overlays for the items of a scene, as the engine reports them (`metadata.items`, world meters): named
 * features as colored polylines, pattern anchors as two-axis frames, decal sheets as outlines, and the declared drag
 * handles. A handle links a dimension to one control of its item: dragging it along its axis asks for a new value of
 * that control and nothing else; the engine regenerates the item and the handle follows the dimension.
 *
 * Overlays are drawn in the image only: they never cast or receive shadows or occlusion and are no part of a scene.
 */
export interface OverlayHandle { id: string; parameter: string; position: number[]; axis: number[]; scale: number; value: number; min?: number; max?: number }
export interface OverlayItem {
  id: string;
  features: { name: string; points: number[][]; closed: boolean }[];
  anchors: { origin: number[]; u: number[]; v: number[] }[];
  decals: { id: string; corners: number[][] }[];
  handles: OverlayHandle[];
}
/** A handle asks for a value of its control: while it is dragged (`done` false) and once when it is let go. */
export type HandleChange = (item: string, handle: OverlayHandle, value: number, done: boolean) => void;

/** One color per feature name; names outside the list are white. */
export const FEATURE_COLORS: Record<string, number> = {
  base: 0x38b6ff, top: 0xffd23f, sill: 0xff7a3d, "opening-edge": 0xb28dff, joint: 0x3dffa2, end: 0xff4da6, "bracket-contact": 0xd4ff3d,
};
const HANDLE_RADIUS = 0.04;
/** Values a drag asks for are whole millimeters. */
const HANDLE_STEP = 0.001;

/** A drag in progress: the axis and value it started from, so a rebuilt overlay does not disturb it. */
interface Drag { item: string; handle: OverlayHandle; origin: THREE.Vector3; axis: THREE.Vector3; start: number; value: number; pointer: number }

export class ItemOverlays {
  readonly group = new THREE.Group();
  private grips: { mesh: THREE.Mesh; item: string; handle: OverlayHandle }[] = [];
  private drag?: Drag;
  private change?: HandleChange;
  /** Set by a drag, so the click that ends it selects nothing. */
  private dragged = false;

  constructor(private readonly canvas: HTMLCanvasElement, private readonly camera: THREE.Camera, private readonly controls: { enabled: boolean },
    private readonly invalidate: () => void, private readonly state: (summary: string | undefined) => void) {
    // Before the orbit controls hear the press: a press on a handle is not the start of an orbit.
    canvas.addEventListener("pointerdown", this.press, { capture: true });
    canvas.addEventListener("pointermove", this.move);
    canvas.addEventListener("pointerup", this.release);
    canvas.addEventListener("pointercancel", this.release);
  }

  /** Whether the click now being handled ended a drag (asked once per click). */
  consumedClick(): boolean {
    const consumed = this.dragged;
    this.dragged = false;
    return consumed;
  }

  /** Replaces the overlays. `features` shows features, anchors and decal outlines; `handles` shows the drag handles. */
  show(items: OverlayItem[], options: { features: boolean; handles: boolean }, change?: HandleChange): void {
    this.clear();
    this.change = change;
    const line = (points: number[][], closed: boolean, color: number): void => {
      if (points.length < 2) return;
      const geometry = new THREE.BufferGeometry().setFromPoints(points.map(point => new THREE.Vector3(...point)));
      const material = new THREE.LineBasicMaterial({ color, depthTest: false, transparent: true });
      const drawn = closed ? new THREE.LineLoop(geometry, material) : new THREE.Line(geometry, material);
      drawn.renderOrder = 10;
      this.group.add(drawn);
    };
    let features = 0, anchors = 0, decals = 0;
    for (const item of items) {
      if (options.features) {
        for (const feature of item.features) { line(feature.points, feature.closed, FEATURE_COLORS[feature.name] ?? 0xffffff); features++; }
        for (const anchor of item.anchors) {
          const origin = new THREE.Vector3(...anchor.origin);
          for (const [axis, color] of [[anchor.u, 0xff5555], [anchor.v, 0x55ff77]] as const) {
            const arrow = new THREE.ArrowHelper(new THREE.Vector3(...axis).normalize(), origin, 0.25, color, 0.06, 0.035);
            arrow.traverse(object => { const material = (object as THREE.Mesh).material as THREE.Material | undefined; if (material) { material.depthTest = false; material.transparent = true; } object.renderOrder = 10; });
            this.group.add(arrow);
          }
          anchors++;
        }
        for (const decal of item.decals) { line(decal.corners, true, 0xffffff); decals++; }
      }
      if (options.handles) for (const handle of item.handles) {
        const mesh = new THREE.Mesh(new THREE.SphereGeometry(HANDLE_RADIUS, 16, 12), new THREE.MeshBasicMaterial({ color: 0xff8a1f, depthTest: false, transparent: true, opacity: 0.95 }));
        mesh.position.set(handle.position[0], handle.position[1], handle.position[2]);
        mesh.renderOrder = 11;
        mesh.name = `${item.id}/${handle.id}`;
        this.group.add(mesh);
        const arrow = new THREE.ArrowHelper(new THREE.Vector3(...handle.axis).normalize(), mesh.position, 0.22, 0xff8a1f, 0.07, 0.04);
        arrow.traverse(object => { const material = (object as THREE.Mesh).material as THREE.Material | undefined; if (material) { material.depthTest = false; material.transparent = true; } object.renderOrder = 11; });
        this.group.add(arrow);
        this.grips.push({ mesh, item: item.id, handle });
      }
    }
    this.report(features, anchors, decals);
    this.invalidate();
  }

  /** States what is drawn, with each handle's place on the canvas in CSS pixels, for inspection and tests. */
  private report(features: number, anchors: number, decals: number): void {
    const bounds = this.canvas.getBoundingClientRect();
    this.camera.updateMatrixWorld();
    this.state(this.group.children.length ? JSON.stringify({ features, anchors, decals,
      handles: this.grips.map(({ mesh, item, handle }) => {
        const at = mesh.position.clone().project(this.camera);
        const along = mesh.position.clone().add(new THREE.Vector3(...handle.axis).normalize()).project(this.camera);
        const screen = (point: THREE.Vector3) => [+((point.x + 1) / 2 * bounds.width).toFixed(1), +((1 - point.y) / 2 * bounds.height).toFixed(1)];
        // `meter` is where a point one meter along the axis is drawn.
        return { item, id: handle.id, parameter: handle.parameter, value: handle.value, screen: screen(at), meter: screen(along) };
      }) }) : undefined);
  }

  private ray(event: PointerEvent): THREE.Ray {
    const bounds = this.canvas.getBoundingClientRect();
    const pointer = new THREE.Vector2((event.clientX - bounds.left) / bounds.width * 2 - 1, 1 - (event.clientY - bounds.top) / bounds.height * 2);
    const caster = new THREE.Raycaster();
    caster.setFromCamera(pointer, this.camera);
    return caster.ray;
  }

  /** How far along the drag's axis the pointer's ray passes closest, in meters from the drag's origin. */
  private along(drag: Pick<Drag, "origin" | "axis">, ray: THREE.Ray): number {
    const offset = drag.origin.clone().sub(ray.origin);
    const b = drag.axis.dot(ray.direction), d = drag.axis.dot(offset), e = ray.direction.dot(offset);
    const denominator = 1 - b * b;
    // An axis seen end on gives no reading.
    return Math.abs(denominator) < 1e-6 ? 0 : (b * e - d) / denominator;
  }

  private readonly press = (event: PointerEvent): void => {
    if (event.button !== 0 || !this.grips.length || !this.change) return;
    const ray = this.ray(event);
    const caster = new THREE.Raycaster(ray.origin, ray.direction);
    const hit = caster.intersectObjects(this.grips.map(grip => grip.mesh))[0];
    const grip = hit && this.grips.find(candidate => candidate.mesh === hit.object);
    if (!grip) return;
    event.stopImmediatePropagation();
    event.preventDefault();
    const origin = grip.mesh.position.clone(), axis = new THREE.Vector3(...grip.handle.axis).normalize();
    this.drag = { item: grip.item, handle: grip.handle, origin, axis, start: this.along({ origin, axis }, ray), value: grip.handle.value, pointer: event.pointerId };
    this.controls.enabled = false;
    this.canvas.setPointerCapture(event.pointerId);
  };

  private value(drag: Drag, event: PointerEvent): number {
    const moved = this.along(drag, this.ray(event)) - drag.start;
    let value = drag.handle.value + moved * drag.handle.scale;
    if (drag.handle.min !== undefined) value = Math.max(drag.handle.min, value);
    if (drag.handle.max !== undefined) value = Math.min(drag.handle.max, value);
    return Math.round(value / HANDLE_STEP) * HANDLE_STEP;
  }

  private readonly move = (event: PointerEvent): void => {
    const drag = this.drag;
    if (!drag || event.pointerId !== drag.pointer) return;
    const value = this.value(drag, event);
    if (value === drag.value) return;
    drag.value = value;
    this.dragged = true;
    this.change?.(drag.item, drag.handle, value, false);
  };

  private readonly release = (event: PointerEvent): void => {
    const drag = this.drag;
    if (!drag || event.pointerId !== drag.pointer) return;
    this.drag = undefined;
    this.controls.enabled = true;
    if (this.canvas.hasPointerCapture(event.pointerId)) this.canvas.releasePointerCapture(event.pointerId);
    // The value the drag ends on is asked for once more, as settled, even when it is the one last shown.
    if (this.dragged) this.change?.(drag.item, drag.handle, event.type === "pointercancel" ? drag.value : this.value(drag, event), true);
  };

  private clear(): void {
    this.group.traverse(object => {
      const drawable = object as THREE.Mesh;
      drawable.geometry?.dispose();
      const materials = Array.isArray(drawable.material) ? drawable.material : [drawable.material];
      for (const material of materials) material?.dispose();
    });
    this.group.clear();
    this.grips = [];
  }

  dispose(): void {
    this.clear();
    this.canvas.removeEventListener("pointerdown", this.press, { capture: true });
    this.canvas.removeEventListener("pointermove", this.move);
    this.canvas.removeEventListener("pointerup", this.release);
    this.canvas.removeEventListener("pointercancel", this.release);
  }
}
