// Opt-in derived pack whose every texture is a Basis Universal KTX2 image. The
// browser transcodes each one to the GPU's own block format (ASTC or ETC2 on
// phones, BC7 on desktop), so a texture costs a quarter or an eighth of RGBA8.
import { randomUUID } from "node:crypto";
import { createHash } from "node:crypto";
import { mkdir, readFile, realpath, rename, rm, stat, writeFile } from "node:fs/promises";
import { basename, dirname, relative, resolve } from "node:path";
import { MeshoptDecoder } from "meshoptimizer";
import sharp from "sharp";
import { chartCoverage, fillUncharted } from "./atlas-coverage.mjs";
import { cityCompanionUris, entryBytes, localPath, readCityEntry, realDestination, retargetLod, settlePublishedFiles,
  verifiedLod } from "./city-data-textures.mjs";
import { exclusiveTextureLods } from "./city-texture-lod.mjs";
import { decodeGpuTexture, gpuTextureKey, gpuTexturePool, gpuTextureProfiles, inspectKtx2 } from "./gpu-texture-codec.mjs";
import { copyImmutable, writeImmutable } from "./immutable-file.mjs";

const BASIS = "KHR_texture_basisu", WEBP = "EXT_texture_webp", MESHOPT = "EXT_meshopt_compression";
const digest = bytes => createHash("sha256").update(bytes).digest("hex").slice(0, 24);
const block = value => Math.max(4, Math.ceil(value / 4) * 4);
const MIN_STREAMED_TEXELS = 256 * 256;
// The WebAssembly Basis encoder refuses more source texels than this.
const MAX_ENCODER_TEXELS = 12 * 1024 * 1024;

/** Map content per delivery tier. Medium changes no map; the reduced tiers
 * trade the maps the project owner allowed them to lose, and publish quarter
 * edge companions so a viewer starts on those and fetches near textures later.
 * `streaming: true` pairs what the Medium texture LOD policy pairs; "all" pairs
 * every surface family. A viewer that does not stream shows the near textures. */
export const gpuTextureTiers = Object.freeze({
  // Medium keeps every map as authored, and colour in the codec that shows it best.
  medium: Object.freeze({ colorCodec: "uastc" }),
  // Low and Very Low favour size (project owner, 2026-11-22). Low drops normal
  // pages whose RMS tilt is under 6/255 (about 3 degrees), halves the rest and
  // the roughness pages, and compresses data maps harder.
  low: Object.freeze({ emissiveScale: .25, normalScale: .5, dataScale: .5, weakNormalRms: 6, streaming: "all",
    profiles: Object.freeze({ data: "data-small" }) }),
  // Very Low is colour, emission and finish detail with constant roughness and metalness.
  "very-low": Object.freeze({ emissiveScale: .25, constantSurface: true, streaming: "all",
    profiles: Object.freeze({ "color-etc1s": "color-etc1s-small", data: "data-small" }) }),
});

/** Images every surface family may stream: used only as colour, normal or
 * roughness, in one role, and never through a reference outside those slots. */
function streamableImages(json) {
  const uses = new Map(), blocked = new Set();
  const use = (texture, role) => {
    if (!["color", "normal", "mr"].includes(role)) { blocked.add(texture); return; }
    if (!uses.has(texture)) uses.set(texture, new Set());
    uses.get(texture).add(role);
  };
  for (const material of json.materials ?? []) {
    for (const [role, owner, key] of slots(material)) use(owner[key].index, role);
    const detail = material.extras?.worldSpecSurface?.detailTexture;
    if (detail !== undefined) blocked.add(detail);
  }
  for (const scene of json.scenes ?? []) if (scene.extras?.worldSpecConcreteDetail?.texture !== undefined) blocked.add(scene.extras.worldSpecConcreteDetail.texture);
  const images = new Map();
  for (const [index, texture] of (json.textures ?? []).entries()) {
    const image = imageOf(texture), group = images.get(image) ?? { textures: [], roles: new Set(), blocked: false };
    group.textures.push(index);
    if (blocked.has(index) || !uses.has(index)) group.blocked = true;
    else for (const role of uses.get(index)) group.roles.add(role);
    images.set(image, group);
  }
  return new Map([...images].filter(([, group]) => !group.blocked && group.roles.size === 1));
}

