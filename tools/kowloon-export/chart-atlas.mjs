/** Building atlas pages laid out by chart at a tier's own texel density.
 *
 * The GPU bake gives every receiver its own atlas at the authored density. This module
 * takes the charts out of those atlases, sizes each for the tier, packs them across the
 * building's pages and filters each chart on its own: the inner texels are area-filtered
 * from the authored bake and the gutter is rebuilt at the tier's resolution, so a chart
 * never takes colour from a neighbour however far the tier is from the authored density. */
import { createHash } from "node:crypto";

/** Numbers per triangle in an engine layout record (`LAYOUT_RECORD` in `authoring/layout.rs`). */
const RECORD = 38, ATLAS_UVS = 32;

/** Texel density of a graphics tier as a fraction of the authored (Medium) density. */
export const tierDensities = { medium: 1, low: 0.25, "very-low": 0.125 };

/** Every setting that changes a baked page's pixels or where a chart lies on it. */
export function bakeSettings({ density = 1, layout = "charts", pageLimit = layout === "receivers" ? 2048 : 4096 } = {}) {
  if (!(density > 0 && density <= 1)) throw Error("Bake density is a fraction of the authored density: above 0, at most 1.");
  if (!Number.isInteger(pageLimit) || pageLimit < 64 || pageLimit > 16384) throw Error("The page limit is 64 to 16384 texels.");
  if (layout !== "charts" && layout !== "receivers") throw Error("The page layout is charts or receivers.");
  if (layout === "receivers" && density !== 1) throw Error("Whole receiver atlases are only published at the authored density.");
  return { density, pageLimit, layout };
}

/** The identity of one building's bake: its producer, source and receivers with the page settings. */
export function bakeKey(producer, source, receivers, settings) {
  const { density, pageLimit, layout } = bakeSettings(settings);
  return createHash("sha256").update(JSON.stringify([producer, source, receivers, ["pages-v2", density, pageLimit, layout]])).digest("hex");
}

/** The fraction of the authored texel density each image of a published city already holds, by
 * image index: the `worldSpecTexelDensity` its pages were baked at. An image that any material
 * uses without stating one is left out, and a delivery scales it by its tier as before. */
export function bakedTexelDensities(json) {
  const found = new Map(), unstated = new Set();
  for (const material of json.materials ?? []) {
    const density = material.extras?.worldSpecTexelDensity;
    // A finish's detail image is made from its page (material-library.mjs) and follows it.
    const detail = material.extras?.worldSpecSurface?.detailTexture;
    for (const info of [material.pbrMetallicRoughness?.baseColorTexture, material.pbrMetallicRoughness?.metallicRoughnessTexture,
      material.normalTexture, material.emissiveTexture, material.occlusionTexture, detail === undefined ? undefined : { index: detail }]) {
      const texture = info && json.textures?.[info.index];
      if (!texture) continue;
      for (const image of new Set([texture.source, ...Object.values(texture.extensions ?? {}).map(extension => extension?.source)])) {
        if (!Number.isInteger(image)) continue;
        if (!(density > 0 && density <= 1) || (found.has(image) && found.get(image) !== density)) unstated.add(image);
        else found.set(image, density);
      }
    }
  }
  for (const image of unstated) found.delete(image);
  return found;
}

/** The size a delivery at `tierScale` of the authored density gives a page already baked at
 * `density` of it: only what is left of the reduction, and no edge limit, because the bake
 * chose the page's size for its own tier. Sides are whole multiples of `multiple`. */
export function bakedPageSize(width, height, density, tierScale, multiple = 1) {
  const scale = Math.min(1, tierScale / density);
  const edge = value => Math.max(multiple, Math.ceil(Math.max(1, Math.round(value * scale)) / multiple) * multiple);
  return { width: edge(width), height: edge(height) };
}

/** The charts of one receiver atlas and the chart each triangle lies in, or undefined when a
 * triangle reaches outside its chart's rectangle and the atlas has to stay whole. Rectangles
 * are in atlas texels with rows from the upper edge and include the gutter. */
