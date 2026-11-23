// Which texels of an atlas page a city's geometry actually samples, and a fill
// for the rest. Atlas pages publish unused area as white, black or (0,255,0);
// any filter that shrinks a page mixes that into the charts beside it.

const componentArrays = { 5120: Int8Array, 5121: Uint8Array, 5122: Int16Array, 5123: Uint16Array, 5125: Uint32Array, 5126: Float32Array };
const normalizers = { 5120: 127, 5121: 255, 5122: 32767, 5123: 65535 };
const textureSlots = material => [
  material.pbrMetallicRoughness?.baseColorTexture, material.pbrMetallicRoughness?.metallicRoughnessTexture,
  material.normalTexture, material.occlusionTexture, material.emissiveTexture,
].filter(Boolean);

/** Mark the texels whose centres a UV triangle covers. The ring around a chart
 * is not marked: at a reduced size it holds fill, and counting it as chart
 * would spread that fill. A triangle too thin to cover any centre (a pipe, a
 * strap) marks the texel under its centroid so the part still has a colour. */
export function markTriangle(mask, width, height, x0, y0, x1, y1, x2, y2) {
  const reach = .01;
  let marked = false;
  const left = Math.max(0, Math.floor(Math.min(x0, x1, x2) - reach)), right = Math.min(width - 1, Math.ceil(Math.max(x0, x1, x2) + reach) - 1);
  const top = Math.max(0, Math.floor(Math.min(y0, y1, y2) - reach)), bottom = Math.min(height - 1, Math.ceil(Math.max(y0, y1, y2) + reach) - 1);
  const centroid = () => {
    const x = Math.floor((x0 + x1 + x2) / 3), y = Math.floor((y0 + y1 + y2) / 3);
    if (x >= 0 && y >= 0 && x < width && y < height) mask[y * width + x] = 1;
  };
  if (right < left || bottom < top) { centroid(); return; }
  const area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0), sign = area < 0 ? -1 : 1;
  const edges = [[x0, y0, x1, y1], [x1, y1, x2, y2], [x2, y2, x0, y0]].map(([ax, ay, bx, by]) => {
    const length = Math.hypot(bx - ax, by - ay);
    return length ? [ax, ay, (bx - ax) / length, (by - ay) / length] : null;
  });
  for (let y = top; y <= bottom; y++) for (let x = left; x <= right; x++) {
    const px = x + .5, py = y + .5;
    let inside = true;
    // Signed distance to each edge, in texels; a sliver's bounding box already limits the rest.
    for (const edge of edges) if (edge && sign * ((py - edge[1]) * edge[2] - (px - edge[0]) * edge[3]) < -reach) { inside = false; break; }
    if (inside) { mask[y * width + x] = 1; marked = true; }
  }
  if (!marked) centroid();
}

/** Coverage masks from a city's own UVs. `readView(index)` returns the decoded
 * bytes of a buffer view; `imageOf(texture)` names a texture's image. */
