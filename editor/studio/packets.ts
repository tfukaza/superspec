import { validatePacket, type SceneMesh, type ScenePacket, type SceneMaterial } from "../../browser/model/packet.ts";
import type { Receiver } from "../../browser/runtime/authoring.ts";

/** Extracts selected render receivers with mesh-local indices; the accepted base packet stays immutable. */
export function receivers(packet: ScenePacket, ids: ReadonlySet<string>, transforms?: Record<string, number[]>): Receiver[] {
  return packet.manifest.meshes.filter(mesh => mesh.visible && ids.has(mesh.id)).map(mesh => ({
    id: mesh.id,
    objectId: mesh.objectId,
    positions: Array.from(packet.positions.slice(mesh.vertexStart * 3, (mesh.vertexStart + mesh.vertexCount) * 3)),
    normals: Array.from(packet.normals.slice(mesh.vertexStart * 3, (mesh.vertexStart + mesh.vertexCount) * 3)),
    uvs: Array.from(packet.uvs.slice(mesh.vertexStart * 2, (mesh.vertexStart + mesh.vertexCount) * 2)),
    indices: Array.from(packet.indices.slice(mesh.indexStart, mesh.indexStart + mesh.indexCount)),
    objectToWorld: transforms?.[mesh.id],
  }));
}

/** Replaces only authored render bindings, retaining physical metadata and original source provenance. */
export function mergeReceivers(base: ScenePacket, replacement: ScenePacket, ids: ReadonlySet<string>): ScenePacket {
  const updated = new Map(replacement.manifest.meshes.filter(mesh => mesh.visible).map(mesh => [mesh.id, mesh]));
  for (const id of ids) if (!updated.has(id)) throw new Error(`Material result omitted receiver ${id}.`);
  const positions: number[] = [], normals: number[] = [], uvs: number[] = [], colors: number[] = [], indices: number[] = [];
  const meshes: SceneMesh[] = [];
  for (const original of base.manifest.meshes) {
    const changed = original.visible && ids.has(original.id);
    const mesh = changed ? updated.get(original.id)! : original;
    const packet = changed ? replacement : base;
    const vertexStart = positions.length / 3;
    const indexStart = indices.length;
    for (let i = mesh.vertexStart * 3; i < (mesh.vertexStart + mesh.vertexCount) * 3; i++) {
      positions.push(packet.positions[i]); normals.push(packet.normals[i]); colors.push(packet.colors[i]);
    }
    for (let i = mesh.vertexStart * 2; i < (mesh.vertexStart + mesh.vertexCount) * 2; i++) uvs.push(packet.uvs[i]);
    for (let i = mesh.indexStart; i < mesh.indexStart + mesh.indexCount; i++) indices.push(packet.indices[i]);
    meshes.push({ ...mesh, id: original.id, objectId: original.objectId, source: original.source, vertexStart, indexStart });
  }
  const materialMap = new Map<string, SceneMaterial>(base.manifest.materials.map(material => [material.id, material]));
  for (const material of replacement.manifest.materials) materialMap.set(material.id, material);
  const used = new Set(meshes.map(mesh => mesh.material));
  const materials = [...materialMap.values()].filter(material => used.has(material.id));
  const requiredAssets = new Set(materials.flatMap(material => [material.texture, material.normalTexture, material.metallicRoughnessTexture, material.heightTexture, material.emissiveTexture ?? null, material.occlusionTexture ?? null]).filter((id): id is string => id !== null));
  const images = new Map<string, { mime: "image/png" | "image/jpeg" | "image/ktx2"; bytes: Uint8Array }>();
  for (const packet of [base, replacement]) for (const asset of packet.manifest.assets) {
    if (requiredAssets.has(asset.id)) images.set(asset.id, { mime: asset.mime, bytes: packet.assets.slice(asset.byteStart, asset.byteStart + asset.byteLength) });
  }
  const byteLength = [...images.values()].reduce((total, image) => total + image.bytes.length, 0);
  const assets = new Uint8Array(byteLength);
  let cursor = 0;
  const descriptors = [...images].map(([id, image]) => {
    const descriptor = { id, mime: image.mime, byteStart: cursor, byteLength: image.bytes.length };
    assets.set(image.bytes, cursor); cursor += image.bytes.length;
    return descriptor;
  });
  const meshTangents = { ...base.manifest.meshTangents };
  for (const id of ids) {
    delete meshTangents[id];
    const replacementTangents = replacement.manifest.meshTangents?.[id];
    if (replacementTangents) meshTangents[id] = replacementTangents;
  }
  return validatePacket({
    manifest: { ...base.manifest, revision: replacement.manifest.revision, meshes, materials, assets: descriptors,
      vertexCount: positions.length / 3, indexCount: indices.length, assetByteCount: assets.length,
      meshTangents: Object.keys(meshTangents).length ? meshTangents : undefined },
    positions: new Float32Array(positions), normals: new Float32Array(normals), uvs: new Float32Array(uvs),
    colors: new Float32Array(colors), indices: new Uint32Array(indices), assets,
  });
}
