import * as THREE from "three";
import { hasDeclaredLodOwner } from "./lodOwnership.ts";
import { batchableGlazing, glazingPaneBounds } from "./glazing.ts";

type Attribute = THREE.BufferAttribute | THREE.InterleavedBufferAttribute;
type Placement = { id: number; index: number; matrix: THREE.Matrix4; box: THREE.Box3; diameter: number; depth: number };
type Source = { mesh: THREE.Mesh; geometry: THREE.BufferGeometry; matrix: THREE.Matrix4; instanceVersion: number;
  attributes: [string, Attribute, number][]; placements: Placement[]; far: number[]; near: number[];
  subset?: THREE.InstancedMesh; members: number[] };
type Group = { batch: THREE.InstancedMesh; sources: Source[]; selected: Placement[]; members: number[] };

/** Distant view-only instancing. All authored panes, UVs, normals, colors
 * and emissive materials remain; inter-material alpha ordering is approximate.
 * The published source is static. Recreate after edits; unsupported actors stay
 * on their original path. Restore before view culling and every shadow capture. */
export class DistantGlazingDraws {
  readonly stats = { sourceDraws: 0, batchedDraws: 0, nearDraws: 0, savedDraws: 0, farPlacements: 0 };
  private readonly groups: Group[] = [];
  private readonly hidden: THREE.Mesh[] = [];
  private readonly nearShown: THREE.InstancedMesh[] = [];
  private readonly matrix = new THREE.Matrix4();
  private readonly eye = new THREE.Vector3();
  private readonly minDistance: number;
  private readonly maxPanePixels: number;
  private readonly root: THREE.Object3D;
  private readonly rootMatrix: THREE.Matrix4;

  constructor(root: THREE.Object3D,
    options: { minDistance?: number; maxPanePixels?: number; maxInstances?: number } = {}) {
    this.root = root;
    root.updateWorldMatrix(true, true);
    this.rootMatrix = root.matrixWorld.clone();
    this.minDistance = Math.max(1, options.minDistance ?? 25);
    this.maxPanePixels = Math.max(1, options.maxPanePixels ?? 64);
    const maxInstances = options.maxInstances ?? 8192;
    const candidates = new Map<string, THREE.Mesh[]>();
    root.updateWorldMatrix(true, true);
    const toRoot = root.matrixWorld.clone().invert();
    root.traverseVisible(object => {
      if (!(object instanceof THREE.Mesh) || hasDeclaredLodOwner(object) || (object as THREE.SkinnedMesh).isSkinnedMesh || (object as THREE.BatchedMesh).isBatchedMesh
        || object.children.length || object.morphTargetInfluences?.length || object.animations.length || !object.frustumCulled
        || !batchableGlazing(object.material as THREE.Material)) return;
      const material = object.material as THREE.MeshStandardMaterial, geometry = object.geometry;
      const proto = object instanceof THREE.InstancedMesh ? THREE.InstancedMesh.prototype : THREE.Mesh.prototype;
      if ((["onBeforeRender", "onAfterRender", "onBeforeShadow", "onAfterShadow", "raycast"] as const).some(key => object[key] !== proto[key])
        || (object instanceof THREE.InstancedMesh && (object.instanceColor || object.morphTexture))
        || material.displacementMap || material.alphaTest || material.alphaHash || !material.depthTest || material.wireframe
        || material.clippingPlanes?.length || (material as THREE.MeshPhysicalMaterial).transmission > 0
        || geometry.groups.length || geometry.drawRange.start || geometry.drawRange.count < (geometry.index?.count ?? geometry.getAttribute("position")?.count ?? 0)
        || Object.keys(geometry.morphAttributes).length || (geometry as THREE.InstancedBufferGeometry).isInstancedBufferGeometry
        || !geometry.getAttribute("position") || Object.values(geometry.attributes).some(a => a instanceof THREE.InstancedBufferAttribute
          || (a instanceof THREE.InterleavedBufferAttribute && a.data instanceof THREE.InstancedInterleavedBuffer))) return;
      // Reuse the exact source geometry, including normalized interleaved buffers.
      // The normal/UV shader path remains USE_INSTANCING, as in the published city.
      const key = [material.uuid, geometry.uuid, object.layers.mask, object.renderOrder, object.receiveShadow].join("/");
      const list = candidates.get(key) ?? []; list.push(object); candidates.set(key, list);
    });
    let usedInstances = 0;
    for (const meshes of candidates.values()) {
      if (meshes.length < 2) continue;
      const count = meshes.reduce((sum, mesh) => sum + (mesh instanceof THREE.InstancedMesh ? mesh.count : 1), 0);
      if (!count || usedInstances + count > maxInstances) continue;
      const batch = new THREE.InstancedMesh(meshes[0].geometry, meshes[0].material, count);
      batch.name = "Distant glazing / " + (meshes[0].material as THREE.Material).name;
      batch.castShadow = false; batch.receiveShadow = meshes[0].receiveShadow;
      batch.layers.mask = meshes[0].layers.mask; batch.renderOrder = meshes[0].renderOrder;
      batch.visible = false; batch.raycast = () => {}; batch.matrixAutoUpdate = false;
      const sources: Source[] = [];
      let placementId = 0;
      for (const mesh of meshes) {
        const pane = glazingPaneBounds(mesh.geometry), placements: Placement[] = [];
        const n = mesh instanceof THREE.InstancedMesh ? mesh.count : 1;
        const transforms: { local: THREE.Matrix4; world: THREE.Matrix4 }[] = [];
        for (let i = 0; i < n; i++) {
          const world = mesh.matrixWorld.clone();
          if (mesh instanceof THREE.InstancedMesh) { mesh.getMatrixAt(i, this.matrix); world.multiply(this.matrix); }
          const local = new THREE.Matrix4().multiplyMatrices(toRoot, world);
          if (!positiveOrthogonal(local)) { transforms.length = 0; break; }
          transforms.push({ local, world });
        }
        if (transforms.length !== n) continue;
        for (let i = 0; i < n; i++) {
          const { local, world } = transforms[i];
          placements.push({ id: placementId++, index: i, matrix: local, box: pane.bounds.clone().applyMatrix4(world),
            diameter: pane.diameter * scaleBound(world), depth: 0 });
        }
        sources.push({ mesh, geometry: mesh.geometry, matrix: mesh.matrixWorld.clone(),
          instanceVersion: mesh instanceof THREE.InstancedMesh ? mesh.instanceMatrix.version : 0,
          attributes: [...Object.entries(mesh.geometry.attributes), ...(mesh.geometry.index ? [["$index", mesh.geometry.index] as const] : [])]
            .map(([name, a]) => [name, a as Attribute, attributeVersion(a as Attribute)]), placements, far: [], near: [], members: [] });
      }
      if (sources.length < 2) { batch.dispose(); continue; }
      usedInstances += count;
      root.add(batch); batch.updateWorldMatrix(true, false);
      this.groups.push({ batch, sources, selected: [], members: [] });
    }
  }

