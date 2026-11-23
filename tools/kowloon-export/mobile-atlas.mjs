/** Merge each building's baked atlas pages into a few pages per material class, so the
 * building's surfaces share materials and draw together. The bake gives every page its
 * own material and the viewer draws each primitive on its own, so one building costs
 * dozens of draws. At a reduced tier the pages are a quarter or an eighth of their baked
 * edge, and a whole building's charts fit on one or two pages.
 *
 * Charts are found from the geometry, resampled from the full-resolution pages straight
 * to the tier's density into a padded cell, and the primitives that now share a material
 * are joined into one, with their distance levels joined level by level. Identical parts
 * can also share one chart's texels (`share`), keeping their own colour as a vertex tint. */
import { MeshoptDecoder, MeshoptEncoder } from "meshoptimizer";
import sharp from "sharp";

/** Per tier: `pageLimit` is the largest merged page edge. `gutter` is the padding on every
 * side of a chart, in texels of the merged page, and also the grid charts align to: a
 * chart keeps its own colour around it for log2(gutter) mip levels, which covers the
 * quarter-edge streaming companion at 4. Charts under four gutters across take
 * `smallGutter`: a building has tens of thousands of hardware islands a few texels
 * wide, and the full gutter would quadruple their texels. `chartJoin` joins islands
 * that lie within that many baked texels of each other, as the faces of one part do.
 * `unify` lists the material differences the tier gives up:
 *   "noEdge"        hardware loses its edge mask ("edge" instead gives every chart one);
 *   "plain"         surfaces without a finish (masonry, concrete, painted atlas) fold together;
 *   "hardware"      shop fittings take the building hardware's finish;
 *   "glazingSides"  window glass is drawn from outside only, in one pass;
 *   "glazingGlow"   lit and unlit panes share a material.
 * `mergeBlend` also merges the glazing. `linearLight` averages colour in linear light,
 * as a mip level does. `placedMeshes` names separately placed meshes whose atlas-mapped
 * primitives are first joined into one mesh (the arena's concrete), and `placedCells`
 * splits that mesh into a ground grid so it still culls and occludes by region.
 * `flatColours` joins a mesh's untextured primitives that differ only in base colour.
 * `primitiveIndexLimit` splits a merged primitive so none exceeds that many indices
 * (0: no limit). Above 36000 the viewer cuts a primitive into spatial cells and
 * simplifies and culls loose hardware by cell, sooner than it does for a whole
 * building: fewer triangles and draws, and fittings that simplify from 12 m.
 * `share.representatives` charts keep their texels per group of identical parts and
 * the rest reuse them when no more than `maxDifference` apart; 0 keeps every chart.
 * A sharing chart is tinted only when its colour is more than `sameColour` from its
 * representative's: closer than that, the difference is weathering, which it now shares. */
export const atlasTiers = {
  low: { pageLimit: 4096, gutter: 4, smallGutter: 2, chartJoin: 3, meshes: "^building-\\d+$", unify: ["noEdge", "plain", "hardware", "glazingSides", "glazingGlow"], mergeBlend: true, flatColours: true,
    linearLight: true, primitiveIndexLimit: 0, placedMeshes: "^(brush-\\d+|masonry-)", placedCells: 3, share: { representatives: 2, maxDifference: .25, sameColour: .3 } },
  "very-low": { pageLimit: 4096, gutter: 4, smallGutter: 2, chartJoin: 3, meshes: "^building-\\d+$", unify: ["noEdge", "plain", "hardware", "glazingSides", "glazingGlow"], mergeBlend: true, flatColours: true,
    linearLight: true, primitiveIndexLimit: 0, placedMeshes: "^(brush-\\d+|masonry-)", placedCells: 3, share: { representatives: 2, maxDifference: .25, sameColour: .3 } },
};

const SIGNS = /\b(?:sign|signage|lettering|poster|posters|tube-light|lens|emissive)\b/i;
const ROLES = ["base", "normal", "surface", "edge", "emissive", "detail"];
const NEUTRAL = { base: [255, 255, 255, 255], normal: [128, 128, 255, 255], surface: [0, 255, 0, 255], edge: [0, 0, 0, 255],
  emissive: [0, 0, 0, 255], detail: [128, 128, 128, 0] };
const COMPONENTS = { 5120: [1, "getInt8", 127], 5121: [1, "getUint8", 255], 5122: [2, "getInt16", 32767], 5123: [2, "getUint16", 65535],
  5125: [4, "getUint32", 1], 5126: [4, "getFloat32", 1] };
const WIDTHS = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4 };
const STRIDE = 28, CHAINS = ["levels", "detailLevels", "distantLevels"];
/** Texels of gutter the building bake leaves around every chart (GUTTER in the engine's layout). */
const BAKED_GUTTER = 2;
const pow2 = value => 2 ** Math.ceil(Math.log2(Math.max(1, value)));
const up = (value, step) => Math.ceil(value / step) * step;

function hash(text) {
  let value = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) value = Math.imul(value ^ text.charCodeAt(i), 0x01000193);
  return value >>> 0;
}

const toLinear = Float32Array.from({ length: 256 }, (_, i) => {
  const v = i / 255;
  return v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4;
});
const fromLinear = Uint8Array.from({ length: 4097 }, (_, i) => {
  const v = i / 4096;
  return Math.round(255 * (v <= .0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - .055));
});

function roleInfos(material) {
  const pbr = material.pbrMetallicRoughness ?? {};
  return { base: pbr.baseColorTexture, normal: material.normalTexture, surface: pbr.metallicRoughnessTexture,
    edge: material.occlusionTexture, emissive: material.emissiveTexture };
}

/** The material a class of charts shares, and the key that says which materials may
 * share it. Everything the viewer keys a shader program or a rendering path on stays in
 * the key: alpha mode, sides, the maps present, the finish and wear, emission, and
 * whether a sign keeps the hardware from being simplified away. */
export function materialClass(material, unify = []) {
  const merged = structuredClone(material), extras = merged.extras ??= {}, surface = extras.worldSpecSurface;
  const alpha = merged.alphaMode ?? "OPAQUE";
  const emissive = !!merged.emissiveTexture || !!merged.emissiveFactor?.some(value => value > 0);
  let sign = Array.isArray(extras.sourceFinishes) && extras.sourceFinishes.some(finish => SIGNS.test(finish));
  delete merged.name;
  delete extras.sourceName;
  delete extras.sourceFinishes;
  if (surface) delete surface.label;
  const detail = surface?.detailTexture !== undefined;
  if (surface) delete surface.detailTexture;
  if (unify.includes("hardware") && surface?.recipe === "stock-hardware" && surface.family === "Building atlases" && alpha === "OPAQUE" && !emissive) {
    // Shop fittings take the building hardware's finish and are simplified with it.
    Object.assign(surface, { recipe: "weathered-steel", finish: "weathered-steel", wear: "worn", reflection: "box" });
    delete extras.worldSpecOutline;
  }
  // One pass draws a pane's outward face; its inward face is only seen from indoors.
  if (unify.includes("glazingSides") && surface?.recipe === "architectural-glazing" && alpha === "BLEND") delete merged.doubleSided;
  if (unify.includes("noEdge") && alpha === "OPAQUE" && merged.occlusionTexture && extras.materialSpecEdge) {
    delete merged.occlusionTexture;
    delete extras.materialSpecEdge;
  }
  const plain = alpha === "OPAQUE" && !emissive && surface && surface.finish === undefined && !detail;
  if (plain && unify.includes("plain")) {
    // Without a finish nothing but grain, probes and the edge polish told these apart,
    // and the reduced tiers switch the first two off.
    merged.extras = { ...(extras.materialSpecNormalFrame ? { materialSpecNormalFrame: true } : {}),
      // Arena concrete keeps its smooth bounce light and its front-face shadow side.
      ...(extras.worldSpecConcrete && !extras.materialSpecNormalFrame ? { worldSpecConcrete: true } : {}),
      worldSpecSurface: { family: "Masonry", surface: "Concrete and masonry", recipe: "architectural-concrete", reflection: "none" } };
    delete merged.occlusionTexture;
    sign = false;
  }
  let edge;
  if (merged.occlusionTexture && merged.extras.materialSpecEdge && unify.includes("edge") && !plain) {
    edge = { occlusion: { ...merged.occlusionTexture, index: undefined }, edge: merged.extras.materialSpecEdge };
    delete merged.occlusionTexture;
    delete merged.extras.materialSpecEdge;
  }
  // Lit and unlit panes share a material: an unlit pane emits black.
  let glow;
  if (unify.includes("glazingGlow") && surface?.recipe === "architectural-glazing" && alpha === "BLEND" && merged.emissiveTexture
      && !merged.emissiveTexture.texCoord && !merged.emissiveTexture.extensions) {
    glow = { emissiveTexture: { ...merged.emissiveTexture, index: undefined }, emissiveFactor: merged.emissiveFactor, extensions: merged.extensions };
    delete merged.emissiveTexture; delete merged.emissiveFactor; delete merged.extensions;
  }
  const roles = Object.entries(roleInfos(merged)).filter(([, info]) => info).map(([role]) => role);
  for (const info of Object.values(roleInfos(merged))) if (info) {
    if (info.texCoord || info.extensions) return;
    delete info.index;
  }
  if (detail) roles.push("detail");
  const shareable = alpha === "OPAQUE" && !emissive && !sign;
  return { key: JSON.stringify([merged, detail, sign]), template: merged, roles, alpha, sign, edge, glow, shareable,
    edgeKey: edge ? JSON.stringify(edge) : "", glowKey: glow ? JSON.stringify(glow) : "" };
}

