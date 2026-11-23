import assert from "node:assert/strict";
import test from "node:test";
import { MeshoptDecoder, MeshoptEncoder } from "meshoptimizer";
import sharp from "sharp";
import { mergeMobileAtlases } from "./mobile-atlas.mjs";
import { mobileLodSidecar } from "./mobile-geometry.mjs";

const PAGE = 256, SCALE = .25;
const tier = { pageLimit: 1024, gutter: 4, smallGutter: 2, chartJoin: 3, meshes: "^building-\\d+$", unify: ["edge", "plain"], mergeBlend: true,
  linearLight: true, share: { representatives: 0, maxDifference: .2 } };
const textureSize = (width, height) => ({ width: Math.round(width * SCALE), height: Math.round(height * SCALE) });

/** A building whose pages each carry rectangular charts, one quad per chart, as the bake
 * publishes them: `pages[i]` is `{ charts: [{ x, y, w, h, color | paint, tint }], alpha }`.
 * Every vertex has its own position, so a merged vertex can be traced to its source. */
async function building(pages, { layers = ["base", "normal", "surface"], lods, tier: overrides } = {}) {
  const views = [], json = { asset: { version: "2.0" }, scenes: [{ nodes: [0] }], nodes: [{ mesh: 0 }], buffers: [{ byteLength: 0 }],
    bufferViews: [], accessors: [], images: [], textures: [], samplers: [{ minFilter: 9987 }], materials: [], meshes: [{ name: "building-0", primitives: [] }] };
  const view = (bytes, extra = {}) => { views.push(Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength)); return json.bufferViews.push({ buffer: 0, byteLength: bytes.byteLength, ...extra }) - 1; };
  const accessor = (array, type, componentType, extra = {}) => json.accessors.push({ bufferView: view(array), type, componentType,
    count: array.length / { SCALAR: 1, VEC2: 2, VEC3: 3 }[type], ...extra }) - 1;
  const source = new Map(), lodEntries = new Map();
  let vertexId = 0;
  for (const [pageIndex, page] of pages.entries()) {
    const texture = async paint => {
      const raw = Buffer.alloc(PAGE * PAGE * 4);
      for (let y = 0; y < PAGE; y++) for (let x = 0; x < PAGE; x++) raw.set(paint(x, y), (y * PAGE + x) * 4);
      const png = await sharp(raw, { raw: { width: PAGE, height: PAGE, channels: 4 } }).png().toBuffer();
      const image = json.images.push({ mimeType: "image/png", bufferView: view(png) }) - 1;
      return json.textures.push({ source: image, sampler: 0 }) - 1;
    };
    // Outside every chart the page is a colour no chart has, as an unused atlas area is.
    const inside = (x, y) => page.charts.find(chart => x >= chart.x && x < chart.x + chart.w && y >= chart.y && y < chart.y + chart.h);
    const base = await texture((x, y) => { const chart = inside(x, y); return chart ? chart.paint?.(x, y) ?? [...chart.color, chart.alpha ?? 255] : [255, 0, 255, 255]; });
    const material = { name: `building-0 / part / sheet ${pageIndex}`, doubleSided: true, ...(page.alpha ? { alphaMode: page.alpha } : {}),
      extras: { materialSpecNormalFrame: true, sourceFinishes: ["wall / front"], worldSpecSurface: { family: "Masonry", surface: "Concrete and masonry",
        recipe: "architectural-concrete", reflection: "none", label: "sheet " + pageIndex } },
      pbrMetallicRoughness: { baseColorTexture: { index: base } } };
    if (layers.includes("surface")) material.pbrMetallicRoughness.metallicRoughnessTexture = { index: await texture(() => [0, 200, 0, 255]) };
    if (layers.includes("normal")) material.normalTexture = { index: await texture(() => [128, 128, 255, 255]) };
    if (page.emissive) Object.assign(material, { emissiveTexture: { index: await texture((x, y) => inside(x, y) ? [...page.emissive, 255] : [0, 0, 0, 255]) }, emissiveFactor: [1, 1, 1] });
    if (page.surface) Object.assign(material.extras.worldSpecSurface, page.surface);
    if (page.edge) Object.assign(material, { occlusionTexture: { index: await texture(() => [200, 100, 0, 255]), strength: 0 } }, (material.extras.materialSpecEdge = { from: .3, to: .06 }, {}));
    const positions = [], uvs = [], colors = [], aspects = [], indices = [];
    for (const chart of page.charts) {
      const first = positions.length / 3;
      for (const [cx, cy] of [[0, 0], [1, 0], [1, 1], [0, 1]]) {
        positions.push(vertexId, pageIndex, 0);
        source.set(vertexId++, { page: pageIndex, chart, u: (chart.x + cx * chart.w) / PAGE, v: (chart.y + cy * chart.h) / PAGE, alpha: page.alpha ?? "OPAQUE" });
        uvs.push(Math.round((chart.x + cx * chart.w) / PAGE * 65535), Math.round((chart.y + cy * chart.h) / PAGE * 65535));
        colors.push(...(chart.tint ?? [255, 255, 255]));
        aspects.push(chart.w / chart.h);
      }
      indices.push(first, first + 1, first + 2, first, first + 2, first + 3);
    }
    const primitive = { mode: 4, material: json.materials.push(material) - 1, indices: accessor(Uint16Array.from(indices), "SCALAR", 5123),
      attributes: { POSITION: accessor(Int16Array.from(positions), "VEC3", 5122, { normalized: true, min: [0, 0, 0], max: [vertexId, pages.length, 0] }),
        NORMAL: accessor(Int16Array.from(positions.map((_, i) => i % 3 === 2 ? 32767 : 0)), "VEC3", 5122, { normalized: true }),
        COLOR_0: accessor(Uint8Array.from(colors), "VEC3", 5121, { normalized: true }), TEXCOORD_0: accessor(Uint16Array.from(uvs), "VEC2", 5123, { normalized: true }),
        _MS_ASPECT: accessor(Float32Array.from(aspects), "SCALAR", 5126) } };
    json.meshes[0].primitives.push(primitive);
    if (lods?.[pageIndex]) lodEntries.set(primitive, lods[pageIndex](Uint16Array.from(indices), positions.length / 3));
  }
  const replacement = new Map();
  const { pages: finished, report } = await mergeMobileAtlases(json, { tier: { ...tier, ...overrides }, read: async id => views[id], replacement, lods: lodEntries, textureSize });
  // Read the merged pack back the way a viewer would.
  const bytesOf = id => replacement.get(id)?.bytes ?? finished.get(id) ?? views[id];
  const read = id => {
    const a = json.accessors[id], width = { SCALAR: 1, VEC2: 2, VEC3: 3 }[a.type], bytes = bytesOf(a.bufferView);
    const [size, get, divisor] = { 5121: [1, "getUint8", 255], 5122: [2, "getInt16", 1], 5123: [2, "getUint16", 65535], 5125: [4, "getUint32", 1], 5126: [4, "getFloat32", 1] }[a.componentType];
    const data = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength), stride = json.bufferViews[a.bufferView].byteStride ?? size * width;
    return Array.from({ length: a.count * width }, (_, i) => data[get]((a.byteOffset ?? 0) + Math.floor(i / width) * stride + (i % width) * size, true)
      / (a.normalized && a.componentType !== 5122 ? divisor : 1));
  };
  const images = new Map();
  const image = async texture => {
    const index = json.textures[texture].source;
    if (!images.has(index)) {
      const { data, info } = await sharp(bytesOf(json.images[index].bufferView)).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
      images.set(index, { data, width: info.width, height: info.height });
    }
    return images.get(index);
  };
  const primitives = [];
  for (const primitive of json.meshes[0].primitives) {
    const material = json.materials[primitive.material], position = read(primitive.attributes.POSITION);
    primitives.push({ primitive, material, base: await image(material.pbrMetallicRoughness.baseColorTexture.index), uv: read(primitive.attributes.TEXCOORD_0),
      emissive: material.emissiveTexture && await image(material.emissiveTexture.index),
      color: read(primitive.attributes.COLOR_0), aspect: read(primitive.attributes._MS_ASPECT), indices: read(primitive.indices), position,
      sources: Array.from({ length: position.length / 3 }, (_, i) => source.get(position[i * 3])) });
  }
  return { json, primitives, report, lods: lodEntries, source };
}

