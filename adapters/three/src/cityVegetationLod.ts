import * as THREE from "three";
import { hasDeclaredLodOwner } from "./lodOwnership.ts";
import type { CityLodLevels } from "./cityLods.ts";

export type CityVegetationLodOptions = {
  /** Maximum projected geometric error, in drawing-buffer pixels. Default 1.5. */
  errorPixels?: number;
  /** Preserve the complete motif within this many metres. Default 24, minimum 12. */
  minDistance?: number;
};

type Entry = {
  mesh: THREE.Mesh;
  near: THREE.BufferGeometry;
  material: THREE.Material;
  materialVersion: number;
  levels: CityLodLevels;
  bounds: THREE.Box3;
  matrixWorld: THREE.Matrix4;
  instanceVersion?: number;
  instanceCount?: number;
  previous: number;
  applied?: THREE.BufferGeometry;
  raycast: THREE.Mesh["raycast"];
  originalRaycast: THREE.Mesh["raycast"];
};

function eligible(mesh: THREE.Mesh): boolean {
  const material = mesh.material;
  if (Array.isArray(material) || !(material instanceof THREE.MeshStandardMaterial)) return false;
  const surface = material.userData.worldSpecSurface;
  if (surface?.family !== "Vegetation" || surface?.recipe !== "Weed underpainting"
    || material.userData.grassField || material.userData.castsShadow !== false || mesh.castShadow
    || !material.transparent || material.displacementMap || material.wireframe
    || mesh.children.length || mesh instanceof THREE.SkinnedMesh || mesh.morphTargetInfluences?.length
    || mesh.geometry instanceof THREE.InstancedBufferGeometry
    || mesh instanceof THREE.InstancedMesh && mesh.morphTexture) return false;
  const prototype = mesh instanceof THREE.InstancedMesh ? THREE.InstancedMesh.prototype : THREE.Mesh.prototype;
  return !(["onBeforeRender", "onAfterRender", "onBeforeShadow", "onAfterShadow", "raycast"] as const)
    .some(callback => mesh[callback] !== prototype[callback])
    && !Object.values(mesh.geometry.attributes).some(attribute => attribute instanceof THREE.InstancedBufferAttribute
      || attribute instanceof THREE.InterleavedBufferAttribute && attribute.data instanceof THREE.InstancedInterleavedBuffer);
}

/** Camera-only LOD for the published weed underpainting motifs, not continuous
 * turf or lawn strokes. Existing index streams share every authored attribute.
 * Construct after CityVisibility; call frame after its frame, then restore in the
 * render finally path. No meshes, materials, GPU buffers or draw calls are added.
 * Recreate after editing source transforms, instance matrices, geometry or materials. */
export class CityVegetationLod {
  private readonly entries: Entry[] = [];
  private readonly eye = new THREE.Vector3();
  private readonly errorPixels: number;
  private readonly minDistance: number;
  private readonly rootMatrix: THREE.Matrix4;
  private readonly errorScale: number;

  constructor(private readonly root: THREE.Object3D,
    lods: ReadonlyMap<THREE.BufferGeometry, CityLodLevels>, options: CityVegetationLodOptions = {}) {
    this.errorPixels = options.errorPixels !== undefined && Number.isFinite(options.errorPixels) && options.errorPixels > 0
      ? options.errorPixels : 1.5;
    this.minDistance = Math.max(12, options.minDistance !== undefined && Number.isFinite(options.minDistance)
      ? options.minDistance : 24);
    root.updateWorldMatrix(true, true);
    this.rootMatrix = root.matrixWorld.clone();
    const e = this.rootMatrix.elements;
    // Source error already includes glTF placement scales. Only the host's root
    // transform is additional; this induced-norm bound also covers root shear.
    const columns = Math.max(Math.abs(e[0]) + Math.abs(e[1]) + Math.abs(e[2]),
      Math.abs(e[4]) + Math.abs(e[5]) + Math.abs(e[6]), Math.abs(e[8]) + Math.abs(e[9]) + Math.abs(e[10]));
    const rows = Math.max(Math.abs(e[0]) + Math.abs(e[4]) + Math.abs(e[8]),
      Math.abs(e[1]) + Math.abs(e[5]) + Math.abs(e[9]), Math.abs(e[2]) + Math.abs(e[6]) + Math.abs(e[10]));
    this.errorScale = Math.sqrt(columns * rows);
    root.traverseVisible(mesh => {
      if (!(mesh instanceof THREE.Mesh) || hasDeclaredLodOwner(mesh) || !eligible(mesh)) return;
      const near = mesh.geometry, levels = lods.get(near);
      // A legacy chain without measured errors is insufficient for alpha-painted
      // coverage. Never replace the motif with an empty or differently shaded mesh.
      if (!levels?.length || levels.some(level => !level.index?.count
        || !Number.isFinite(level.userData.cityLodErrorMeters) || level.userData.cityLodErrorMeters < 0
        || Object.keys(near.attributes).length !== Object.keys(level.attributes).length
        || Object.entries(near.attributes).some(([name, attribute]) => level.attributes[name] !== attribute))) return;
      let bounds: THREE.Box3;
      if (mesh instanceof THREE.InstancedMesh) {
        mesh.computeBoundingBox();
        bounds = mesh.boundingBox!.clone();
      } else {
        if (!near.boundingBox) near.computeBoundingBox();
        bounds = near.boundingBox!.clone();
      }
      bounds.applyMatrix4(mesh.matrixWorld);
      if (bounds.isEmpty()) return;
      const material = mesh.material as THREE.Material;
      const entry = { mesh, near, material, materialVersion: material.version, levels, bounds, matrixWorld: mesh.matrixWorld.clone(), previous: 0,
        instanceVersion: mesh instanceof THREE.InstancedMesh ? mesh.instanceMatrix.version : undefined,
        instanceCount: mesh instanceof THREE.InstancedMesh ? mesh.count : undefined,
        originalRaycast: mesh.raycast } as Entry;
      entry.raycast = function(this: THREE.Mesh, raycaster, intersects) {
        if (!entry.applied) return entry.originalRaycast.call(this, raycaster, intersects);
        const selected = this.geometry;
        this.geometry = near;
        try { entry.originalRaycast.call(this, raycaster, intersects); }
        finally { this.geometry = selected; }
      };
      mesh.raycast = entry.raycast;
      this.entries.push(entry);
    });
  }

