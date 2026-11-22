import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";

const taggedMeshes = new WeakMap<THREE.Mesh, string>();
export const primitiveId = (mesh: number, primitive: number) => mesh + "/" + primitive;

/** GLTFLoader provides primitive associations on ordinary meshes. Its instancing
 * extension can replace those meshes, preserving primitive order on the group. */
export function cityLodPrimitiveIds(gltf: GLTF, ids = new WeakMap<THREE.Mesh, string>()) {
  if (!gltf.scene.userData.cityLod) return ids;
  gltf.scene.traverse(node => {
    const association = gltf.parser.associations.get(node);
    if (!association) return;
    if (node instanceof THREE.Mesh && association.meshes !== undefined && association.primitives !== undefined) {
      ids.set(node, primitiveId(association.meshes, association.primitives));
      return;
    }
    const mesh = association.meshes ?? (association.nodes === undefined ? undefined : gltf.parser.json.nodes[association.nodes].mesh);
    if (mesh === undefined) return;
    const count = gltf.parser.json.meshes[mesh].primitives.length;
    const parts = node instanceof THREE.Mesh ? [node] : node.children.slice(0, count);
    parts.forEach((part, i) => {
      if (part instanceof THREE.Mesh) ids.set(part, primitiveId(mesh, i));
    });
  });
  return ids;
}

/** For hosts that supply a tagged scene root instead of the complete GLTF. */
export function tagCityLodPrimitives(gltf: GLTF): void {
  cityLodPrimitiveIds(gltf, taggedMeshes);
}

/** Draw and picking data stay shared. Only the geometry identity used to look
 * up the authored primitive's LOD chain needs to differ. */
function primitiveGeometry(source: THREE.BufferGeometry): THREE.BufferGeometry {
  const geometry = new THREE.BufferGeometry();
  geometry.name = source.name;
  geometry.attributes = { ...source.attributes };
  geometry.morphAttributes = { ...source.morphAttributes };
  geometry.morphTargetsRelative = source.morphTargetsRelative;
  geometry.setIndex(source.index);
  geometry.setIndirect(source.indirect, source.indirectOffset);
  geometry.groups = source.groups.map(group => ({ ...group }));
  geometry.setDrawRange(source.drawRange.start, source.drawRange.count);
  geometry.boundingBox = source.boundingBox?.clone() ?? null;
  geometry.boundingSphere = source.boundingSphere?.clone() ?? null;
  geometry.userData = { ...source.userData };
  return geometry;
}

export function* cityLodBindings(root: THREE.Object3D, records: ReadonlyMap<string, unknown>, ids = taggedMeshes) {
  const byGeometry = new Map<THREE.BufferGeometry, Map<string, THREE.Mesh[]>>();
  root.traverse(node => {
    if (!(node instanceof THREE.Mesh)) return;
    const id = ids.get(node) ?? "";
    let owners = byGeometry.get(node.geometry);
    if (!owners) byGeometry.set(node.geometry, owners = new Map());
    const meshes = owners.get(id);
    if (meshes) meshes.push(node); else owners.set(id, [node]);
  });
  for (const [source, owners] of byGeometry) {
    // Any placement lacking a sidecar must keep a canonical geometry with no
    // LOD lookup. Otherwise keep one authored owner on the original geometry.
    const ownerIds = [...owners.keys()];
    const originalOwner = ownerIds.every(id => records.has(id)) ? ownerIds.sort()[0] : undefined;
    for (const [id, meshes] of owners) {
      if (records.has(id)) yield { id, meshes, geometry: id === originalOwner ? source : primitiveGeometry(source) };
    }
  }
}
