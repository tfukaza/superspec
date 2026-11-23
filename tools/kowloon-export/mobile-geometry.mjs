import { MeshoptDecoder, MeshoptEncoder } from "meshoptimizer";

const componentBytes = { 5121: 1, 5122: 2, 5123: 2, 5125: 4, 5126: 4 };
const readers = { 5121: "getUint8", 5122: "getInt16", 5123: "getUint16", 5125: "getUint32", 5126: "getFloat32" };
const components = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4 };
const chains = ["levels", "detailLevels", "distantLevels"];
const compactible = (primitive, material) => !primitive.targets && (material?.alphaMode ?? "OPAQUE") === "OPAQUE"
  && !material?.extras?.cityDetailKeep && !material?.emissiveTexture
  && !material?.emissiveFactor?.some(value => value > 0) && !material?.extensions?.KHR_materials_emissive_strength;
/** The shadowless weed underpainting the viewer's vegetation LOD also selects (cityVegetationLod.ts). */
const paintedWash = (primitive, material) => !primitive.targets && material?.alphaMode === "BLEND"
  && material.extras?.castsShadow === false && material.extras?.worldSpecSurface?.family === "Vegetation"
  && material.extras.worldSpecSurface.recipe === "Weed underpainting" && !material.extras.grassField;

/** The stream a pack draws at arm's length. A player can walk up to any city
 * surface, so its error has one small bound for every material family. The
 * coarser streams remain distance levels selected by the viewer. */
export function selectMobileIndexTier(primitive, material, record, budget) {
  // A weed's soft underpainting is a blended wash without edges of its own, so
  // it takes a wider bound than surfaces a player reads up close.
  const bound = paintedWash(primitive, material) ? budget.washErrorMeters
    : compactible(primitive, material) ? budget.nearErrorMeters : 0;
  if (!record || !(bound > 0)) return;
  let best;
  for (const level of [...record.levels ?? [], ...record.detailLevels ?? []]) {
    if (!level.bytes || level.count <= 0 || !Number.isFinite(level.errorMeters)
        || level.errorMeters > bound) continue;
    if (!best || level.count < best.count) best = level;
  }
  return best;
}

function readIndices(accessor, bytes) {
  const Type = accessor.componentType === 5123 ? Uint16Array : Uint32Array;
  const offset = accessor.byteOffset ?? 0, size = componentBytes[accessor.componentType];
  if (!size || accessor.count * size + offset > bytes.length) throw Error("Invalid city index accessor.");
  const data = new DataView(bytes.buffer, bytes.byteOffset + offset, accessor.count * size);
  return Type.from({ length: accessor.count }, (_, index) => data[readers[accessor.componentType]](index * size, true));
}

function updateBounds(accessor, bytes, stride) {
  if (!accessor.min && !accessor.max) return;
  const width = components[accessor.type], size = componentBytes[accessor.componentType];
  const get = readers[accessor.componentType], data = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const min = Array(width).fill(Infinity), max = Array(width).fill(-Infinity);
  const offset = accessor.byteOffset ?? 0;
  for (let vertex = 0; vertex < accessor.count; vertex++) for (let component = 0; component < width; component++) {
    const value = data[get](vertex * stride + offset + component * size, true);
    min[component] = Math.min(min[component], value);
    max[component] = Math.max(max[component], value);
  }
  if (accessor.min) accessor.min = min;
  if (accessor.max) accessor.max = max;
}

function decodeLevel(lod, level) {
  if (![2, 4].includes(level.componentBytes) || !Number.isSafeInteger(level.count) || level.count <= 0 || level.count % 3
      || level.offset + level.bytes > lod.payload.length) throw Error("Invalid city LOD triangle stream.");
  const data = new (level.componentBytes === 2 ? Uint16Array : Uint32Array)(level.count);
  MeshoptDecoder.decodeIndexBuffer(new Uint8Array(data.buffer), data.length, level.componentBytes,
    lod.payload.subarray(level.offset, level.offset + level.bytes));
  return data;
}

/** Promote bounded, authored index LODs and compact their shared vertex streams.
 * Every remaining distance level is carried onto the compacted vertices. */