  restore(): void {
    for (const mesh of this.hidden) mesh.visible = true;
    for (const mesh of this.nearShown) mesh.visible = false;
    for (const group of this.groups) group.batch.visible = false;
    this.hidden.length = 0; this.nearShown.length = 0;
  }

  frame(camera: THREE.Camera, height = 1080, width = height * (camera as THREE.PerspectiveCamera).aspect): void {
    this.restore();
    Object.assign(this.stats, { sourceDraws: 0, batchedDraws: 0, nearDraws: 0, savedDraws: 0, farPlacements: 0 });
    this.root.updateWorldMatrix(true, false);
    if (!this.root.matrixWorld.equals(this.rootMatrix)) return;
    if (!(camera instanceof THREE.PerspectiveCamera) || !(width > 0 && height > 0)) return;
    camera.updateWorldMatrix(true, false); this.eye.setFromMatrixPosition(camera.matrixWorld);
    const focal = 0.5 * Math.max(width * Math.abs(camera.projectionMatrix.elements[0]), height * Math.abs(camera.projectionMatrix.elements[5]));
    for (const { batch, sources, selected, members } of this.groups) {
      selected.length = 0;
      let removed = 0;
      for (const source of sources) {
        source.far.length = 0; source.near.length = 0;
        const mesh = source.mesh;
        if (!hierarchyVisible(mesh) || !mesh.layers.test(camera.layers) || !batchableGlazing(mesh.material as THREE.Material)
          || mesh.material !== batch.material || mesh.geometry !== source.geometry || !mesh.matrixWorld.equals(source.matrix)
          || !mesh.frustumCulled || mesh.layers.mask !== batch.layers.mask || mesh.receiveShadow !== batch.receiveShadow || mesh.renderOrder !== batch.renderOrder
          || (mesh.material as THREE.MeshStandardMaterial).displacementMap
          || source.attributes.some(([name, attribute, version]) => attributeVersion(attribute) !== version
            || (name === "$index" ? mesh.geometry.index : mesh.geometry.getAttribute(name)) !== attribute)
          || (mesh instanceof THREE.InstancedMesh && (mesh.count !== source.placements.length || mesh.instanceMatrix.version !== source.instanceVersion))) continue;
        for (const p of source.placements) {
          const far = p.box.distanceToPoint(this.eye) >= this.minDistance
            && projectedDiameter(p.box, p.diameter, camera.matrixWorldInverse, focal) <= this.maxPanePixels;
          (far ? source.far : source.near).push(p.index);
        }
        if (source.far.length && !source.near.length) removed++;
      }
      // Splitting a nearby source adds a near draw. Two completely distant
      // sources guarantee a net submission reduction even with these splits.
      if (removed < 2) continue;
      batch.visible = true; this.stats.batchedDraws++;
      for (const source of sources) {
        if (!source.far.length) continue;
        const mesh = source.mesh;
        mesh.visible = false; this.hidden.push(mesh); this.stats.sourceDraws++;
        for (const i of source.far) {
          const p = source.placements[i], m = camera.matrixWorldInverse.elements, b = p.box;
          p.depth = -m[14] - 0.5 * (m[2] * (b.min.x + b.max.x) + m[6] * (b.min.y + b.max.y) + m[10] * (b.min.z + b.max.z));
          selected.push(p);
        }
        this.stats.farPlacements += source.far.length;
        if (!source.near.length || !(mesh instanceof THREE.InstancedMesh)) continue;
        let subset = source.subset;
        if (!subset) {
          subset = mesh.clone(false); subset.castShadow = false; subset.raycast = () => {};
          mesh.parent!.add(subset); source.subset = subset;
        }
        if (source.members.length !== source.near.length || source.near.some((index, i) => source.members[i] !== index)) {
          source.members = source.near.slice();
          for (let i = 0; i < source.near.length; i++) { mesh.getMatrixAt(source.near[i], this.matrix); subset.setMatrixAt(i, this.matrix); }
          subset.instanceMatrix.clearUpdateRanges(); subset.instanceMatrix.addUpdateRange(0, source.near.length * 16);
          subset.instanceMatrix.needsUpdate = true;
        }
        subset.count = source.near.length; subset.visible = true; this.nearShown.push(subset); this.stats.nearDraws++;
      }
      selected.sort((a, b) => b.depth - a.depth || a.id - b.id);
      if (members.length !== selected.length || selected.some((p, i) => members[i] !== p.id)) {
        members.length = selected.length;
        for (let i = 0; i < selected.length; i++) { members[i] = selected[i].id; batch.setMatrixAt(i, selected[i].matrix); }
        batch.count = selected.length;
        batch.instanceMatrix.clearUpdateRanges(); batch.instanceMatrix.addUpdateRange(0, selected.length * 16);
        batch.instanceMatrix.needsUpdate = true;
        batch.computeBoundingBox(); batch.computeBoundingSphere();
      }
    }
    this.stats.savedDraws = this.stats.sourceDraws - this.stats.nearDraws - this.stats.batchedDraws;
  }