export function receiverCharts({ width, height, gutter, triangles, records }) {
  const charts = [], byPlace = new Map(), owner = new Uint32Array(triangles);
  for (let triangle = 0; triangle < triangles; triangle++) {
    const at = triangle * RECORD;
    const x = records[at], y = records[at + 1], w = records[at + 2], h = records[at + 3];
    if (!(Number.isInteger(x) && Number.isInteger(y) && w > 2 * gutter && h > 2 * gutter && x >= 0 && y >= 0 && x + w <= width && y + h <= height)) return undefined;
    const place = x + y * width;
    let index = byPlace.get(place);
    if (index === undefined) {
      index = charts.push({ x, y, w, h }) - 1;
      byPlace.set(place, index);
    } else if (charts[index].w !== w || charts[index].h !== h) return undefined;
    for (let corner = 0; corner < 3; corner++) {
      const u = records[at + ATLAS_UVS + corner * 2] * width, v = (1 - records[at + ATLAS_UVS + corner * 2 + 1]) * height;
      // Corners lie on the inner rectangle. Anything past the gutter reads another chart.
      if (!(u >= x + gutter - 0.01 && u <= x + w - gutter + 0.01 && v >= y + gutter - 0.01 && v <= y + h - gutter + 0.01)) return undefined;
    }
    owner[triangle] = index;
  }
  return { charts, owner };
}

/** Inner texels of a chart edge at a fraction of its baked length. Every chart keeps one. */
export const scaledEdge = (texels, scale) => scale === 1 ? texels : Math.max(1, Math.round(texels * scale));

const whole = (value, multiple) => Math.ceil(value / multiple) * multiple;

function shelves(order, sizes, width, limit) {
  const pages = [], open = [];
  let narrowest = Infinity;
  for (const index of order) narrowest = Math.min(narrowest, sizes[index][0]);
  for (const index of order) {
    const [w, h] = sizes[index];
    let shelf;
    for (let k = 0; k < open.length; k++) {
      const candidate = open[k];
      if (candidate.height >= h && candidate.end + w <= width) { shelf = candidate; break; }
    }
    if (!shelf) {
      let page = pages.find(page => page.used + h <= limit);
      if (!page) pages.push(page = { used: 0, width: 0, places: [] });
      shelf = { page, top: page.used, height: h, end: 0 };
      page.used += h;
      open.push(shelf);
    }
    shelf.page.places.push([index, shelf.end, shelf.top]);
    shelf.end += w;
    shelf.page.width = Math.max(shelf.page.width, shelf.end);
    if (shelf.end + narrowest > width) open.splice(open.indexOf(shelf), 1);
  }
  return pages;
}

/** Pack charts of inner size `w` by `h`, each with `pad` texels of gutter on every side, onto as
 * few pages as hold them. Charts keep their orientation and size. Pages are at most `limit`
 * on a side (a chart longer than that gets a page of its own length) and their sides are whole
 * multiples of `multiple`. */
export function packCharts(charts, limit, pad, { multiple = 4 } = {}) {
  if (!charts.length) return [];
  const sizes = charts.map(({ w, h }) => [w + 2 * pad, h + 2 * pad]);
  let longest = 0, area = 0;
  for (const [w, h] of sizes) { longest = Math.max(longest, w, h); area += w * h; }
  limit = whole(Math.max(limit, longest), multiple);
  const order = sizes.map((_, index) => index);
  order.sort((a, b) => sizes[b][1] - sizes[a][1] || sizes[b][0] - sizes[a][0] || a - b);
  let pages = shelves(order, sizes, limit, limit);
  // A lone or last page is narrowed towards a square: full-width shelves would leave a strip.
  const last = pages.at(-1), rest = last.places.map(([index]) => index);
  let best = [last], cost = whole(last.width, multiple) * whole(last.used, multiple);
  let widest = 0;
  for (const index of rest) widest = Math.max(widest, sizes[index][0]);
  for (let width = limit / 2; width >= Math.max(widest, 16); width /= 2) {
    const trial = shelves(rest, sizes, Math.floor(width), limit);
    if (trial.length !== 1 || trial[0].used > 2 * width) break;
    const size = whole(trial[0].width, multiple) * whole(trial[0].used, multiple);
    // Prefer the squarer page unless it costs more than a few percent of texels.
    if (size <= cost * 1.03) { best = trial; cost = Math.min(cost, size); }
  }
  pages = [...pages.slice(0, -1), ...best];
  return pages.map(page => ({
    width: whole(page.width, multiple), height: whole(page.used, multiple),
    items: page.places.map(([index, x, y]) => ({ chart: charts[index], x: x + pad, y: y + pad })),
  }));
}