/** GPU-style mip chain (2x2 box) and bilinear lookup with clamped edges. */
function mips(image, levels) {
  const chain = [image];
  for (let level = 1; level <= levels; level++) {
    const from = chain.at(-1), width = from.width / 2, height = from.height / 2, data = new Float64Array(width * height * 4);
    for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) for (let c = 0; c < 4; c++)
      data[(y * width + x) * 4 + c] = (from.data[(2 * y * from.width + 2 * x) * 4 + c] + from.data[(2 * y * from.width + 2 * x + 1) * 4 + c]
        + from.data[((2 * y + 1) * from.width + 2 * x) * 4 + c] + from.data[((2 * y + 1) * from.width + 2 * x + 1) * 4 + c]) / 4;
    chain.push({ data, width, height });
  }
  return chain;
}
function bilinear(image, u, v) {
  const x = u * image.width - .5, y = v * image.height - .5, x0 = Math.floor(x), y0 = Math.floor(y), fx = x - x0, fy = y - y0;
  const at = (i, j, c) => image.data[(Math.max(0, Math.min(image.height - 1, j)) * image.width + Math.max(0, Math.min(image.width - 1, i))) * 4 + c];
  return [0, 1, 2, 3].map(c => (at(x0, y0, c) * (1 - fx) + at(x0 + 1, y0, c) * fx) * (1 - fy) + (at(x0, y0 + 1, c) * (1 - fx) + at(x0 + 1, y0 + 1, c) * fx) * fy);
}
const close = (actual, expected, tolerance, message) => {
  for (let c = 0; c < expected.length; c++) assert.ok(Math.abs(actual[c] - expected[c]) <= tolerance, `${message}: ${actual.map(Math.round)} is not ${expected}`);
};
/** Points across a quad's own texels: its corners, edge midpoints and interior. */
const quadPoints = [[0, 0], [1, 0], [1, 1], [0, 1], [.5, 0], [1, .5], [.5, 1], [0, .5], [.5, .5], [.21, .68], [.83, .37]];