async function readAccessor(json, bytesOf, id) {
  const accessor = json.accessors[id], view = json.bufferViews[accessor.bufferView];
  if (!accessor || accessor.sparse || !view) throw Error("Unsupported city accessor " + id);
  const [size, method, divisor] = COMPONENTS[accessor.componentType], width = WIDTHS[accessor.type];
  const bytes = await bytesOf(accessor.bufferView), stride = view.byteStride ?? size * width, base = accessor.byteOffset ?? 0;
  const data = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const raw = new (accessor.componentType === 5126 ? Float32Array : accessor.componentType === 5125 ? Uint32Array : Int32Array)(accessor.count * width);
  for (let i = 0; i < accessor.count; i++) for (let c = 0; c < width; c++)
    raw[i * width + c] = data[method](base + i * stride + c * size, true);
  return { raw, width, count: accessor.count, normalized: !!accessor.normalized, divisor, type: accessor.componentType };
}

/** Group a primitive's triangles into charts: islands of connected texture coordinates,
 * joined where their texel boxes overlap. `uv` is in page units, the page is
 * `width` x `height` texels. Returns the texel box of each chart and each vertex's chart. */
export function findCharts(uv, indices, width, height) {
  const count = uv.length / 2, parent = new Int32Array(count).map((_, i) => i);
  const find = i => { while (parent[i] !== i) i = parent[i] = parent[parent[i]]; return i; };
  for (let i = 0; i < indices.length; i += 3) {
    const a = find(indices[i]), b = find(indices[i + 1]), c = find(indices[i + 2]);
    parent[b] = a; parent[find(c)] = a;
  }
  const used = new Uint8Array(count);
  for (const index of indices) used[index] = 1;
  const boxes = new Map();
  for (let i = 0; i < count; i++) {
    if (!used[i]) continue;
    const root = find(i), u = uv[i * 2] * width, v = uv[i * 2 + 1] * height;
    const box = boxes.get(root);
    if (!box) boxes.set(root, [u, v, u, v]);
    else { box[0] = Math.min(box[0], u); box[1] = Math.min(box[1], v); box[2] = Math.max(box[2], u); box[3] = Math.max(box[3], v); }
  }
  // Coordinates are quantized to a fraction of a texel. Snap a box to whole texels, and
  // only outward when it reaches well into the next one.
  const rects = [...boxes].map(([root, [u0, v0, u1, v1]]) => {
    let x0 = Math.max(0, Math.floor(u0 + .3)), y0 = Math.max(0, Math.floor(v0 + .3));
    let x1 = Math.min(width, Math.ceil(u1 - .3)), y1 = Math.min(height, Math.ceil(v1 - .3));
    if (x1 <= x0) { x0 = Math.min(width - 1, Math.max(0, Math.floor((u0 + u1) / 2))); x1 = x0 + 1; }
    if (y1 <= y0) { y0 = Math.min(height - 1, Math.max(0, Math.floor((v0 + v1) / 2))); y1 = y0 + 1; }
    return { roots: [root], x0, y0, x1, y1 };
  });
  const charts = mergeRects(rects), chartOf = new Map();
  for (const [index, chart] of charts.entries()) for (const root of chart.roots) chartOf.set(root, index);
  const vertexChart = new Int32Array(count).fill(-1);
  for (let i = 0; i < count; i++) if (used[i]) vertexChart[i] = chartOf.get(find(i));
  return { charts: charts.map(({ x0, y0, x1, y1 }) => ({ x0, y0, x1, y1 })), vertexChart };
}

/** Join rectangles that share area, or lie within `join` texels of each other, until none do. */
export function mergeRects(rects, join = 0) {
  let live = rects.map(rect => ({ ...rect, roots: [...rect.roots] }));
  for (let changed = true; changed;) {
    changed = false;
    live.sort((a, b) => a.x0 - b.x0);
    const dead = new Uint8Array(live.length);
    for (let i = 0; i < live.length; i++) {
      if (dead[i]) continue;
      const a = live[i];
      for (let j = i + 1; j < live.length && live[j].x0 < a.x1 + join; j++) {
        const b = live[j];
        if (dead[j] || b.y0 >= a.y1 + join || b.y1 + join <= a.y0) continue;
        a.x1 = Math.max(a.x1, b.x1); a.y0 = Math.min(a.y0, b.y0); a.y1 = Math.max(a.y1, b.y1);
        a.roots.push(...b.roots);
        dead[j] = 1;
        changed = true;
      }
    }
    live = live.filter((_, index) => !dead[index]);
  }
  return live;
}

/** The padding a chart gets on every side: `gutter` texels, or `smallGutter` when the
 * chart's shorter edge is under four gutters and the padding would outweigh the chart. */
export function chartGutter(slot, { gutter, smallGutter = gutter }) {
  return Math.min(slot.width, slot.height) >= 4 * gutter ? gutter : Math.min(gutter, smallGutter);
}

/** Place padded cells on shelves. Each slot is `width` x `height` texels and gets its
 * gutter on every side; its origin and its cell both sit on that gutter's grid, so a mip
 * level whose texels are no larger than the gutter never mixes two charts.
 * Pages are a power of two wide and a multiple of 64 texels high, up to `limit`. */
export function packSlots(slots, { limit, gutter, smallGutter = gutter }) {
  const cell = slot => {
    // A chart the bake already placed at this tier's density (chart-atlas.mjs) keeps its
    // rectangle and the bake's own gutter: no grid, no wider padding.
    if (slot.page?.placed) return { slot, pad: BAKED_GUTTER, grid: 1, w: slot.width + 2 * BAKED_GUTTER, h: slot.height + 2 * BAKED_GUTTER };
    const pad = chartGutter(slot, { gutter, smallGutter });
    return { slot, pad, grid: pad, w: up(slot.width, pad) + 2 * pad, h: up(up(slot.height, pad) + 2 * pad, gutter) };
  };
  const cells = slots.map(cell).sort((a, b) => b.h - a.h || b.w - a.w);
  for (const { w, h, slot } of cells) if (w > limit || h > limit) throw Error(`A ${slot.width} x ${slot.height} chart exceeds the ${limit} texel page.`);
  const area = cells.reduce((sum, { w, h }) => sum + w * h, 0), widest = cells.reduce((max, { w }) => Math.max(max, w), 0);
  const layout = width => {
    const pages = [];
    for (const item of cells) {
      let placed;
      for (const page of pages) {
        for (const shelf of page.shelves) if (shelf.h >= item.h && up(shelf.x, item.grid) + item.w <= width
            && (!placed || shelf.h < placed.shelf.h)) placed = { page, shelf };
        if (placed) break;
        if (page.height + item.h <= limit) {
          const shelf = { y: page.height, h: item.h, x: 0 };
          page.shelves.push(shelf);
          page.height += item.h;
          placed = { page, shelf };
          break;
        }
      }
      if (!placed) {
        const shelf = { y: 0, h: item.h, x: 0 }, page = { shelves: [shelf], height: item.h, items: [] };
        pages.push(page);
        placed = { page, shelf };
      }
      const x = up(placed.shelf.x, item.grid);
      placed.page.items.push({ slot: item.slot, pad: item.pad, x: x + item.pad, y: placed.shelf.y + item.pad, cell: [x, placed.shelf.y, item.w, item.h] });
      placed.shelf.x = x + item.w;
    }
    return pages.map(page => ({ width, height: Math.min(limit, up(page.height, 64)), items: page.items }));
  };
  let best;
  for (let width = Math.min(limit, Math.max(64, pow2(widest))); width <= limit; width *= 2) {
    const pages = layout(width), texels = pages.reduce((sum, page) => sum + page.width * page.height, 0);
    // Fewer pages first: each one is a draw. Then the smaller total, leaning to the squarer page.
    const score = texels * (1 + .02 * Math.abs(Math.log2(width / Math.max(1, pages[0]?.height ?? width))));
    if (!best || pages.length < best.pages.length || pages.length === best.pages.length && score < best.score) best = { pages, texels, score };
  }
  return { pages: best?.pages ?? [], texels: best?.texels ?? 0, cellTexels: area };
}

/** Area-filter the `source` rectangle (in texels of a `sw`-wide RGBA image, fractions
 * allowed) into a `dw` x `dh` block of `target` at (`dx`, `dy`). Reads stay inside the
 * rectangle. `mode` is "color" (sRGB, averaged in linear light), "alpha" (the same,
 * weighted by coverage) or "data". */
