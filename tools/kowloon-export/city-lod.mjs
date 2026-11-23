import { writeImmutable } from "./immutable-file.mjs";
import { createHash, randomUUID } from "node:crypto";
import { on } from "node:events";
import { availableParallelism } from "node:os";
import { Worker } from "node:worker_threads";
import { readFile, rename, rm, writeFile } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { aggressiveFarLevels, detailLevels, simplifyPrimitive } from "./city-lod-worker.mjs";

/** Inventory retains each item's bulk. Hardware may prune subpixel fittings.
 * Both keep the original vertex colors, UVs, normals and material bindings. */
export function cityDetailPolicy(material) {
  const surface = material?.extras?.worldSpecSurface;
  const inventory = surface?.family === "Shop inventory";
  const hardware = surface?.recipe === "stock-hardware"
    || (surface?.family === "Building atlases" && surface?.surface === "Mixed surfaces with metal");
  if (!inventory && !hardware) return;
  return { levels: detailLevels, flags: inventory ? ["ErrorAbsolute", "Permissive"] : ["ErrorAbsolute", "Prune", "Permissive"] };
}

/** Optional skyline/detail tier for the theme's building hardware. Do not infer
 * eligibility from display names, or extend it to gameplay geometry, signs,
 * glazing, foliage or light emitters that must remain legible at a distance. */
export function cityFarPolicy(material) {
  const surface = material?.extras?.worldSpecSurface;
  if (surface?.family !== "Building atlases" || surface?.surface !== "Mixed surfaces with metal"
      || !["weathered-steel", "stock-hardware"].includes(surface?.recipe)) return;
  if ((material.alphaMode ?? "OPAQUE") !== "OPAQUE" || material.emissiveTexture
      || material.emissiveFactor?.some(value => value > 0)
      || material.extensions?.KHR_materials_emissive_strength) return;
  const finishes = material.extras?.sourceFinishes;
  if (material.extras?.cityDetailKeep || finishes !== undefined && (!Array.isArray(finishes)
      || finishes.some(finish => typeof finish !== "string"
        || /\b(?:sign|signage|lettering|poster|posters|tube-light|lens|emissive)\b/i.test(finish)))) return;
  return { levels: aggressiveFarLevels, flags: ["ErrorAbsolute", "Prune", "Permissive"] };
}
/** A material that holds nothing but signs, posters and lamps. The bake gives these finishes pages of
 * their own (chart-atlas.mjs), and a level's share of so small a primitive would come out of an emblem's
 * teeth or a letter's strokes, so it keeps every triangle at every distance. So does a page of roof tiles
 * (`cityDetailKeep` from the bake): the viewer picks a primitive's level by its largest connected piece, and a
 * primitive of nothing but tiles would be judged by one tile and pruned away while its roof is in plain view. */
export function keepsEveryTriangle(material) {
  const finishes = material?.extras?.sourceFinishes;
  return material?.extras?.cityDetailKeep === true || Array.isArray(finishes) && finishes.length > 0 && finishes.every(finish => typeof finish === "string"
    && /\b(?:sign|signage|lettering|poster|posters|tube-light|lens|emissive)\b/i.test(finish));
}
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const components = {
  5120: [1, "getInt8", 127], 5121: [1, "getUint8", 255],
  5122: [2, "getInt16", 32767], 5123: [2, "getUint16", 65535],
  5125: [4, "getUint32", 4294967295], 5126: [4, "getFloat32", 1],
};
const widths = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4 };
const attributeWeights = [["NORMAL", 0.5], ["TEXCOORD_0", 2], ["COLOR_0", 0.4], ["_MS_ASPECT", 0.1]];

async function replaceFile(destination, bytes, expectedCurrent) {
  const staging = destination + "." + randomUUID() + ".tmp";
  try {
    await writeFile(staging, bytes, { flag: "wx" });
    if (expectedCurrent && !(await readFile(destination)).equals(expectedCurrent))
      throw Error("The map changed during LOD generation. Re-run against the new export.");
    await rename(staging, destination);
  } finally {
    await rm(staging, { force: true });
  }
}

export function entryBytes(json) {
  const text = Buffer.from(JSON.stringify(json)), length = (text.length + 3) & ~3;
  const bytes = Buffer.alloc(20 + length, 0x20);
  bytes.write("glTF"); bytes.writeUInt32LE(2, 4); bytes.writeUInt32LE(bytes.length, 8);
  bytes.writeUInt32LE(length, 12); bytes.write("JSON", 16); text.copy(bytes, 20);
  return bytes;
}

