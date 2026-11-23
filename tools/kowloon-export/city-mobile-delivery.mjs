import { createHash, randomUUID } from "node:crypto";
import { mkdir, readFile, realpath, rename, rm, stat } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { MeshoptEncoder } from "meshoptimizer";
import sharp from "sharp";
import { cityCompanionUris, entryBytes, localPath, readCityEntry, realDestination, retargetLod, settlePublishedFiles,
  verifiedLod } from "./city-data-textures.mjs";
import { atlasFillRoles, gpuTextureImage, gpuTextureRoles, stageCityGpuTextures } from "./city-gpu-textures.mjs";
import { chartCoverage, fillUncharted } from "./atlas-coverage.mjs";
import { bakedPageSize, bakedTexelDensities } from "./chart-atlas.mjs";
import { copyImmutable, writeImmutable } from "./immutable-file.mjs";
import { atlasTiers, mergeMobileAtlases } from "./mobile-atlas.mjs";
import { mobileGeometry, mobileLodSidecar } from "./mobile-geometry.mjs";
import { pruneMobileResources } from "./mobile-resources.mjs";
import { applyGraphicsVisibility } from "../../adapters/three/src/graphicsVisibility.ts";
import { kowloonDecorationMinimum } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/graphics-visibility.mjs";

const WEBP = "EXT_texture_webp", MESHOPT = "EXT_meshopt_compression";
// Textures keep a fraction of their authored density, so a 2048 atlas and a
// small trim strip lose the same proportion. Near geometry stays within
// nearErrorMeters of the original; farther levels remain viewer-selected.
const tiers = {
  low: { textureScale: .25, maxDimension: 512, nearErrorMeters: 0, washErrorMeters: .02 },
  "very-low": { textureScale: .125, maxDimension: 256, nearErrorMeters: .035, washErrorMeters: .05 },
};
const MIN_TEXTURE_EDGE = 16, UNSCALED_TEXTURE_EDGE = 128;

/** Scale both edges together. Images already at or below the unscaled edge and
 * the short edge of a narrow strip keep enough texels to stay recognisable. */
export function mobileTextureSize(width, height, budget) {
  // GPU block formats store 4x4 texel blocks. Sizing to whole blocks here means
  // the page is filtered once, not again when it is compressed.
  const whole = value => budget.blockAligned ? Math.max(4, Math.ceil(value / 4) * 4) : value;
  const longest = Math.max(width, height);
  if (longest <= UNSCALED_TEXTURE_EDGE) return { width: whole(width), height: whole(height) };
  const scale = Math.min(1, Math.max(UNSCALED_TEXTURE_EDGE, Math.min(budget.maxDimension, Math.round(longest * budget.textureScale))) / longest);
  const edge = value => whole(Math.max(Math.min(value, MIN_TEXTURE_EDGE), Math.round(value * scale)));
  return { width: edge(width), height: edge(height) };
}
const extensions = new Set(["EXT_mesh_gpu_instancing", "KHR_materials_emissive_strength", "KHR_mesh_quantization", "MSFT_lod",
  "KHR_lights_punctual", "KHR_texture_transform", WEBP]);
const empty = Buffer.alloc(0);
const align = bytes => Math.ceil(bytes / 4) * 4;
const digest = bytes => createHash("sha256").update(bytes).digest("hex").slice(0, 24);

function imageBindings(json) {
  const users = new Map(), images = new Map();
  for (const texture of json.textures ?? []) {
    const source = texture.extensions?.[WEBP]?.source ?? texture.source;
    if (!users.has(source)) users.set(source, []);
    users.get(source).push(texture);
  }
  const accessors = new Set(json.accessors.flatMap(a => [a.bufferView, a.sparse?.indices?.bufferView, a.sparse?.values?.bufferView]));
  for (const [index, image] of (json.images ?? []).entries()) {
    if (image.uri || !Number.isInteger(image.bufferView) || accessors.has(image.bufferView) || images.has(image.bufferView)
        || !["image/png", "image/jpeg", "image/webp"].includes(image.mimeType) || !users.get(index)?.length)
      throw Error("Expected exclusive embedded PNG, JPEG or WebP city images.");
    images.set(image.bufferView, { image, index, textures: users.get(index) });
  }
  return images;
}