test("a remapped chart shows the texels it showed before", async () => {
  // Each chart is a smooth ramp of its own, so the colour at a point says which chart
  // and where on it. Area-filtering a ramp to a quarter keeps its value at the centre.
  const ramp = (chart, hue) => ({ ...chart, paint: (x, y) => [60 + hue * 40 + 60 * (x - chart.x) / chart.w, 70 + 100 * (y - chart.y) / chart.h, 200 - hue * 50, 255] });
  const pages = [
    { charts: [ramp({ x: 8, y: 8, w: 120, h: 80 }, 0), ramp({ x: 140, y: 12, w: 64, h: 160 }, 1), ramp({ x: 16, y: 120, w: 96, h: 96 }, 2)] },
    { charts: [ramp({ x: 20, y: 20, w: 200, h: 48 }, 3), ramp({ x: 24, y: 100, w: 40, h: 136 }, 1), ramp({ x: 100, y: 96, w: 128, h: 128 }, 0)] },
  ];
  const { primitives, report } = await building(pages);
  assert.equal(primitives.length, 1, "both pages share one material and one primitive");
  assert.equal(report.materialsAfter, 1);
  const [{ base, uv, indices, sources, aspect }] = primitives;
  assert.equal(indices.length, 36);
  for (let quad = 0; quad < sources.length; quad += 4) {
    const { chart } = sources[quad], corner = k => [uv[(quad + k) * 2], uv[(quad + k) * 2 + 1]];
    const [u0, v0] = corner(0), [u1, v1] = corner(2);
    for (const [s, t] of quadPoints) {
      // Stay half a final texel inside, where the original sampled only this chart too.
      const inset = (value, size) => Math.min(1 - 2 / size, Math.max(2 / size, value));
      const a = inset(s, chart.w), b = inset(t, chart.h);
      close(bilinear(base, u0 + (u1 - u0) * a, v0 + (v1 - v0) * b), chart.paint(chart.x + a * chart.w, chart.y + b * chart.h).slice(0, 3), 5, "chart colour");
    }
    // The normal frame stays metric: a chart twice as wide as it is tall still says so.
    const texelsWide = (u1 - u0) * base.width, texelsHigh = (v1 - v0) * base.height;
    assert.ok(Math.abs(texelsWide / texelsHigh / (chart.w / chart.h) - 1) < .15, "a chart keeps its proportions");
    assert.ok(Math.abs(aspect[quad] / ((u1 - u0) / (v1 - v0)) - 1) < .02, "the chart aspect follows the new page");
  }
});

test("no chart takes colour from a neighbour, down to the mip its gutter covers", async () => {
  // Flat, strongly different colours: any bleed shows as a mixed colour.
  const colors = [[200, 40, 40], [40, 200, 40], [40, 40, 200], [200, 200, 40], [40, 200, 200], [200, 40, 200], [120, 120, 120], [230, 230, 230]];
  const pages = [0, 1].map(page => ({ charts: [
    { x: 8, y: 8, w: 96, h: 64 }, { x: 112, y: 8, w: 128, h: 100 }, { x: 8, y: 120, w: 24, h: 24 }, { x: 40, y: 120, w: 8, h: 40 },
    { x: 60, y: 120, w: 12, h: 12 }, { x: 80, y: 124, w: 100, h: 9 }, { x: 8, y: 180, w: 200, h: 64 }, { x: 216, y: 120, w: 32, h: 128 },
  ].map((chart, index) => ({ ...chart, color: colors[(index + page * 3) % colors.length] })) }));
  const { primitives } = await building(pages);
  assert.equal(primitives.length, 1);
  const [{ base, uv, sources }] = primitives, chain = mips(base, 2);
  let checked = 0;
  for (let quad = 0; quad < sources.length; quad += 4) {
    const { chart } = sources[quad];
    // The tier's rule: a chart at least four gutters across gets the full gutter.
    const final = Math.min(chart.w, chart.h) * SCALE, gutter = final >= 4 * tier.gutter ? tier.gutter : tier.smallGutter;
    for (let level = 0; level <= Math.log2(gutter); level++) for (const [s, t] of quadPoints) {
      const u = uv[quad * 2] + (uv[(quad + 2) * 2] - uv[quad * 2]) * s, v = uv[quad * 2 + 1] + (uv[(quad + 2) * 2 + 1] - uv[quad * 2 + 1]) * t;
      close(bilinear(chain[level], u, v), chart.color, 2.5, `mip ${level} of a ${chart.w}x${chart.h} chart`);
      checked++;
    }
  }
  assert.ok(checked > 300);
  // The quarter-edge streaming companion is mip 2: full-gutter charts hold there.
  assert.ok(sources.some(({ chart }) => Math.min(chart.w, chart.h) * SCALE >= 4 * tier.gutter));
});

test("opaque, masked and blended surfaces never share a material", async () => {
  const chart = (x, color, alpha) => ({ x, y: 16, w: 48, h: 48, color, alpha });
  const pages = [
    { charts: [chart(8, [200, 60, 60]), chart(72, [60, 200, 60])] }, { charts: [chart(8, [60, 60, 200])] },
    { alpha: "BLEND", charts: [chart(8, [90, 90, 90], 128)] }, { alpha: "BLEND", charts: [chart(72, [150, 150, 150], 64)] },
    { alpha: "MASK", charts: [chart(8, [10, 200, 200], 255)] }, { alpha: "MASK", charts: [chart(136, [200, 200, 10], 255)] },
  ];
  const { primitives } = await building(pages);
  assert.equal(primitives.length, 3);
  for (const { material, sources, base, uv } of primitives) {
    const modes = new Set(sources.map(vertex => vertex.alpha));
    assert.deepEqual([...modes], [material.alphaMode ?? "OPAQUE"]);
    // Coverage survives the merge: a half-clear pane is still half clear.
    for (let quad = 0; quad < sources.length; quad += 4) {
      const u = (uv[quad * 2] + uv[(quad + 2) * 2]) / 2, v = (uv[quad * 2 + 1] + uv[(quad + 2) * 2 + 1]) / 2, { chart } = sources[quad];
      close(bilinear(base, u, v), [...chart.color, chart.alpha ?? 255], 2.5, "colour and coverage");
    }
  }
  const unmerged = await building(pages, { tier: { mergeBlend: false } });
  assert.equal(unmerged.primitives.filter(({ material }) => material.alphaMode === "BLEND").length, 2, "glazing can be left as published");
});

