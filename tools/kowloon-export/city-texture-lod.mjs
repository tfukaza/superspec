import { copyImmutable, writeImmutable } from "./immutable-file.mjs";
import { createHash, randomUUID } from "node:crypto";
import { mkdir, readFile, realpath, rename, rm, writeFile } from "node:fs/promises";
import { basename, dirname, extname, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import sharp from "sharp";
import { cityCompanionUris, entryBytes, imageBytes, localPath, readCityEntry, realDestination,
  retargetLod, verifiedLod } from "./city-data-textures.mjs";

export const TEXTURE_LOD_RECIPE = `superspec-city-mip2-lanczos3-linear-colour-v1-sharp${sharp.versions.sharp}-vips${sharp.versions.vips}`;
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
export const textureLodKey = (bytes, space) => createHash("sha256").update(TEXTURE_LOD_RECIPE + "\0" + space + "\0").update(bytes).digest("hex");

function materialSlots(material) {
  const surface = material.extras?.worldSpecSurface, finishes = material.extras?.sourceFinishes;
  if ((material.alphaMode ?? "OPAQUE") === "MASK" || material.alphaCutoff !== undefined
    || material.emissiveTexture || material.emissiveFactor?.some(value => value > 0)
    || material.extensions?.KHR_materials_emissive_strength || material.extras?.cityDetailKeep
    || finishes !== undefined && (!Array.isArray(finishes) || finishes.some(value => typeof value !== "string"
      || /\b(?:sign|signage|lettering|poster|posters|tube-light|lens|emissive)\b/i.test(value)))) return [];
  if (!surface || /\b(?:sign|signage|lettering|poster|posters|tube-light|lens|emissive)\b/i.test(surface.recipe ?? "")) return [];
  const detail = surface.family === "Shop inventory"
    || surface.family === "Building atlases" && ["Mixed surfaces with metal", "Transparent surfaces"].includes(surface.surface);
  if (detail) return ["baseColorTexture", "normalTexture", "metallicRoughnessTexture"];
  if (surface.family === "Building atlases" || surface.family === "Masonry" && surface.recipe === "architectural-concrete")
    return ["metallicRoughnessTexture"];
  return [];
}

/** A texture and its image must be exclusive to supported material slots. Any
 * unknown index/extras alias, AO/edge/emission use or shared protected material
 * disqualifies it. Color and data uses cannot share one generated image. */
export function exclusiveTextureLods(json) {
  const uses = new Map(), blocked = new Set(), opaque = new Set();
  const policies = (json.materials ?? []).map(materialSlots);
  const walk = (value, path = [], unknown = false) => {
    if (unknown && Number.isInteger(value)) opaque.add(value);
    if (!value || typeof value !== "object") return;
    for (const [key, child] of Object.entries(value)) {
      if (key === "index" && Number.isInteger(child)) {
        const role = path.at(-1), material = Number(path[1]);
        const standard = path[0] === "materials" && (
          path.length === 4 && path[2] === "pbrMetallicRoughness" && ["baseColorTexture", "metallicRoughnessTexture"].includes(role)
          || path.length === 3 && role === "normalTexture");
        if (!standard || !policies[material]?.includes(role)) blocked.add(child);
        else {
          const spaces = uses.get(child) ?? new Set(); spaces.add(role === "baseColorTexture" ? "srgb" : "linear"); uses.set(child, spaces);
        }
      }
      walk(child, [...path, key], unknown || key === "extras" || key === "extensions");
    }
  };
  walk(json);
  const images = new Map();
  for (const [index, texture] of (json.textures ?? []).entries()) {
    if (!Number.isInteger(texture.source)) continue;
    const group = images.get(texture.source) ?? { textures: [], spaces: new Set(), blocked: false };
    const spaces = uses.get(index);
    group.textures.push(index);
    if (blocked.has(index) || opaque.has(index) || !spaces || spaces.size !== 1 || texture.extensions) group.blocked = true;
    else for (const space of spaces) group.spaces.add(space);
    images.set(texture.source, group);
  }
  return new Map([...images].filter(([index, group]) => !group.blocked && group.spaces.size === 1
    && !opaque.has(index) && !json.images?.[index]?.extensions)
    .map(([index, group]) => [index, { textures: group.textures, space: [...group.spaces][0] }]));
}

export async function resizeTextureLod(bytes, { width, height, space }) {
  let image = sharp(bytes, { ignoreIcc: true });
  // glTF pigment is sRGB; filtering happens in linear light. Data channels are
  // already linear numbers. Alpha is retained and premultiplied by libvips while resizing.
  if (space === "srgb") image = image.pipelineColourspace("scrgb").toColourspace("srgb");
  return image.resize({ width, height, fit: "fill", kernel: "lanczos3", fastShrinkOnLoad: false })
    .png({ compressionLevel: 6, adaptiveFiltering: false }).toBuffer();
}

async function cachedImage(job, cache, resize, readSource) {
  const imagePath = resolve(cache, job.key + ".png"), receiptPath = imagePath + ".json";
  try {
    const receipt = JSON.parse(await readFile(receiptPath, "utf8")), bytes = await readFile(imagePath);
    const metadata = await sharp(bytes).metadata();
    if (receipt.key !== job.key || receipt.sourceSha256 !== job.sourceSha256 || receipt.recipe !== TEXTURE_LOD_RECIPE
      || receipt.space !== job.space || receipt.width !== job.width || receipt.height !== job.height
      || receipt.sourceWidth !== job.sourceWidth || receipt.sourceHeight !== job.sourceHeight
      || receipt.artifactSha256 !== hash(bytes) || receipt.byteLength !== bytes.length
      || metadata.width !== job.width || metadata.height !== job.height) throw Error("Invalid texture LOD cache receipt: " + job.key);
    return { ...receipt, imagePath, reused: true };
  } catch (error) { if (error.code !== "ENOENT") throw error; }
  const source = await readSource(job.image);
  if (hash(source) !== job.sourceSha256 || textureLodKey(source, job.space) !== job.key)
    throw Error("Source image changed during texture LOD publication: " + job.key);
  const bytes = await resize(source, job), metadata = await sharp(bytes).metadata();
  if (metadata.width !== job.width || metadata.height !== job.height || metadata.format !== "png")
    throw Error("Texture LOD encoder returned incompatible dimensions or format: " + job.key);
  const receipt = { key: job.key, recipe: TEXTURE_LOD_RECIPE, sourceSha256: job.sourceSha256, space: job.space,
    sourceWidth: job.sourceWidth, sourceHeight: job.sourceHeight, width: job.width, height: job.height,
    artifactSha256: hash(bytes), byteLength: bytes.length };
  await writeImmutable(imagePath, bytes); await writeImmutable(receiptPath, Buffer.from(JSON.stringify(receipt)));
  return { ...receipt, imagePath, reused: false };
}

/** Publish optional quarter-size companions without changing any existing glTF
 * texture/material binding. A matching adapter chooses near/far texture indices.
 * All outputs are staged in a separate directory and the entry is published last. */
export async function stageCityTextureLods({ source, destination, cacheDirectory = "generated/city-texture-lod-cache", workers = 2,
  resize = resizeTextureLod }) {
  source = await realpath(resolve(source)); destination = resolve(destination);
  const directory = dirname(source), output = await realDestination(dirname(destination)), cache = resolve(cacheDirectory);
  if (!relative(directory, output)) throw Error("Stage into a separate directory; the source pack must remain unchanged.");
  destination = resolve(output, basename(destination));
  const occupied = await realpath(destination).catch(error => { if (error.code !== "ENOENT") throw error; });
  if (occupied && !relative(source, occupied)) throw Error("Staged entry must not alias the source entry.");
  const original = await readFile(source);
  const { entry, report } = await attachCityTextureLods(destination, original, { directory, cacheDirectory: cache, workers, resize });
  for (const uri of cityCompanionUris(readCityEntry(original).json))
    await copyImmutable(localPath(directory, uri), localPath(output, uri));
  await mkdir(output, { recursive: true });
  const pending = destination + "." + randomUUID() + ".tmp";
  try {
    await writeFile(pending, entry, { flag: "wx" });
    if (!(await readFile(source)).equals(original)) throw Error("Source entry changed during texture LOD publication.");
    await rename(pending, destination);
  } finally { await rm(pending, { force: true }); }
  return { source, destination, ...report };
}

/** Attach distant images to the pending pack. The caller publishes the returned entry last. */
export async function attachCityTextureLods(destination, original, { directory = dirname(destination),
  cacheDirectory = "generated/city-texture-lod-cache", workers = 2, resize = resizeTextureLod } = {}) {
  const output = dirname(destination), cache = resolve(cacheDirectory);
  if (!Number.isInteger(workers) || workers < 1 || workers > 4) throw Error("workers must be an integer from 1 to 4.");
  const started = performance.now(), { json, tail } = readCityEntry(original);
  const scene = json.scenes?.[json.scene ?? 0];
  if (!scene) throw Error("Texture LOD publication requires a default scene.");
  if (scene.extras?.worldSpecTextureLod) throw Error("Entry already has texture LOD companions; stage from its original publication.");
  const lod = await verifiedLod(json, tail, directory), candidates = exclusiveTextureLods(json);
  const identities = new Map(), forbiddenHashes = new Set(), hashSpaces = new Map();
  for (const [index, image] of (json.images ?? []).entries()) {
    const bytes = await imageBytes(json, tail, directory, image), digest = hash(bytes);
    identities.set(index, digest);
    if (!candidates.has(index)) forbiddenHashes.add(digest);
    else { const spaces = hashSpaces.get(digest) ?? new Set(); spaces.add(candidates.get(index).space); hashSpaces.set(digest, spaces); }
  }
  const records = [], jobs = new Map();
  for (const [index, { textures, space }] of candidates) {
    const sourceSha256 = identities.get(index);
    if (forbiddenHashes.has(sourceSha256) || hashSpaces.get(sourceSha256).size > 1) continue;
    const image = json.images[index];
    if (!["image/png", "image/jpeg"].includes(image.mimeType ?? ({ ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg" }[extname(image.uri ?? "")]))) continue;
    const bytes = await imageBytes(json, tail, directory, image), metadata = await sharp(bytes).metadata();
    if (!(metadata.width * metadata.height >= 1024 * 1024)) continue;
    const key = textureLodKey(bytes, space), width = Math.max(1, Math.floor(metadata.width / 4)), height = Math.max(1, Math.floor(metadata.height / 4));
    if (!jobs.has(key)) jobs.set(key, { image: index, key, sourceSha256, sourceWidth: metadata.width, sourceHeight: metadata.height, width, height, space });
    records.push({ image: index, textures, key });
  }
  const queue = [...jobs.values()], results = new Map();
  let cursor = 0, failure;
  const encodeStarted = performance.now();
  await Promise.all(Array.from({ length: Math.min(workers, queue.length) }, async () => {
    while (!failure && cursor < queue.length) {
      const job = queue[cursor++];
      try { results.set(job.key, await cachedImage(job, cache, resize,
        index => imageBytes(json, tail, directory, json.images[index]))); }
      catch (error) { failure ??= error; }
    }
  }));
  if (failure) throw failure;
  const encodeMs = performance.now() - encodeStarted, folder = basename(destination, ".glb") + "-data", appended = new Map(), pairs = [];
  for (const record of records) {
    const artifact = results.get(record.key);
    if (!appended.has(artifact.artifactSha256)) {
      const uri = folder + "/texture-lod-" + artifact.artifactSha256 + ".png";
      await copyImmutable(artifact.imagePath, localPath(output, uri));
      appended.set(artifact.artifactSha256, json.images.push({ mimeType: "image/png", uri }) - 1);
    }
    for (const near of record.textures) {
      const far = json.textures.push({ ...structuredClone(json.textures[near]), source: appended.get(artifact.artifactSha256) }) - 1;
      pairs.push({ near, far });
    }
    Object.assign(record, artifact); delete record.imagePath;
  }
  if (pairs.length) {
    (scene.extras ??= {}).worldSpecTextureLod = { version: 1, pairs };
    await retargetLod(json, lod, output, folder);
  }
  const unique = [...results.values()];
  return { entry: pairs.length ? entryBytes(json, tail) : original, report: { sourceSha256: hash(original), recipe: TEXTURE_LOD_RECIPE,
    pairs: pairs.length, images: records.length, uniqueImages: unique.length, reused: unique.filter(row => row.reused).length,
    encodedBytes: unique.reduce((sum, row) => sum + row.byteLength, 0),
    sourceRgbaMipBytes: unique.reduce((sum, row) => sum + Math.round(row.sourceWidth * row.sourceHeight * 16 / 3), 0),
    farRgbaMipBytes: unique.reduce((sum, row) => sum + Math.round(row.width * row.height * 16 / 3), 0),
    encodeMs, elapsedMs: performance.now() - started, records } };
}

async function main() {
  const { values, positionals } = parseArgs({ allowPositionals: true, options: {
    out: { type: "string" }, cache: { type: "string" }, workers: { type: "string" }, help: { type: "boolean" },
  } });
  if (values.help) { console.log("Usage: node tools/kowloon-export/city-texture-lod.mjs ENTRY.glb --out STAGED.glb [--cache DIR --workers 2]\nOptional quarter-dimension detail textures. Original full-resolution glTF bindings remain intact. Source and output directories must differ."); return; }
  if (positionals.length !== 1 || !values.out) throw Error("Supply ENTRY.glb and --out STAGED.glb. Use --help for usage.");
  const report = await stageCityTextureLods({ source: positionals[0], destination: values.out, cacheDirectory: values.cache, workers: Number(values.workers ?? 2) });
  await writeFile(resolve(dirname(report.destination), "city-texture-lod.json"), JSON.stringify(report, null, 2));
  const { records, ...summary } = report; console.log(JSON.stringify(summary));
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href)
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
