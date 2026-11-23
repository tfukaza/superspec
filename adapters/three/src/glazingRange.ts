import * as THREE from "three";
import { hasDeclaredLodOwner } from "./lodOwnership.ts";

type Pane = { mesh: THREE.Mesh; boxes: THREE.Box3[]; subset?: THREE.InstancedMesh; members: number[] };

/** Window glass is a blended draw for each sheet of each building, over a frame and a dark
 * interior that carry the window on their own. Beyond the range the glass is left out. This
 * is a visible simplification for the light presets: distant panes lose their sheen and tint.
 * Glass that emits light is always drawn. Restore before shadow passes and view culling. */
export class GlazingRange {
  private readonly panes: Pane[] = [];
  private readonly hidden: THREE.Mesh[] = [];
  private readonly shown: THREE.InstancedMesh[] = [];
  private readonly eye = new THREE.Vector3();
  private readonly matrix = new THREE.Matrix4();
  private readonly keep: number[] = [];

  /** `distance` is in metres from the camera to the nearest point of a pane's bounds. */
  constructor(root: THREE.Object3D, public distance: number) {
    root.updateWorldMatrix(true, true);
    root.traverseVisible(object => {
      if (!(object instanceof THREE.Mesh) || hasDeclaredLodOwner(object) || object.children.length || Array.isArray(object.material)
        || !distantGlass(object.material)) return;
      const proto = object instanceof THREE.InstancedMesh ? THREE.InstancedMesh.prototype : THREE.Mesh.prototype;
      if (object.onBeforeRender !== proto.onBeforeRender || object.onAfterRender !== proto.onAfterRender
        || (object instanceof THREE.InstancedMesh && (object.instanceColor || object.morphTexture))) return;
      if (!object.geometry.boundingBox) object.geometry.computeBoundingBox();
      const local = object.geometry.boundingBox!, boxes: THREE.Box3[] = [];
      const count = object instanceof THREE.InstancedMesh ? object.count : 1;
      for (let i = 0; i < count; i++) {
        if (object instanceof THREE.InstancedMesh) { object.getMatrixAt(i, this.matrix); this.matrix.premultiply(object.matrixWorld); }
        else this.matrix.copy(object.matrixWorld);
        boxes.push(local.clone().applyMatrix4(this.matrix));
      }
      this.panes.push({ mesh: object, boxes, members: [] });
    });
  }

  restore(): void {
    for (const mesh of this.hidden) mesh.visible = true;
    for (const mesh of this.shown) mesh.visible = false;
    this.hidden.length = this.shown.length = 0;
  }

  /** Call after view culling has hidden what the camera cannot see. */
  frame(camera: THREE.Camera): void {
    this.restore();
    if (!(this.distance >= 0) || !Number.isFinite(this.distance)) return;
    camera.updateWorldMatrix(true, false);
    this.eye.setFromMatrixPosition(camera.matrixWorld);
    for (const pane of this.panes) {
      const { mesh, boxes } = pane;
      // Another controller hid it, or the material changed since construction.
      if (!mesh.visible || Array.isArray(mesh.material) || !distantGlass(mesh.material)) continue;
      const keep = this.keep;
      keep.length = 0;
      for (let i = 0; i < boxes.length; i++) if (boxes[i].distanceToPoint(this.eye) <= this.distance) keep.push(i);
      if (keep.length === boxes.length) continue;
      mesh.visible = false;
      this.hidden.push(mesh);
      if (!keep.length || !(mesh instanceof THREE.InstancedMesh)) continue;
      let subset = pane.subset;
      if (!subset) {
        subset = pane.subset = mesh.clone(false) as THREE.InstancedMesh;
        subset.castShadow = false;
        subset.raycast = () => {};
        mesh.parent!.add(subset);
      }
      if (pane.members.length !== keep.length || keep.some((index, i) => pane.members[i] !== index)) {
        pane.members = keep.slice();
        for (let i = 0; i < keep.length; i++) { mesh.getMatrixAt(keep[i], this.matrix); subset.setMatrixAt(i, this.matrix); }
        subset.instanceMatrix.needsUpdate = true;
        // Three culls by the bounds of the instances a mesh holds now.
        subset.boundingBox = subset.boundingSphere = null;
      }
      subset.count = keep.length;
      subset.visible = true;
      this.shown.push(subset);
    }
  }

  dispose(): void {
    this.restore();
    for (const { subset } of this.panes) { subset?.removeFromParent(); subset?.dispose(); }
    this.panes.length = 0;
  }
}

function distantGlass(material: THREE.Material): boolean {
  const glass = material as THREE.MeshStandardMaterial;
  return material.transparent && material.userData.worldSpecSurface?.recipe === "architectural-glazing"
    && !glass.emissiveMap && !(glass.emissiveIntensity > 0 && glass.emissive?.getHex() !== 0);
}