test("merged primitives keep valid distance levels that draw what their members drew", async () => {
  await Promise.all([MeshoptDecoder.ready, MeshoptEncoder.ready]);
  const charts = count => Array.from({ length: count }, (_, i) => ({ x: 8 + (i % 4) * 60, y: 8 + Math.floor(i / 4) * 60, w: 48, h: 48, color: [100 + i * 10, 90, 80] }));
  const stream = indices => MeshoptEncoder.encodeIndexBuffer(new Uint8Array(Uint16Array.from(indices).buffer), indices.length, 2);
  // Page 0 drops charts with distance, page 1 keeps a single coarser level, page 2 has no levels.
  const lods = {
    0: (indices, vertexCount) => ({ vertexCount, nearCount: indices.length, levels: [
      { count: indices.length, componentBytes: 2, errorMeters: 0 }, { count: 24, componentBytes: 2, errorMeters: .02, stream: stream(indices.slice(0, 24)) }],
      detailLevels: [{ count: indices.length, componentBytes: 2, errorMeters: 0 }, { count: 24, componentBytes: 2, errorMeters: .02, stream: stream(indices.slice(0, 24)) },
        { count: 6, componentBytes: 2, errorMeters: .3, stream: stream(indices.slice(6, 12)) }, { count: 0, componentBytes: 2, errorMeters: .9 }] }),
    1: (indices, vertexCount) => ({ vertexCount, nearCount: indices.length, levels: [{ count: indices.length, componentBytes: 2, errorMeters: 0 },
      { count: 12, componentBytes: 2, errorMeters: .05, stream: stream(indices.slice(12, 24)) }] }),
  };
  const { json, primitives, lods: entries } = await building([{ charts: charts(8) }, { charts: charts(6) }, { charts: charts(3) }], { lods });
  assert.equal(primitives.length, 1);
  const sidecar = mobileLodSidecar(json, entries), [{ primitive, indices, position, sources }] = primitives;
  assert.equal(sidecar.manifest.primitives.length, 1);
  const [record] = sidecar.manifest.primitives;
  assert.deepEqual([record.mesh, record.primitive], [0, 0]);
  assert.equal(record.vertexCount, json.accessors[primitive.attributes.POSITION].count);
  assert.equal(record.nearCount, indices.length);
  assert.ok(record.levels.length >= 2 && record.levels.length <= 4);
  const trianglesOf = list => { const set = []; for (let i = 0; i < list.length; i += 3) set.push([list[i], list[i + 1], list[i + 2]].map(v => position[v * 3]).join()); return set; };
  const levels = (record.detailLevels ?? [...record.levels, ...record.distantLevels ?? []]).map(level => {
    assert.ok(level.count % 3 === 0 && level.count <= record.nearCount && [2, 4].includes(level.componentBytes));
    if (level.count !== record.nearCount) assert.equal(level.count === 0, level.bytes === 0);
    if (level.count === record.nearCount && !level.bytes) return indices;
    const data = new (level.componentBytes === 2 ? Uint16Array : Uint32Array)(level.count);
    if (level.count) MeshoptDecoder.decodeIndexBuffer(new Uint8Array(data.buffer), level.count, level.componentBytes, sidecar.payload.subarray(level.offset, level.offset + level.bytes));
    for (const index of data) assert.ok(index < record.vertexCount);
    return [...data];
  });
  // What each source page drew at each level, by the positions of its own vertices.
  const pageOf = id => sources.find((vertex, index) => position[index * 3] === id).page;
  const expected = level => {
    const all = trianglesOf(indices), page = triangle => pageOf(+triangle.split(",")[0]), own = n => all.filter(triangle => page(triangle) === n);
    const zero = own(0), one = own(1);
    return [...[zero, zero.slice(0, 8), zero.slice(2, 4), []][level], ...[one, one.slice(4, 8), one.slice(4, 8), one.slice(4, 8)][level], ...own(2)].sort();
  };
  const sorted = list => trianglesOf(list).sort();
  assert.equal(levels.length, 4);
  for (const [level, list] of levels.entries()) assert.deepEqual(sorted(list), expected(level), "level " + level);
  // A level is never drawn before every member's own level would have been.
  const errors = (record.detailLevels ?? record.levels).map(level => level.errorMeters);
  assert.deepEqual(errors.map(value => +value.toFixed(3)), [0, .05, .3, .9]);
});