/** Root-mean-square tilt of a tangent-space normal page, in 8-bit steps. */
export function normalStrength(rgba, mask) {
  let sum = 0, count = 0;
  if (mask && !mask.includes(1)) mask = undefined;
  for (let i = 0; i < rgba.length; i += 4) {
    if (mask && !mask[i >> 2]) continue;
    sum += (rgba[i] - 128) ** 2 + (rgba[i + 1] - 128) ** 2; count += 2;
  }
  return Math.sqrt(sum / count);
}

const slots = material => [
  ["color", material.pbrMetallicRoughness, "baseColorTexture"], ["mr", material.pbrMetallicRoughness, "metallicRoughnessTexture"],
  ["normal", material, "normalTexture"], ["occlusion", material, "occlusionTexture"], ["emissive", material, "emissiveTexture"],
].filter(([, owner, key]) => owner?.[key]);
const imageOf = texture => texture?.extensions?.[BASIS]?.source ?? texture?.extensions?.[WEBP]?.source ?? texture?.source;
export { imageOf as gpuTextureImage };
/** Roles whose pages are atlases with unused area that must not bleed into charts.
 * Emission is left alone: its unused area is black, which is no emission. */
export const atlasFillRoles = Object.freeze(["color", "mr", "normal", "occlusion"]);

/** Roles of every image, including the references outside glTF materials. */
export function gpuTextureRoles(json) {
  const roles = new Map();
  const add = (texture, role) => {
    const image = imageOf(json.textures?.[texture]);
    if (!Number.isInteger(image) || !json.images?.[image]) throw Error(`Invalid city texture reference ${texture}.`);
    if (!roles.has(image)) roles.set(image, new Set());
    roles.get(image).add(role);
  };
  for (const material of json.materials ?? []) {
    for (const [role, owner, key] of slots(material)) add(owner[key].index, role);
    const detail = material.extras?.worldSpecSurface?.detailTexture;
    if (detail !== undefined) add(detail, "detail");
  }
  for (const scene of json.scenes ?? []) {
    const grain = scene.extras?.worldSpecConcreteDetail?.texture;
    if (grain !== undefined) add(grain, "grain");
  }
  return roles;
}

/** The codec recipe for an image, from every role it is sampled in. */
export function gpuTextureProfileName(roles, hasAlpha, colorCodec = "etc1s") {
  // Cutouts and blended glass keep UASTC: ETC1S alpha is a second, coarser image.
  if (roles.has("color")) return hasAlpha || colorCodec === "uastc" ? "color" : "color-etc1s";
  if (roles.has("emissive")) return roles.size === 1 ? "emissive" : "color";
  return roles.size === 1 && roles.has("normal") ? "normal" : "data";
}

function scaleFor(roles, maps) {
  if (roles.size !== 1) return 1;
  if (roles.has("emissive")) return maps.emissiveScale ?? 1;
  if (roles.has("normal")) return maps.normalScale ?? 1;
  if (roles.has("mr")) return maps.dataScale ?? 1;
  return 1;
}

async function decode(bytes) {
  const { data, info } = await sharp(bytes).toColourspace("srgb").ensureAlpha().raw({ depth: "uchar" }).toBuffer({ resolveWithObject: true });
  if (info.channels !== 4) throw Error("Expected an image that decodes to RGBA8.");
  return { rgba: data, width: info.width, height: info.height };
}

/** Colour and alpha are filtered separately: data maps carry unrelated values in alpha. */
async function resize({ rgba, width, height }, toWidth, toHeight) {
  if (width === toWidth && height === toHeight) return rgba;
  const raw = { raw: { width, height, channels: 4 } }, size = { width: toWidth, height: toHeight, fit: "fill" };
  const [colour, alpha] = await Promise.all([
    sharp(rgba, raw).removeAlpha().resize(size).raw().toBuffer(),
    sharp(rgba, raw).extractChannel(3).resize(size).raw().toBuffer(),
  ]);
  const out = Buffer.alloc(toWidth * toHeight * 4);
  for (let i = 0, c = 0, a = 0; i < out.length; i += 4, c += 3, a++) {
    out[i] = colour[c]; out[i + 1] = colour[c + 1]; out[i + 2] = colour[c + 2]; out[i + 3] = alpha[a];
  }
  return out;
}

