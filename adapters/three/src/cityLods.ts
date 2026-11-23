import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { cityLodBindings, cityLodPrimitiveIds, primitiveId } from "./cityLodPrimitives.ts";
export { tagCityLodPrimitives } from "./cityLodPrimitives.ts";

export type CityLodLevels = readonly THREE.BufferGeometry[];
type Level = { offset: number; bytes: number; count: number; componentBytes: number; errorMeters?: number };
type RecordEntry = { mesh: number; primitive: number; vertexCount: number; nearCount: number; levels: Level[]; distantLevels?: Level[]; detailLevels?: Level[] };

/** Resolve the sidecar beside its GLB. Existing hosts may also pass a root
 * whose authored primitives they have already tagged. */
export async function loadCityLods(source: GLTF | THREE.Object3D, entryUrl: string | URL,
  signal?: AbortSignal): Promise<Map<THREE.BufferGeometry, CityLodLevels>> {
  const result = new Map<THREE.BufferGeometry, CityLodLevels>();
  const root = "scene" in source ? source.scene : source;
  const path = root.userData.cityLod;
  if (typeof path !== "string" || !/^[\w-]+(?:\.[\w-]+)*-data\/lod-[a-f0-9]+\.bin$/.test(path)) return result;
  const created: THREE.BufferGeometry[] = [];
  try {
    const ids = "scene" in source ? cityLodPrimitiveIds(source) : undefined;
    const response = await fetch(new URL(path, entryUrl), { signal });
    if (!response.ok) throw Error("HTTP " + response.status);
    const buffer = await response.arrayBuffer(), header = new DataView(buffer);
    if (buffer.byteLength < 12 || header.getUint32(0) !== 0x534c4f44 || header.getUint32(4, true) !== 1) throw Error("Unexpected SLOD header");
    const length = header.getUint32(8, true);
    if (length > buffer.byteLength - 12) throw Error("Truncated SLOD manifest");
    const entries: RecordEntry[] = JSON.parse(new TextDecoder().decode(new Uint8Array(buffer, 12, length))).primitives;
    const start = 12 + Math.ceil(length / 4) * 4;
    if (!Array.isArray(entries) || start > buffer.byteLength) throw Error("Invalid SLOD manifest");
    const { MeshoptDecoder } = await import("three/examples/jsm/libs/meshopt_decoder.module.js");
    await MeshoptDecoder.ready;
    signal?.throwIfAborted();
    const records = new Map<string, RecordEntry | undefined>();
    for (const entry of entries) {
      const id = primitiveId(entry.mesh, entry.primitive);
      records.set(id, records.has(id) ? undefined : entry);
    }
    const bindings = [...cityLodBindings(root, records, ids)];
    const decoded = new Map<Level, THREE.BufferAttribute>();
    for (const { id, geometry: geom } of bindings) {
      const entry = records.get(id);
      if (!entry) throw Error("Duplicate city LOD primitive " + id);
      if (entry.nearCount !== geom.index?.count || entry.vertexCount !== geom.attributes.position.count || !Array.isArray(entry.levels) || entry.levels.length < 2 || entry.levels.length > 4)
        throw Error("City LOD primitive has incompatible geometry");
      if (entry.distantLevels !== undefined && (!Array.isArray(entry.distantLevels) || entry.levels.length + entry.distantLevels.length > 8))
        throw Error("City LOD primitive has invalid distant levels");
      if (entry.detailLevels !== undefined && (!Array.isArray(entry.detailLevels) || entry.detailLevels.length < 2 || entry.detailLevels.length > 8))
        throw Error("City LOD primitive has invalid detail levels");
      const read = (level: Level): THREE.BufferGeometry => {
        if (level.errorMeters !== undefined && (!Number.isFinite(level.errorMeters) || level.errorMeters < 0))
          throw Error("Invalid city LOD error bound");
        if (level.count === entry.nearCount && level.bytes === 0) return geom;
        if (!Number.isSafeInteger(level.count) || level.count < 0 || level.count > entry.nearCount || level.count % 3 ||
            !Number.isSafeInteger(level.offset) || level.offset < 0 || !Number.isSafeInteger(level.bytes) || level.bytes < 0 || start + level.offset + level.bytes > buffer.byteLength ||
            ![2,4].includes(level.componentBytes) || (level.count === 0) !== (level.bytes === 0))
          throw Error("Invalid city LOD triangle stream");
        let index = decoded.get(level);
        if (!index) {
          const indices = level.componentBytes === 2 ? new Uint16Array(level.count) : new Uint32Array(level.count);
          if (level.count) {
            MeshoptDecoder.decodeIndexBuffer(new Uint8Array(indices.buffer), level.count, level.componentBytes,
              new Uint8Array(buffer, start + level.offset, level.bytes));
            if (indices.some(index => index >= entry.vertexCount)) throw Error("City LOD index out of range");
          }
          index = new THREE.BufferAttribute(indices, 1);
          decoded.set(level, index);
        }
        const alternate = new THREE.BufferGeometry();
        for (const [name, attribute] of Object.entries(geom.attributes)) alternate.setAttribute(name, attribute);
        alternate.setIndex(index);
        alternate.boundingBox = geom.boundingBox;
        alternate.boundingSphere = geom.boundingSphere;
        alternate.userData.cityLodErrorMeters = level.errorMeters;
        created.push(alternate);
        return alternate;
      };
      result.set(geom, (entry.detailLevels ?? [...entry.levels, ...entry.distantLevels ?? []]).map(read));
    }
    // Publish only after every stream validates. A bad record leaves the
    // entire glTF exactly as the loader supplied it, including shared geometry.
    for (const { meshes, geometry } of bindings) {
      if (result.get(geometry)!.includes(geometry)) geometry.userData.cityLodErrorMeters = 0;
      for (const mesh of meshes) mesh.geometry = geometry;
    }
  } catch (error) {
    for (const geom of created) geom.dispose();
    result.clear();
    signal?.throwIfAborted();
    console.warn("Using full-detail city geometry:", error);
  }
  return result;
}
