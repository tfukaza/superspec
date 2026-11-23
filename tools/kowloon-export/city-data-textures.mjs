import { copyImmutable, writeImmutable } from "./immutable-file.mjs";
import { createHash, randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdir, open, readFile, readdir, realpath, rename, rm, writeFile } from "node:fs/promises";
import { basename, dirname, extname, resolve, relative, isAbsolute } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import sharp from "sharp";

export const DATA_RECIPE = "superspec-city-data-uastc-q75-e2-clamp-v1\0";
const BASIS = "KHR_texture_basisu";
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
export const dataTextureKey = bytes => createHash("sha256").update(DATA_RECIPE).update(bytes).digest("hex");
const ktxMagic = Buffer.from([0xab, 0x4b, 0x54, 0x58, 0x20, 0x32, 0x30, 0xbb, 13, 10, 26, 10]);

export function readCityEntry(bytes) {
  if (bytes.length < 20 || bytes.toString("ascii", 0, 4) !== "glTF" || bytes.readUInt32LE(4) !== 2
    || bytes.readUInt32LE(8) !== bytes.length || bytes.toString("ascii", 16, 20) !== "JSON"
    || 20 + bytes.readUInt32LE(12) > bytes.length) throw Error("Expected a complete glTF 2 binary entry.");
  const end = 20 + bytes.readUInt32LE(12);
  return { json: JSON.parse(bytes.toString("utf8", 20, end)), tail: bytes.subarray(end) };
}

export function entryBytes(json, tail) {
  const text = Buffer.from(JSON.stringify(json)), padded = (text.length + 3) & ~3;
  const bytes = Buffer.alloc(20 + padded, 0x20);
  bytes.write("glTF"); bytes.writeUInt32LE(2, 4); bytes.writeUInt32LE(bytes.length + tail.length, 8);
  bytes.writeUInt32LE(padded, 12); bytes.write("JSON", 16); text.copy(bytes, 20);
  return Buffer.concat([bytes, tail]);
}

export async function verifiedLod(json, tail, directory) {
  const scene = json.scenes?.[json.scene ?? 0], uri = scene?.extras?.cityLod;
  if (!uri) return;
  // The city LOD producer publishes external geometry buffers and hashes its
  // canonical JSON-only entry. Embedded-BIN LOD provenance is not this format.
  if (tail.length) throw Error("City LOD provenance requires a JSON-only published entry.");
  const original = await readFile(localPath(directory, uri));
  if (original.length < 12 || original.toString("ascii", 0, 4) !== "SLOD" || original.readUInt32LE(4) !== 1)
    throw Error(`Invalid LOD sidecar: ${uri}`);
  const length = original.readUInt32LE(8), payloadAt = 12 + Math.ceil(length / 4) * 4;
  if (payloadAt > original.length) throw Error(`Truncated LOD sidecar: ${uri}`);
  const manifest = JSON.parse(original.toString("utf8", 12, 12 + length));
  const source = structuredClone(json);
  delete source.scenes[source.scene ?? 0].extras.cityLod;
  if (manifest.sourceSha256 !== hash(entryBytes(source, Buffer.alloc(0)))) throw Error(`LOD source provenance does not match the entry: ${uri}`);
  return { manifest, payload: original.subarray(payloadAt) };
}

export async function retargetLod(json, lod, output, folder) {
  if (!lod) return;
  const scene = json.scenes[json.scene ?? 0], { manifest, payload } = lod;
  delete scene.extras.cityLod;
  manifest.sourceSha256 = hash(entryBytes(json, Buffer.alloc(0)));
  const text = Buffer.from(JSON.stringify(manifest)), header = Buffer.alloc(12 + Math.ceil(text.length / 4) * 4);
  header.write("SLOD"); header.writeUInt32LE(1, 4); header.writeUInt32LE(text.length, 8); text.copy(header, 12);
  const bytes = Buffer.concat([header, payload]);
  const next = folder + "/lod-" + hash(bytes).slice(0, 24) + ".bin";
  const destination = localPath(output, next);
  await writeImmutable(destination, bytes);
  scene.extras.cityLod = next;
}

/** Unknown extra/extension integers conservatively count as potential texture or
 * image aliases. Only the standard metallicRoughnessTexture slot is admitted. */
