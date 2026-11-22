import { PLACEMENT_FLOATS, type BatchBuffers, type SceneBatch, type ScenePacket } from "../model/packet.ts";

/** Exact comparison of validated JSON records, including the local canopy sites and twigs. */
function equal(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (!a || !b || typeof a !== "object" || typeof b !== "object") return false;
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) if (!equal(a[i], b[i])) return false;
    return true;
  }
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every(key => Object.hasOwn(b, key)
    && equal((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key]));
}

function sameRange(a: Float32Array | Uint32Array, start: number, b: Float32Array | Uint32Array, next: number, count: number): boolean {
  if (a === b && start === next) return true;
  const old = new Uint32Array(a.buffer, a.byteOffset + start * 4, count);
  const incoming = new Uint32Array(b.buffer, b.byteOffset + next * 4, count);
  for (let i = 0; i < count; i++) if (old[i] !== incoming[i]) return false;
  return true;
}

/** Storage offsets and source locations may shift when an earlier object changes. */
function sameLevel(a: SceneBatch, old: BatchBuffers, b: SceneBatch, next: BatchBuffers): boolean {
  const shape = ({ source, vertexStart, indexStart, instanceStart, valueStart, tintStart, attributes, ...look }: SceneBatch) => look;
  if (!equal(shape(a), shape(b))) return false;
  if ((a.valueStart === undefined) !== (b.valueStart === undefined) || (a.tintStart === undefined) !== (b.tintStart === undefined)) return false;
  if (!sameRange(old.placements, a.instanceStart * PLACEMENT_FLOATS, next.placements, b.instanceStart * PLACEMENT_FLOATS, a.instanceCount * PLACEMENT_FLOATS)
    || !sameRange(old.indices, a.indexStart, next.indices, b.indexStart, a.indexCount)) return false;
  for (const [name, size] of [["positions", 3], ["normals", 3], ["uvs", 2], ["colors", 3]] as const) {
    if (!sameRange(old[name], a.vertexStart * size, next[name], b.vertexStart * size, a.vertexCount * size)) return false;
  }
  if (a.valueStart !== undefined && !sameRange(old.values, a.valueStart * 4, next.values, b.valueStart! * 4, a.instanceCount * 4)) return false;
  if (a.tintStart !== undefined && !sameRange(old.tints, a.tintStart * 3, next.tints, b.tintStart! * 3, a.instanceCount * 3)) return false;
  const left = a.attributes ?? [], right = b.attributes ?? [];
  return left.length === right.length && left.every((attr, i) => {
    const other = right[i];
    const count = (attr.domain === "vertex" ? a.vertexCount : a.instanceCount) * attr.components;
    return attr.name === other.name && attr.domain === other.domain && attr.components === other.components
      && sameRange(old.attributes, attr.start, next.attributes, other.start, count);
  });
}

function patches(packet: ScenePacket) {
  const groups = new Map<string, SceneBatch[]>();
  for (const batch of packet.manifest.batches ?? []) {
    if (!batch.foliage) continue;
    const id = batch.foliage.patch;
    const group = groups.get(id);
    if (group) group.push(batch); else groups.set(id, [batch]);
  }
  return groups;
}

/** A painted crown and its matching bark are one rendered plant. Retain all its draws or rebuild all of them. */
export function retainedPlants(previous: ScenePacket | undefined, next: ScenePacket, drawn: (id: string) => boolean): Set<string> {
  const kept = new Set<string>();
  if (!previous?.batches || !next.batches) return kept;
  const before = patches(previous);
  for (const [id, group] of patches(next)) {
    const old = before.get(id);
    if (!old || old.length !== group.length || !group.some(b => b.foliage?.mass)) continue;
    if (group.every((b, i) => b.material === null && (b.foliage?.family === "bark" || b.foliage?.family === "shadow")
      && drawn(b.id) && sameLevel(old[i], previous.batches!, b, next.batches!))) {
      for (const batch of group) kept.add(batch.id);
    }
  }
  return kept;
}