export function chartCoverage(json, readView, imageOf) {
  const primitives = new Map(), accessors = new Map();
  for (const mesh of json.meshes ?? []) for (const part of mesh.primitives ?? []) {
    if (part.material === undefined || (part.mode ?? 4) !== 4) continue;
    if (!primitives.has(part.material)) primitives.set(part.material, []);
    primitives.get(part.material).push(part);
  }
  const read = index => {
    if (!accessors.has(index)) {
      if (accessors.size >= 64) accessors.delete(accessors.keys().next().value);
      accessors.set(index, (async () => {
        const accessor = json.accessors[index], view = json.bufferViews[accessor.bufferView], Type = componentArrays[accessor.componentType];
        const size = { SCALAR: 1, VEC2: 2 }[accessor.type];
        if (!view || accessor.sparse || !Type || !size) return null;
        const bytes = await readView(accessor.bufferView), stride = view.byteStride ?? Type.BYTES_PER_ELEMENT * size;
        const data = new DataView(bytes.buffer, bytes.byteOffset + (accessor.byteOffset ?? 0)), out = new Float64Array(accessor.count * size);
        const get = { 5120: "getInt8", 5121: "getUint8", 5122: "getInt16", 5123: "getUint16", 5125: "getUint32", 5126: "getFloat32" }[accessor.componentType];
        const scale = accessor.normalized ? 1 / normalizers[accessor.componentType] : 1;
        for (let i = 0; i < accessor.count; i++) for (let c = 0; c < size; c++)
          out[i * size + c] = data[get](i * stride + c * Type.BYTES_PER_ELEMENT, true) * scale;
        return out;
      })());
    }
    return accessors.get(index);
  };
  /** Each [material, texture info] that samples the image through a glTF slot. */
  const users = image => {
    const list = [];
    for (const [material, definition] of (json.materials ?? []).entries())
      for (const info of textureSlots(definition)) if (imageOf(json.textures[info.index]) === image) list.push([material, info]);
    return list;
  };
  /** One byte per texel, 1 where a triangle samples it; null when the image
   * tiles or is not reached through plain atlas UVs. */
  async function mask(list, width, height) {
    if (!list.length) return null;
    const result = new Uint8Array(width * height);
    for (const [material, info] of list) {
      const transform = info.extensions?.KHR_texture_transform;
      if (transform?.rotation) return null;
      const [su, sv] = transform?.scale ?? [1, 1], [ou, ov] = transform?.offset ?? [0, 0], set = transform?.texCoord ?? info.texCoord ?? 0;
      for (const part of primitives.get(material) ?? []) {
        const uvAccessor = part.attributes?.["TEXCOORD_" + set];
        if (uvAccessor === undefined) return null;
        const uv = await read(uvAccessor), indices = part.indices === undefined ? null : await read(part.indices);
        if (!uv || part.indices !== undefined && !indices) return null;
        const count = indices ? indices.length : uv.length / 2;
        for (let i = 0; i + 2 < count; i += 3) {
          const a = indices ? indices[i] : i, b = indices ? indices[i + 1] : i + 1, c = indices ? indices[i + 2] : i + 2;
          const u0 = uv[a * 2] * su + ou, v0 = uv[a * 2 + 1] * sv + ov, u1 = uv[b * 2] * su + ou, v1 = uv[b * 2 + 1] * sv + ov;
          const u2 = uv[c * 2] * su + ou, v2 = uv[c * 2 + 1] * sv + ov;
          // A coordinate outside the page means the texture repeats: there is no unused area.
          if (Math.min(u0, v0, u1, v1, u2, v2) < -.002 || Math.max(u0, v0, u1, v1, u2, v2) > 1.002) return null;
          markTriangle(result, width, height, u0 * width, v0 * height, u1 * width, v1 * height, u2 * width, v2 * height);
        }
      }
    }
    return result;
  }
  return { users, mask };
}

/** Replace unsampled texels by the colour of the charts around them (push-pull):
 * each coarser level averages only sampled texels, and unsampled ones take the
 * nearest level that has any. Sampled texels are returned untouched. */
export function fillUncharted(rgba, width, height, mask) {
  let charted = 0;
  for (let i = 0; i < mask.length; i++) charted += mask[i];
  if (!charted || charted === mask.length) return rgba;
  const levels = [];
  let colour = new Float32Array(width * height * 4), weight = new Float32Array(width * height), w = width, h = height;
  for (let i = 0; i < mask.length; i++) if (mask[i]) { weight[i] = 1; for (let c = 0; c < 4; c++) colour[i * 4 + c] = rgba[i * 4 + c]; }
  levels.push({ colour, weight, w, h });
  while (w > 1 || h > 1) {
    const nw = Math.max(1, Math.ceil(w / 2)), nh = Math.max(1, Math.ceil(h / 2)), next = new Float32Array(nw * nh * 4), total = new Float32Array(nw * nh);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const from = y * w + x, share = weight[from];
      if (!share) continue;
      const to = (y >> 1) * nw + (x >> 1);
      total[to] += share;
      for (let c = 0; c < 4; c++) next[to * 4 + c] += colour[from * 4 + c] * share;
    }
    for (let i = 0; i < total.length; i++) if (total[i]) { for (let c = 0; c < 4; c++) next[i * 4 + c] /= total[i]; total[i] = 1; }
    colour = next; weight = total; w = nw; h = nh;
    levels.push({ colour, weight, w, h });
  }
  for (let level = levels.length - 2; level >= 0; level--) {
    const fine = levels[level], coarse = levels[level + 1];
    for (let y = 0; y < fine.h; y++) for (let x = 0; x < fine.w; x++) {
      const to = y * fine.w + x;
      if (fine.weight[to]) continue;
      const from = (y >> 1) * coarse.w + (x >> 1);
      for (let c = 0; c < 4; c++) fine.colour[to * 4 + c] = coarse.colour[from * 4 + c];
      fine.weight[to] = 1;
    }
  }
  const out = Buffer.from(rgba), filled = levels[0].colour;
  for (let i = 0; i < mask.length; i++) if (!mask[i]) for (let c = 0; c < 4; c++) out[i * 4 + c] = Math.round(filled[i * 4 + c]);
  return out;
}