/** Lay one receiver's charts out as a block of their own, so they stay neighbours on the page:
 * a distance level that collapses a part through its chart seams, and a coarse mip, then mix
 * texels of that part only. Returns the block's size and each chart's inner origin in it. */
export function packBlock(charts, pad) {
  const sizes = charts.map(({ w, h }) => [w + 2 * pad, h + 2 * pad]);
  const order = sizes.map((_, index) => index).sort((a, b) => sizes[b][1] - sizes[a][1] || sizes[b][0] - sizes[a][0] || a - b);
  let widest = 0, area = 0;
  for (const [w, h] of sizes) { widest = Math.max(widest, w); area += w * h; }
  let best;
  // Near-square first; a wider block is taken only when it wastes fewer texels.
  for (const stretch of [1, 1.25, 1.5, 2, 3]) {
    const [page] = shelves(order, sizes, Math.max(widest, Math.ceil(Math.sqrt(area) * stretch)), Infinity);
    if (!best || page.width * page.used < best.width * best.used) best = page;
  }
  return { width: best.width, height: best.used, items: best.places.map(([index, x, y]) => ({ chart: charts[index], x: x + pad, y: y + pad })) };
}

/** Finishes a publisher never thins out or streams: `sourceFinishes` entries of a material
 * that the distance levels, detail culling and texture streaming leave alone (city-lod.mjs,
 * city-texture-lod.mjs, the adapter's cityVisibility). A page holds these or the rest, not both. */
export const keptFinish = finish => /\b(?:sign|signage|lettering|poster|posters|tube-light|lens|emissive)\b/i.test(finish)
  // The Kowloon building's signs: hanging blades, boards and flat wall signs, with their emblems and brackets.
  || /^(?:blade|board|flat) \/ /.test(finish);

/** Finishes the material library reads as dielectric whatever their texels say (material-library.mjs). */
export const dielectricFinish = finish => /^(?:wall|roof|coping|tiles?-?\d*|tileStart|tileEnd) \/ (?:side|top|bottom|edge|front|reveal)$/.test(finish)
  || / \/ (?:concrete|wall|board|pane|glass|tube-light|balcony-plants)$/.test(finish);

const decode = new Float32Array(256), encode = new Uint8Array(4097);
for (let value = 0; value < 256; value++) {
  const c = value / 255;
  decode[value] = c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}
for (let value = 0; value <= 4096; value++) {
  const c = value / 4096;
  encode[value] = Math.round(255 * (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055));
}

/** Source texels and weights under each target texel along one axis. The target runs from
 * `-pad` to `target + pad`; its footprint is clamped to the source chart with its gutter, and a
 * target texel wholly outside repeats the nearest one that is not. */
function taps(inner, gutter, target, pad) {
  const extent = inner + 2 * gutter, step = inner / target;
  const starts = new Int32Array(target + 2 * pad + 1), indices = [], weights = [];
  const span = texel => [Math.min(extent, Math.max(0, texel * step + gutter)), Math.min(extent, Math.max(0, (texel + 1) * step + gutter))];
  for (let texel = -pad; texel < target + pad; texel++) {
    starts[texel + pad] = indices.length;
    let [low, high] = span(texel);
    // Past the baked gutter there is nothing to average: repeat the nearest texel that has a footprint.
    for (let inward = texel; high - low < 1e-6;) [low, high] = span(inward += texel < 0 ? 1 : -1);
    for (let source = Math.floor(low + 1e-9); source < high - 1e-9; source++) {
      indices.push(source);
      weights.push((Math.min(high, source + 1) - Math.max(low, source)) / (high - low));
    }
  }
  starts[target + 2 * pad] = indices.length;
  return { starts, indices: Int32Array.from(indices), weights: Float32Array.from(weights) };
}

