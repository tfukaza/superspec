import type { AuthoringResult } from "./authoring.ts";
import type { ProjectLayouts, ReceiverLayout } from "../renderer/atlas.ts";

type Chart = { receiver: ReceiverLayout; records: Float32Array };
type ChartSpan = [offset: number, bytes: number, base?: string];
type Blocks = { version: number; charts: [string, string, string][];
  materials: Record<string, unknown>; features: Record<string, [number, number, number?]> };
type MeshFeatures = { features: string | number[]; layerFeatures?: (string | number[])[] };

/** The exact blocks retained by one revision. An in-flight request keeps its own
 * snapshot so a cancelled edit can never exchange keys with a different response. */
export class NativeShadingBlocks {
  readonly charts = new Map<string, Chart>();
  readonly materials = new Map<string, { value: unknown; bytes: number }>();
  readonly features = new Map<string, number[]>();
  keys() { return [...this.charts.keys(), ...this.materials.keys(), ...this.features.keys()]; }
}

/** Each group begins with a mask; changed words follow in lane order. Keep all 32 bits,
 * including signed zero, and reject malformed masks before accepting a new snapshot. */
function applyChartDelta(target: Uint32Array, delta: Uint32Array) {
  let at = 0;
  for (let start = 0; start < target.length; start += 32) {
    if (at >= delta.length) throw Error("Incomplete native chart delta.");
    let mask = delta[at++];
    while (mask) {
      const lane = 31 - Math.clz32(mask & -mask);
      if (start + lane >= target.length || at >= delta.length) throw Error("Invalid native chart delta.");
      target[start + lane] = delta[at++];
      mask = (mask & (mask - 1)) >>> 0;
    }
  }
  if (at !== delta.length) throw Error("Invalid native chart delta length.");
}

function expandBlocks(result: AuthoringResult, blocks: Blocks, data: ArrayBuffer, payload: number,
    previous: NativeShadingBlocks | undefined, spans?: ChartSpan[], layoutOffset?: number,
    layoutCount?: number): NativeShadingBlocks {
  if (blocks.version !== 1 || !result.metadata) throw Error("Unsupported native shading report.");
  const next = new NativeShadingBlocks();
  let resident = payload;
  const materials = result.metadata.materials as Record<string, unknown>;
  for (const [id, ref] of Object.entries(materials)) {
    const key = String(ref);
    let material = next.materials.get(key) ?? previous?.materials.get(key);
    if (!material && Object.hasOwn(blocks.materials, key)) {
      const value = blocks.materials[key];
      material = { value, bytes: JSON.stringify(value).length };
    }
    if (!material) throw Error("Missing retained material " + key);
    if (!next.materials.has(key)) resident += material.bytes;
    next.materials.set(key, material);
    materials[id] = material.value;
  }
  const features = (ref: string | number[]): number[] => {
    if (Array.isArray(ref)) return ref;
    let values = next.features.get(ref) ?? previous?.features.get(ref);
    if (!values && Object.hasOwn(blocks.features, ref)) {
      const [offset, count, packedBytes] = blocks.features[ref];
      if (packedBytes === undefined) {
        values = Array.from(new Float64Array(data, payload + offset, count));
      } else {
        const start = payload + offset, view = new DataView(data, start, packedBytes);
        let at = Math.ceil(count / 4);
        if (!Number.isSafeInteger(count) || count < 0 || count % 12 || at > packedBytes)
          throw Error("Invalid native feature count.");
        values = new Array<number>(count);
        for (let i = 0; i < count; i++) {
          const tag = (view.getUint8(i >> 2) >> (2 * (i % 4))) & 3;
          if (tag === 0) values[i] = 0;
          else if (tag === 1 && i >= 12) values[i] = values[i - 12];
          else if (tag === 2) { values[i] = view.getFloat32(at, true); at += 4; }
          else if (tag === 3) { values[i] = view.getFloat64(at, true); at += 8; }
          else throw Error("Invalid native feature coordinate.");
        }
        if (at !== packedBytes) throw Error("Invalid native feature block length.");
      }
    }
    if (!values) throw Error("Missing retained receiver features " + ref);
    if (!next.features.has(ref)) resident += values.length * 8;
    next.features.set(ref, values);
    return values;
  };
  for (const mesh of Object.values(result.metadata.shading as Record<string, MeshFeatures>)) {
    mesh.features = features(mesh.features);
    if (mesh.layerFeatures) mesh.layerFeatures = mesh.layerFeatures.map(features);
  }
  if (result.layouts) {
    const changed = result.layouts;
    type Incoming = Chart & { delta?: Uint32Array };
    const received = new Map<string, Incoming>();
    if (spans && spans.length !== changed.receivers.length) throw Error("Invalid native chart span count.");
    let cursor = layoutOffset ?? 0, word = 0;
    for (const [index, receiver] of changed.receivers.entries()) {
      const words = receiver.triangles * changed.record;
      if (!Number.isSafeInteger(words) || words < 0 || receiver.offset !== word)
        throw Error("Invalid native chart word count.");
      word += words;
      if (spans) {
        const [offset, bytes, base] = spans[index];
        if (offset !== cursor || !Number.isSafeInteger(bytes) || bytes < 0 || bytes % 4
            || (payload + offset) % 4 || payload + offset + bytes > data.byteLength)
          throw Error("Invalid native chart span range.");
        cursor += bytes;
        if (base === undefined) {
          if (bytes !== words * 4) throw Error("Invalid native chart size.");
          received.set(receiver.id, { receiver, records: new Float32Array(data, payload + offset, words) });
        } else {
          const records = previous?.charts.get(base)?.records;
          if (!records || records.length !== words) throw Error("Missing retained native chart delta base " + base);
          received.set(receiver.id, { receiver, records, delta: new Uint32Array(data, payload + offset, bytes / 4) });
        }
      } else {
        if (word > changed.records.length) throw Error("Incomplete receiver chart " + receiver.id);
        received.set(receiver.id, { receiver, records: changed.records.subarray(receiver.offset, word) });
      }
    }
    if (spans && (payload + cursor !== data.byteLength || word !== layoutCount))
      throw Error("Invalid native chart packet size.");
    const resolved = new Map<string, Incoming>();
    const bound: { key: string; receiver: ReceiverLayout; source: Incoming }[] = [];
    let length = 0;
    for (const [id, material, key] of blocks.charts) {
      const chart = received.get(id) ?? resolved.get(key) ?? previous?.charts.get(key);
      if (!chart) throw Error("Missing retained receiver chart " + id);
      resolved.set(key, chart);
      bound.push({ key, receiver: { ...chart.receiver, id, material, offset: length }, source: chart });
      length += chart.records.length;
    }
    // Keep the new snapshot backed by this revision only. Subarrays of an old
    // full report would otherwise pin every earlier generation as corners move.
    const records = new Float32Array(length);
    for (const { key, receiver, source } of bound) {
      records.set(source.records, receiver.offset);
      if (source.delta) applyChartDelta(new Uint32Array(records.buffer, receiver.offset * 4, source.records.length), source.delta);
      next.charts.set(key, { receiver, records: records.subarray(receiver.offset, receiver.offset + source.records.length) });
    }
    result.layouts = { ...changed, receivers: bound.map(entry => entry.receiver), records };
    resident += records.byteLength;
  }
  result.retainedBytes = resident;
  return next;
}