export async function mobileGeometry(json, lod, read, budget) {
  await Promise.all([MeshoptDecoder.ready, MeshoptEncoder.ready]);
  const groups = new Map(), owners = new Map(), byView = new Map();
  const replacement = new Map(), lods = new Map();
  const records = new Map((lod?.manifest.primitives ?? []).map(record => [record.mesh + "/" + record.primitive, record]));
  for (const [index, accessor] of json.accessors.entries()) {
    if (!byView.has(accessor.bufferView)) byView.set(accessor.bufferView, []);
    byView.get(accessor.bufferView).push(index);
  }
  const recorded = [];
  for (const [mesh, { primitives }] of json.meshes.entries()) for (const [index, primitive] of primitives.entries()) {
    const record = records.get(mesh + "/" + index);
    if (record) recorded.push({ primitive, record });
    if (primitive.mode !== undefined && primitive.mode !== 4 || primitive.indices === undefined) continue;
    const view = json.accessors[primitive.attributes.POSITION]?.bufferView;
    if (view === undefined) continue;
    if (!groups.has(view)) groups.set(view, []);
    groups.get(view).push({ primitive, record });
    owners.set(primitive.indices, (owners.get(primitive.indices) ?? 0) + 1);
  }
  const oldIndexViews = new Set();
  let trianglesRemoved = 0, verticesRemoved = 0;
  for (const [viewId, group] of groups) {
    const view = json.bufferViews[viewId], stride = view.byteStride;
    if (!stride || stride % 4 || view.byteLength % stride) continue;
    const vertexCount = view.byteLength / stride;
    const attributes = new Set(group.flatMap(({ primitive }) => Object.values(primitive.attributes)));
    // A view shared with an instancing or otherwise unclassified accessor keeps its original vertices.
    if (group.some(({ primitive }) => primitive.targets || owners.get(primitive.indices) !== 1 ||
        ![5123, 5125].includes(json.accessors[primitive.indices].componentType)) ||
        byView.get(viewId).some(id => !attributes.has(id)) || [...attributes].some(id => {
          const a = json.accessors[id];
          return a.bufferView !== viewId || a.count !== vertexCount || a.sparse;
        })) continue;
    const selected = [];
    let changed = false;
    for (const { primitive, record } of group) {
      const accessor = json.accessors[primitive.indices];
      const matches = record && record.vertexCount === vertexCount && record.nearCount === accessor.count;
      const tier = matches ? selectMobileIndexTier(primitive, json.materials[primitive.material], record, budget) : undefined;
      if (tier && tier.count < accessor.count) {
        selected.push({ matches, indices: decodeLevel(lod, tier), errorMeters: tier.errorMeters });
        changed = true;
      } else selected.push({ matches });
    }
    if (!changed) continue;
    const remap = new Int32Array(vertexCount).fill(-1);
    const original = await read(viewId);
    const packed = Buffer.allocUnsafe(original.length);
    let newCount = 0;
    const keep = vertex => {
      if (vertex >= vertexCount) throw Error("Invalid city vertex reference.");
      if (remap[vertex] === -1) {
        remap[vertex] = newCount++;
        original.copy(packed, remap[vertex] * stride, vertex * stride, (vertex + 1) * stride);
      }
      return remap[vertex];
    };
    for (const [index, { primitive }] of group.entries()) {
      const accessor = json.accessors[primitive.indices];
      const source = selected[index].indices ?? readIndices(accessor, await read(accessor.bufferView));
      const oldView = accessor.bufferView;
      const indices = Buffer.allocUnsafe(source.length * 4);
      let max = 0;
      for (let i = 0; i < source.length; i++) {
        const vertex = keep(source[i]);
        max = Math.max(max, vertex);
        indices.writeUInt32LE(vertex, i * 4);
      }
      const data = max <= 65535 ? Buffer.from(Uint16Array.from(new Uint32Array(indices.buffer, indices.byteOffset, source.length)).buffer) : indices;
      trianglesRemoved += (accessor.count - source.length) / 3;
      accessor.bufferView = json.bufferViews.length;
      accessor.byteOffset = 0;
      accessor.count = source.length;
      accessor.componentType = data === indices ? 5125 : 5123;
      // These accessors do not require extrema.
      delete accessor.min;
      delete accessor.max;
      json.bufferViews.push({ buffer: json.bufferViews[oldView].buffer, byteLength: data.length, target: 34963 });
      replacement.set(accessor.bufferView, { bytes: data, mode: "TRIANGLES", stride: componentBytes[accessor.componentType] });
      oldIndexViews.add(oldView);
    }
    // Distance levels were simplified independently from the original vertices,
    // so each one can reference vertices the promoted near stream does not.
    const carried = [];
    for (const [index, { primitive, record }] of group.entries()) {
      if (!selected[index].matches) continue;
      const nearCount = json.accessors[primitive.indices].count, nearError = selected[index].errorMeters ?? 0;
      const entry = { vertexCount: 0, nearCount };
      for (const chain of chains) if (record[chain]) entry[chain] = record[chain].map(level => {
        if (!level.bytes || level.count >= nearCount)
          return { count: nearCount, componentBytes: level.componentBytes, errorMeters: Math.max(nearError, level.errorMeters ?? 0) };
        const indices = decodeLevel(lod, level);
        for (let i = 0; i < indices.length; i++) keep(indices[i]);
        return { count: level.count, errorMeters: level.errorMeters, indices };
      });
      carried.push(entry);
      lods.set(primitive, entry);
    }
    const width = newCount <= 65536 ? 2 : 4;
    for (const entry of carried) {
      entry.vertexCount = newCount;
      for (const chain of chains) for (const level of entry[chain] ?? []) {
        if (!level.indices) continue;
        const data = new (width === 2 ? Uint16Array : Uint32Array)(level.count);
        for (let i = 0; i < data.length; i++) data[i] = remap[level.indices[i]];
        level.stream = MeshoptEncoder.encodeIndexBuffer(new Uint8Array(data.buffer), level.count, width);
        level.componentBytes = width;
        delete level.indices;
      }
    }
    verticesRemoved += vertexCount - newCount;
    const bytes = packed.subarray(0, newCount * stride);
    for (const id of attributes) {
      const accessor = json.accessors[id];
      accessor.count = newCount;
      updateBounds(accessor, bytes, stride);
    }
    replacement.set(viewId, { bytes, mode: "ATTRIBUTES", stride });
  }
  // Unchanged geometry keeps its published distance streams byte for byte.
  for (const { primitive, record } of recorded) {
    if (lods.has(primitive)) continue;
    if (record.nearCount !== json.accessors[primitive.indices]?.count
        || record.vertexCount !== json.accessors[primitive.attributes.POSITION]?.count) continue;
    const entry = { vertexCount: record.vertexCount, nearCount: record.nearCount };
    for (const chain of chains) if (record[chain]) entry[chain] = record[chain].map(level => ({
      count: level.count, componentBytes: level.componentBytes, errorMeters: level.errorMeters,
      stream: level.bytes ? lod.payload.subarray(level.offset, level.offset + level.bytes) : undefined,
    }));
    lods.set(primitive, entry);
  }
  // Unchanged primitives can still use a shared old index view. Split their
  // accessors as well so the unused portions of the stream are never shipped.
  for (const old of oldIndexViews) {
    const pending = json.accessors.filter(a => a.bufferView === old);
    if (pending.some(a => a.sparse || a.type !== "SCALAR" || ![5123, 5125].includes(a.componentType))) continue;
    const source = pending.length && await read(old);
    for (const accessor of pending) {
      const stride = componentBytes[accessor.componentType], offset = accessor.byteOffset ?? 0;
      const bytes = source.subarray(offset, offset + accessor.count * stride);
      accessor.bufferView = json.bufferViews.length;
      accessor.byteOffset = 0;
      json.bufferViews.push({ buffer: json.bufferViews[old].buffer, byteLength: bytes.length, target: 34963 });
      replacement.set(accessor.bufferView, { bytes, mode: "TRIANGLES", stride });
    }
  }
  return { replacement, trianglesRemoved, verticesRemoved, lods };
}

/** Address the carried distance levels by the pack's final mesh and primitive
 * positions. Call after visibility filtering and resource pruning. */
export function mobileLodSidecar(json, lods) {
  const primitives = [], payload = [];
  let offset = 0;
  for (const [mesh, { primitives: parts }] of json.meshes.entries()) for (const [primitive, part] of parts.entries()) {
    const entry = lods.get(part);
    if (!entry || !chains.some(chain => entry[chain]?.some(level => level.count < entry.nearCount))) continue;
    const record = { mesh, primitive, vertexCount: entry.vertexCount, nearCount: entry.nearCount };
    for (const chain of chains) if (entry[chain]) record[chain] = entry[chain].map(level => {
      const bytes = level.stream?.length ?? 0, placed = { offset, bytes, count: level.count,
        componentBytes: level.componentBytes, errorMeters: level.errorMeters };
      if (bytes) payload.push(level.stream);
      offset += bytes;
      return placed;
    });
    primitives.push(record);
  }
  if (primitives.length) return { manifest: { primitives }, payload: Buffer.concat(payload, offset) };
}