test("identical parts share texels and keep their own colour as a tint", async () => {
  // Three parts of one shape, weathered the same way in three shades, and one odd part.
  const pattern = (chart, shade) => ({ ...chart, shade, paint: (x, y) => { const k = shade * ((x - chart.x) < chart.w / 2 ? 1 : .8); return [200 * k, 180 * k, 160 * k, 255]; } });
  const pages = [{ charts: [pattern({ x: 8, y: 8, w: 96, h: 64 }, 1), pattern({ x: 120, y: 8, w: 96, h: 64 }, .6), pattern({ x: 8, y: 100, w: 96, h: 64 }, .8),
    { x: 120, y: 100, w: 80, h: 80, color: [30, 90, 200] }] }, { charts: [{ x: 8, y: 8, w: 40, h: 40, color: [90, 30, 20] }] }];
  const unique = await building(pages), shared = await building(pages, { tier: { share: { representatives: 1, maxDifference: .2 } } });
  assert.equal(shared.report.sharedCharts, 2);
  assert.ok(shared.report.chartTexels < unique.report.chartTexels - 2 * 20 * 14);
  const linear = value => (value / 255) ** 2.2;
  for (const { base, uv, color, sources } of shared.primitives) for (let quad = 0; quad < sources.length; quad += 4) {
    const { chart } = sources[quad];
    for (const [s, t] of [[.25, .5], [.75, .5]]) {
      const u = uv[quad * 2] + (uv[(quad + 2) * 2] - uv[quad * 2]) * s, v = uv[quad * 2 + 1] + (uv[(quad + 2) * 2 + 1] - uv[quad * 2 + 1]) * t;
      // The viewer multiplies the texel by the vertex colour in linear light.
      const shown = bilinear(base, u, v).slice(0, 3).map((value, c) => linear(value) * color[quad * 3 + c]);
      const meant = (chart.paint?.(chart.x + s * chart.w, chart.y + t * chart.h) ?? chart.color).slice(0, 3).map(linear);
      for (let c = 0; c < 3; c++) assert.ok(Math.abs(shown[c] - meant[c]) <= .03 + .08 * meant[c], `a ${chart.shade ?? "plain"} part shows ${shown} for ${meant}`);
    }
  }
});

test("an index limit splits a merged primitive without splitting its material", async () => {
  const charts = Array.from({ length: 6 }, (_, i) => ({ x: 8 + (i % 3) * 80, y: 8 + Math.floor(i / 3) * 80, w: 64, h: 64, color: [90 + i * 20, 120, 60] }));
  const pages = [{ charts }, { charts }, { charts }];
  const whole = await building(pages), limited = await building(pages, { tier: { primitiveIndexLimit: 40 } });
  assert.equal(whole.primitives.length, 1);
  assert.equal(limited.primitives.length, 3);
  assert.equal(new Set(limited.primitives.map(({ primitive }) => primitive.material)).size, 1);
  for (const { indices } of limited.primitives) assert.ok(indices.length <= 40);
  assert.equal(limited.primitives.reduce((sum, { indices }) => sum + indices.length, 0), whole.primitives[0].indices.length);
});

