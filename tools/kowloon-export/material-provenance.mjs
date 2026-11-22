import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { createHash } from "node:crypto";
import sharp from "sharp";

export const pigmentSignature = async (bytes) =>
  createHash("sha256")
    .update(
      await sharp(bytes)
        .resize(24, 24, { fit: "fill" })
        .ensureAlpha()
        .raw()
        .toBuffer(),
    )
    .digest("hex");
const key = (sig, m) =>
  [
    sig,
    m.alphaMode ?? "OPAQUE",
    !!m.extras?.materialSpecEdge,
    m.extras?.worldSpecOutline === false,
    !!m.emissiveTexture,
  ].join("-");
async function glb(path) {
  const data = await readFile(path),
    length = data.readUInt32LE(12);
  return {
    json: JSON.parse(data.toString("utf8", 20, 20 + length)),
    bin: data.subarray(28 + length),
  };
}
function boundsKey(j, p) {
  const a = j.accessors[p.attributes.POSITION];
  return [...a.min, ...a.max]
    .map((n) => Math.round(n * 1000))
    .concat(j.accessors[p.indices ?? p.attributes.POSITION].count)
    .join(",");
}
// Recover source chart labels from an older bake by matching native triangle
// count and millimetre bounds, then track the original packed pigment page.
export async function readSourceLabels(directory, owners) {
  const result = new Map();
  if (!directory) return result;
  for (const name of owners.filter((n) => /^building-\d+$/.test(n))) {
    const { json: native } = await glb(resolve(directory, name + ".glb"));
    const { json: baked, bin } = await glb(
      resolve(directory, name + "-pbr.glb"),
    );
    const sources = new Map(),
      pages = new Map();
    for (const m of native.meshes)
      for (const p of m.primitives) {
        const id = boundsKey(native, p),
          label = m.name?.split("/").slice(-2).join(" / ");
        if (!sources.has(id)) sources.set(id, new Set());
        sources.get(id).add(label);
      }
    for (const mesh of baked.meshes)
      for (const p of mesh.primitives) {
        const labels = sources.get(boundsKey(baked, p));
        if (!labels) continue;
        const m = baked.materials[p.material],
          img =
            baked.textures[m.pbrMetallicRoughness.baseColorTexture.index]
              .source;
        if (!pages.has(img)) {
          const v = baked.bufferViews[baked.images[img].bufferView],
            start = v.byteOffset ?? 0;
          pages.set(
            img,
            await pigmentSignature(bin.subarray(start, start + v.byteLength)),
          );
        }
        const id = key(pages.get(img), m);
        if (!result.has(id)) result.set(id, new Set());
        for (const l of labels) result.get(id).add(l);
      }
  }
  return result;
}
export function labelsFor(labels, sig, m) {
  return Array.from(labels.get(key(sig, m)) ?? []).sort();
}
