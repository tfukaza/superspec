import { createHash } from "node:crypto";
import { open } from "node:fs/promises";
import { resolve } from "node:path";

const canonical = (o) =>
  Array.isArray(o)
    ? o.map(canonical)
    : o && typeof o === "object"
      ? Object.fromEntries(
          Object.keys(o)
            .sort()
            .map((k) => [k, canonical(o[k])]),
        )
      : o;
/** Compare complete texture bindings. Vertex UVs, alpha, masks, texture transforms,
 * and source-specific material behavior survive the remap. Names are kept as aliases. */
export async function coalesceBindings(json, directory) {
  const textures = new Map(),
    images = new Map(),
    samplers = new Map(),
    remap = new Map();
  for (const [i, t] of json.textures.entries()) {
    if (!images.has(t.source)) {
      const img = json.images[t.source],
        v = json.bufferViews[img.bufferView];
      if (!v) continue;
      const f = await open(resolve(directory, json.buffers[v.buffer].uri));
      try {
        const bytes = Buffer.allocUnsafe(v.byteLength);
        const { bytesRead } = await f.read(
          bytes,
          0,
          bytes.length,
          v.byteOffset ?? 0,
        );
        if (bytesRead !== bytes.length)
          throw Error("Incomplete image " + t.source);
        images.set(
          t.source,
          createHash("sha256").update(img.mimeType).update(bytes).digest("hex"),
        );
      } finally {
        await f.close();
      }
    }
    if (!samplers.has(t.sampler))
      samplers.set(
        t.sampler,
        JSON.stringify(
          canonical({
            magFilter: 9729,
            minFilter: 9987,
            wrapS: 10497,
            wrapT: 10497,
            ...json.samplers?.[t.sampler],
          }),
        ),
      );
    const id = JSON.stringify(
      canonical({
        ...t,
        name: undefined,
        source: images.get(t.source),
        sampler: samplers.get(t.sampler),
      }),
    );
    if (!textures.has(id)) textures.set(id, i);
    remap.set(i, textures.get(id));
  }
  const unique = [],
    seen = new Map(),
    aliases = [];
  for (const m of json.materials) {
    const rewrite = (o) => {
      if (!o || typeof o !== "object") return;
      for (const [k, v] of Object.entries(o)) {
        if (k.endsWith("Texture") && v && typeof v.index === "number")
          v.index = remap.get(v.index) ?? v.index;
        else rewrite(v);
      }
    };
    rewrite(m);
    const key = JSON.stringify(canonical(m));
    if (!seen.has(key)) {
      seen.set(key, unique.length);
      unique.push(m);
    }
    aliases.push(seen.get(key));
  }
  for (const mesh of json.meshes)
    for (const p of mesh.primitives) p.material = aliases[p.material];
  json.materials = unique;
  return aliases;
}
// Atlas pages were initialized to black for charts without the live edge layer.
// Verify the actual triangles (including a bilinear guard band) before sharing
// the masked PBR binding between authored edge and plain charts.
export async function mergeEdgeVariants(json, rows, uses, reader, readImage) {
  const groups = new Map(),
    remap = json.materials.map((_, i) => i);
  for (const [i, m] of json.materials.entries()) {
    if (!m.extras?.sourceName?.startsWith("ItemSpec ")) continue;
    if (
      m.occlusionTexture &&
      (!m.extras.materialSpecEdge || m.occlusionTexture.strength !== 0)
    )
      continue;
    const v = structuredClone(m);
    delete v.name;
    delete v.occlusionTexture;
    delete v.extras.materialSpecEdge;
    delete v.extras.sourceFinishes;
    delete v.extras.worldSpecSurface.label;
    const k = JSON.stringify(canonical(v));
    if (!groups.has(k)) groups.set(k, []);
    groups.get(k).push(i);
  }
  for (const group of groups.values()) {
    const donors = group.filter((i) => json.materials[i].occlusionTexture);
    const peers = group.filter((i) => !json.materials[i].occlusionTexture);
    if (donors.length !== 1 || !peers.length) continue;
    const target = donors[0],
      texture = json.materials[target].occlusionTexture.index;
    const { data, info } = await readImage(texture),
      w = info.width,
      h = info.height,
      stride = w + 1;
    const summed = new Uint32Array(stride * (h + 1));
    for (let y = 0; y < h; y++) {
      let hits = 0;
      for (let x = 0; x < w; x++) {
        const p = 4 * (y * w + x);
        hits += Number(data[p] > 0 || data[p + 1] > 0);
        summed[(y + 1) * stride + x + 1] = hits + summed[y * stride + x + 1];
      }
    }
    for (const i of peers) {
      let blank = true;
      for (const { prim } of uses[i]) {
        const uv = await reader.read(prim.attributes.TEXCOORD_0),
          idx = await reader.read(prim.indices, Uint32Array);
        for (let k = 0; k < idx.length && blank; k += 3) {
          const [a, b, c] = [2 * idx[k], 2 * idx[k + 1], 2 * idx[k + 2]];
          const x0 = Math.max(
              0,
              Math.floor(w * Math.min(uv[a], uv[b], uv[c])) - 1,
            ),
            x1 = Math.min(w, Math.ceil(w * Math.max(uv[a], uv[b], uv[c])) + 1);
          const y0 = Math.max(
              0,
              Math.floor(h * Math.min(uv[a + 1], uv[b + 1], uv[c + 1])) - 1,
            ),
            y1 = Math.min(
              h,
              Math.ceil(h * Math.max(uv[a + 1], uv[b + 1], uv[c + 1])) + 1,
            );
          blank =
            summed[y1 * stride + x1] -
              summed[y0 * stride + x1] -
              summed[y1 * stride + x0] +
              summed[y0 * stride + x0] ===
            0;
        }
        if (!blank) break;
      }
      if (blank) remap[i] = target;
    }
  }
  const unique = [],
    inventory = [],
    slots = new Map();
  for (let i = 0; i < remap.length; i++) {
    const id = remap[i];
    if (slots.has(id)) continue;
    slots.set(id, unique.length);
    unique.push(json.materials[id]);
    const members = rows.filter((r) => remap[r.index] === id);
    const regions = [
      ...new Set(members.flatMap((r) => r.regions ?? [])),
    ].sort();
    if (regions.length) json.materials[id].extras.sourceFinishes = regions;
    inventory.push({
      ...rows[id],
      index: inventory.length,
      regions,
      sourceRecords: members
        .flatMap((r) => r.sourceRecords)
        .sort((a, b) => a - b),
    });
  }
  for (const mesh of json.meshes)
    for (const p of mesh.primitives) p.material = slots.get(remap[p.material]);
  json.materials = unique;
  return inventory;
}