/** Publish a visual-only city delivery. All map topology, lights and playable transforms remain downstream of the same AML. */
export async function stageMobileCity(source, destination, preset, { legacyKowloonLabels = false, gpuTextures, blockAligned = false, fillAtlas = true, atlas } = {}) {
  const budget = Object.hasOwn(tiers, preset) && { ...tiers[preset], blockAligned };
  if (!budget) throw Error("Choose a low or very-low mobile delivery.");
  if (gpuTextures) {
    // KTX2 delivery: the same mobile pack with block-aligned pages, then every
    // texture compressed for the GPU with this tier's map content and streaming.
    const final = resolve(destination), temporary = resolve(dirname(final), ".mobile-" + randomUUID());
    try {
      const staged = resolve(temporary, basename(final));
      // `maps.fillAtlas: false` publishes the pages as they are, for comparison.
      const report = await stageMobileCity(source, staged, preset, { legacyKowloonLabels, blockAligned: true, atlas,
        fillAtlas: gpuTextures.maps?.fillAtlas !== false });
      const { records, ...gpu } = await stageCityGpuTextures({ source: staged, destination: final, tier: preset,
        ...(gpuTextures === true ? {} : gpuTextures) });
      return { ...report, destination: gpu.destination, deliveryBytes: gpu.deliveryBytes, gpuTextures: gpu };
    } finally { await rm(temporary, { recursive: true, force: true }); }
  }
  source = await realpath(resolve(source));
  const input = dirname(source), output = await realDestination(dirname(resolve(destination)));
  destination = resolve(output, basename(destination));
  if (input === output) throw Error("Stage the mobile pack in a separate directory.");
  const { json, tail } = readCityEntry(await readFile(source));
  if (tail.length || !json.scenes?.[json.scene ?? 0]?.extras?.worldSpec || json.animations?.length || json.skins?.length ||
      json.buffers?.some(buffer => !buffer.uri || buffer.extensions) || json.bufferViews?.some(view => view.extensions) ||
      [...(json.extensionsUsed ?? []), ...(json.extensionsRequired ?? [])].some(extension => !extensions.has(extension)))
    throw Error("Expected a published city with external buffers and PNG, JPEG or WebP textures.");
  const scene = json.scenes[json.scene ?? 0].extras;
  if (scene.worldSpecTextureLod || scene.worldSpecBlockProxy) throw Error("Use the city pack before optional streaming or block proxies.");
  // A page the bake already made at a reduced texel density (chart-atlas.mjs) loses only what
  // is left of this tier's reduction, usually nothing. Other images scale by the tier as before.
  const deliveredSize = document => {
    const baked = bakedTexelDensities(document);
    return (width, height, image) => baked.has(image)
      ? bakedPageSize(width, height, baked.get(image), budget.textureScale, budget.blockAligned ? 4 : 1)
      : mobileTextureSize(width, height, budget);
  };
  const companions = cityCompanionUris(json);
  let sourceBytes = (await stat(source)).size;
  for (const uri of companions) sourceBytes += (await stat(localPath(input, uri))).size;
  const lod = await verifiedLod(json, tail, input);
  const bufferCache = new Map();
  const inputBuffers = json.buffers;
  const sourceBuffer = async (index) => {
    const buffer = inputBuffers[index];
    if (!bufferCache.has(index)) {
      if (bufferCache.size >= 6) bufferCache.delete(bufferCache.keys().next().value);
      bufferCache.set(index, readFile(localPath(input, buffer.uri)).then(bytes => {
        if (bytes.length !== buffer.byteLength) throw Error("Invalid city buffer size.");
        return bytes;
      }));
    }
    return bufferCache.get(index);
  };
  const read = async (index) => {
    const view = json.bufferViews[index];
    const data = await sourceBuffer(view.buffer), start = view.byteOffset ?? 0;
    if (start + view.byteLength > data.length) throw Error("Invalid city buffer view.");
    return data.subarray(start, start + view.byteLength);
  };
  const { replacement, trianglesRemoved, verticesRemoved, lods } = await mobileGeometry(json, lod, read, budget);
  if (legacyKowloonLabels) {
    for (const mesh of json.meshes) for (const part of mesh.primitives) {
      const tier = kowloonDecorationMinimum(json.materials?.[part.material]?.name);
      if (tier && part.extras?.worldSpecMinGraphics === undefined)
        (part.extras ??= {}).worldSpecMinGraphics = tier;
    }
  }
  const selection = applyGraphicsVisibility(json, preset);
  delete scene.cityLod;
  if (preset === "very-low") delete scene.paintedLawn; // The opaque turf and its tips remain.
  scene.worldSpecDelivery = { version: 1, preset };
  // Optional: each building's atlas pages merged into a few per material class (mobile-atlas.mjs).
  const merged = atlas ? await mergeMobileAtlases(json, { tier: { ...atlasTiers[preset], ...(atlas === true ? {} : atlas) }, read, replacement,
    lods, textureSize: deliveredSize(json) }) : undefined;
  const { views, raw, prunedMeshes, prunedImages } = pruneMobileResources(json);
  const distanceLods = mobileLodSidecar(json, lods);
  const images = imageBindings(json), textureSize = deliveredSize(json);
  const liveViews = new Map([...new Set(views.map(item => item.bufferView))].sort((a, b) => a - b).map((id, index) => [id, index]));
  const plan = inputBuffers.map(() => []);
  for (const id of liveViews.keys()) plan[json.bufferViews[id].buffer].push(id);
  const buffers = [];
  const folder = basename(destination, ".glb") + "-data";
  const produced = new Set();
  let fallbackBytes = 0, geometryBytes = 0, decodedTextureBytes = 0, imageBytes = 0, filledPages = 0;
  // Only the KTX2 delivery repairs atlas fill; the default pack keeps its published pixels.
  const atlasRoles = budget.blockAligned && fillAtlas ? gpuTextureRoles(json) : undefined;
  // Views are re-placed as they are packed below, so geometry is read where it was published.
  const published = json.bufferViews.map(view => ({ buffer: view.buffer, byteOffset: view.byteOffset ?? 0, byteLength: view.byteLength }));
  const coverage = atlasRoles && chartCoverage(json, async id => {
    if (replacement.has(id)) return replacement.get(id).bytes;
    const place = published[id], data = await sourceBuffer(place.buffer);
    return data.subarray(place.byteOffset, place.byteOffset + place.byteLength);
  }, gpuTextureImage);
  await MeshoptEncoder.ready;
  const enabled = new Set();
  async function publishBuffer(bytes) {
    const uri = folder + "/mobile-" + digest(bytes) + ".bin";
    await writeImmutable(localPath(output, uri), bytes);
    produced.add(uri);
    buffers.push({ uri, byteLength: bytes.length });
  }
  for (const [buffer, views] of plan.entries()) {
    if (!views.length) continue;
    const packedBuffer = buffers.length;
    const parts = [];
    let end = 0;
    for (const id of views) {
      const view = json.bufferViews[id], image = images.get(id), revised = replacement.get(id);
      const page = merged?.pages.get(id); // A merged atlas page is already at this tier's density.
      let bytes = revised?.bytes ?? page ?? await read(id);
      let codec = revised;
      if (!codec && view.target === 34962 && view.byteStride && view.byteStride % 4 === 0 && bytes.length % view.byteStride === 0)
        codec = { mode: "ATTRIBUTES", stride: view.byteStride };
      if (image) {
        const source = await sharp(bytes).metadata();
        let pixels = sharp(bytes);
        const roles = atlasRoles?.get(image.index);
        if (roles?.size && [...roles].every(role => atlasFillRoles.includes(role))) {
          // Shrinking a page mixes its unused area into the charts beside it. At a
          // quarter or an eighth of the edge that turns thin parts the fill colour,
          // so the unused area first takes the colour of the charts around it.
          const charted = await coverage.mask(coverage.users(image.index), source.width, source.height);
          if (charted?.includes(0) && charted.includes(1)) {
            const raw = await sharp(bytes).toColourspace("srgb").ensureAlpha().raw({ depth: "uchar" }).toBuffer();
            pixels = sharp(fillUncharted(raw, source.width, source.height, charted), { raw: { width: source.width, height: source.height, channels: 4 } });
            filledPages++;
          }
        }
        const { data, info } = await pixels.resize({ ...(page ? { width: source.width, height: source.height } : textureSize(source.width, source.height, image.index)),
          fit: "fill" }).webp({ lossless: true, exact: true, effort: 1 })
          .toBuffer({ resolveWithObject: true });
        bytes = data;
        image.image.mimeType = "image/webp";
        imageBytes += bytes.length;
        decodedTextureBytes += info.width * info.height * 4;
        enabled.add(WEBP);
        for (const texture of image.textures) {
          delete texture.source;
          texture.extensions = { ...texture.extensions, [WEBP]: { source: image.index } };
        }
      }
      if (codec || view.target === 34962 || view.target === 34963) geometryBytes += bytes.length;
      view.byteLength = bytes.length;
      view.byteOffset = end;
      view.buffer = packedBuffer;
      if (codec && bytes.length >= 32) {
        const count = bytes.length / codec.stride;
        const compressed = MeshoptEncoder.encodeGltfBuffer(bytes, count, codec.stride, codec.mode);
        if (compressed.length < bytes.length) {
          enabled.add(MESHOPT);
          view.extensions = { [MESHOPT]: { buffer: packedBuffer, byteOffset: end, byteLength: compressed.length,
            byteStride: codec.stride, count, mode: codec.mode } };
          view.buffer = -1;
          view.byteOffset = fallbackBytes;
          fallbackBytes += align(bytes.length);
          bytes = compressed;
        }
      }
      parts.push(bytes, Buffer.alloc(align(bytes.length) - bytes.length));
      end += align(bytes.length);
    }
    await publishBuffer(Buffer.concat(parts, end));
  }
  const rawBuffers = new Map();
  for (const record of raw) {
    const original = record.buffer;
    if (!rawBuffers.has(original)) {
      rawBuffers.set(original, buffers.length);
      await publishBuffer(await sourceBuffer(original));
    }
    record.buffer = rawBuffers.get(original);
  }
  json.bufferViews = [...liveViews.keys()].map(id => json.bufferViews[id]);
  for (const reference of views) reference.bufferView = liveViews.get(reference.bufferView);
  if (fallbackBytes) {
    const index = buffers.length;
    for (const view of json.bufferViews) if (view.buffer === -1) view.buffer = index;
    buffers.push({ byteLength: fallbackBytes, extensions: { [MESHOPT]: { fallback: true } } });
  }
  json.buffers = buffers;
  json.extensionsUsed = [...new Set([...(json.extensionsUsed ?? []), ...enabled])];
  json.extensionsRequired = [...new Set([...(json.extensionsRequired ?? []), ...enabled])];
  // The sidecar records the entry it belongs to, so write it once the entry is final.
  await retargetLod(json, distanceLods, output, folder);
  if (scene.cityLod) produced.add(scene.cityLod);
  const result = entryBytes(json, empty);
  let deliveryBytes = result.length;
  for (const uri of cityCompanionUris(json)) {
    const path = localPath(output, uri);
    if (!produced.has(uri)) await copyImmutable(localPath(input, uri), path);
    deliveryBytes += (await stat(path)).size;
  }
  await mkdir(output, { recursive: true });
  const pending = destination + "." + randomUUID() + ".tmp";
  try {
    await writeImmutable(pending, result);
    await rename(pending, destination);
  } finally { await rm(pending, { force: true }); }
  // Republishing over an earlier delivery leaves none of its unreferenced files behind.
  const removedFiles = await settlePublishedFiles(destination, json);
  return { preset, destination, sourceBytes, deliveryBytes, geometryBytes, imageBytes, decodedTextureBytes,
    trianglesRemoved, verticesRemoved, ...selection, prunedMeshes, prunedImages, removedFiles, ...(atlasRoles ? { filledPages } : {}),
    ...(merged ? { atlas: merged.report } : {}) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { values, positionals } = parseArgs({ allowPositionals: true, options: { out: { type: "string" },
    preset: { type: "string" }, "legacy-kowloon-labels": { type: "boolean" }, "gpu-textures": { type: "boolean" },
    "color-codec": { type: "string" }, "texture-cache": { type: "string" }, atlas: { type: "boolean" }, "atlas-tier": { type: "string" } } });
  if (positionals.length !== 1 || !values.out || !values.preset)
    throw Error("Usage: node city-mobile-delivery.mjs CITY.glb --preset low|very-low --out CITY.PRESET.glb [--legacy-kowloon-labels]\n"
      + "  [--gpu-textures [--color-codec etc1s|uastc] [--texture-cache DIR]]  KTX2 textures with the tier's map content and streaming\n"
      + "  [--atlas [--atlas-tier JSON]]  merge each building's atlas pages and primitives per material class (mobile-atlas.mjs)");
  const gpuTextures = values["gpu-textures"] && { colorCodec: values["color-codec"], cacheDirectory: values["texture-cache"] };
  const atlas = (values.atlas || values["atlas-tier"]) && JSON.parse(values["atlas-tier"] ?? "{}");
  console.log(JSON.stringify(await stageMobileCity(positionals[0], values.out, values.preset,
    { legacyKowloonLabels: values["legacy-kowloon-labels"], gpuTextures, atlas }), null, 2));
}
