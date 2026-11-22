import { Box3, BufferAttribute, BufferGeometry, Color, DoubleSide, FrontSide, Material, Mesh, MeshLambertMaterial, type MeshStandardMaterial, Sphere, Vector3 } from "three";
import type { SceneMesh, ScenePacket } from "../model/packet.ts";

export type FootprintGeometry = Mesh<BufferGeometry, Material[]>;
export interface FootprintCoverage {
  triangles: number; shaded: number; retainedAtlas: number; procedural: number; texture: number; flat: number; fallback: number;
}
export interface FootprintLook { retained: Mesh }
export function disposeFootprintGeometry(mesh: FootprintGeometry): void {
  mesh.geometry.dispose();
  for (const child of mesh.children) (child as Mesh).geometry.dispose();
  for (const material of mesh.userData.ownedMaterials as Material[]) material.dispose();
}

/** Group ordinary receivers by their on-screen program. Reuse their packet arrays and rebase only the indices.
 * Large or mixed instanced selections continue using the affordable lot preview until their fully shaded revision. */
export async function footprintGeometry(packet: ScenePacket, owners: Set<string>, current: () => boolean,
  look?: (source: SceneMesh) => Material | FootprintLook | undefined): Promise<FootprintGeometry | undefined> {
  const { manifest } = packet;
  if (!current()) return;
  // Batch prototypes can own the geometry drawn for other placements. The ordinary path needs exact individual owners.
  if (manifest.batches?.some(batch => {
    const ids = manifest.batchPlacementOwners?.[batch.objectId];
    return owners.has(batch.objectId) || (batch.template && !ids) || ids?.some(id => owners.has(id));
  })) return;
  const included = new Set<string>(), groups = new Map<string | null | Material, SceneMesh[]>();
  const retained = new Set<Mesh>();
  const coverage: FootprintCoverage = { triangles: 0, shaded: 0, retainedAtlas: 0, procedural: 0, texture: 0, flat: 0, fallback: 0 };
  const decals = new Set(manifest.decals);
  const bounds = new Box3(), point = new Vector3();
  let first = Infinity, end = 0, elements = 0, sliceStart = performance.now();
  for (const mesh of manifest.meshes) {
    if (!mesh.visible || !owners.has(mesh.objectId) || mesh.indexCount === 0) continue;
    included.add(mesh.objectId);
    // The lookup can check an entire unchanged receiver's vertex streams. Yield between those checks.
    if (performance.now() - sliceStart > 4) {
      await new Promise<void>(resolve => setTimeout(resolve, 0));
      if (!current()) return;
      sliceStart = performance.now();
    }
    const found = look?.(mesh);
    coverage.triangles += mesh.indexCount / 3;
    bounds.expandByPoint(point.fromArray(mesh.bounds[0]));
    bounds.expandByPoint(point.fromArray(mesh.bounds[1]));
    if (found && !(found instanceof Material)) {
      retained.add(found.retained);
      coverage.retainedAtlas += mesh.indexCount / 3;
      continue;
    }
    const key = found ?? mesh.material;
    let group = groups.get(key);
    if (!group) groups.set(key, group = []);
    group.push(mesh);
    first = Math.min(first, mesh.vertexStart);
    end = Math.max(end, mesh.vertexStart + mesh.vertexCount);
    elements += mesh.indexCount;
  }
  // Include even unused vertices between the selected ranges when budgeting the borrowed GPU buffers.
  const tangents = manifest.meshTangents, usesTangents = tangents && [...groups.values()].some(group => group.some(receiver => tangents[receiver.id]));
  if (!elements) first = end = 0;
  if (!coverage.triangles || included.size !== owners.size || groups.size > (look ? 4096 : 64) || (end - first) * (usesTangents ? 60 : 44) + elements * 4 > 64 * 1024 * 1024) return;
  const indices = new Uint32Array(elements), runs: { id: string | null | Material; start: number; count: number; meshes: SceneMesh[] }[] = [];
  let cursor = 0;
  for (const [id, meshes] of groups) {
    const start = cursor;
    for (const mesh of meshes) {
      const stop = mesh.indexStart + mesh.indexCount, base = mesh.vertexStart - first;
      for (let i = mesh.indexStart; i < stop;) {
        const chunk = Math.min(i + 65536, stop);
        for (; i < chunk; i++) indices[cursor++] = base + packet.indices[i];
        if (performance.now() - sliceStart > 4) {
          await new Promise<void>(resolve => setTimeout(resolve, 0));
          if (!current()) return;
          sliceStart = performance.now();
        }
      }
    }
    runs.push({ id, start, count: cursor - start, meshes });
  }
  if (!current()) return;
  const geometry = new BufferGeometry(), materials: Material[] = [], owned: Material[] = [];
  const mesh = new Mesh(geometry, materials);
  mesh.userData.ownedMaterials = owned;
  mesh.userData.retained = retained;
  const definitions = new Map(manifest.materials.map(material => [material.id, material]));
  for (const channel of ["position", "normal", "color"] as const) {
    const values = channel === "position" ? packet.positions : channel === "normal" ? packet.normals : packet.colors;
    geometry.setAttribute(channel, new BufferAttribute(values.subarray(first * 3, end * 3), 3));
  }
  if (packet.uvs) geometry.setAttribute("uv", new BufferAttribute(packet.uvs.subarray(first * 2, end * 2), 2));
  const tangent = usesTangents ? new BufferAttribute(new Float32Array((end - first) * 4), 4) : undefined;
  geometry.setIndex(new BufferAttribute(indices, 1));
  // The renderer sorts transparent draws by this center even with culling disabled.
  geometry.boundingBox = bounds;
  geometry.boundingSphere = bounds.getBoundingSphere(new Sphere());
  for (const run of runs) {
    const { id, start, count } = run;
    let material: Material;
    if (id instanceof Material) {
      material = id;
      const pbr = material as MeshStandardMaterial;
      if (material.userData.programUniforms) coverage.procedural += count / 3;
      else if (pbr.map || pbr.normalMap || pbr.roughnessMap || pbr.metalnessMap || pbr.emissiveMap || pbr.aoMap) coverage.texture += count / 3;
      else coverage.flat += count / 3;
    } else {
      coverage.fallback += count / 3;
      const definition = id === null ? undefined : definitions.get(id);
      const color = definition?.baseColorFactor ? new Color().fromArray(definition.baseColorFactor)
        : new Color(definition?.color ?? "#b9c7d1");
      const opacity = definition?.alphaMode === "OPAQUE" ? 1 : definition?.opacity ?? 1;
      const transparent = definition?.alphaMode == null ? opacity < 1 : definition.alphaMode === "BLEND";
      material = new MeshLambertMaterial({ color, vertexColors: true, transparent, depthWrite: !transparent,
        opacity, side: definition?.doubleSided === false ? FrontSide : DoubleSide });
      owned.push(material);
    }
    // Keep camera-sort centers, decal draw order and explicit tangent frames while sharing the packet buffers.
    if (look && (material.transparent || run.meshes.some(receiver => decals.has(receiver.id) || tangents?.[receiver.id]))) {
      let offset = start;
      for (const receiver of run.meshes) {
        const single = new BufferGeometry();
        for (const [name, attr] of Object.entries(geometry.attributes)) single.setAttribute(name, attr);
        if (tangent && tangents?.[receiver.id]) {
          const values = tangents[receiver.id];
          for (let i = 0; i < values.length; i++) tangent.array.set(values[i], (receiver.vertexStart - first + i) * 4);
          single.setAttribute("tangent", tangent);
        }
        single.setIndex(geometry.index);
        single.setDrawRange(offset, receiver.indexCount);
        offset += receiver.indexCount;
        single.boundingBox = new Box3(new Vector3(...receiver.bounds[0]), new Vector3(...receiver.bounds[1]));
        single.boundingSphere = single.boundingBox.getBoundingSphere(new Sphere());
        const part = new Mesh(single, material);
        if (decals.has(receiver.id)) { part.userData.decal = true; part.renderOrder = 1; }
        mesh.add(part);
      }
    } else geometry.addGroup(start, count, materials.length);
    materials.push(material);
  }
  coverage.shaded = coverage.retainedAtlas + coverage.procedural + coverage.texture;
  mesh.userData.coverage = coverage;
  // The building is already on screen and this short-lived draw has no source/picking role.
  mesh.frustumCulled = false;
  return mesh;
}