export function exclusiveDataTextures(json) {
  const allowed = new Set(), forbidden = new Set(), opaqueReferences = new Set();
  const walk = (value, path = [], opaque = false) => {
    if (opaque && Number.isInteger(value)) opaqueReferences.add(value);
    if (!value || typeof value !== "object") return;
    for (const [key, child] of Object.entries(value)) {
      const next = [...path, key];
      if (key === "index" && Number.isInteger(child)) {
        const mr = path.length === 4 && path[0] === "materials" && path[2] === "pbrMetallicRoughness"
          && path[3] === "metallicRoughnessTexture";
        (mr ? allowed : forbidden).add(child);
      }
      walk(child, next, opaque || key === "extras" || key === "extensions");
    }
  };
  walk(json);
  const eligible = new Set([...allowed].filter(index => !forbidden.has(index) && !opaqueReferences.has(index)
    && json.textures?.[index] && !json.textures[index].extensions));
  const images = new Map();
  for (const [index, texture] of (json.textures ?? []).entries()) {
    if (!Number.isInteger(texture.source)) continue;
    if (!images.has(texture.source)) images.set(texture.source, []);
    images.get(texture.source).push(index);
  }
  return new Map([...images].filter(([image, textures]) => !opaqueReferences.has(image)
    && !json.images?.[image]?.extensions && textures.every(index => eligible.has(index))));
}