/** Read only the referenced geometry chunks. Positions in the published map are interleaved
 * normalized integers; meshoptimizer must measure errors after dequantization into metres. */
export class GeometryReader {
  constructor(json, directory) { this.json = json; this.directory = directory; this.buffers = new Map(); }
  async read(index, Type = Float32Array) {
    const acc = this.json.accessors[index], view = this.json.bufferViews[acc.bufferView];
    if (!this.buffers.has(view.buffer)) {
      const source = this.json.buffers[view.buffer];
      // Concurrent primitives often share an interleaved input buffer.
      this.buffers.set(view.buffer, readFile(resolve(this.directory, source.uri)).then(bytes => {
        if (bytes.length !== source.byteLength) throw Error("Incomplete GLB buffer " + source.uri);
        return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      }).catch(error => {
        this.buffers.delete(view.buffer);
        throw error;
      }));
    }
    const bytes = await this.buffers.get(view.buffer), [size, method, divisor] = components[acc.componentType];
    const width = widths[acc.type], stride = view.byteStride ?? size * width;
    const base = (acc.byteOffset ?? 0) + (view.byteOffset ?? 0);
    const result = new Type(acc.count * width);
    for (let i = 0; i < acc.count; i++) for (let c = 0; c < width; c++) {
      const v = bytes[method](base + i * stride + c * size, true);
      result[i * width + c] = acc.normalized ? Math.max(-1, v / divisor) : v;
    }
    return result;
  }
}

/** Mark the vertices where two atlas charts meet: the same position carrying
 * different texture coordinates. A permissive collapse through such a vertex
 * stretches one chart's triangle over its neighbour's surface, and on a flat
 * surface nothing geometric resists it, so the texels smear into fans. The
 * marks are meshoptimizer's per-vertex Protect bit. */
export function chartSeamVertices(positions, uvs, reach = 0) {
  const protect = new Uint8Array(positions.length / 3), first = new Map();
  for (let vertex = 0; vertex < protect.length; vertex++) {
    const key = positions[vertex * 3] + "," + positions[vertex * 3 + 1] + "," + positions[vertex * 3 + 2];
    const other = first.get(key);
    if (other === undefined) first.set(key, vertex);
    else if (Math.abs(uvs[other * 2] - uvs[vertex * 2]) > reach || Math.abs(uvs[other * 2 + 1] - uvs[vertex * 2 + 1]) > reach)
      protect[other] = protect[vertex] = 2;
  }
  return protect;
}

async function primitiveInputs(reader, part, scale, policy, farPolicy, seams) {
  const indices = await reader.read(part.indices, Uint32Array);
  const positions = await reader.read(part.attributes.POSITION), vertexCount = positions.length / 3;
  const permissive = [policy, farPolicy].some(plan => plan?.flags.includes("Permissive"));
  const protect = seams !== "permissive" && permissive && part.attributes.TEXCOORD_0 !== undefined
    ? chartSeamVertices(positions, await reader.read(part.attributes.TEXCOORD_0), Number(process.env.SUPERSPEC_CITY_LOD_SEAM_REACH ?? 0)) : undefined;
  for (let i = 0; i < positions.length; i++) positions[i] *= scale;
  const attributes = [], weights = [];
  for (const [name, weight] of attributeWeights) {
    if (part.attributes[name] === undefined) continue;
    const values = await reader.read(part.attributes[name]), width = values.length / vertexCount;
    attributes.push({ values, width }); weights.push(...Array(width).fill(weight));
  }
  const fields = new Float32Array(vertexCount * weights.length);
  let lane = 0;
  for (const { values, width } of attributes) {
    for (let i = 0; i < vertexCount; i++) for (let c = 0; c < width; c++)
      fields[i * weights.length + lane + c] = values[i * width + c];
    lane += width;
  }
  return { indices, positions, fields, weights, policy, farPolicy, protect, protectError: seams === "near" ? Number(process.env.SUPERSPEC_CITY_LOD_SEAM_ERROR ?? 0.18) : Infinity };
}