export function resample(source, sw, sh, rect, target, tw, dx, dy, dw, dh, mode = "data") {
  const spans = (from, size, count, limit) => {
    const lo = Math.max(0, Math.min(limit - 1, Math.floor(from))), hi = Math.max(lo + 1, Math.min(limit, Math.ceil(from + size)));
    const step = size / count, list = [];
    for (let i = 0; i < count; i++) {
      let a = from + i * step, b = a + step;
      if (step < 1) { a = (a + b) / 2 - .5; b = a + 1; }
      const taps = [];
      for (let t = Math.floor(a); t < Math.ceil(b); t++) {
        const weight = Math.min(b, t + 1) - Math.max(a, t);
        if (weight > 1e-6) taps.push(Math.max(lo, Math.min(hi - 1, t)), weight);
      }
      list.push(taps);
    }
    return list;
  };
  const columns = spans(rect.x, rect.width, dw, sw), rows = spans(rect.y, rect.height, dh, sh);
  const linear = mode !== "data";
  for (let j = 0; j < dh; j++) {
    const ys = rows[j];
    for (let i = 0; i < dw; i++) {
      const xs = columns[i];
      let r = 0, g = 0, b = 0, a = 0, total = 0, coverage = 0;
      for (let y = 0; y < ys.length; y += 2) for (let x = 0; x < xs.length; x += 2) {
        const at = (ys[y] * sw + xs[x]) * 4, weight = ys[y + 1] * xs[x + 1], alpha = source[at + 3];
        const w = mode === "alpha" ? weight * alpha : weight;
        if (linear) { r += toLinear[source[at]] * w; g += toLinear[source[at + 1]] * w; b += toLinear[source[at + 2]] * w; }
        else { r += source[at] * w; g += source[at + 1] * w; b += source[at + 2] * w; }
        a += alpha * weight; total += weight; coverage += w;
      }
      const out = ((dy + j) * tw + dx + i) * 4;
      if (!(coverage > 0)) {
        // Fully clear texels keep the average of the hidden colour beneath them.
        r = g = b = 0; coverage = 0;
        for (let y = 0; y < ys.length; y += 2) for (let x = 0; x < xs.length; x += 2) {
          const at = (ys[y] * sw + xs[x]) * 4, weight = ys[y + 1] * xs[x + 1];
          r += toLinear[source[at]] * weight; g += toLinear[source[at + 1]] * weight; b += toLinear[source[at + 2]] * weight; coverage += weight;
        }
      }
      if (linear) {
        target[out] = fromLinear[Math.min(4096, Math.round(r / coverage * 4096))];
        target[out + 1] = fromLinear[Math.min(4096, Math.round(g / coverage * 4096))];
        target[out + 2] = fromLinear[Math.min(4096, Math.round(b / coverage * 4096))];
      } else {
        target[out] = Math.round(r / coverage); target[out + 1] = Math.round(g / coverage); target[out + 2] = Math.round(b / coverage);
      }
      target[out + 3] = Math.round(a / total);
    }
  }
}

/** Repeat each chart's border through its cell, then give the unused page the mean. */
export function fillCells(target, page, fallback) {
  const { width } = page, row = (y, x0, x1) => target.subarray((y * width + x0) * 4, (y * width + x1) * 4);
  const owned = new Uint8Array(width * page.height);
  const sum = [0, 0, 0, 0];
  let texels = 0;
  for (const { slot, x, y, cell: [cx, cy, cw, ch] } of page.items) {
    const w = slot.width, h = slot.height;
    for (let j = y; j < y + h; j++) {
      const first = (j * width + x) * 4, last = (j * width + x + w - 1) * 4;
      for (let i = cx; i < x; i++) target.copyWithin((j * width + i) * 4, first, first + 4);
      for (let i = x + w; i < cx + cw; i++) target.copyWithin((j * width + i) * 4, last, last + 4);
      for (let i = first; i <= last; i += 4) { sum[0] += target[i]; sum[1] += target[i + 1]; sum[2] += target[i + 2]; sum[3] += target[i + 3]; }
    }
    texels += w * h;
    for (let j = cy; j < y; j++) row(j, cx, cx + cw).set(row(y, cx, cx + cw));
    for (let j = y + h; j < cy + ch; j++) row(j, cx, cx + cw).set(row(y + h - 1, cx, cx + cw));
    for (let j = cy; j < cy + ch; j++) owned.fill(1, j * width + cx, j * width + cx + cw);
  }
  const mean = new Uint32Array(Uint8Array.from(texels ? sum.map(value => Math.round(value / texels)) : fallback).buffer)[0];
  const words = new Uint32Array(target.buffer, target.byteOffset, owned.length);
  for (let i = 0; i < owned.length; i++) if (!owned[i]) words[i] = mean;
}

/** Halve a block of RGBA texels `factor` times over by plain averaging. */
function shrink(source, width, height, factor) {
  if (factor === 1) return { data: source, width, height };
  const w = width / factor, h = height / factor, data = new Uint8Array(w * h * 4);
  resample(source, width, height, { x: 0, y: 0, width, height }, data, w, 0, 0, w, h, "data");
  return { data, width: w, height: h };
}

function decodeStream(level, vertexCount) {
  const data = new (level.componentBytes === 2 ? Uint16Array : Uint32Array)(level.count);
  if (level.count) MeshoptDecoder.decodeIndexBuffer(new Uint8Array(data.buffer), data.length, level.componentBytes, level.stream);
  for (const index of data) if (index >= vertexCount) throw Error("City LOD index out of range.");
  return data;
}

/** Join the distance levels of primitives that become one. `members` are in vertex
 * order: `{ entry, indices, vertexCount }`, where a member without an entry draws its
 * full triangles at every level. Level i of the result is level i of every member (its
 * last one if it has fewer), and carries the largest of their errors. */
export function mergeLodEntries(members) {
  const lists = members.map(({ entry }) => entry && (entry.detailLevels ?? [...entry.levels ?? [], ...entry.distantLevels ?? []]));
  const length = Math.max(0, ...lists.map(list => list?.length ?? 0));
  const vertexCount = members.reduce((sum, member) => sum + member.vertexCount, 0);
  const nearCount = members.reduce((sum, member) => sum + member.indices.length, 0);
  if (length < 2) return;
  const width = vertexCount <= 65536 ? 2 : 4, decoded = new Map();
  let floor = 0;
  const merged = Array.from({ length }, (_, index) => {
    const picks = lists.map(list => list?.[Math.min(index, list.length - 1)]);
    const count = picks.reduce((sum, level, member) => sum + (level ? level.count : members[member].indices.length), 0);
    floor = Math.max(floor, ...picks.map(level => level?.errorMeters ?? 0));
    if (picks.every((level, member) => !level || !level.stream && level.count === members[member].indices.length))
      return { count: nearCount, componentBytes: width, errorMeters: floor };
    const data = new (width === 2 ? Uint16Array : Uint32Array)(count);
    let at = 0, base = 0;
    for (const [member, level] of picks.entries()) {
      const { indices, vertexCount: vertices } = members[member];
      let source = indices;
      if (level?.stream) {
        if (!decoded.has(level)) decoded.set(level, decodeStream(level, vertices));
        source = decoded.get(level);
      } else if (level && level.count !== indices.length) source = [];
      for (let i = 0; i < source.length; i++) data[at++] = source[i] + base;
      base += vertices;
    }
    if (at !== count) throw Error("City LOD level lost triangles in an atlas merge.");
    return { count, componentBytes: width, errorMeters: floor,
      stream: count ? MeshoptEncoder.encodeIndexBuffer(new Uint8Array(data.buffer), count, width) : undefined };
  });
  if (!merged.some(level => level.count < nearCount)) return;
  const entry = { vertexCount, nearCount, levels: merged.slice(0, 2) };
  if (lists.some((list, member) => list && members[member].entry.detailLevels)) entry.detailLevels = merged;
  else if (merged.length > 2) entry.distantLevels = merged.slice(2);
  return entry;
}

const samplerOf = (json, info) => json.textures[info.index].sampler;
const imageOf = (json, texture) => {
  const item = json.textures[texture];
  return item.extensions?.EXT_texture_webp?.source ?? item.source;
};

function nodeMatrix(node) {
  if (node.matrix) return node.matrix;
  const [x, y, z, w] = node.rotation ?? [0, 0, 0, 1], [sx, sy, sz] = node.scale ?? [1, 1, 1], [tx, ty, tz] = node.translation ?? [0, 0, 0];
  return [(1 - 2 * (y * y + z * z)) * sx, 2 * (x * y + z * w) * sx, 2 * (x * z - y * w) * sx, 0,
    2 * (x * y - z * w) * sy, (1 - 2 * (x * x + z * z)) * sy, 2 * (y * z + x * w) * sy, 0,
    2 * (x * z + y * w) * sz, 2 * (y * z - x * w) * sz, (1 - 2 * (x * x + y * y)) * sz, 0, tx, ty, tz, 1];
}
const multiply = (a, b) => Array.from({ length: 16 }, (_, i) => {
  const column = i >> 2, row = i & 3;
  return a[row] * b[column * 4] + a[4 + row] * b[column * 4 + 1] + a[8 + row] * b[column * 4 + 2] + a[12 + row] * b[column * 4 + 3];
});

