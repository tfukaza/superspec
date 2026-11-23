import { parentPort, workerData } from "node:worker_threads";
import { MeshoptDecoder, MeshoptEncoder, MeshoptSimplifier } from "meshoptimizer";

const ready = Promise.all([MeshoptEncoder.ready, MeshoptDecoder.ready, MeshoptSimplifier.ready]);
const levels = [
  { fraction: 0.48, errorMeters: 0.035 },
  { fraction: 0.16, errorMeters: 0.11 },
];
const distantLevels = [{ fraction: 0.05, errorMeters: 0.35 }];
export const detailLevels = [
  { fraction: 0.24, errorMeters: 0.035 },
  { fraction: 0.12, errorMeters: 0.07 },
  { fraction: 0.06, errorMeters: 0.11 },
  { fraction: 0.03, errorMeters: 0.18 },
  { fraction: 0.015, errorMeters: 0.35 },
];
export const aggressiveFarLevels = [
  { fraction: 0.005, errorMeters: 0.7 },
  { fraction: 0.0015, errorMeters: 1.4 },
];

function verifyIndices(source, decoded) {
  // Triangle encoding may cycle its three corners; winding and triangle order are invariant.
  for (let i = 0; i < source.length; i += 3) {
    const a = source[i], b = source[i+1], c = source[i+2];
    const x = decoded[i], y = decoded[i+1], z = decoded[i+2];
    if (!(x === a && y === b && z === c || x === b && y === c && z === a || x === c && y === a && z === b))
      throw Error("LOD index stream changed triangle winding or vertices");
  }
}

/** Compute every requested stream from the same source geometry in one worker. Offsets are
 * relative to this primitive until publication joins the buffers in original mesh order. */
export async function simplifyPrimitive({ indices, positions, fields, weights, policy, farPolicy, protect, protectError = Infinity }) {
  await ready;
  const vertexCount = positions.length / 3, componentBytes = vertexCount <= 65536 ? 2 : 4;
  const record = { vertexCount, nearCount: indices.length, levels: [] };
  const additional = [];
  if (policy) record.detailLevels = additional;
  else record.distantLevels = additional;
  const plans = [
    { levels, records: record.levels, flags: ["ErrorAbsolute", "Prune"] },
    { levels: policy?.levels ?? distantLevels, records: additional, flags: policy?.flags ?? ["ErrorAbsolute", "Prune"] },
  ];
  // Append to the selected chain. Compatibility streams and every existing detail
  // tier are still computed independently from the unchanged original vertices.
  if (farPolicy) plans.push({ ...farPolicy, records: additional, preserveReduction: true });
  const chunks = [];
  let offset = 0, previous;
  for (const plan of plans) for (const level of plan.levels) {
    const target = 3 * Math.floor(indices.length * level.fraction / 3);
    let [simplified, error] = MeshoptSimplifier.simplifyWithAttributes(
      // Chart-seam marks only matter where a plan may collapse through attribute seams.
      indices, positions, 3, fields, weights.length, weights,
      plan.flags.includes("Permissive") && level.errorMeters <= protectError ? protect ?? null : null, target, level.errorMeters, plan.flags,
    );
    // Independent error thresholds can leave a few more triangles after a
    // different collapse sequence. Never make an optional farther tier heavier.
    if (plan.preserveReduction && (simplified.length > previous.indices.length
        || simplified.length === previous.indices.length && error >= previous.error))
      ({ indices: simplified, error } = previous);
    previous = { indices: simplified, error };
    const count = simplified.length;
    let bytes = 0;
    if (error > level.errorMeters + 1e-5) throw Error("LOD geometric error exceeded its metre budget");
    if (count && count !== indices.length) {
      const Type = componentBytes === 2 ? Uint16Array : Uint32Array;
      for (const v of simplified) if (v >= vertexCount) throw Error("LOD references an absent vertex");
      const typed = Type.from(simplified);
      const stream = MeshoptEncoder.encodeIndexBuffer(new Uint8Array(typed.buffer), count, componentBytes);
      const check = new Type(count);
      MeshoptDecoder.decodeIndexBuffer(new Uint8Array(check.buffer), count, componentBytes, stream);
      verifyIndices(typed, check);
      chunks.push(stream); bytes = stream.length;
    }
    // Old SLOD consumers use the two compatibility levels. Further levels are optional.
    plan.records.push({ offset, bytes, count, componentBytes, errorMeters: error });
    offset += bytes;
  }
  const payload = new Uint8Array(offset);
  let at = 0;
  for (const chunk of chunks) { payload.set(chunk, at); at += chunk.length; }
  return { record, payload };
}

if (workerData === "superspec-city-lod") {
  parentPort.on("message", async job => {
    try {
      const result = await simplifyPrimitive(job);
      parentPort.postMessage(result, [result.payload.buffer]);
    } catch (error) {
      parentPort.postMessage({ error });
    }
  });
}