test("separately placed meshes join into one material where they stood, split by ground cell", async () => {
  // Three walls, each its own mesh, node and page; a fourth tiles its texture and must stay.
  const views = [], json = { asset: { version: "2.0" }, scenes: [{ nodes: [] }], nodes: [], buffers: [{ byteLength: 0 }], bufferViews: [], accessors: [],
    images: [], textures: [], samplers: [{}], materials: [], meshes: [] };
  const view = bytes => { views.push(Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength)); return json.bufferViews.push({ buffer: 0, byteLength: bytes.byteLength }) - 1; };
  const accessor = (array, type, componentType, extra = {}) => json.accessors.push({ bufferView: view(array), type, componentType, count: array.length / { SCALAR: 1, VEC2: 2, VEC3: 3 }[type], ...extra }) - 1;
  const colors = [[200, 60, 40], [40, 180, 60], [50, 70, 210], [128, 128, 128]], expected = [];
  const quarter = Math.SQRT1_2, placements = [{ translation: [-20, 0, -20], scale: [4, 4, 4] }, { translation: [20, 1, -20], scale: [2, 2, 2], rotation: [0, quarter, 0, quarter] },
    { translation: [20, 0, 20], scale: [3, 3, 3] }, { translation: [0, 0, 0] }];
  for (const [index, placement] of placements.entries()) {
    const raw = Buffer.alloc(64 * 64 * 4);
    for (let i = 0; i < raw.length; i += 4) raw.set([...colors[index], 255], i);
    const texture = json.textures.push({ sampler: 0, source: json.images.push({ mimeType: "image/png",
      bufferView: view(await sharp(raw, { raw: { width: 64, height: 64, channels: 4 } }).png().toBuffer()) }) - 1 }) - 1;
    const material = json.materials.push({ name: "Arena concrete " + index, extras: { worldSpecConcrete: true, worldSpecSurface: { family: "Masonry",
      surface: "Concrete and masonry", recipe: "architectural-concrete", wear: "masonry", reflection: "masonry" } }, pbrMetallicRoughness: { baseColorTexture: { index: texture } } }) - 1;
    const tiles = index === 3, local = [[-1, 0, 0], [1, 0, 0], [1, 1, 0], [-1, 1, 0]], uv = [[.1, .1], [.9, .1], [.9, .9], [.1, .9]].map(pair => pair.map(value => tiles ? value * 4 : value));
    json.meshes.push({ name: "brush-" + index, primitives: [{ mode: 4, material, indices: accessor(Uint16Array.from([0, 1, 2, 0, 2, 3]), "SCALAR", 5123),
      attributes: { POSITION: accessor(Float32Array.from(local.flat()), "VEC3", 5126, { min: [-1, 0, 0], max: [1, 1, 0] }),
        NORMAL: accessor(Float32Array.from(local.flatMap(() => [0, 0, 1])), "VEC3", 5126), TEXCOORD_0: accessor(Float32Array.from(uv.flat()), "VEC2", 5126) } }] });
    json.scenes[0].nodes.push(json.nodes.push({ name: "arena", mesh: index, ...placement }) - 1);
    // Where each corner stands in the world, worked out by hand: scale, a quarter turn about Y for the second wall, then the offset.
    const s = placement.scale?.[0] ?? 1, turned = !!placement.rotation;
    if (!tiles) for (const [x, y] of local) expected.push({ index, at: [placement.translation[0] + (turned ? 0 : x * s), placement.translation[1] + y * s, placement.translation[2] + (turned ? -x * s : 0)],
      normal: turned ? [1, 0, 0] : [0, 0, 1] });
  }
  const replacement = new Map();
  const { pages, report } = await mergeMobileAtlases(json, { tier: { ...tier, unify: ["plain"], placedMeshes: "^brush-\\d+$", placedCells: 2 }, read: async id => views[id], replacement, textureSize });
  assert.equal(report.placedPrimitives, 3);
  const bytesOf = id => replacement.get(id)?.bytes ?? pages.get(id) ?? views[id];
  const read = id => {
    const a = json.accessors[id], width = { SCALAR: 1, VEC2: 2, VEC3: 3 }[a.type], bytes = bytesOf(a.bufferView), stride = json.bufferViews[a.bufferView].byteStride;
    const [size, get, divisor] = { 5121: [1, "getUint8", 255], 5122: [2, "getInt16", 32767], 5123: [2, "getUint16", 65535], 5126: [4, "getFloat32", 1] }[a.componentType];
    const data = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    return Array.from({ length: a.count * width }, (_, i) => data[get]((a.byteOffset ?? 0) + Math.floor(i / width) * (stride ?? size * width) + (i % width) * size, true) / (a.normalized ? divisor : 1));
  };
  const placedNodes = json.nodes.filter(node => node.mesh !== undefined), joined = placedNodes.find(node => json.meshes[node.mesh].name.startsWith("atlas/"));
  assert.deepEqual(placedNodes.map(node => json.meshes[node.mesh].name).sort(), ["atlas/placed", "brush-3"], "emptied placements are gone, the tiling wall stays");
  const merged = json.meshes[joined.mesh].primitives;
  assert.equal(merged.length, 3, "one primitive per occupied ground cell");
  assert.equal(new Set(merged.map(primitive => primitive.material)).size, 1);
  assert.equal(json.materials[merged[0].material].extras.worldSpecConcrete, true);
  const found = new Set();
  for (const primitive of merged) {
    const position = read(primitive.attributes.POSITION), normal = read(primitive.attributes.NORMAL), uv = read(primitive.attributes.TEXCOORD_0);
    const info = json.materials[primitive.material].pbrMetallicRoughness.baseColorTexture, { data, info: size } = await sharp(bytesOf(json.images[json.textures[info.index].source].bufferView))
      .ensureAlpha().raw().toBuffer({ resolveWithObject: true });
    for (let vertex = 0; vertex < position.length / 3; vertex++) {
      const world = [0, 1, 2].map(k => position[vertex * 3 + k] * joined.scale[k] + joined.translation[k]);
      const match = expected.findIndex(corner => Math.hypot(...corner.at.map((value, k) => value - world[k])) < .01);
      assert.ok(match >= 0, "a vertex stands where its placement put it: " + world);
      found.add(match);
      for (let k = 0; k < 3; k++) assert.ok(Math.abs(normal[vertex * 3 + k] - expected[match].normal[k]) < .01, "normals turn with the placement");
      // The wall's own colour, sampled at the middle of its chart.
      if (vertex % 4 === 0) close(bilinear({ data, width: size.width, height: size.height }, (uv[vertex * 2] + uv[(vertex + 2) * 2]) / 2, (uv[vertex * 2 + 1] + uv[(vertex + 2) * 2 + 1]) / 2),
        colors[expected[match].index], 2.5, "wall colour");
    }
  }
  assert.equal(found.size, expected.length);
});

test("what a tier gives up is merged, and nothing else", async () => {
  const chart = (x, color, alpha) => ({ x, y: 16, w: 64, h: 64, color, alpha });
  const glazing = { family: "Building atlases", surface: "Transparent surfaces", recipe: "architectural-glazing" };
  const steel = { family: "Building atlases", surface: "Mixed surfaces with metal", recipe: "weathered-steel", finish: "weathered-steel", wear: "worn", reflection: "box" };
  const fittings = { ...steel, recipe: "stock-hardware", finish: "stock-hardware", wear: "maintained", reflection: "virtual" };
  const pages = [
    { alpha: "BLEND", surface: glazing, emissive: [250, 220, 120], charts: [chart(8, [90, 110, 120], 120)] }, { alpha: "BLEND", surface: glazing, charts: [chart(100, [60, 80, 90], 90)] },
    { surface: steel, edge: true, charts: [chart(8, [150, 150, 160])] }, { surface: steel, charts: [chart(100, [120, 90, 70])] }, { surface: fittings, charts: [chart(8, [200, 180, 40])] },
  ];
  const kept = await building(pages, { tier: { unify: [] } });
  assert.equal(kept.primitives.length, 5, "without leave to unify, every difference keeps its material");
  const { primitives } = await building(pages, { tier: { unify: ["noEdge", "hardware", "glazingSides", "glazingGlow"] } });
  assert.equal(primitives.length, 2);
  const glass = primitives.find(({ material }) => material.alphaMode === "BLEND"), metal = primitives.find(({ material }) => material.alphaMode !== "BLEND");
  assert.ok(!glass.material.doubleSided && metal.material.doubleSided, "only the glass turns single-sided");
  assert.ok(!metal.material.occlusionTexture && !metal.material.extras.materialSpecEdge);
  assert.equal(metal.material.extras.worldSpecSurface.finish, "weathered-steel");
  assert.equal(metal.sources.length, 12);
  // The lit pane still glows its own colour and the unlit pane emits nothing.
  for (let quad = 0; quad < glass.sources.length; quad += 4) {
    const u = (glass.uv[quad * 2] + glass.uv[(quad + 2) * 2]) / 2, v = (glass.uv[quad * 2 + 1] + glass.uv[(quad + 2) * 2 + 1]) / 2;
    close(bilinear(glass.emissive, u, v), pages[glass.sources[quad].page].emissive ?? [0, 0, 0], 2.5, "pane emission");
    close(bilinear(glass.base, u, v), [...glass.sources[quad].chart.color, glass.sources[quad].chart.alpha], 2.5, "pane colour and coverage");
  }
});