const hasAlpha = rgba => { for (let i = 3; i < rgba.length; i += 4) if (rgba[i] !== 255) return true; return false; };
/** Every texel is the unperturbed tangent-space normal. */
export function flatNormals(rgba) {
  for (let i = 0; i < rgba.length; i += 4)
    if ((rgba[i] !== 127 && rgba[i] !== 128) || (rgba[i + 1] !== 127 && rgba[i + 1] !== 128) || rgba[i + 2] !== 255) return false;
  return true;
}
/** Mean roughness (G) and metalness (B) of the charted texels. Unused atlas
 * area is published as (0,255,0) or black and would bias a plain average. */
export function surfaceConstants(rgba, mask) {
  let rough = 0, metal = 0, count = 0;
  // With a coverage mask the charted texels are known; without one they are told from the fill colours.
  if (mask && !mask.includes(1)) mask = undefined;
  for (let i = 0; i < rgba.length; i += 4) {
    if (mask ? !mask[i >> 2] : rgba[i] === 0 && rgba[i + 2] === 0 && (rgba[i + 1] === 255 || rgba[i + 1] === 0)) continue;
    rough += rgba[i + 1]; metal += rgba[i + 2]; count++;
  }
  return count ? { roughness: rough / count / 255, metalness: metal / count / 255 } : { roughness: 1, metalness: 0 };
}

function chainBytes(width, height, bytesPerBlock) {
  let bytes = 0;
  for (let w = width, h = height; ; w = Math.max(1, w >> 1), h = Math.max(1, h >> 1)) {
    bytes += bytesPerBlock ? Math.ceil(w / 4) * Math.ceil(h / 4) * bytesPerBlock : w * h * 4;
    if (w === 1 && h === 1) return bytes;
  }
}

const psnr = (a, b, channels) => {
  let error = 0;
  for (let i = 0; i < a.length; i += 4) for (const c of channels) error += (a[i + c] - b[i + c]) ** 2;
  return error ? 10 * Math.log10(255 * 255 * (a.length / 4) * channels.length / error) : 99;
};
const measuredChannels = { normal: [0, 1], mr: [1, 2], occlusion: [0, 1] };

async function limited(items, limit, work) {
  let next = 0;
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (next < items.length) await work(items[next++]);
  }));
}

/** Stage `source` as a pack that requires KHR_texture_basisu. Geometry, nodes,
 * lights, materials and every non-texture record keep their indices. */