  dispose(): void {
    this.restore();
    for (const { batch, sources } of this.groups) {
      for (const { subset } of sources) { subset?.removeFromParent(); subset?.dispose(); }
      batch.removeFromParent(); batch.dispose();
    }
    this.groups.length = 0;
  }
}

function attributeVersion(attribute: Attribute): number {
  return attribute instanceof THREE.InterleavedBufferAttribute ? attribute.data.version : attribute.version;
}

function hierarchyVisible(mesh: THREE.Object3D): boolean {
  for (let object: THREE.Object3D | null = mesh; object; object = object.parent) if (!object.visible) return false;
  return true;
}

function positiveOrthogonal(matrix: THREE.Matrix4): boolean {
  if (!(matrix.determinant() > 0)) return false;
  const e = matrix.elements;
  for (const [a, b] of [[0, 4], [0, 8], [4, 8]]) {
    const dot = e[a] * e[b] + e[a + 1] * e[b + 1] + e[a + 2] * e[b + 2];
    const lengths = Math.hypot(e[a], e[a + 1], e[a + 2]) * Math.hypot(e[b], e[b + 1], e[b + 2]);
    if (Math.abs(dot) > lengths * 1e-6) return false;
  }
  return matrix.elements.every(Number.isFinite);
}
function scaleBound(matrix: THREE.Matrix4): number {
  const m = matrix.elements;
  const one = Math.max(Math.abs(m[0]) + Math.abs(m[1]) + Math.abs(m[2]), Math.abs(m[4]) + Math.abs(m[5]) + Math.abs(m[6]), Math.abs(m[8]) + Math.abs(m[9]) + Math.abs(m[10]));
  const infinity = Math.max(Math.abs(m[0]) + Math.abs(m[4]) + Math.abs(m[8]), Math.abs(m[1]) + Math.abs(m[5]) + Math.abs(m[9]), Math.abs(m[2]) + Math.abs(m[6]) + Math.abs(m[10]));
  return Math.sqrt(one * infinity);
}
function projectedDiameter(box: THREE.Box3, diameter: number, view: THREE.Matrix4, focal: number): number {
  const m = view.elements, x = (box.min.x + box.max.x) / 2, y = (box.min.y + box.max.y) / 2, z = (box.min.z + box.max.z) / 2;
  const dx = (box.max.x - box.min.x) / 2, dy = (box.max.y - box.min.y) / 2, dz = (box.max.z - box.min.z) / 2;
  const nearest = -m[14] - m[2] * x - m[6] * y - m[10] * z - Math.abs(m[2]) * dx - Math.abs(m[6]) * dy - Math.abs(m[10]) * dz;
  if (!(nearest > 0)) return Infinity;
  const maxX = Math.abs(m[12] + m[0] * x + m[4] * y + m[8] * z) + Math.abs(m[0]) * dx + Math.abs(m[4]) * dy + Math.abs(m[8]) * dz;
  const maxY = Math.abs(m[13] + m[1] * x + m[5] * y + m[9] * z) + Math.abs(m[1]) * dx + Math.abs(m[5]) * dy + Math.abs(m[9]) * dz;
  return diameter * focal / nearest * Math.sqrt(1 + (maxX * maxX + maxY * maxY) / (nearest * nearest));
}
