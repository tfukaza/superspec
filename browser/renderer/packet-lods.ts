import * as THREE from "three";
import type { GeometryLodGroup } from "../model/geometry-lods.ts";
import { selectAuthoredLod, type AuthoredLodHint } from "../../adapters/three/src/lodSelection.ts";

type Group = { levels: THREE.Mesh[][]; hint: AuthoredLodHint; level: number };
export type PacketLodMaterials = {
  restoreMaterials?: () => void;
  frameMaterials?: (camera: THREE.Camera, height: number, editing: boolean) => void;
};

/** Flat packet geometry stays addressable by its compiler ID for baking, edits,
 * collision and canonical picking. Only camera visibility selects alternatives. */
export class PacketLods {
  private readonly groups: Group[] = [];
  private readonly canonical = new Set<THREE.Object3D>();
  private readonly raycasts = new Map<THREE.Mesh, THREE.Mesh["raycast"]>();
  private readonly sourceVisibility = new Map<THREE.Mesh, boolean>();
  private readonly bounds = new THREE.Box3();
  private readonly part = new THREE.Box3();
  private readonly before: THREE.Mesh;
  private readonly after: THREE.Mesh;
  private camera?: THREE.Camera;
  private height = 1080;
  private materialHeight = 1080;
  private editing = false;

  constructor(scene: THREE.Scene, models: THREE.Object3D, definitions: readonly GeometryLodGroup[], private readonly materials: PacketLodMaterials = {}) {
    const objects = new Map(models.children.filter((object): object is THREE.Mesh => object instanceof THREE.Mesh).map(object => [object.name, object]));
    for (const definition of definitions) {
      const levels = definition.levels.map(level => [...(level.meshes ?? []), ...(level.batches ?? [])].map(id => objects.get(id)!));
      if (levels.some(level => !level.length || level.some(object => !object))) continue;
      // TreeSpec's existing foliage controller already selects its own complete
      // per-tile levels, including independent canopy shadow proxies.
      if (levels.every(level => level.every(object => object.userData.foliage))) continue;
      if (levels.some(level => level.some(object => object.userData.foliage))) continue;
      const coverage = definition.levels.every(level => level.screenCoverage !== undefined);
      this.groups.push({ levels, level: 0, hint: { kind: coverage ? "coverage" : "distance",
        values: definition.levels.map(level => coverage ? level.screenCoverage! : level.distance) } });
      levels[0].forEach(object => this.canonical.add(object));
      for (const [i, level] of levels.entries()) for (const object of level) {
        this.sourceVisibility.set(object, object.userData.source?.visible ?? object.visible);
        if (i) { this.raycasts.set(object, object.raycast); object.raycast = () => {}; }
      }
    }
    // Three traverses shadow casters in scene child order. These zero-area draws
    // restore full casters for every light, then restore the selected camera view.
    const geometry = new THREE.BufferGeometry().setAttribute("position", new THREE.Float32BufferAttribute(new Float32Array(9), 3));
    const material = new THREE.MeshBasicMaterial({ colorWrite: false, depthWrite: false });
    this.before = new THREE.Mesh(geometry, material); this.after = new THREE.Mesh(geometry, material);
    for (const marker of [this.before, this.after]) { marker.castShadow = true; marker.frustumCulled = false; marker.raycast = () => {}; marker.layers.enableAll(); }
    this.before.onBeforeShadow = () => this.restore();
    this.after.onBeforeShadow = () => { if (this.camera) this.frame(this.camera, this.height, this.editing, this.materialHeight); };
    if (this.groups.length || materials.restoreMaterials || materials.frameMaterials) {
      scene.add(this.before, this.after);
      scene.children.splice(scene.children.indexOf(this.before), 1); scene.children.unshift(this.before);
    }
    this.restore();
  }

  isCanonical(object: THREE.Object3D): boolean { return this.canonical.has(object); }

  frame(camera: THREE.Camera, height: number, editing = false, materialHeight = height): void {
    this.camera = camera; this.height = height; this.editing = editing; this.materialHeight = materialHeight;
    this.restore();
    if (editing) { this.materials.frameMaterials?.(camera, materialHeight, editing); return; }
    camera.updateWorldMatrix(true, false);
    for (const group of this.groups) {
      this.bounds.makeEmpty();
      for (const object of group.levels[0]) {
        object.updateWorldMatrix(true, false);
        if (object instanceof THREE.InstancedMesh) {
          if (!object.boundingBox) object.computeBoundingBox();
          this.part.copy(object.boundingBox!);
        } else {
          if (!object.geometry.boundingBox) object.geometry.computeBoundingBox();
          this.part.copy(object.geometry.boundingBox!);
        }
        this.bounds.union(this.part.applyMatrix4(object.matrixWorld));
      }
      group.level = selectAuthoredLod(this.bounds, camera, height, group.hint, group.level);
      for (const [i, level] of group.levels.entries()) for (const object of level)
        object.visible = i === group.level && this.sourceVisibility.get(object)!;
    }
    this.materials.frameMaterials?.(camera, materialHeight, editing);
  }

  restore(): void {
    this.materials.restoreMaterials?.();
    for (const group of this.groups) for (const [i, level] of group.levels.entries()) for (const object of level)
      object.visible = i === 0 && this.sourceVisibility.get(object)!;
  }

  dispose(): void {
    this.restore();
    for (const [object, raycast] of this.raycasts) object.raycast = raycast;
    this.before.removeFromParent(); this.after.removeFromParent();
    this.before.geometry.dispose(); (this.before.material as THREE.Material).dispose();
    this.groups.length = 0; this.canonical.clear(); this.raycasts.clear(); this.sourceVisibility.clear();
  }
}