export async function stageCityGpuTextures({ source, destination, tier, maps = gpuTextureTiers[tier ?? "medium"], colorCodec,
  cacheDirectory = "generated/city-gpu-texture-cache", workers, measure = false }) {
  if (!maps) throw Error("Choose a medium, low or very-low GPU texture tier.");
  colorCodec ??= maps.colorCodec ?? "etc1s";
  if (!["etc1s", "uastc"].includes(colorCodec)) throw Error("colorCodec must be etc1s or uastc.");
  maps = { dropFlatNormals: true, ...maps };
  source = await realpath(resolve(source));
  const input = dirname(source), output = await realDestination(dirname(resolve(destination))), cache = resolve(cacheDirectory);
  destination = resolve(output, basename(destination));
  if (!relative(input, output)) throw Error("Stage into a separate directory; the source pack must remain unchanged.");
  const started = performance.now(), { json, tail } = readCityEntry(await readFile(source));
  const scene = json.scenes?.[json.scene ?? 0]?.extras;
  if (tail.length || !scene?.worldSpec || json.buffers?.some(buffer => !buffer.uri && !buffer.extensions?.[MESHOPT]?.fallback))
    throw Error("Expected a published city with external buffers.");
  if (scene.worldSpecTextureLod || scene.worldSpecBlockProxy) throw Error("Use the city pack before optional streaming or block proxies.");
  if (json.extensionsUsed?.includes(BASIS)) throw Error("The city already uses KTX2 textures.");
  const physical = view => view.extensions?.[MESHOPT] ?? view;
  const accessors = new Set((json.accessors ?? []).flatMap(a => [a.bufferView, a.sparse?.indices?.bufferView, a.sparse?.values?.bufferView]));
  const imageViews = new Map();
  for (const [index, image] of (json.images ?? []).entries()) {
    const view = json.bufferViews?.[image.bufferView];
    if (image.uri || !view || view.extensions || accessors.has(image.bufferView) || imageViews.has(image.bufferView)
        || !["image/png", "image/jpeg", "image/webp"].includes(image.mimeType))
      throw Error("Expected exclusive embedded PNG, JPEG or WebP city images.");
    imageViews.set(image.bufferView, index);
  }
  let sourceBytes = (await stat(source)).size;
  for (const uri of cityCompanionUris(json)) sourceBytes += (await stat(localPath(input, uri))).size;
  const lod = await verifiedLod(json, tail, input);

  // Planned material edits decide which images are encoded, measured or dropped.
  const original = gpuTextureRoles(json);
  const remaining = new Map([...original].map(([image, roles]) => [image, new Set(roles)]));
  if (maps.constantSurface) for (const roles of remaining.values()) for (const role of ["mr", "normal", "occlusion"]) roles.delete(role);
  const buffers = new Map(), sourceBuffers = json.buffers.map(buffer => ({ ...buffer })), sourceViews = json.bufferViews;
  const bufferBytes = index => {
    if (!buffers.has(index)) {
      if (buffers.size >= 8) buffers.delete(buffers.keys().next().value);
      buffers.set(index, readFile(localPath(input, sourceBuffers[index].uri)).then(bytes => {
        if (bytes.length !== sourceBuffers[index].byteLength) throw Error("Invalid city buffer size.");
        return bytes;
      }));
    }
    return buffers.get(index);
  };
  const viewBytes = async view => {
    const place = physical(view), data = await bufferBytes(place.buffer), start = place.byteOffset ?? 0;
    if (start + place.byteLength > data.length) throw Error("Invalid city buffer view.");
    return data.subarray(start, start + place.byteLength);
  };
  // Geometry is read only to learn which texels of each atlas page it samples.
  const coverage = chartCoverage(json, async index => {
    const view = sourceViews[index], packed = view.extensions?.[MESHOPT], bytes = await viewBytes(view);
    if (!packed) return bytes;
    await MeshoptDecoder.ready;
    const decoded = new Uint8Array(packed.count * packed.byteStride);
    MeshoptDecoder.decodeGltfBuffer(decoded, packed.count, packed.byteStride, bytes, packed.mode, packed.filter);
    return decoded;
  }, imageOf);

  await mkdir(cache, { recursive: true });
  const pool = gpuTexturePool(workers), records = new Array(json.images?.length ?? 0), encoding = new Map();
  let encodeMs = 0, encoded = 0, reused = 0;
  // Encoded bytes stay in the cache until the buffers are written; only their
  // size is kept, so a Medium city does not hold gigabytes in memory.
  const encodeCached = async (rgba, width, height, profile) => {
    const key = gpuTextureKey(profile, width, height, rgba), path = resolve(cache, key + ".ktx2");
    if (!encoding.has(key)) encoding.set(key, (async () => {
      let ktx = await readFile(path).catch(error => { if (error.code !== "ENOENT") throw error; });
      if (ktx) reused++;
      else {
        // The pool takes ownership of the texels it encodes.
        const texels = new Uint8Array(rgba.length); texels.set(rgba);
        ktx = Buffer.from(await pool.encode(texels, width, height, profile));
        await writeImmutable(path, ktx);
        encoded++;
      }
      const info = inspectKtx2(ktx);
      if (info.width !== width || info.height !== height || info.levels < 2) throw Error(`Invalid encoded KTX2 texture ${key}.`);
      return ktx.length;
    })());
    return { key, path, bytes: await encoding.get(key) };
  };
  // Distant companions follow the publisher's existing material policy, read
  // from the materials as this tier will publish them.
  const streamed = new Map();
  if (maps.streaming) {
    const planned = structuredClone(json);
    planned.textures = (planned.textures ?? []).map(texture => ({ sampler: texture.sampler, source: imageOf(texture) }));
    if (maps.constantSurface) for (const material of planned.materials ?? []) {
      delete material.normalTexture; delete material.occlusionTexture; delete material.pbrMetallicRoughness?.metallicRoughnessTexture;
    }
    for (const [image, value] of maps.streaming === "all" ? streamableImages(planned) : exclusiveTextureLods(planned)) streamed.set(image, value);
  }
  const encodeStarted = performance.now();
  try {
    await limited([...(json.images ?? []).keys()], pool.size + 2, async index => {
      const image = json.images[index], before = original.get(index) ?? new Set(), roles = remaining.get(index) ?? new Set();
      const bytes = await viewBytes(json.bufferViews[image.bufferView]);
      const record = records[index] = { image: index, roles: [...before].sort().join("+") || "unreferenced", sourceBytes: bytes.length };
      const constants = maps.constantSurface && before.has("mr");
      if (before.size && !roles.size && !constants) {
        const meta = await sharp(bytes).metadata();
        Object.assign(record, { sourceWidth: meta.width, sourceHeight: meta.height, dropped: "tier" });
        return;
      }
      const decoded = await decode(bytes);
      Object.assign(record, { sourceWidth: decoded.width, sourceHeight: decoded.height });
      const atlas = before.size > 0 && [...before].every(role => atlasFillRoles.includes(role));
      const users = atlas ? coverage.users(index) : [];
      const charted = atlas && maps.fillAtlas !== false ? await coverage.mask(users, decoded.width, decoded.height) : null;
      if (constants) {
        // A page is shared by materials of different surfaces (wall paint, bare metal):
        // each takes the mean of the texels its own triangles sample.
        record.constants = surfaceConstants(decoded.rgba, charted);
        record.materialConstants = new Map();
        for (const [material, info] of maps.materialConstants === false ? [] : users) {
          if (info !== json.materials[material].pbrMetallicRoughness?.metallicRoughnessTexture) continue;
          const own = await coverage.mask([[material, info]], decoded.width, decoded.height);
          if (own?.includes(1)) record.materialConstants.set(material, surfaceConstants(decoded.rgba, own));
        }
      }
      if (before.size && !roles.size) { record.dropped = "tier"; return; }
      if (roles.size === 1 && roles.has("normal")) {
        if (maps.dropFlatNormals && flatNormals(decoded.rgba)) { record.dropped = "flat"; return; }
        if (maps.weakNormalRms && normalStrength(decoded.rgba, charted) <= maps.weakNormalRms) { record.dropped = "weak"; return; }
      }
      // Unused atlas area takes the colour of the charts around it before any
      // filter (the resize, the distant companion, the mip chain) can mix it in.
      if (charted) { decoded.rgba = fillUncharted(decoded.rgba, decoded.width, decoded.height, charted); record.filled = true; }
      const scale = scaleFor(roles, maps);
      const width = block(Math.round(decoded.width * scale)), height = block(Math.round(decoded.height * scale));
      if (width * height > MAX_ENCODER_TEXELS) {
        // Larger than the encoder accepts: the page stays as published, at its full density.
        Object.assign(record, { width: decoded.width, height: decoded.height, alpha: hasAlpha(decoded.rgba), kept: true, bytes: bytes.length,
          key: "kept:" + index });
        return;
      }
      const rgba = await resize(decoded, width, height), alpha = hasAlpha(rgba);
      const role = gpuTextureProfileName(roles, alpha, colorCodec), name = maps.profiles?.[role] ?? role, profile = gpuTextureProfiles[name];
      if (!profile) throw Error("Unknown GPU texture profile: " + name);
      Object.assign(record, { width, height, alpha, profile: name, codec: profile.codec, ...await encodeCached(rgba, width, height, profile) });
      // The distant companion is a quarter of each edge, in the same codec.
      if (streamed.has(index) && width * height >= MIN_STREAMED_TEXELS && Math.min(width, height) >= 64) {
        const farWidth = block(Math.round(width / 4)), farHeight = block(Math.round(height / 4));
        const far = await resize({ rgba, width, height }, farWidth, farHeight);
        record.far = { width: farWidth, height: farHeight, alpha: hasAlpha(far), ...await encodeCached(far, farWidth, farHeight, profile) };
      }
      const path = record.path;
      if (measure) {
        const shown = await decodeGpuTexture(await readFile(path)), role = [...roles][0];
        record.psnr = psnr(rgba, shown.rgba, measuredChannels[roles.size === 1 ? role : ""] ?? [0, 1, 2]);
        if (alpha) record.alphaPsnr = psnr(rgba, shown.rgba, [3]);
      }
    });
    encodeMs = performance.now() - encodeStarted;
  } finally { await pool.close(); }

  // Material edits follow the measured content.
  const constants = new Map(records.filter(record => record.constants).map(record => [record.image, record.constants]));
  const flat = new Set(records.filter(record => record.dropped === "flat" || record.dropped === "weak").map(record => record.image));
  for (const [index, material] of (json.materials ?? []).entries()) {
    for (const [role, owner, key] of slots(material)) {
      const image = imageOf(json.textures[owner[key].index]);
      if (maps.constantSurface && role === "mr") {
        const value = records[image].materialConstants.get(index) ?? constants.get(image);
        owner.roughnessFactor = +((owner.roughnessFactor ?? 1) * value.roughness).toFixed(4);
        owner.metallicFactor = +((owner.metallicFactor ?? 1) * value.metalness).toFixed(4);
        delete owner[key];
      } else if (maps.constantSurface && (role === "normal" || role === "occlusion") || role === "normal" && flat.has(image)) delete owner[key];
    }
  }
  const roles = gpuTextureRoles(json);
  const liveImages = new Set(records.filter(record => !record.dropped).map(record => record.image));
  for (const image of roles.keys()) if (!liveImages.has(image)) throw Error(`City image ${image} was dropped while still referenced.`);
  const textureIds = new Map(), imageIds = new Map(), textureCount = json.textures?.length ?? 0;
  json.textures = (json.textures ?? []).filter((texture, index) => liveImages.has(imageOf(texture)) && textureIds.set(index, textureIds.size));
  const bindings = new Map();
  for (const [index] of (json.images ?? []).entries()) if (liveImages.has(index)) imageIds.set(index, imageIds.size);
  for (const texture of json.textures) {
    const source = imageOf(texture), image = imageIds.get(source);
    if (records[source].kept) {
      if (texture.extensions?.[WEBP]) texture.extensions[WEBP].source = image; else texture.source = image;
      continue;
    }
    delete texture.source;
    if (texture.extensions) delete texture.extensions[WEBP];
    texture.extensions = { ...texture.extensions, [BASIS]: { source: image } };
    bindings.set(image, (bindings.get(image) ?? 0) + 1);
  }
  const texture = index => {
    if (!textureIds.has(index)) throw Error(`Invalid retained city texture ${index}.`);
    return textureIds.get(index);
  };
  for (const material of json.materials ?? []) {
    if (JSON.stringify(material.extensions ?? {}).includes('"index"') && textureIds.size !== textureCount)
      throw Error("Material extension textures cannot be renumbered.");
    for (const [, owner, key] of slots(material)) owner[key].index = texture(owner[key].index);
    const surface = material.extras?.worldSpecSurface;
    if (surface?.detailTexture !== undefined) surface.detailTexture = texture(surface.detailTexture);
  }
  for (const item of json.scenes ?? []) {
    const grain = item.extras?.worldSpecConcreteDetail;
    if (grain?.texture !== undefined) grain.texture = texture(grain.texture);
  }

  // Take the images out of the geometry buffers. Buffers keep their indices.
  const rawBuffers = new Set((json.scenes ?? []).flatMap(item => Object.values(item.extras ?? {})).map(record => record?.buffer).filter(Number.isInteger));
  const touched = new Map(), folder = basename(destination, ".glb") + "-data", produced = new Set();
  for (const [index, view] of json.bufferViews.entries()) {
    const buffer = physical(view).buffer;
    if (!touched.has(buffer)) touched.set(buffer, { views: [], images: false });
    if (imageViews.has(index)) touched.get(buffer).images = true;
    else touched.get(buffer).views.push(index);
  }
  for (const [buffer, { views, images }] of touched) {
    if (!images) continue;
    if (rawBuffers.has(buffer)) throw Error("A raw city record shares a buffer with images.");
    const data = await bufferBytes(buffer), parts = [];
    let end = 0;
    for (const index of views.sort((a, b) => (physical(json.bufferViews[a]).byteOffset ?? 0) - (physical(json.bufferViews[b]).byteOffset ?? 0))) {
      const place = physical(json.bufferViews[index]), start = place.byteOffset ?? 0, padded = Math.ceil(place.byteLength / 4) * 4;
      parts.push(data.subarray(start, start + place.byteLength), Buffer.alloc(padded - place.byteLength));
      place.byteOffset = end;
      end += padded;
    }
    // A buffer that held only images keeps its index with a stub nothing reads.
    const bytes = end ? Buffer.concat(parts, end) : Buffer.alloc(4), uri = folder + "/gpu-" + digest(bytes) + ".bin";
    await writeImmutable(localPath(output, uri), bytes);
    produced.add(uri);
    json.buffers[buffer] = { ...json.buffers[buffer], uri, byteLength: bytes.length };
  }
  // Buffer views are referenced by accessors and WorldSpec scene records.
  const viewIds = new Map(), sourceImages = json.images ?? [];
  json.bufferViews = json.bufferViews.filter((_, index) => !imageViews.has(index) && viewIds.set(index, viewIds.size));
  const renumber = value => {
    if (!value || typeof value !== "object") return;
    for (const [key, child] of Object.entries(value)) {
      if (key === "bufferView" && Number.isInteger(child)) {
        if (!viewIds.has(child)) throw Error(`A city record references image buffer view ${child}.`);
        value[key] = viewIds.get(child);
      } else renumber(child);
    }
  };
  for (const key of Object.keys(json)) if (key !== "bufferViews" && key !== "images") renumber(json[key]);

  // Textures get buffers of their own. What a first frame needs (every unpaired
  // texture and the distant companions) is separate from the near textures a
  // viewer fetches as the camera approaches. Identical textures share a view.
  const groups = { eager: { limit: 8 << 20, parts: [], end: 0, bytes: 0 }, near: { limit: 4 << 20, parts: [], end: 0, bytes: 0 } };
  const flush = async group => {
    if (!group.end) return;
    const bytes = Buffer.concat(group.parts, group.end), uri = folder + "/gpu-" + digest(bytes) + ".bin";
    await writeImmutable(localPath(output, uri), bytes);
    produced.add(uri);
    json.buffers.push({ uri, byteLength: bytes.length });
    group.parts = []; group.end = 0;
  };
  const sharedViews = new Map();
  const place = async (texture, group) => {
    if (sharedViews.has(texture.key)) return sharedViews.get(texture.key);
    const bytes = texture.kept ? await viewBytes(sourceViews[sourceImages[texture.image].bufferView]) : await readFile(texture.path);
    const padded = Math.ceil(bytes.length / 4) * 4;
    if (group.end && group.end + padded > group.limit) await flush(group);
    const view = json.bufferViews.push({ buffer: json.buffers.length, byteOffset: group.end, byteLength: bytes.length }) - 1;
    group.parts.push(bytes, Buffer.alloc(padded - bytes.length));
    group.end += padded; group.bytes += bytes.length;
    sharedViews.set(texture.key, view);
    return view;
  };
  // A group is one open buffer at a time, so each is written before the other starts.
  const live = records.filter(record => !record.dropped), images = new Array(live.length), pairs = [];
  for (const [index, record] of live.entries()) if (!record.far) {
    const { name, mimeType } = sourceImages[record.image];
    images[index] = { ...(name === undefined ? {} : { name }), mimeType: record.kept ? mimeType : "image/ktx2", bufferView: await place(record, groups.eager) };
  }
  for (const record of live) if (record.far) {
    const image = images.push({ mimeType: "image/ktx2", bufferView: await place(record.far, groups.eager) }) - 1;
    for (const [near, definition] of json.textures.entries()) {
      if (definition.extensions[BASIS].source !== imageIds.get(record.image) || pairs.some(pair => pair.far === near)) continue;
      pairs.push({ near, far: json.textures.push({ ...structuredClone(definition), extensions: { [BASIS]: { source: image } } }) - 1 });
    }
  }
  await flush(groups.eager);
  for (const [index, record] of live.entries()) if (record.far) {
    const name = sourceImages[record.image].name;
    images[index] = { ...(name === undefined ? {} : { name }), mimeType: "image/ktx2", bufferView: await place(record, groups.near) };
  }
  await flush(groups.near);
  json.images = images;
  // Every texture reference in a staged city is a glTF slot, a finish detail or
  // the concrete grain (gpuTextureRoles), so other integers in extras name no texture.
  if (pairs.length) scene.worldSpecTextureLod = { version: 1, exclusive: true, pairs };
  const textureBytes = groups.eager.bytes + groups.near.bytes;
  const webp = json.textures.some(item => item.extensions?.[WEBP]);
  for (const list of ["extensionsUsed", "extensionsRequired"])
    json[list] = [...new Set([...(json[list] ?? []).filter(name => webp || name !== WEBP), ...(json.images.length ? [BASIS] : [])])];
  await retargetLod(json, lod, output, folder);
  if (scene.cityLod) produced.add(scene.cityLod);
  const result = entryBytes(json, tail);
  let deliveryBytes = result.length;
  for (const uri of cityCompanionUris(json)) {
    const path = localPath(output, uri);
    if (!produced.has(uri)) await copyImmutable(localPath(input, uri), path);
    deliveryBytes += (await stat(path)).size;
  }
  await mkdir(output, { recursive: true });
  const pending = destination + "." + randomUUID() + ".tmp";
  try {
    await writeFile(pending, result, { flag: "wx" });
    await rename(pending, destination);
  } finally { await rm(pending, { force: true }); }
  const removedFiles = await settlePublishedFiles(destination, json);

  // Resident sizes are computed from dimensions, formats and full mip chains.
  // BC7 and ASTC 4x4 are 16 bytes a block. three.js sends ETC1S to ETC2 on a
  // phone: 8 bytes a block when opaque, 16 with alpha.
  const resident = (texture, codec) => ({ desktop: chainBytes(texture.width, texture.height, 16),
    phone: chainBytes(texture.width, texture.height, codec === "etc1s" && !texture.alpha ? 8 : 16) });
  const byRole = {};
  const total = { images: 0, sourceImageBytes: 0, textureBytes, rgba8Bytes: 0, desktopBytes: 0, phoneBytes: 0 };
  // Before a camera approaches anything: unpaired textures and distant companions.
  const firstFrame = { textures: 0, downloadBytes: groups.eager.bytes, desktopBytes: 0, phoneBytes: 0 };
  for (const record of records) {
    const row = byRole[record.roles] ??= { images: 0, dropped: 0, sourceBytes: 0, bytes: 0, sourceTexels: 0, texels: 0, rgba8Bytes: 0,
      desktopBytes: 0, phoneBytes: 0, codecs: {}, psnr: [], alphaPsnr: [] };
    row.images++; row.sourceBytes += record.sourceBytes; row.sourceTexels += record.sourceWidth * record.sourceHeight;
    row.rgba8Bytes += chainBytes(record.sourceWidth, record.sourceHeight);
    total.images++; total.sourceImageBytes += record.sourceBytes; total.rgba8Bytes += chainBytes(record.sourceWidth, record.sourceHeight);
    if (record.dropped) { row.dropped++; continue; }
    const { desktop, phone } = resident(record, record.codec), first = resident(record.far ?? record, record.codec);
    firstFrame.textures++; firstFrame.desktopBytes += first.desktop; firstFrame.phoneBytes += first.phone;
    row.bytes += record.bytes + (record.far?.bytes ?? 0);
    row.texels += record.width * record.height; row.desktopBytes += desktop; row.phoneBytes += phone;
    row.codecs[record.profile] = (row.codecs[record.profile] ?? 0) + 1;
    if (record.psnr !== undefined) row.psnr.push(record.psnr);
    if (record.alphaPsnr !== undefined) row.alphaPsnr.push(record.alphaPsnr);
    total.desktopBytes += desktop; total.phoneBytes += phone;
  }
  for (const row of Object.values(byRole)) for (const key of ["psnr", "alphaPsnr"]) {
    const values = row[key];
    row[key] = values.length ? { mean: +(values.reduce((sum, value) => sum + value, 0) / values.length).toFixed(2), min: +Math.min(...values).toFixed(2) } : undefined;
  }
  return { source, destination, tier: tier ?? null, maps, colorCodec, sourceBytes, deliveryBytes, ...total, byRole,
    firstFrame, texturePairs: pairs.length, streamedImages: live.filter(record => record.far).length,
    encodedTextures: encoded, reusedTextures: reused, uniqueTextures: sharedViews.size,
    droppedFlatNormals: records.filter(record => record.dropped === "flat").length,
    droppedWeakNormals: records.filter(record => record.dropped === "weak").length, droppedImages: records.filter(record => record.dropped).length, removedFiles,
    workers: pool.size, encodeMs, elapsedMs: performance.now() - started,
    filledPages: records.filter(record => record.filled).length,
    records: records.map(({ path, far, materialConstants, ...record }) => far ? { ...record, far: { width: far.width, height: far.height, bytes: far.bytes } } : record) };
}