  /** `nearScale` below one brings the full-motif distance closer for a lower
   * graphics preset, to no less than the 12 metre floor. */
  frame(camera: THREE.Camera, viewportHeight = 1080, nearScale = 1): void {
    this.restore();
    if (!(camera instanceof THREE.PerspectiveCamera) || !(viewportHeight > 0) || !Number.isFinite(viewportHeight)) return;
    this.root.updateWorldMatrix(true, false);
    if (!this.root.matrixWorld.equals(this.rootMatrix)) return;
    camera.updateWorldMatrix(true, false);
    this.eye.setFromMatrixPosition(camera.matrixWorld);
    const m = camera.matrixWorldInverse.elements;
    const focal = viewportHeight * Math.max(Math.abs(camera.projectionMatrix.elements[5]),
      Math.abs(camera.projectionMatrix.elements[0]) * camera.aspect) / 2;
    const minDistance = nearScale > 0 && nearScale < 1 ? Math.max(12, this.minDistance * nearScale) : this.minDistance;
    for (const entry of this.entries) {
      const { mesh, near, bounds: b } = entry;
      if (!mesh.visible || !mesh.layers.test(camera.layers) || mesh.geometry !== near || mesh.castShadow
        || mesh.material !== entry.material || entry.material.version !== entry.materialVersion
        || !mesh.matrixWorld.equals(entry.matrixWorld)
        || mesh instanceof THREE.InstancedMesh && (mesh.instanceMatrix.version !== entry.instanceVersion
          || mesh.count !== entry.instanceCount)) continue;
      const distance = b.distanceToPoint(this.eye);
      if (distance < minDistance * (entry.previous ? .9 : 1)) { entry.previous = 0; continue; }
      const cx = (b.min.x + b.max.x) / 2, cy = (b.min.y + b.max.y) / 2, cz = (b.min.z + b.max.z) / 2;
      const hx = (b.max.x - b.min.x) / 2, hy = (b.max.y - b.min.y) / 2, hz = (b.max.z - b.min.z) / 2;
      const depth = -(m[2] * cx + m[6] * cy + m[10] * cz + m[14])
        - Math.abs(m[2]) * hx - Math.abs(m[6]) * hy - Math.abs(m[10]) * hz;
      const x = Math.abs(m[0] * cx + m[4] * cy + m[8] * cz + m[12])
        + Math.abs(m[0]) * hx + Math.abs(m[4]) * hy + Math.abs(m[8]) * hz;
      const y = Math.abs(m[1] * cx + m[5] * cy + m[9] * cz + m[13])
        + Math.abs(m[1]) * hx + Math.abs(m[5]) * hy + Math.abs(m[9]) * hz;
      let selected = 0;
      for (let i = entry.levels.length - 1; i >= 0; i--) {
        const error = entry.levels[i].userData.cityLodErrorMeters * this.errorScale;
        const nearest = depth - error;
        if (!(nearest > camera.near)) continue;
        const projectedError = error * focal / nearest * Math.sqrt(1 + (x * x + y * y) / (nearest * nearest));
        if (projectedError <= this.errorPixels * (entry.previous > i ? 1.15 : 1)) { selected = i + 1; break; }
      }
      entry.previous = selected;
      if (selected) mesh.geometry = entry.applied = entry.levels[selected - 1];
    }
  }

  restore(): void {
    for (const entry of this.entries) {
      if (entry.applied && entry.mesh.geometry === entry.applied) entry.mesh.geometry = entry.near;
      entry.applied = undefined;
    }
  }

  dispose(): void {
    this.restore();
    for (const entry of this.entries) if (entry.mesh.raycast === entry.raycast) entry.mesh.raycast = entry.originalRaycast;
    this.entries.length = 0;
  }
}