async function simplifyCity(jobs, reader, workers, seams) {
  // Largest jobs first avoids a long single-worker tail. Results stay in source order,
  // which determines both manifest and payload bytes.
  const queue = jobs.map((job, index) => ({ job, index })).sort((a, b) => b.job.work - a.job.work);
  const triangles = jobs.reduce((n, job) => n + job.count / 3, 0);
  const lanes = Math.min(workers, jobs.length, Math.max(1, Math.floor(triangles / 50000)));
  const results = new Array(jobs.length);
  let cursor = 0, failure;
  async function run() {
    let worker, replies, current;
    try {
      if (lanes > 1) {
        // This standalone .mjs entry needs no caller-side loaders, stdin or eval flags.
        worker = new Worker(new URL("./city-lod-worker.mjs", import.meta.url), {
          workerData: "superspec-city-lod", execArgv: [],
        });
        replies = on(worker, "message", { close: ["exit"] });
      }
      while (!failure && cursor < queue.length) {
        const { job, index } = queue[cursor++];
        current = job;
        const inputs = await primitiveInputs(reader, job.part, job.scale, job.policy, job.farPolicy, seams);
        if (!worker) {
          results[index] = await simplifyPrimitive(inputs);
          continue;
        }
        // Transfer one primitive once for all levels. Each input owns its buffer.
        worker.postMessage(inputs, [inputs.indices.buffer, inputs.positions.buffer, inputs.fields.buffer,
          ...(inputs.protect ? [inputs.protect.buffer] : [])]);
        const reply = await replies.next();
        if (reply.done) throw Error("City LOD worker exited before returning its primitive");
        const [result] = reply.value;
        if (result.error) throw result.error;
        results[index] = result;
      }
    } catch (error) {
      failure ??= current ? new Error(`City LOD failed for mesh ${current.meshIndex}, primitive ${current.primitive}`, { cause: error }) : error;
    } finally {
      await replies?.return();
      if (worker) await worker.terminate();
    }
  }
  await Promise.all(Array.from({ length: lanes }, run));
  if (failure) throw failure;
  return results;
}