export function localPath(root, uri) {
  if (typeof uri !== "string" || /^[a-z][a-z\d+.-]*:/i.test(uri) || /[\\?#]/.test(uri)) throw Error(`Expected a local companion URI: ${uri}`);
  const path = resolve(root, decodeURIComponent(uri));
  const child = relative(root, path);
  if (!child || child === ".." || child.startsWith("..\\") || child.startsWith("../") || isAbsolute(child))
    throw Error(`Companion URI escapes its entry directory: ${uri}`);
  return path;
}

/** glTF resources and file references in the published WorldSpec records. */
export function cityCompanionUris(json) {
  const uris = new Set();
  const collect = (value, key = "") => {
    // Embedded textures may use their original source path as a display name.
    if (typeof value === "string" && key !== "name" && (key === "uri" || key === "cityLod" || value.includes("/"))
      && !/^[a-z][a-z\d+.-]*:/i.test(value) && /\.(?:bin|png|jpe?g|webp|ktx2|json|glb)$/i.test(value)) uris.add(value);
    else if (value && typeof value === "object") for (const [key, child] of Object.entries(value)) collect(child, key);
  };
  collect(json);
  return uris;
}

/** Record the companions a published entry references, and remove the ones an
 * earlier publication of the same entry recorded that nothing references now.
 * Files without such a record, or recorded for a sibling entry, are never removed. */
export async function settlePublishedFiles(destination, json) {
  const output = dirname(destination), name = basename(destination), suffix = ".files.json";
  const files = [...cityCompanionUris(json)].sort(), keep = new Set(files), removed = [];
  const recorded = async path => {
    try {
      const record = JSON.parse(await readFile(path, "utf8"));
      return record?.version === 1 && Array.isArray(record.files) ? record.files.filter(uri => typeof uri === "string") : [];
    } catch (error) { if (error.code === "ENOENT" || error instanceof SyntaxError) return []; throw error; }
  };
  const previous = await recorded(destination + suffix);
  if (previous.some(uri => !keep.has(uri)))
    for (const sibling of await readdir(output))
      if (sibling.endsWith(".glb" + suffix) && sibling !== name + suffix) for (const uri of await recorded(resolve(output, sibling))) keep.add(uri);
  for (const uri of previous) {
    if (keep.has(uri)) continue;
    await rm(localPath(output, uri), { force: true });
    removed.push(uri);
  }
  await writeFile(destination + suffix, JSON.stringify({ version: 1, entry: name, files }));
  return removed;
}

export async function imageBytes(json, tail, directory, image) {
  if (image.uri) {
    if (image.uri.startsWith("data:")) return Buffer.from(image.uri.split(",")[1], "base64");
    return readFile(localPath(directory, image.uri));
  }
  const view = json.bufferViews?.[image.bufferView], buffer = json.buffers?.[view?.buffer];
  if (!view || !buffer) throw Error("Image does not reference a valid buffer view.");
  const offset = view.byteOffset ?? 0;
  if (!Number.isSafeInteger(offset) || offset < 0 || !Number.isSafeInteger(view.byteLength) || view.byteLength < 1
    || !Number.isSafeInteger(buffer.byteLength) || offset + view.byteLength > buffer.byteLength)
    throw Error("Image buffer view exceeds its declared buffer bounds.");
  if (!buffer.uri) {
    if (view.buffer !== 0 || tail.length < 8 || tail.readUInt32LE(4) !== 0x004e4942) throw Error("Image references a missing BIN chunk.");
    if (tail.readUInt32LE(0) > tail.length - 8 || offset + view.byteLength > tail.readUInt32LE(0))
      throw Error("Image buffer view exceeds its BIN chunk.");
    return tail.subarray(8 + offset, 8 + offset + view.byteLength);
  }
  const file = await open(localPath(directory, buffer.uri), "r");
  try {
    if ((await file.stat()).size < buffer.byteLength) throw Error(`Incomplete image buffer: ${buffer.uri}`);
    const bytes = Buffer.alloc(view.byteLength);
    const { bytesRead } = await file.read(bytes, 0, bytes.length, offset);
    if (bytesRead !== bytes.length) throw Error(`Incomplete image buffer: ${buffer.uri}`);
    return bytes;
  } finally { await file.close(); }
}

async function encodeNative(manifest, cli) {
  const command = resolve(cli ?? process.env.SUPERSPEC_MATERIALS ?? "target/release/superspec-materials" + (process.platform === "win32" ? ".exe" : ""));
  await new Promise((done, reject) => {
    const child = spawn(command, ["encode-city-data-textures", manifest], { windowsHide: true, stdio: ["ignore", "inherit", "inherit"] });
    child.once("error", reject);
    child.once("exit", code => code === 0 ? done() : reject(Error(`Data texture encoder exited with status ${code}.`)));
  });
}

export async function realDestination(path) {
  try { return await realpath(path); }
  catch (error) {
    if (error.code !== "ENOENT") throw error;
    return resolve(await realDestination(dirname(path)), basename(path));
  }
}

/** Opt-in derived pack. Every existing record/index and companion remains intact;
 * compressed images are appended only after every selected job succeeds. */
export async function stageCityDataTextures({ source, destination, cacheDirectory = "generated/city-data-texture-cache", cli, workers = 4, encode = encodeNative }) {
  source = await realpath(resolve(source)); destination = resolve(destination);
  const directory = dirname(source), output = await realDestination(dirname(destination)), cache = resolve(cacheDirectory);
  if (!relative(directory, output)) throw Error("Stage into a separate directory; the source pack must remain unchanged.");
  destination = resolve(output, basename(destination));
  const occupied = await realpath(destination).catch(error => { if (error.code !== "ENOENT") throw error; });
  if (occupied && !relative(source, occupied)) throw Error("Staged entry must not alias the source entry.");
  if (!Number.isInteger(workers) || workers < 1 || workers > 6) throw Error("workers must be an integer from 1 to 6.");
  const started = performance.now(), original = await readFile(source), { json, tail } = readCityEntry(original);
  const lod = await verifiedLod(json, tail, directory);
  const candidates = exclusiveDataTextures(json), records = [], nonDataHashes = new Set(), identities = new Map();
  // Content aliases can exist under different image/texture indices or files.
  for (const [index, image] of (json.images ?? []).entries()) {
    const bytes = await imageBytes(json, tail, directory, image), digest = hash(bytes);
    identities.set(index, digest);
    if (!candidates.has(index)) nonDataHashes.add(digest);
  }
  await mkdir(cache, { recursive: true });
  const temporary = resolve(cache, ".jobs-" + randomUUID());
  await mkdir(temporary);
  try {
    const jobs = new Map();
    for (const [index, textures] of candidates) {
      if (nonDataHashes.has(identities.get(index))) continue;
      const image = json.images[index];
      if (!["image/png", "image/jpeg"].includes(image.mimeType ?? ({ ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg" }[extname(image.uri ?? "")]))) continue;
      const bytes = await imageBytes(json, tail, directory, image), { width, height } = await sharp(bytes).metadata();
      if (!(width * height >= 1024 * 1024) || width % 4 || height % 4) continue;
      const key = dataTextureKey(bytes), target = resolve(cache, key + ".ktx2");
      if (!jobs.has(key)) {
        const input = resolve(temporary, key + ".source");
        await writeFile(input, bytes);
        jobs.set(key, { input, output: target, key });
      }
      records.push({ image: index, textures, key, width, height });
    }
    const manifest = resolve(temporary, "jobs.json");
    await writeFile(manifest, JSON.stringify({ version: 1, workers, jobs: [...jobs.values()] }));
    const encodeStarted = performance.now();
    if (jobs.size) await encode(manifest, cli);
    const encodeMs = performance.now() - encodeStarted;
    const folder = basename(destination, ".glb") + "-data", appended = new Map();
    let encodedBytes = 0;
    for (const record of records) {
      const bytes = await readFile(jobs.get(record.key).output);
      if (bytes.length < 80 || !bytes.subarray(0, 12).equals(ktxMagic) || bytes.readUInt32LE(20) !== record.width
        || bytes.readUInt32LE(24) !== record.height || !bytes.readUInt32LE(40)) throw Error(`Invalid encoded KTX2: ${record.key}`);
      record.artifactSha256 = hash(bytes);
      if (!appended.has(record.artifactSha256)) {
        const uri = folder + "/data-" + record.artifactSha256 + ".ktx2";
        await copyImmutable(jobs.get(record.key).output, localPath(output, uri));
        appended.set(record.artifactSha256, json.images.push({ mimeType: "image/ktx2", uri }) - 1);
        encodedBytes += bytes.length;
      }
      for (const texture of record.textures) json.textures[texture].extensions = { [BASIS]: { source: appended.get(record.artifactSha256) } };
    }
    if (records.length) json.extensionsUsed = [...new Set([...(json.extensionsUsed ?? []), BASIS])];
    if (records.length) await retargetLod(json, lod, output, folder);
    // Buffer/image URIs and sidecars such as cityLod are the complete published dependencies.
    for (const uri of cityCompanionUris(readCityEntry(original).json))
      await copyImmutable(localPath(directory, uri), localPath(output, uri));
    await mkdir(output, { recursive: true });
    const pending = destination + "." + randomUUID() + ".tmp";
    try {
      await writeFile(pending, records.length ? entryBytes(json, tail) : original, { flag: "wx" });
      await rename(pending, destination);
    } finally { await rm(pending, { force: true }); }
    const texels = records.reduce((sum, row) => sum + row.width * row.height, 0);
    return { source, destination, recipe: DATA_RECIPE.slice(0, -1), convertedImages: records.length, uniqueJobs: jobs.size, encodedBytes,
      estimatedRgbaMipBytes: Math.round(texels * 4 * 4 / 3), estimatedBc7MipBytes: Math.round(texels * 4 / 3),
      encodeMs, elapsedMs: performance.now() - started, records };
  } finally { await rm(temporary, { recursive: true, force: true }); }
}

async function main() {
  const { values, positionals } = parseArgs({ allowPositionals: true, options: {
    out: { type: "string" }, cache: { type: "string" }, cli: { type: "string" }, workers: { type: "string" }, help: { type: "boolean" },
    "gpu-textures": { type: "string" }, "color-codec": { type: "string" }, psnr: { type: "boolean" }, streaming: { type: "string" },
  } });
  if (values.help) { console.log("Usage: node tools/kowloon-export/city-data-textures.mjs ENTRY.glb --out STAGED.glb [--cache DIR --cli superspec-materials --workers 4]\nOpt-in UASTC metallic/roughness atlases; preserves original PNG/JPEG fallback and all other maps. Source and output directories must differ.\n\n  --gpu-textures medium|low|very-low  Publish every texture as KTX2 (KHR_texture_basisu, required) with that tier's map content.\n  --color-codec etc1s|uastc           Colour pages with --gpu-textures (etc1s). --psnr measures each encoded texture.\n  --streaming off|policy|all          Quarter-edge distant companions: none, the Medium texture LOD policy, or every surface family."); return; }
  if (positionals.length !== 1 || !values.out) throw Error("Supply ENTRY.glb and --out STAGED.glb. Use --help for usage.");
  if (values["gpu-textures"]) {
    const { gpuTextureTiers, stageCityGpuTextures } = await import("./city-gpu-textures.mjs");
    const tier = gpuTextureTiers[values["gpu-textures"]], streaming = { off: false, policy: true, all: "all" }[values.streaming ?? ""];
    if (!tier || values.streaming !== undefined && streaming === undefined) throw Error("--gpu-textures takes medium, low or very-low and --streaming takes off, policy or all.");
    const report = await stageCityGpuTextures({ source: positionals[0], destination: values.out, tier: values["gpu-textures"],
      maps: values.streaming === undefined ? tier : { ...tier, streaming },
      colorCodec: values["color-codec"], cacheDirectory: values.cache, workers: values.workers && Number(values.workers), measure: values.psnr });
    await writeFile(resolve(dirname(report.destination), "city-gpu-textures.json"), JSON.stringify(report, null, 2));
    const { records, ...summary } = report;
    console.log(JSON.stringify(summary, null, 2));
    return;
  }
  const report = await stageCityDataTextures({ source: positionals[0], destination: values.out, cacheDirectory: values.cache, cli: values.cli, workers: Number(values.workers ?? 4) });
  await writeFile(resolve(dirname(report.destination), "city-data-textures.json"), JSON.stringify(report, null, 2));
  const { records, ...summary } = report;
  console.log(JSON.stringify(summary));
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href)
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