/** Filter one chart from a baked RGBA image onto a page.
 *
 * `source` holds the chart at (`x`, `y`) with `w` by `h` texels including `gutter` on every
 * side. The page receives `tw` by `th` inner texels at (`tx`, `ty`) and `pad` texels around
 * them. `kind` says how texels average: "color" in linear light (sRGB bytes), "data" as
 * stored, "normal" as unit vectors. Equal sizes copy the texels unchanged. */
export function filterChart(source, sourceWidth, { x, y, w, h, gutter }, page, pageWidth, { x: tx, y: ty, w: tw, h: th, pad }, kind) {
  const iw = w - 2 * gutter, ih = h - 2 * gutter;
  const columns = taps(iw, gutter, tw, pad), rows = taps(ih, gutter, th, pad);
  const exact = iw === tw && ih === th;
  for (let row = 0; row < th + 2 * pad; row++) {
    const rowStart = rows.starts[row], rowEnd = rows.starts[row + 1];
    let out = ((ty - pad + row) * pageWidth + tx - pad) * 4;
    for (let column = 0; column < tw + 2 * pad; column++, out += 4) {
      const columnStart = columns.starts[column], columnEnd = columns.starts[column + 1];
      if (exact || (rowEnd - rowStart === 1 && columnEnd - columnStart === 1)) {
        const from = ((y + rows.indices[rowStart]) * sourceWidth + x + columns.indices[columnStart]) * 4;
        page[out] = source[from]; page[out + 1] = source[from + 1]; page[out + 2] = source[from + 2]; page[out + 3] = source[from + 3];
        continue;
      }
      let r = 0, g = 0, b = 0, a = 0;
      for (let j = rowStart; j < rowEnd; j++) {
        const line = ((y + rows.indices[j]) * sourceWidth + x) * 4, weight = rows.weights[j];
        for (let i = columnStart; i < columnEnd; i++) {
          const from = line + columns.indices[i] * 4, part = weight * columns.weights[i];
          if (kind === "color") { r += part * decode[source[from]]; g += part * decode[source[from + 1]]; b += part * decode[source[from + 2]]; }
          else { r += part * source[from]; g += part * source[from + 1]; b += part * source[from + 2]; }
          a += part * source[from + 3];
        }
      }
      if (kind === "color") {
        page[out] = encode[Math.round(Math.min(1, r) * 4096)]; page[out + 1] = encode[Math.round(Math.min(1, g) * 4096)];
        page[out + 2] = encode[Math.round(Math.min(1, b) * 4096)];
      } else if (kind === "normal") {
        const nx = r / 127.5 - 1, ny = g / 127.5 - 1, nz = b / 127.5 - 1, length = Math.hypot(nx, ny, nz) || 1;
        page[out] = Math.round((nx / length + 1) * 127.5); page[out + 1] = Math.round((ny / length + 1) * 127.5);
        page[out + 2] = Math.round((nz / length + 1) * 127.5);
      } else { page[out] = Math.round(r); page[out + 1] = Math.round(g); page[out + 2] = Math.round(b); }
      page[out + 3] = Math.round(a);
    }
  }
}

/** Where a point of a baked chart lies on its page, in texels: a function of the point's baked
 * atlas texel position. The chart's inner rectangle maps onto the page's inner rectangle. */
export function chartPlacement({ x, y, w, h, gutter }, { x: tx, y: ty, w: tw, h: th }) {
  const sx = tw / (w - 2 * gutter), sy = th / (h - 2 * gutter);
  return { sx, sy, ox: tx - (x + gutter) * sx, oy: ty - (y + gutter) * sy };
}