export async function attachCityLod(destination, sourceEntry, {
  workers = Number(process.env.SUPERSPEC_CITY_LOD_WORKERS ?? Math.max(1, Math.min(4, availableParallelism() - 1))),
  farProfile = process.env.SUPERSPEC_CITY_LOD_FAR ?? "standard",
  // "protect" keeps every atlas chart's texels on its own surface in the detail and far
  // chains. Reduced building renditions need it: their flat tiled surfaces are many
  // coplanar charts. "permissive" is the long-standing behaviour, with fewer triangles.
  // "near" protects them in the levels a viewer meets up close (within 18 cm of the surface)
  // and lets the farther ones collapse through: parts keep their own texels where that shows.
  seams = process.env.SUPERSPEC_CITY_LOD_SEAMS ?? "permissive",
} = {}) {
  if (!Number.isSafeInteger(workers) || workers < 1 || workers > 4) throw RangeError("City LOD workers must be between 1 and 4.");
  if (!["standard", "aggressive"].includes(farProfile)) throw RangeError("City LOD far profile must be standard or aggressive.");
  if (!["permissive", "protect", "near"].includes(seams)) throw RangeError("City LOD seams must be permissive, protect or near.");
  const json = JSON.parse(sourceEntry.toString("utf8", 20, 20 + sourceEntry.readUInt32LE(12)));
  const scene = json.scenes[json.scene ?? 0], directory = dirname(destination);
  delete scene.extras.cityLod;
  const manifest = { sourceSha256: digest(entryBytes(json)), primitives: [] };
  const reader = new GeometryReader(json, directory), payload = [];
  const levelCount = detailLevels.length + (farProfile === "aggressive" ? aggressiveFarLevels.length : 0);
  const totals = Array(levelCount + 1).fill(0);
  const jobs = [], placements = [], jobIndices = new Map();
  for (const [meshIndex, mesh] of json.meshes.entries()) {
    // Native authored chains are already complete and intentionally hierarchical.
    // Keep their alternatives out of this root-placement index-stream optimizer.
    if (mesh.extras?.worldSpecDeclaredLod) continue;
    // The arena retains full detail including its playable sides, ramps, stairs and cover.
    if (!/^building-\d+$/.test(mesh.name) && !mesh.name?.startsWith("city/prototype/")) continue;
    if (mesh.name.endsWith("/lettering")) continue;
    let scale = 0;
    for (const nodeIndex of scene.nodes) {
      const node = json.nodes[nodeIndex];
      if (node.mesh !== meshIndex) continue;
      const instance = node.extensions?.EXT_mesh_gpu_instancing?.attributes.SCALE;
      const scaleValues = instance === undefined ? [1] : await reader.read(instance);
      let placementScale = 0;
      for (const factor of scaleValues) placementScale = Math.max(placementScale, Math.abs(factor));
      // Published nodes use TRS. A glTF matrix also represents decomposable TRS,
      // whose column lengths give the absolute scale along its three axes.
      const m = node.matrix;
      const nodeScale = m ? Math.max(Math.hypot(m[0], m[1], m[2]), Math.hypot(m[4], m[5], m[6]),
        Math.hypot(m[8], m[9], m[10])) : Math.max(...(node.scale ?? [1]).map(Math.abs));
      scale = Math.max(scale, nodeScale * placementScale);
    }
    if (!(scale > 0)) throw Error("Building mesh lacks a root placement: " + meshIndex);
    for (const [primitive, part] of mesh.primitives.entries()) {
      // Each turf vertex establishes the surface sampled by the blade roots.
      // Independent building LOD simplification would move their receiving surface.
      if (json.materials[part.material]?.extras?.grassField || keepsEveryTriangle(json.materials[part.material])) continue;
      if ((part.mode ?? 4) !== 4 || part.indices === undefined) continue;
      const count = json.accessors[part.indices].count;
      if (count < 120) continue;
      const policy = cityDetailPolicy(json.materials[part.material]);
      const farPolicy = farProfile === "aggressive" ? cityFarPolicy(json.materials[part.material]) : undefined;
      // Distinct placements/material bindings can reference the same immutable glTF accessors.
      const key = JSON.stringify([part.indices, part.attributes.POSITION,
        ...attributeWeights.map(([name]) => part.attributes[name]), scale, policy, farPolicy]);
      if (!jobIndices.has(key)) {
        jobIndices.set(key, jobs.length);
        jobs.push({ meshIndex, primitive, part, count, scale, policy, farPolicy,
          work: count * (2 + (policy?.levels.length ?? 1) + (farPolicy?.levels.length ?? 0)) });
      }
      placements.push({ meshIndex, primitive, job: jobIndices.get(key) });
    }
  }
  const results = await simplifyCity(jobs, reader, workers, seams);
  let offset = 0;
  for (const { meshIndex, primitive, job } of placements) {
    const { record: relative, payload: bytes } = results[job];
    const place = level => ({ ...level, offset: level.offset + offset });
    const record = { ...relative, levels: relative.levels.map(place) };
    if (record.detailLevels) record.detailLevels = relative.detailLevels.map(place);
    else record.distantLevels = relative.distantLevels.map(place);
    payload.push(bytes);
    offset += bytes.length;
    const selected = record.detailLevels ?? [...record.levels, ...record.distantLevels];
    totals[0] += record.nearCount;
    for (let i = 0; i < levelCount; i++) totals[i + 1] += selected[Math.min(i, selected.length - 1)].count;
    if (selected.some(lod => lod.count < record.nearCount)) manifest.primitives.push({ mesh: meshIndex, primitive, ...record });
  }
  const text = Buffer.from(JSON.stringify(manifest)), header = Buffer.alloc(12 + ((text.length + 3) & ~3));
  header.write("SLOD"); header.writeUInt32LE(1, 4); header.writeUInt32LE(text.length, 8); text.copy(header, 12);
  const sidecar = Buffer.concat([header, ...payload]);
  const uri = basename(destination, ".glb") + "-data/lod-" + digest(sidecar).slice(0, 24) + ".bin";
  await writeImmutable(resolve(directory, uri), sidecar);
  scene.extras.cityLod = uri;
  console.log("City LOD", uri, sidecar.length, "bytes; prototype triangles by detail level", totals.map(n => n/3));
  return entryBytes(json);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (!process.argv[2] || process.argv.slice(3).some(arg => !/^--(far-profile=(standard|aggressive)|seams=(permissive|protect|near))$/.test(arg)))
    throw Error("Usage: node city-lod.mjs MAP.glb [--far-profile=standard|aggressive] [--seams=permissive|protect|near]");
  const destination = resolve(process.argv[2]);
  const source = await readFile(destination);
  const option = name => process.argv.slice(3).find(arg => arg.startsWith(`--${name}=`))?.split("=")[1];
  await replaceFile(destination, await attachCityLod(destination, source, { farProfile: option("far-profile"), seams: option("seams") }), source);
}
