import sharp from "sharp";
import { imageBytes } from "./city-data-textures.mjs";

// The source building publisher has already charted the UVs. Preserve that
// registration for every PBR channel, including glass alpha and lit windows.
const tile = 176, gutter = 8, pitch = tile + 2 * gutter;
const linear = x => x <= .04045 ? x / 12.92 : ((x + .055) / 1.055) ** 2.4;
const srgb = x => x <= .0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - .055;
const channels = ["color", "mr", "normal", "emission"];

function imageId(json, info) {
  if (!info) return null;
  const texture = json.textures[info.index];
  if (!texture || texture.extensions || info.extensions || info.texCoord || !json.images[texture.source])
    throw Error("Block proxy requires ordinary base UV source textures.");
  return texture.source;
}

function surface(json, m) {
  const pbr = m.pbrMetallicRoughness ?? {};
  if (!["OPAQUE", "BLEND"].includes(m.alphaMode ?? "OPAQUE") ||
      Object.keys(m.extensions ?? {}).some(k => k !== "KHR_materials_emissive_strength") ||
      (m.normalTexture?.scale ?? 1) !== 1 || m.occlusionTexture && (m.occlusionTexture.strength ?? 1) !== 0)
    throw Error("Unsupported block proxy material: " + m.name);
  const strength = m.extensions?.KHR_materials_emissive_strength?.emissiveStrength ?? 1;
  return {
    color: imageId(json, pbr.baseColorTexture), mr: imageId(json, pbr.metallicRoughnessTexture),
    normal: imageId(json, m.normalTexture), emission: imageId(json, m.emissiveTexture),
    rgba: pbr.baseColorFactor ?? [1, 1, 1, 1], roughness: pbr.roughnessFactor ?? 1,
    metallic: pbr.metallicFactor ?? 1, radiance: (m.emissiveFactor ?? [0, 0, 0]).map(v => v * strength),
  };
}

export async function bakeBlockAtlas(json, directory, materials) {
  const slots = new Map(), ids = new Map();
  for (const id of materials) {
    const s = surface(json, json.materials[id]), key = JSON.stringify(s);
    if (!slots.has(key)) slots.set(key, { index: slots.size, ...s });
    ids.set(id, slots.get(key).index);
  }
  const cols = Math.ceil(Math.sqrt(slots.size)), rows = Math.ceil(slots.size / cols);
  const width = cols * pitch, height = rows * pitch;
  if (width > 4096) throw Error("Selected block exceeds the single-atlas budget.");
  const scale = Math.max(1, ...Array.from(slots.values()).flatMap(s => s.radiance));
  const images = {};
  for (const kind of channels) {
    const atlas = Buffer.alloc(width * height * 4), decoded = new Map();
    for (const slot of slots.values()) {
      const id = slot[kind];
      if (id !== null && !decoded.has(id)) {
        const bytes = await imageBytes(json, Buffer.alloc(0), directory, json.images[id]);
        let image = sharp(bytes, { ignoreIcc: true });
        if (kind === "color" || kind === "emission") image = image.pipelineColourspace("scrgb").toColourspace("srgb");
        decoded.set(id, await image.resize(tile, tile, { fit: "fill", fastShrinkOnLoad: false })
          .toColourspace("srgb").ensureAlpha().raw().toBuffer());
      }
      const source = decoded.get(id), patch = Buffer.alloc(tile * tile * 4);
      if (source) source.copy(patch);
      else if (kind === "normal") patch.fill(Buffer.from([128, 128, 255, 255]));
      else patch.fill(255);
      const tint = kind === "emission" ? slot.radiance.map(v => v / scale) : slot.rgba;
      for (let i = 0; i < patch.length; i += 4) {
        if (kind === "color" || kind === "emission") {
          for (let c = 0; c < 3; c++) if (tint[c] !== 1)
            patch[i+c] = Math.round(255 * srgb(linear(patch[i+c] / 255) * tint[c]));
          patch[i+3] = kind === "color" ? Math.round(patch[i+3] * tint[3]) : 255;
        } else if (kind === "mr") {
          patch[i+1] = Math.round(patch[i+1] * slot.roughness);
          patch[i+2] = Math.round(patch[i+2] * slot.metallic);
        }
      }
      const left = slot.index % cols * pitch, top = Math.floor(slot.index / cols) * pitch;
      for (let y = 0; y < pitch; y++) {
        const row = Math.max(0, Math.min(tile - 1, y - gutter)) * tile * 4;
        const dest = ((top+y) * width + left) * 4;
        atlas.fill(patch.subarray(row, row + 4), dest, dest + 4*gutter);
        patch.copy(atlas, dest + 4*gutter, row, row + 4*tile);
        atlas.fill(patch.subarray(row + 4*(tile-1), row + 4*tile), dest + 4*(gutter+tile), dest + 4*pitch);
      }
    }
    images[kind] = await sharp(atlas, { raw: { width, height, channels: 4 } }).png().toBuffer();
  }
  return { images, width, height, scale, tiles: slots.size, uv(id, u, v) {
    if (!(u >= 0 && u <= 1 && v >= 0 && v <= 1)) throw Error("Block proxy requires charted UVs in [0,1].");
    const index = ids.get(id);
    return [(index % cols * pitch + gutter + u * tile) / width,
      (Math.floor(index / cols) * pitch + gutter + v * tile) / height];
  } };
}