/** Bring the atlas-mapped primitives of separately placed meshes (`pattern`) into one mesh
 * under one new node, with each placement's transform applied to its vertices, so their
 * pages can merge as a building's do. Primitives with tiling coordinates, tangents or no
 * texture stay where they are, as do instanced, animated and declared-LOD placements.
 * Returns the new mesh, or nothing when fewer than two primitives qualify. */
export async function joinPlacedMeshes(json, { pattern, bytesOf, replacement, unify = [], name = "atlas/placed" }) {
  const scene = json.scenes[json.scene ?? 0], world = new Map(), users = new Map(), alternates = new Set();
  const visit = (id, parent) => {
    const node = json.nodes[id], matrix = parent ? multiply(parent, nodeMatrix(node)) : nodeMatrix(node);
    world.set(id, matrix);
    for (const child of node.children ?? []) visit(child, matrix);
  };
  for (const root of scene.nodes ?? []) visit(root);
  for (const [id, node] of json.nodes.entries()) {
    if (node.mesh !== undefined) users.set(node.mesh, [...(users.get(node.mesh) ?? []), id]);
    for (const alternate of node.extensions?.MSFT_lod?.ids ?? []) alternates.add(alternate);
  }
  const picked = [];
  for (const [id, node] of json.nodes.entries()) {
    const mesh = json.meshes[node.mesh];
    if (!mesh || !pattern.test(mesh.name ?? "") || !world.has(id) || users.get(node.mesh).length !== 1 || node.extensions || node.skin !== undefined
        || node.children?.length || alternates.has(id) || mesh.extras || mesh.weights) continue;
    const m = world.get(id), determinant = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[4] * (m[1] * m[10] - m[2] * m[9]) + m[8] * (m[1] * m[6] - m[2] * m[5]);
    if (!(determinant > 0)) continue;
    for (const primitive of mesh.primitives) {
      const material = json.materials[primitive.material], attributes = Object.keys(primitive.attributes).sort().join();
      if (!material || (primitive.mode ?? 4) !== 4 || primitive.indices === undefined || primitive.targets || primitive.extras
          || !["COLOR_0,NORMAL,POSITION,TEXCOORD_0", "NORMAL,POSITION,TEXCOORD_0"].includes(attributes)
          || !material.pbrMetallicRoughness?.baseColorTexture || material.pbrMetallicRoughness.baseColorFactor || material.extensions
          || !materialClass(material, unify)) continue;
      const [uv, position, normal, index] = await Promise.all([primitive.attributes.TEXCOORD_0, primitive.attributes.POSITION, primitive.attributes.NORMAL,
        primitive.indices].map(accessor => readAccessor(json, bytesOf, accessor)));
      const color = primitive.attributes.COLOR_0 !== undefined ? await readAccessor(json, bytesOf, primitive.attributes.COLOR_0) : undefined;
      const scaled = (data, i) => data.normalized ? Math.max(-1, data.raw[i] / data.divisor) : data.raw[i];
      const coordinates = Float64Array.from(uv.raw, (_, i) => scaled(uv, i));
      if (coordinates.some(value => !(value >= -1e-4 && value <= 1 + 1e-4)) || color && (color.type !== 5121 || !color.normalized || color.width < 3)) continue;
      // Normals follow the inverse transpose, which for a rigid or evenly scaled placement is the placement itself.
      const a = (row, column) => m[column * 4 + row];
      const c = [a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1), a(1, 2) * a(2, 0) - a(1, 0) * a(2, 2), a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0),
        a(0, 2) * a(2, 1) - a(0, 1) * a(2, 2), a(0, 0) * a(2, 2) - a(0, 2) * a(2, 0), a(0, 1) * a(2, 0) - a(0, 0) * a(2, 1),
        a(0, 1) * a(1, 2) - a(0, 2) * a(1, 1), a(0, 2) * a(1, 0) - a(0, 0) * a(1, 2), a(0, 0) * a(1, 1) - a(0, 1) * a(1, 0)];
      const points = new Float64Array(position.count * 3), normals = new Float64Array(position.count * 3);
      for (let i = 0; i < position.count; i++) {
        const x = scaled(position, i * 3), y = scaled(position, i * 3 + 1), z = scaled(position, i * 3 + 2);
        const nx = scaled(normal, i * 3), ny = scaled(normal, i * 3 + 1), nz = scaled(normal, i * 3 + 2);
        for (let k = 0; k < 3; k++) points[i * 3 + k] = m[k] * x + m[4 + k] * y + m[8 + k] * z + m[12 + k];
        const n = [c[0] * nx + c[1] * ny + c[2] * nz, c[3] * nx + c[4] * ny + c[5] * nz, c[6] * nx + c[7] * ny + c[8] * nz], length = Math.hypot(...n) || 1;
        for (let k = 0; k < 3; k++) normals[i * 3 + k] = n[k] / length;
      }
      picked.push({ node, mesh, primitive, points, normals, coordinates, color, index: index.raw, count: position.count });
    }
  }
  if (picked.length < 2) return;
  const min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
  for (const { points } of picked) for (let i = 0; i < points.length; i++) { min[i % 3] = Math.min(min[i % 3], points[i]); max[i % 3] = Math.max(max[i % 3], points[i]); }
  const center = min.map((value, k) => (value + max[k]) / 2), half = Math.max(1e-6, ...max.map((value, k) => (value - min[k]) / 2));
  const joined = { name, primitives: [] };
  for (const item of picked) {
    const vertices = Buffer.alloc(item.count * STRIDE), low = [32767, 32767, 32767], high = [-32767, -32767, -32767];
    for (let i = 0; i < item.count; i++) {
      const at = i * STRIDE;
      for (let k = 0; k < 3; k++) {
        const value = Math.round((item.points[i * 3 + k] - center[k]) / half * 32767);
        vertices[at + k] = item.color ? item.color.raw[i * item.color.width + k] : 255;
        vertices.writeInt16LE(value, at + 8 + k * 2);
        vertices.writeInt16LE(Math.round(item.normals[i * 3 + k] * 32767), at + 16 + k * 2);
        low[k] = Math.min(low[k], value); high[k] = Math.max(high[k], value);
      }
      vertices.writeFloatLE(1, at + 4);
      vertices.writeUInt16LE(Math.round(Math.max(0, Math.min(1, item.coordinates[i * 2])) * 65535), at + 24);
      vertices.writeUInt16LE(Math.round(Math.max(0, Math.min(1, item.coordinates[i * 2 + 1])) * 65535), at + 26);
    }
    const wide = item.count > 65536, indices = (wide ? Uint32Array : Uint16Array).from(item.index);
    const buffer = json.bufferViews[json.accessors[item.primitive.attributes.POSITION].bufferView].buffer;
    const vertexView = json.bufferViews.push({ buffer, byteLength: vertices.length, byteStride: STRIDE, target: 34962 }) - 1;
    replacement.set(vertexView, { bytes: vertices, mode: "ATTRIBUTES", stride: STRIDE });
    const indexBytes = Buffer.from(indices.buffer, indices.byteOffset, indices.byteLength);
    const indexView = json.bufferViews.push({ buffer, byteLength: indexBytes.length, target: 34963 }) - 1;
    replacement.set(indexView, { bytes: indexBytes, mode: "TRIANGLES", stride: wide ? 4 : 2 });
    const accessor = (type, componentType, byteOffset, extra = {}) => json.accessors.push({ type, componentType, count: item.count, bufferView: vertexView, byteOffset, ...extra }) - 1;
    joined.primitives.push({ mode: 4, material: item.primitive.material, attributes: { COLOR_0: accessor("VEC3", 5121, 0, { normalized: true }),
      _MS_ASPECT: accessor("SCALAR", 5126, 4), POSITION: accessor("VEC3", 5122, 8, { normalized: true, min: low, max: high }),
      NORMAL: accessor("VEC3", 5122, 16, { normalized: true }), TEXCOORD_0: accessor("VEC2", 5123, 24, { normalized: true }) },
      indices: json.accessors.push({ type: "SCALAR", componentType: wide ? 5125 : 5123, count: indices.length, bufferView: indexView, byteOffset: 0 }) - 1 });
    item.mesh.primitives = item.mesh.primitives.filter(primitive => primitive !== item.primitive);
    if (!item.mesh.primitives.length) delete item.node.mesh;
  }
  // A mesh left without primitives is no longer placed; the publisher prunes it.
  const meshIndex = json.meshes.push(joined) - 1;
  scene.nodes.push(json.nodes.push({ name: picked[0].node.name ?? name, mesh: meshIndex, translation: center, scale: [half, half, half] }) - 1);
  json.extensionsUsed = [...new Set([...(json.extensionsUsed ?? []), "KHR_mesh_quantization"])];
  return joined;
}