export function decodeNativeReport(bytes: ArrayBuffer, requestId: number, previous?: NativeShadingBlocks,
    retain?: (blocks: NativeShadingBlocks) => void): AuthoringResult {
  const size = new DataView(bytes).getUint32(0, true);
  const report = JSON.parse(new TextDecoder().decode(new Uint8Array(bytes, 4, size)));
  const payload = 4 + size;
  const floats = ([offset, count]: [number, number]) => new Float32Array(bytes.slice(payload + offset, payload + offset + count * 4));
  const unpack = (bake: { width: number; height: number; fields: [number, number][] }) => ({ ...bake, fields: bake.fields.map(floats) });
  const result: AuthoringResult = { requestId, responseBytes: bytes.byteLength, metadata: report.metadata,
    occlusion: report.occlusion ?? undefined, timings: report.timings };
  if (report.fields) result.fields = unpack(report.fields);
  else if (report.projectFields) result.projectFields = report.projectFields.map((entry: { mesh: string; error?: string; fields?: [number, number][]; width: number; height: number }) =>
    entry.fields ? { ...entry, fields: entry.fields.map(floats) } : entry);
  else {
    if (report.packetOffset !== null) result.packet = bytes.slice(payload + (report.packetOffset ?? 0));
    if (report.layouts) result.layouts = { ...report.layouts, records: report.chartRecordSpans ? new Float32Array(0) : report.shadingBlocks
      ? new Float32Array(bytes, payload + report.layoutOffset, report.layoutCount)
      : floats([0, report.layoutCount]) } satisfies ProjectLayouts;
    if (report.shadingBlocks) {
      const next = expandBlocks(result, report.shadingBlocks, bytes, payload, previous,
        report.chartRecordSpans, report.layoutOffset, report.layoutCount);
      if (result.layouts) retain?.((result.retainedBytes ?? 0) <= 128 * 1024 * 1024 ? next : new NativeShadingBlocks());
    }
  }
  return result;
}