test("untextured primitives that differ only in colour become one, in the same colours", async () => {
  const { mergeFlatColours } = await import("./mobile-atlas.mjs");
  const views = [], json = { bufferViews: [], accessors: [], materials: [], meshes: [{ name: "city/prototype/part/run/run/body", primitives: [] }] };
  const accessor = (array, type, componentType, extra = {}) => { views.push(Buffer.from(array.buffer)); return json.accessors.push({ bufferView: json.bufferViews.push({ buffer: 0, byteLength: array.byteLength }) - 1,
    type, componentType, count: array.length / (type === "SCALAR" ? 1 : 3), ...extra }) - 1; };
  const part = (id, factor, extra = {}, vertexColor) => {
    const material = json.materials.push({ name: "Duct register " + id, pbrMetallicRoughness: { baseColorFactor: factor, metallicFactor: 0 }, extras: { worldSpecDuct: true }, ...extra }) - 1;
    json.meshes[0].primitives.push({ mode: 4, material, indices: accessor(Uint16Array.from([0, 1, 2]), "SCALAR", 5123), attributes: {
      POSITION: accessor(Int16Array.from([id * 100, 0, 0, id * 100 + 10, 0, 0, id * 100, 10, 0]), "VEC3", 5122, { normalized: true }),
      NORMAL: accessor(Int16Array.from([0, 0, 32767, 0, 0, 32767, 0, 0, 32767]), "VEC3", 5122, { normalized: true }),
      ...(vertexColor ? { COLOR_0: accessor(Uint8Array.from([...vertexColor, ...vertexColor, ...vertexColor]), "VEC3", 5121, { normalized: true }) } : {}) } });
    return { id, shown: factor.slice(0, 3).map((value, k) => value * (vertexColor ? vertexColor[k] / 255 : 1)) };
  };
  const parts = [part(1, [.8, .2, .1, 1]), part(2, [.1, .5, .9, 1]), part(3, [1, 1, 1, 1], {}, [128, 64, 255]), part(4, [.3, .3, .3, 1], { doubleSided: true })];
  const replacement = new Map(), removed = await mergeFlatColours(json, json.meshes[0], { bytesOf: async id => replacement.get(id)?.bytes ?? views[id], replacement, lods: new Map() });
  assert.equal(removed, 2);
  assert.equal(json.meshes[0].primitives.length, 2, "the double-sided part keeps its own material");
  const merged = json.meshes[0].primitives.find(primitive => json.accessors[primitive.indices].count === 9), material = json.materials[merged.material];
  assert.equal(material.pbrMetallicRoughness.baseColorFactor, undefined);
  assert.equal(material.extras.worldSpecDuct, true);
  const at = id => { const a = json.accessors[id], bytes = replacement.get(a.bufferView).bytes; return { a, bytes, stride: json.bufferViews[a.bufferView].byteStride }; };
  const position = at(merged.attributes.POSITION), color = at(merged.attributes.COLOR_0);
  for (let vertex = 0; vertex < 9; vertex++) {
    const id = Math.round(position.bytes.readInt16LE(position.a.byteOffset + vertex * position.stride) / 100), expected = parts.find(item => item.id === id).shown;
    for (let k = 0; k < 3; k++) assert.ok(Math.abs(color.bytes[color.a.byteOffset + vertex * color.stride + k] / 255 - expected[k]) < .004, "vertex colour times a white factor is the old factor");
  }
});
test("charts that cover different halves of the same rectangle do not share texels", async () => {
  // A wall quad baked as two triangle charts of one size: the lower-left half of one
  // rectangle and the upper-right half of another. Outside its triangle a chart holds no surface.
  const views = [], json = { asset: { version: "2.0" }, scenes: [{ nodes: [0] }], nodes: [{ mesh: 0 }], buffers: [{ byteLength: 0 }], bufferViews: [], accessors: [],
    images: [], textures: [], samplers: [{}], materials: [], meshes: [{ name: "building-0", primitives: [] }] };
  const view = bytes => { views.push(Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength)); return json.bufferViews.push({ buffer: 0, byteLength: bytes.byteLength }) - 1; };
  const accessor = (array, type, componentType, extra = {}) => json.accessors.push({ bufferView: view(array), type, componentType, count: array.length / { SCALAR: 1, VEC2: 2, VEC3: 3 }[type], ...extra }) - 1;
  const rects = [{ x: 16, y: 16, lower: true }, { x: 128, y: 16, lower: false }, { x: 16, y: 128, lower: true }], side = 64, wall = [150, 140, 120], junk = [255, 0, 255];
  const raw = Buffer.alloc(PAGE * PAGE * 4);
  for (let y = 0; y < PAGE; y++) for (let x = 0; x < PAGE; x++) {
    const rect = rects.find(r => x >= r.x && x < r.x + side && y >= r.y && y < r.y + side), s = rect && (x - rect.x) / side, t = rect && (y - rect.y) / side;
    raw.set([...(rect && (rect.lower ? s <= t + .08 : s >= t - .08) ? wall : junk), 255], (y * PAGE + x) * 4);
  }
  const texture = json.textures.push({ sampler: 0, source: json.images.push({ mimeType: "image/png", bufferView: view(await sharp(raw, { raw: { width: PAGE, height: PAGE, channels: 4 } }).png().toBuffer()) }) - 1 }) - 1;
  for (const half of [0, 1]) {
    const positions = [], uvs = [];
    for (const rect of half ? [rects[1]] : [rects[0], rects[2]]) for (const [s, t] of rect.lower ? [[0, 0], [1, 1], [0, 1]] : [[0, 0], [1, 0], [1, 1]]) {
      positions.push(rects.indexOf(rect) * 1000 + s * 100, t * 100, 0);
      uvs.push(Math.round((rect.x + s * side) / PAGE * 65535), Math.round((rect.y + t * side) / PAGE * 65535));
    }
    json.meshes[0].primitives.push({ mode: 4, indices: accessor(Uint16Array.from(positions.map((_, i) => i).slice(0, positions.length / 3)), "SCALAR", 5123),
      material: json.materials.push({ name: "sheet " + half, doubleSided: true, extras: { materialSpecNormalFrame: true, worldSpecSurface: { family: "Masonry", surface: "Concrete and masonry",
        recipe: "architectural-concrete", reflection: "none" } }, pbrMetallicRoughness: { baseColorTexture: { index: texture } } }) - 1,
      attributes: { POSITION: accessor(Int16Array.from(positions), "VEC3", 5122, { normalized: true }), NORMAL: accessor(Int16Array.from(positions.map((_, i) => i % 3 === 2 ? 32767 : 0)), "VEC3", 5122, { normalized: true }),
        COLOR_0: accessor(new Uint8Array(positions.length).fill(255), "VEC3", 5121, { normalized: true }), TEXCOORD_0: accessor(Uint16Array.from(uvs), "VEC2", 5123, { normalized: true }),
        _MS_ASPECT: accessor(new Float32Array(positions.length / 3).fill(1), "SCALAR", 5126) } });
  }
  const replacement = new Map();
  const { pages, report } = await mergeMobileAtlases(json, { tier: { ...tier, share: { representatives: 1, maxDifference: 9 } }, read: async id => views[id], replacement, textureSize });
  assert.equal(report.sharedCharts, 1, "the two lower-left halves share; the upper-right half keeps its own texels");
  const [primitive] = json.meshes[0].primitives, bytesOf = id => replacement.get(id)?.bytes ?? pages.get(id);
  const uv = json.accessors[primitive.attributes.TEXCOORD_0], bytes = bytesOf(uv.bufferView), stride = json.bufferViews[uv.bufferView].byteStride;
  const { data, info } = await sharp(bytesOf(json.images[json.textures[json.materials[primitive.material].pbrMetallicRoughness.baseColorTexture.index].source].bufferView)).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  for (let triangle = 0; triangle < uv.count; triangle += 3) {
    const corner = k => [bytes.readUInt16LE(uv.byteOffset + (triangle + k) * stride) / 65535, bytes.readUInt16LE(uv.byteOffset + (triangle + k) * stride + 2) / 65535];
    const [a, b, c] = [corner(0), corner(1), corner(2)];
    close(bilinear({ data, width: info.width, height: info.height }, (a[0] + b[0] + c[0]) / 3, (a[1] + b[1] + c[1]) / 3), wall, 3, "a triangle shows wall, not the unbaked half of another chart");
  }
});
test("charts the bake already placed keep their rectangle and the bake's gutter", async () => {
  const { packSlots } = await import("./mobile-atlas.mjs");
  const sizes = [[1, 1], [3, 2], [5, 7], [40, 9], [1, 6]];
  const placed = sizes.map(([width, height]) => ({ width, height, page: { placed: true } }));
  const found = sizes.map(([width, height]) => ({ width, height, page: { placed: false } }));
  const tier = { limit: 256, gutter: 4, smallGutter: 2 };
  const baked = packSlots(placed, tier), regridded = packSlots(found, tier);
  for (const page of baked.pages) {
    const taken = new Uint8Array(page.width * page.height);
    for (const { slot, pad, x, y, cell: [cx, cy, cw, ch] } of page.items) {
      assert.equal(pad, 2);
      assert.deepEqual([cw, ch, x - cx, y - cy], [slot.width + 4, slot.height + 4, 2, 2], "two texels on every side, no rounding");
      for (let row = cy; row < cy + ch; row++) for (let column = cx; column < cx + cw; column++) assert.equal(taken[row * page.width + column]++, 0);
    }
  }
  assert.ok(baked.cellTexels < regridded.cellTexels, "a second gutter and its grid cost texels the bake already spent");
});