/** Join a mesh's untextured primitives whose materials differ only in base colour: the
 * colour moves into the vertices, where the viewer multiplies it exactly as it did the
 * factor. Returns how many primitives went away. */
export async function mergeFlatColours(json, mesh, { bytesOf, replacement, lods }) {
  const groups = new Map();
  for (const primitive of mesh.primitives) {
    const material = json.materials[primitive.material], pbr = material?.pbrMetallicRoughness ?? {}, attributes = Object.keys(primitive.attributes).sort().join();
    if (!material || (primitive.mode ?? 4) !== 4 || primitive.indices === undefined || primitive.targets || (material.alphaMode ?? "OPAQUE") !== "OPAQUE"
        || pbr.baseColorTexture || pbr.metallicRoughnessTexture || material.normalTexture || material.occlusionTexture || material.emissiveTexture
        || material.extensions || material.name?.startsWith("builtin:") || !["NORMAL,POSITION", "COLOR_0,NORMAL,POSITION"].includes(attributes)) continue;
    const [position, normal] = [json.accessors[primitive.attributes.POSITION], json.accessors[primitive.attributes.NORMAL]];
    if (position.componentType !== 5122 || ![5120, 5122].includes(normal.componentType) || position.sparse || normal.sparse) continue;
    const shared = structuredClone(material);
    delete shared.name;
    delete shared.pbrMetallicRoughness?.baseColorFactor;
    if (shared.extras?.worldSpecSurface) delete shared.extras.worldSpecSurface.label;
    const key = JSON.stringify([shared, primitive.extras ?? null, !!position.normalized, normal.componentType, !!normal.normalized]);
    (groups.get(key) ?? groups.set(key, { shared, members: [] }).get(key)).members.push({ primitive, material });
  }
  let removed = 0;
  for (const { shared, members } of groups.values()) {
    if (members.length < 2) continue;
    const blocks = [];
    for (const member of members) {
      const a = member.primitive.attributes, [position, normal, index] = await Promise.all([a.POSITION, a.NORMAL, member.primitive.indices].map(id => readAccessor(json, bytesOf, id)));
      const color = a.COLOR_0 !== undefined ? await readAccessor(json, bytesOf, a.COLOR_0) : undefined;
      if (color && (color.type !== 5121 || !color.normalized)) { blocks.length = 0; break; }
      blocks.push({ ...member, position, normal, color, index: index.raw, factor: member.material.pbrMetallicRoughness?.baseColorFactor ?? [1, 1, 1, 1] });
    }
    if (!blocks.length) continue;
    const vertexCount = blocks.reduce((sum, block) => sum + block.position.count, 0), wideNormal = blocks[0].normal.type === 5122, stride = wideNormal ? 20 : 16;
    const vertices = Buffer.alloc(vertexCount * stride), min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
    const indices = new (vertexCount > 65536 ? Uint32Array : Uint16Array)(blocks.reduce((sum, block) => sum + block.index.length, 0));
    let base = 0, at = 0;
    for (const { position, normal, color, index, factor } of blocks) {
      for (let vertex = 0; vertex < position.count; vertex++) for (let k = 0; k < 3; k++) {
        const offset = (base + vertex) * stride, value = position.raw[vertex * 3 + k];
        vertices[offset + k] = Math.min(255, Math.round((color ? color.raw[vertex * color.width + k] : 255) * factor[k]));
        vertices.writeInt16LE(value, offset + 4 + k * 2);
        if (wideNormal) vertices.writeInt16LE(normal.raw[vertex * 3 + k], offset + 12 + k * 2);
        else vertices.writeInt8(normal.raw[vertex * 3 + k], offset + 12 + k);
        min[k] = Math.min(min[k], value); max[k] = Math.max(max[k], value);
      }
      for (let i = 0; i < index.length; i++) indices[at++] = index[i] + base;
      base += position.count;
    }
    const first = blocks[0].primitive, buffer = json.bufferViews[json.accessors[first.attributes.POSITION].bufferView].buffer;
    const vertexView = json.bufferViews.push({ buffer, byteLength: vertices.length, byteStride: stride, target: 34962 }) - 1;
    replacement.set(vertexView, { bytes: vertices, mode: "ATTRIBUTES", stride });
    const indexBytes = Buffer.from(indices.buffer, indices.byteOffset, indices.byteLength);
    const indexView = json.bufferViews.push({ buffer, byteLength: indexBytes.length, target: 34963 }) - 1;
    replacement.set(indexView, { bytes: indexBytes, mode: "TRIANGLES", stride: indices.BYTES_PER_ELEMENT });
    const accessor = (type, componentType, byteOffset, extra = {}) => json.accessors.push({ type, componentType, count: vertexCount, bufferView: vertexView, byteOffset, ...extra }) - 1;
    const material = { ...shared, name: `${mesh.name} / ${blocks[0].material.extras?.worldSpecSurface?.recipe ?? "flat"} colours` };
    if (material.extras?.worldSpecSurface) material.extras.worldSpecSurface.label = material.name;
    const primitive = { mode: 4, material: json.materials.push(material) - 1, attributes: { COLOR_0: accessor("VEC3", 5121, 0, { normalized: true }),
      POSITION: accessor("VEC3", 5122, 4, { normalized: blocks[0].position.normalized || undefined, min, max }),
      NORMAL: accessor("VEC3", blocks[0].normal.type, 12, { normalized: blocks[0].normal.normalized || undefined }) },
      indices: json.accessors.push({ type: "SCALAR", componentType: vertexCount > 65536 ? 5125 : 5123, count: indices.length, bufferView: indexView, byteOffset: 0 }) - 1,
      ...(first.extras ? { extras: structuredClone(first.extras) } : {}) };
    const entry = mergeLodEntries(blocks.map(block => ({ entry: lods.get(block.primitive), indices: block.index, vertexCount: block.position.count })));
    if (entry) lods.set(primitive, entry);
    const gone = new Set(blocks.map(block => block.primitive));
    for (const block of blocks) lods.delete(block.primitive);
    mesh.primitives = mesh.primitives.flatMap(item => item === first ? [primitive] : gone.has(item) ? [] : [item]);
    removed += blocks.length - 1;
  }
  return removed;
}

/** Merge the atlas pages of every mesh `tier.meshes` names. Mutates `json`; new vertex
 * and index streams go into `replacement` as the mobile geometry's do, finished pages
 * are returned in `pages` (bufferView -> encoded image, already at the tier's density)
 * and the joined distance levels replace their members' in `lods`. `tier.dryRun` packs
 * and reports without painting or changing a primitive; `measure` adds each chart's
 * identity, mean colour and brightness picture to the report, for sharing surveys. */
