import { PropertyType } from "@gltf-transform/core";
import { listTextureSlots } from "@gltf-transform/functions";
import sharp from "sharp";

const rgbSlots = new Set(["normalTexture", "metallicRoughnessTexture", "occlusionTexture", "emissiveTexture"]);

/** A texture shared with base color or an extension keeps alpha, including on edge-map aliases. */
export function atlasChannels(texture) {
  if (texture.listParents().some(parent => parent.propertyType !== PropertyType.ROOT && parent.propertyType !== PropertyType.MATERIAL))
    return "rgba";
  const slots = listTextureSlots(texture);
  return slots.length > 0 && slots.every(slot => rgbSlots.has(slot)) ? "rgb" : "rgba";
}

export async function packAtlasPng(original, channels, compressionLevel) {
  const sample = bytes => channels === "rgb" ? sharp(bytes).removeAlpha() : sharp(bytes).ensureAlpha();
  const image = channels === "rgb" ? sharp(original).removeAlpha() : sharp(original);
  const packed = await image.png({ compressionLevel, adaptiveFiltering: false }).toBuffer();
  if (packed.length >= original.length) return original;
  const decode = bytes => sample(bytes).raw().toBuffer({ resolveWithObject: true });
  const before = await decode(original), after = await decode(packed);
  if (before.info.width !== after.info.width || before.info.height !== after.info.height || !before.data.equals(after.data))
    throw Error("Lossless atlas PNG channel verification failed");
  return packed;
}