export async function mergeMobileAtlases(json, { tier, read, replacement = new Map(), lods = new Map(), textureSize, measure = false }) {
  await Promise.all([MeshoptDecoder.ready, MeshoptEncoder.ready]);
  const bytesOf = async view => replacement.get(view)?.bytes ?? await read(view);
  const pages = new Map(), report = { groups: [], primitivesBefore: 0, primitivesAfter: 0, materialsBefore: 0, materialsAfter: 0,
    sourceTexels: 0, chartTexels: 0, cellTexels: 0, pageTexels: 0, pages: 0, skipped: 0, charts: 0, sharedCharts: 0, tintClamped: 0 };
  const pattern = new RegExp(tier.meshes), unify = tier.unify ?? [];
  const sizes = new Map();
  const imageSize = async image => {
    if (!sizes.has(image)) {
      const { width, height } = await sharp(await bytesOf(json.images[image].bufferView)).metadata();
      const final = textureSize(width, height, image);
      sizes.set(image, { width, height, finalWidth: final.width, finalHeight: final.height });
    }
    return sizes.get(image);
  };
  const decoded = new Map();
  const decode = async image => {
    if (!decoded.has(image)) {
      if (decoded.size >= 3) decoded.delete(decoded.keys().next().value);
      decoded.set(image, sharp(await bytesOf(json.images[image].bufferView)).ensureAlpha().raw().toBuffer({ resolveWithObject: true })
        .then(({ data, info }) => ({ data, width: info.width, height: info.height })));
    }
    return decoded.get(image);
  };
  const materialsBefore = new Set(), materialsAfter = new Set();
  // Separately placed meshes with atlas pages of their own (the arena's concrete) first become one mesh.
  const placed = tier.placedMeshes ? await joinPlacedMeshes(json, { pattern: new RegExp(tier.placedMeshes), bytesOf, replacement, unify }) : undefined;
  report.placedPrimitives = placed?.primitives.length ?? 0;
  report.flatColourPrimitives = 0;
  if (tier.flatColours) for (const mesh of json.meshes) if (!mesh.extras?.worldSpecDeclaredLod)
    report.flatColourPrimitives += await mergeFlatColours(json, mesh, { bytesOf, replacement, lods });

  for (const mesh of json.meshes) {
    if (mesh !== placed && !pattern.test(mesh.name ?? "")) continue;
    report.primitivesBefore += mesh.primitives.length;
    // 1. Sort the primitives into classes.
    const groups = new Map();
    for (const primitive of mesh.primitives) {
      materialsBefore.add(primitive.material);
      const material = json.materials[primitive.material];
      const attributes = Object.keys(primitive.attributes).sort().join();
      const cls = material && (primitive.mode ?? 4) === 4 && primitive.indices !== undefined && !primitive.targets
        && attributes === "COLOR_0,NORMAL,POSITION,TEXCOORD_0,_MS_ASPECT" && !material.extensions?.MSFT_lod && material.pbrMetallicRoughness?.baseColorTexture
        && !material.pbrMetallicRoughness.baseColorFactor && materialClass(material, unify);
      if (!cls || cls.alpha !== "OPAQUE" && !tier.mergeBlend) { report.skipped++; continue; }
      // The outline exemption went with the shop fittings' own finish.
      if (unify.includes("hardware") && primitive.extras?.worldSpecOutline === false && !cls.template.extras?.worldSpecOutline
          && json.materials[primitive.material].extras?.worldSpecOutline === false) {
        delete primitive.extras.worldSpecOutline;
        if (!Object.keys(primitive.extras).length) delete primitive.extras;
      }
      const key = cls.key + JSON.stringify(primitive.extras ?? null);
      if (!groups.has(key)) groups.set(key, { cls, members: [] });
      groups.get(key).members.push({ primitive, material, cls });
    }
    const removed = new Set(), inserted = new Map();
    for (const group of groups.values()) {
      // An edge mask needs one set of edge parameters for the class.
      const edges = new Set(group.members.map(member => member.cls.edgeKey).filter(Boolean));
      if (edges.size > 1) { report.skipped += group.members.length; continue; }
      const edge = group.members.find(member => member.cls.edge)?.cls.edge;
      if (new Set(group.members.map(member => member.cls.glowKey).filter(Boolean)).size > 1) { report.skipped += group.members.length; continue; }
      const glow = group.members.find(member => member.cls.glow)?.cls.glow;
      const roles = [...new Set(group.members.flatMap(member => member.cls.roles).concat(edge ? ["edge"] : [], glow ? ["emissive"] : []))]
        .sort((a, b) => ROLES.indexOf(a) - ROLES.indexOf(b));
      // 2. Read the geometry and find the charts on each source page.
      const sourcePages = new Map(), members = [];
      for (const member of group.members) {
        const { primitive, material } = member, a = primitive.attributes;
        const [uv, position, normal, color, aspect, index] = await Promise.all([a.TEXCOORD_0, a.POSITION, a.NORMAL, a.COLOR_0, a._MS_ASPECT,
          primitive.indices].map(id => readAccessor(json, bytesOf, id)));
        const infos = roleInfos(material), base = imageOf(json, infos.base.index), size = await imageSize(base);
        const coordinates = Float64Array.from(uv.raw, value => uv.normalized ? value / uv.divisor : value);
        if (position.type !== 5122 || normal.type !== 5122 || color.type !== 5121 || color.width !== 3 || !color.normalized
            || ![5123, 5125].includes(index.type) || coordinates.some(value => !(value >= -1e-4 && value <= 1 + 1e-4))) {
          report.skipped++;
          continue;
        }
        const sources = { base };
        for (const role of ["normal", "surface", "edge", "emissive"]) if (infos[role]) sources[role] = imageOf(json, infos[role].index);
        const detail = material.extras?.worldSpecSurface?.detailTexture;
        if (detail !== undefined) sources.detail = imageOf(json, detail);
        Object.assign(member, { uv: coordinates, position, normal, color, index: index.raw,
          aspect: Float32Array.from(aspect.raw, value => aspect.normalized ? value / aspect.divisor : value), size, sources,
          found: findCharts(coordinates, index.raw, size.width, size.height) });
        // Charts on one page meet across primitives when two materials sampled the same texels.
        const pageKey = ROLES.map(role => sources[role] ?? "").join();
        // Pages baked at the tier's density are delivered at their own size: their charts are already placed.
        const placed = material.extras?.worldSpecTexelDensity !== undefined && size.finalWidth === size.width && size.finalHeight === size.height;
        if (!sourcePages.has(pageKey)) sourcePages.set(pageKey, { sources, size, placed, rects: [], members: [] });
        const page = sourcePages.get(pageKey);
        member.page = page;
        member.first = page.rects.length;
        page.rects.push(...member.found.charts.map((chart, local) => ({ ...chart, roots: [page.rects.length + local] })));
        page.members.push(member);
        members.push(member);
      }
      if (members.length < 2 && !measure && !(tier.share?.representatives > 0)) { report.skipped += members.length; continue; }
      if (!members.length) continue;
      const charts = [];
      for (const page of sourcePages.values()) {
        const joined = mergeRects(page.rects, page.placed ? 0 : tier.chartJoin ?? 0), chartOf = new Map();
        for (const rect of joined) {
          const chart = { page, x0: rect.x0, y0: rect.y0, x1: rect.x1, y1: rect.y1, triangles: 0, area: 0, uvArea: 0, center: [0, 0, 0] };
          charts.push(chart);
          for (const root of rect.roots) chartOf.set(root, chart);
        }
        for (const member of page.members) member.charts = member.found.charts.map((_, local) => chartOf.get(member.first + local));
      }
      // 3. Describe each chart by what it is, so identical parts can share texels.
      for (const member of members) {
        const { index, position: { raw: p }, uv, found: { vertexChart } } = member;
        for (let i = 0; i < index.length; i += 3) {
          const a = index[i], b = index[i + 1], c = index[i + 2], chart = member.charts[vertexChart[a]];
          const ux = p[b * 3] - p[a * 3], uy = p[b * 3 + 1] - p[a * 3 + 1], uz = p[b * 3 + 2] - p[a * 3 + 2];
          const vx = p[c * 3] - p[a * 3], vy = p[c * 3 + 1] - p[a * 3 + 1], vz = p[c * 3 + 2] - p[a * 3 + 2];
          chart.triangles++;
          chart.area += Math.hypot(uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx) / 2;
          chart.uvArea += Math.abs((uv[b * 2] - uv[a * 2]) * (uv[c * 2 + 1] - uv[a * 2 + 1]) - (uv[c * 2] - uv[a * 2]) * (uv[b * 2 + 1] - uv[a * 2 + 1])) / 2;
          for (let k = 0; k < 3; k++) chart.center[k] += p[a * 3 + k] + p[b * 3 + k] + p[c * 3 + k];
          // Where the triangles lie on the chart: two charts of one size share texels only
          // if they cover the same part of it. A mirrored triangle covers the other half.
          const { size } = member, w = chart.x1 - chart.x0, h = chart.y1 - chart.y0;
          for (const vertex of [a, b, c]) chart.layout = (chart.layout ?? 0) + hash(Math.round((uv[vertex * 2] * size.width - chart.x0) / w * 8) + ","
            + Math.round((uv[vertex * 2 + 1] * size.height - chart.y0) / h * 8)) >>> 0;
        }
      }
      for (const chart of charts) {
        const { size } = chart.page, sw = chart.x1 - chart.x0, sh = chart.y1 - chart.y0;
        chart.width = Math.max(1, Math.ceil(sw * size.finalWidth / size.width - 1e-6));
        chart.height = Math.max(1, Math.ceil(sh * size.finalHeight / size.height - 1e-6));
        const filled = chart.uvArea * size.width * size.height / (sw * sh);
        chart.identity = [sw, sh, chart.triangles, Math.round(20 * Math.log2(chart.area + 1)), Math.round(50 * filled), chart.layout].join();
        chart.order = hash(chart.identity + "|" + chart.center.map(value => Math.round(value / (3 * chart.triangles))).join());
        chart.slot = chart;
        report.sourceTexels += sw * sh;
      }
      // 4. Choose the charts that keep their own texels.
      const share = group.cls.shareable && tier.share?.representatives > 0 ? tier.share : undefined;
      if (share || measure) await describeCharts(charts, decode);
      if (share) {
        const families = new Map();
        for (const chart of charts) (families.get(chart.identity) ?? families.set(chart.identity, []).get(chart.identity)).push(chart);
        for (const family of families.values()) {
          family.sort((a, b) => a.order - b.order);
          // A tint can only darken, so the brightest charts keep their texels.
          const brightness = chart => chart.mean[0] + chart.mean[1] + chart.mean[2];
          const kept = family.toSorted((a, b) => brightness(b) - brightness(a)).slice(0, share.representatives);
          for (const chart of family) {
            if (kept.includes(chart)) continue;
            const pick = kept[chart.order % kept.length], target = brightness(pick) >= brightness(chart) ? pick : kept[0];
            if (chartDifference(chart, target) <= share.maxDifference) chart.slot = target;
          }
        }
      }
      // Where charts meet in space their tints meet too: a vertex takes the mean tint of
      // every chart at its position, so a shared chart shades into its neighbours.
      const ramp = new Map(), one = [1, 1, 1];
      // A tint carries a different pigment. Charts within `sameColour` of each other differ
      // only in how they weathered, and take the shared weathering untinted.
      const tintOf = chart => {
        if (chart.slot === chart) return one;
        if (!chart.tint) {
          const ratio = chart.mean.map((value, k) => value / Math.max(1e-4, chart.slot.mean[k]));
          chart.tint = ratio.some(value => Math.abs(value - 1) > (share?.sameColour ?? 0)) ? ratio : one;
        }
        return chart.tint;
      };
      const place = (p, vertex) => p[vertex * 3] + "," + p[vertex * 3 + 1] + "," + p[vertex * 3 + 2];
      if (charts.some(chart => chart.slot !== chart)) for (const member of members) {
        const { position: { raw: p, count }, found: { vertexChart } } = member;
        for (let vertex = 0; vertex < count; vertex++) {
          const chart = member.charts[vertexChart[vertex]];
          if (!chart) continue;
          const tint = tintOf(chart), key = place(p, vertex), entry = ramp.get(key) ?? ramp.set(key, [0, 0, 0, 0]).get(key);
          for (let k = 0; k < 3; k++) entry[k] += tint[k];
          entry[3]++;
        }
      }
      const slots = charts.filter(chart => chart.slot === chart);
      const packed = packSlots(slots, { limit: tier.pageLimit, gutter: tier.gutter, smallGutter: tier.smallGutter });
      report.charts += charts.length;
      report.sharedCharts += charts.length - slots.length;
      report.chartTexels += slots.reduce((sum, slot) => sum + slot.width * slot.height, 0);
      report.cellTexels += packed.cellTexels;
      report.pageTexels += packed.texels;
      report.pages += packed.pages.length;
      for (const [index, page] of packed.pages.entries()) for (const item of page.items) Object.assign(item.slot, { target: index, x: item.x, y: item.y });
      if (tier.dryRun) {
        report.groups.push({ mesh: mesh.name, material: group.cls.template.extras?.worldSpecSurface?.recipe, alpha: group.cls.alpha, roles,
          primitives: members.length, charts: charts.length, slots: slots.length, pages: packed.pages.map(page => [page.width, page.height]),
          chartTexels: slots.reduce((sum, slot) => sum + slot.width * slot.height, 0), pageTexels: packed.texels,
          ...(measure ? { chartList: charts.map(chart => ({ identity: chart.identity, width: chart.width, height: chart.height,
            mean: chart.mean, thumb: chart.thumb, order: chart.order, shared: chart.slot !== chart })) } : {}) });
        continue;
      }
      // 5. Paint the merged pages, one role and one source image at a time.
      const sampler = samplerOf(json, roleInfos(members[0].material).base), bufferOf = json.bufferViews[json.images[members[0].sources.base].bufferView].buffer;
      const label = [mesh.name, group.cls.template.extras?.worldSpecSurface?.recipe ?? "surface", group.cls.alpha === "OPAQUE" ? "" : group.cls.alpha.toLowerCase(),
        group.cls.sign ? "signs" : ""].filter(Boolean).join(" / ");
      const textures = packed.pages.map(() => ({}));
      for (const role of roles) {
        const mode = !tier.linearLight || !["base", "emissive"].includes(role) ? "data" : role === "base" && group.cls.alpha !== "OPAQUE" ? "alpha" : "color";
        // The finish detail image is baked at a fraction of its page. Keep that fraction.
        let factor = 1;
        if (role === "detail") {
          const ratios = [];
          for (const slot of slots) if (slot.page.sources.detail !== undefined)
            ratios.push(pow2(slot.page.size.finalWidth / (await imageSize(slot.page.sources.detail)).finalWidth - 1e-6));
          factor = Math.max(1, Math.min(tier.gutter, ...ratios));
        }
        const targets = packed.pages.map(page => {
          const texels = new Uint8Array(page.width * page.height * 4);
          new Uint32Array(texels.buffer).fill(new Uint32Array(Uint8Array.from(NEUTRAL[role]).buffer)[0]);
          return texels;
        });
        const bySource = new Map();
        for (const slot of slots) {
          const image = slot.page.sources[role];
          if (image === undefined) continue;
          (bySource.get(image) ?? bySource.set(image, []).get(image)).push(slot);
        }
        let alpha = false;
        for (const [image, users] of bySource) {
          const source = await decode(image);
          for (const slot of users) {
            const { size } = slot.page, fx = source.width / size.width, fy = source.height / size.height;
            resample(source.data, source.width, source.height, { x: slot.x0 * fx, y: slot.y0 * fy, width: (slot.x1 - slot.x0) * fx,
              height: (slot.y1 - slot.y0) * fy }, targets[slot.target], packed.pages[slot.target].width, slot.x, slot.y, slot.width, slot.height, mode);
          }
        }
        for (const [index, page] of packed.pages.entries()) {
          fillCells(targets[index], page, NEUTRAL[role]);
          const out = shrink(targets[index], page.width, page.height, factor);
          // An opaque surface never reads coverage, whatever its page carried for a neighbour.
          for (let i = 3; i < out.data.length && !alpha && (group.cls.alpha !== "OPAQUE" || role === "detail"); i += 4) alpha = out.data[i] !== 255;
          let image = sharp(out.data, { raw: { width: out.width, height: out.height, channels: 4 } });
          if (!alpha) image = image.removeAlpha();
          const bytes = await image.webp({ lossless: true, exact: true, effort: 0 }).toBuffer();
          const view = json.bufferViews.push({ buffer: bufferOf, byteLength: bytes.length }) - 1;
          pages.set(view, bytes);
          const id = json.images.push({ name: `${label} / ${role} ${index}`, mimeType: "image/webp", bufferView: view }) - 1;
          textures[index][role] = json.textures.push({ source: id, ...(sampler !== undefined ? { sampler } : {}) }) - 1;
        }
      }
      // 6. One material and one primitive per merged page.
      const byTarget = packed.pages.map(() => []);
      // A primitive whose charts landed on several pages is split by triangle.
      // Joined placements are also split by ground cell: the viewer culls, and takes its
      // occluding walls, per primitive, and one primitive for a whole arena would do neither.
      const cells = mesh === placed ? Math.max(1, tier.placedCells ?? 1) : 1;
      const column = value => Math.min(cells - 1, Math.max(0, Math.floor((value / 32767 + 1) / 2 * cells)));
      for (const member of members) {
        const parts = new Map(), p = member.position.raw;
        for (let i = 0; i < member.index.length; i += 3) {
          const target = member.charts[member.found.vertexChart[member.index[i]]].slot.target, [a, b, c] = [member.index[i], member.index[i + 1], member.index[i + 2]];
          const cell = cells === 1 ? 0 : column((p[a * 3] + p[b * 3] + p[c * 3]) / 3) + cells * column((p[a * 3 + 2] + p[b * 3 + 2] + p[c * 3 + 2]) / 3);
          const key = target + "/" + cell;
          (parts.get(key) ?? parts.set(key, { target, cell, triangles: [] }).get(key)).triangles.push(i);
        }
        member.split = parts.size > 1;
        for (const { target, cell, triangles } of parts.values()) byTarget[target].push({ member, triangles, cell });
      }
      for (const [target, page] of packed.pages.entries()) {
        const all = byTarget[target];
        if (!all.length) continue;
        const material = structuredClone(group.cls.template), pbr = material.pbrMetallicRoughness ??= {};
        material.name = `${label} / atlas ${target}`;
        material.extras.sourceFinishes = [...new Set(all.flatMap(part => part.member.material.extras?.sourceFinishes ?? []))].sort();
        if (!members.some(member => member.material.extras?.sourceFinishes)) delete material.extras.sourceFinishes;
        if (material.extras.worldSpecSurface) material.extras.worldSpecSurface.label = material.name;
        pbr.baseColorTexture = { ...pbr.baseColorTexture, index: textures[target].base };
        if (textures[target].surface !== undefined) pbr.metallicRoughnessTexture = { ...pbr.metallicRoughnessTexture, index: textures[target].surface };
        if (textures[target].normal !== undefined) material.normalTexture = { ...material.normalTexture, index: textures[target].normal };
        if (glow) Object.assign(material, { emissiveTexture: { ...glow.emissiveTexture, index: textures[target].emissive },
          ...(glow.emissiveFactor ? { emissiveFactor: glow.emissiveFactor } : {}), ...(glow.extensions ? { extensions: structuredClone(glow.extensions) } : {}) });
        else if (textures[target].emissive !== undefined) material.emissiveTexture = { ...material.emissiveTexture, index: textures[target].emissive };
        if (edge) {
          material.occlusionTexture = { ...edge.occlusion, index: textures[target].edge };
          material.extras.materialSpecEdge = edge.edge;
        } else if (textures[target].edge !== undefined) material.occlusionTexture = { ...material.occlusionTexture, index: textures[target].edge };
        if (textures[target].detail !== undefined) material.extras.worldSpecSurface.detailTexture = textures[target].detail;
        const materialIndex = json.materials.push(material) - 1;
        materialsAfter.add(materialIndex);
        // The viewer cuts a primitive over 36000 indices into spatial cells, each with its own
        // distance, and simplifies loose hardware sooner there. A limit keeps primitives whole.
        const cellOf = part => part.cell;
        const chunks = [[]];
        let chunkIndices = 0, chunkCell;
        for (const part of all.toSorted((a, b) => cellOf(a) - cellOf(b))) {
          if (chunks.at(-1).length && (cellOf(part) !== chunkCell
              || chunkIndices + part.triangles.length * 3 > (tier.primitiveIndexLimit || Infinity))) { chunks.push([]); chunkIndices = 0; }
          chunks.at(-1).push(part);
          chunkIndices += part.triangles.length * 3;
          chunkCell = cellOf(part);
        }
        for (const parts of chunks) {
        // Vertices: every member's, in order, with remapped coordinates.
        const lodMembers = [];
        let vertexCount = 0;
        const blocks = parts.map(({ member, triangles }) => {
          let indices = member.index, keep, count = member.position.count;
          if (member.split) {
            // Keep only the vertices these triangles use.
            keep = new Int32Array(count).fill(-1);
            count = 0;
            indices = new Uint32Array(triangles.length * 3);
            for (const [at, triangle] of triangles.entries()) for (let k = 0; k < 3; k++) {
              const vertex = member.index[triangle + k];
              if (keep[vertex] === -1) keep[vertex] = count++;
              indices[at * 3 + k] = keep[vertex];
            }
          }
          lodMembers.push({ entry: member.split ? undefined : lods.get(member.primitive), indices, vertexCount: count });
          const base = vertexCount;
          vertexCount += count;
          return { member, keep, base, count, indices };
        });
        const vertices = Buffer.alloc(vertexCount * STRIDE), min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
        for (const { member, keep, base } of blocks) {
          const { position, normal, color, uv, aspect, found: { vertexChart }, size } = member;
          for (let vertex = 0; vertex < position.count; vertex++) {
            const local = keep ? keep[vertex] : vertex;
            if (local === -1) continue;
            const at = (base + local) * STRIDE, chart = member.charts[vertexChart[vertex]];
            let u = 0, v = 0, ratio = aspect[vertex], tint;
            if (chart) {
              const slot = chart.slot, kx = slot.width / (slot.x1 - slot.x0), ky = slot.height / (slot.y1 - slot.y0);
              u = (slot.x + (uv[vertex * 2] * size.width - chart.x0) * kx) / page.width;
              v = (slot.y + (uv[vertex * 2 + 1] * size.height - chart.y0) * ky) / page.height;
              ratio *= (page.height * size.width * kx) / (page.width * size.height * ky);
              tint = ramp.get(place(position.raw, vertex));
            }
            for (let k = 0; k < 3; k++) {
              let value = color.raw[vertex * 3 + k];
              if (tint) {
                value = Math.round(value * tint[k] / tint[3]);
                if (value > 255) { value = 255; report.tintClamped++; }
              }
              vertices[at + k] = value;
              vertices.writeInt16LE(position.raw[vertex * 3 + k], at + 8 + k * 2);
              vertices.writeInt16LE(normal.raw[vertex * 3 + k], at + 16 + k * 2);
              min[k] = Math.min(min[k], position.raw[vertex * 3 + k]);
              max[k] = Math.max(max[k], position.raw[vertex * 3 + k]);
            }
            vertices.writeFloatLE(ratio, at + 4);
            vertices.writeUInt16LE(Math.max(0, Math.min(65535, Math.round(u * 65535))), at + 24);
            vertices.writeUInt16LE(Math.max(0, Math.min(65535, Math.round(v * 65535))), at + 26);
          }
        }
        const total = blocks.reduce((sum, block) => sum + block.indices.length, 0), wide = vertexCount > 65536;
        const indices = new (wide ? Uint32Array : Uint16Array)(total);
        let at = 0;
        for (const { indices: source, base } of blocks) for (let i = 0; i < source.length; i++) indices[at++] = source[i] + base;
        const geometryBuffer = json.bufferViews[json.accessors[parts[0].member.primitive.attributes.POSITION].bufferView].buffer;
        const vertexView = json.bufferViews.push({ buffer: geometryBuffer, byteLength: vertices.length, byteStride: STRIDE, target: 34962 }) - 1;
        replacement.set(vertexView, { bytes: vertices, mode: "ATTRIBUTES", stride: STRIDE });
        const indexBytes = Buffer.from(indices.buffer, indices.byteOffset, indices.byteLength);
        const indexView = json.bufferViews.push({ buffer: geometryBuffer, byteLength: indexBytes.length, target: 34963 }) - 1;
        replacement.set(indexView, { bytes: indexBytes, mode: "TRIANGLES", stride: wide ? 4 : 2 });
        const accessor = (type, componentType, byteOffset, extra = {}) => json.accessors.push({ type, componentType, count: vertexCount,
          bufferView: vertexView, byteOffset, ...extra }) - 1;
        const first = parts[0].member.primitive;
        const primitive = { attributes: {
          COLOR_0: accessor("VEC3", 5121, 0, { normalized: true }), _MS_ASPECT: accessor("SCALAR", 5126, 4),
          POSITION: accessor("VEC3", 5122, 8, { normalized: parts[0].member.position.normalized || undefined, min, max }),
          NORMAL: accessor("VEC3", 5122, 16, { normalized: parts[0].member.normal.normalized || undefined }),
          TEXCOORD_0: accessor("VEC2", 5123, 24, { normalized: true }) }, mode: 4, material: materialIndex,
          indices: json.accessors.push({ type: "SCALAR", componentType: wide ? 5125 : 5123, count: total, bufferView: indexView, byteOffset: 0 }) - 1,
          ...(first.extras ? { extras: structuredClone(first.extras) } : {}) };
        const entry = mergeLodEntries(lodMembers);
        if (entry) lods.set(primitive, entry);
        inserted.set(first, [...(inserted.get(first) ?? []), primitive]);
        }
      }
      for (const member of members) { removed.add(member.primitive); lods.delete(member.primitive); }
      report.groups.push({ mesh: mesh.name, material: label, primitives: members.length, sourcePages: sourcePages.size, charts: charts.length,
        slots: slots.length, pages: packed.pages.map(page => [page.width, page.height]),
        fill: packed.texels ? slots.reduce((sum, slot) => sum + slot.width * slot.height, 0) / packed.texels : 0,
        ...(measure ? { chartList: charts.map(chart => ({ identity: chart.identity, width: chart.width, height: chart.height,
          mean: chart.mean, thumb: chart.thumb, shareable: group.cls.shareable, order: chart.order })) } : {}) });
    }
    mesh.primitives = mesh.primitives.flatMap(primitive => [...(inserted.get(primitive) ?? []), ...(removed.has(primitive) ? [] : [primitive])]);
    report.primitivesAfter += mesh.primitives.length;
    for (const primitive of mesh.primitives) if (!materialsAfter.has(primitive.material)) materialsAfter.add(primitive.material);
  }
  report.materialsBefore = materialsBefore.size;
  report.materialsAfter = materialsAfter.size;
  return { pages, report };
}

/** Mean colour (linear) and a small brightness picture of each chart's base colour. */
async function describeCharts(charts, decode) {
  const byImage = new Map();
  for (const chart of charts) (byImage.get(chart.page.sources.base) ?? byImage.set(chart.page.sources.base, []).get(chart.page.sources.base)).push(chart);
  const small = new Uint8Array(8 * 8 * 4);
  for (const [image, users] of byImage) {
    const source = await decode(image);
    for (const chart of users) {
      resample(source.data, source.width, source.height, { x: chart.x0, y: chart.y0, width: chart.x1 - chart.x0, height: chart.y1 - chart.y0 },
        small, 8, 0, 0, 8, 8, "color");
      const mean = [0, 0, 0], thumb = new Float32Array(64);
      for (let i = 0; i < 64; i++) {
        const r = toLinear[small[i * 4]], g = toLinear[small[i * 4 + 1]], b = toLinear[small[i * 4 + 2]];
        mean[0] += r / 64; mean[1] += g / 64; mean[2] += b / 64;
        thumb[i] = .2126 * r + .7152 * g + .0722 * b;
      }
      const level = Math.max(1e-4, .2126 * mean[0] + .7152 * mean[1] + .0722 * mean[2]);
      chart.mean = mean;
      chart.thumb = Array.from(thumb, value => value / level);
    }
  }
}

/** How differently two charts are weathered, once their overall colour is set aside:
 * the root mean square difference of their brightness pictures, each relative to its mean. */
export function chartDifference(a, b) {
  let sum = 0;
  for (let i = 0; i < 64; i++) sum += (a.thumb[i] - b.thumb[i]) ** 2;
  return Math.sqrt(sum / 64);
}
