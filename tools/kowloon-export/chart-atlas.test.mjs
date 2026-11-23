import assert from "node:assert/strict";
import test from "node:test";
import { bakeKey, bakeSettings, bakedPageSize, bakedTexelDensities, chartPlacement, dielectricFinish, filterChart, keptFinish, packBlock,
  packCharts, receiverCharts, scaledEdge, tierDensities } from "./chart-atlas.mjs";

const RECORD = 38, GUTTER = 2;

/** A receiver atlas of rectangles side by side, each a pair of triangles on its inner corners. */
function receiver(rectangles, width, height) {
  const records = new Float32Array(rectangles.length * 2 * RECORD);
  rectangles.forEach(({ x, y, w, h }, chart) => {
    const left = (x + GUTTER) / width, right = (x + w - GUTTER) / width, top = 1 - (y + GUTTER) / height, bottom = 1 - (y + h - GUTTER) / height;
    for (const [half, corners] of [[left, top, right, top, left, bottom], [right, bottom, left, bottom, right, top]].entries()) {
      const at = (chart * 2 + half) * RECORD;
      records.set([x, y, w, h], at);
      records.set(corners, at + 32);
    }
  });
  return { width, height, gutter: GUTTER, triangles: rectangles.length * 2, records };
}

function image(width, height, paint) {
  const data = new Uint8Array(width * height * 4);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) data.set(paint(x, y), (y * width + x) * 4);
  return data;
}

test("a chart's gutter holds at the tier's own resolution and takes nothing from its neighbours", () => {
  // Two charts touch in the baked atlas: 64 inner texels each, red beside blue.
  const width = 2 * (64 + 2 * GUTTER), height = 64 + 2 * GUTTER;
  const source = image(width, height, x => x < width / 2 ? [255, 0, 0, 255] : [0, 0, 255, 255]);
  for (const density of [tierDensities.low, tierDensities["very-low"]]) {
    const inner = scaledEdge(64, density), side = inner + 2 * GUTTER;
    assert.equal(inner, 64 * density);
    const page = new Uint8Array(2 * side * side * 4).fill(7);
    for (const [index, x] of [0, width / 2].entries())
      filterChart(source, width, { x, y: 0, w: 64 + 2 * GUTTER, h: height, gutter: GUTTER }, page, 2 * side,
        { x: index * side + GUTTER, y: GUTTER, w: inner, h: inner, pad: GUTTER }, "color");
    for (let y = 0; y < side; y++) for (let x = 0; x < 2 * side; x++) {
      const at = (y * 2 * side + x) * 4;
      assert.deepEqual([...page.subarray(at, at + 4)], x < side ? [255, 0, 0, 255] : [0, 0, 255, 255], `density ${density} texel ${x},${y}`);
    }
  }
  // Shrinking the finished page instead leaves less than one gutter texel between the two.
  assert.ok(GUTTER * tierDensities.low < 1);
});

test("reduced charts average the baked texels under each texel", () => {
  // One-texel stripes: any point sample is black or white, the area under a texel is half of each.
  const size = 32 + 2 * GUTTER;
  const stripes = image(size, size, x => x % 2 ? [255, 255, 255, 255] : [0, 0, 0, 255]);
  const filtered = kind => {
    const page = new Uint8Array(12 * 12 * 4);
    filterChart(stripes, size, { x: 0, y: 0, w: size, h: size, gutter: GUTTER }, page, 12, { x: GUTTER, y: GUTTER, w: 8, h: 8, pad: GUTTER }, kind);
    return page;
  };
  const half = Math.round(255 * (1.055 * 0.5 ** (1 / 2.4) - 0.055));
  const color = filtered("color"), data = filtered("data");
  for (let y = GUTTER; y < 10; y++) for (let x = GUTTER; x < 10; x++) {
    assert.ok(Math.abs(color[(y * 12 + x) * 4] - half) <= 1, "colour averages in linear light");
    assert.ok(Math.abs(data[(y * 12 + x) * 4] - 127.5) <= 1, "data averages as stored");
  }
  // Normals leaning opposite ways average to the flat normal at full length, not a short vector.
  const lean = Math.round(127.5 + 127.5 * Math.SQRT1_2);
  const ridges = image(size, size, x => [x % 2 ? lean : 255 - lean, 128, lean, 255]);
  const page = new Uint8Array(12 * 12 * 4);
  filterChart(ridges, size, { x: 0, y: 0, w: size, h: size, gutter: GUTTER }, page, 12, { x: GUTTER, y: GUTTER, w: 8, h: 8, pad: GUTTER }, "normal");
  for (let at = 0; at < page.length; at += 4) {
    const normal = [0, 1, 2].map(axis => page[at + axis] / 127.5 - 1);
    assert.ok(Math.abs(Math.hypot(...normal) - 1) < 0.02 && normal[2] > 0.98, "unit, flat normal");
  }
});

test("a reduced chart keeps the light of the chart it was baked as", () => {
  // Uneven sizes and scales: the mean of the inner texels in linear light survives the reduction.
  let seed = 99;
  const next = () => (seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 2 ** 32;
  const linear = value => value <= 0.04045 * 255 ? value / 255 / 12.92 : ((value / 255 + 0.055) / 1.055) ** 2.4;
  for (const [iw, ih, tw, th] of [[64, 40, 16, 10], [37, 23, 9, 6], [50, 9, 6, 1], [13, 13, 2, 2], [5, 4, 1, 1]]) {
    const w = iw + 2 * GUTTER, h = ih + 2 * GUTTER;
    const source = image(w + 6, h + 5, () => [Math.floor(next() * 256), Math.floor(next() * 256), Math.floor(next() * 256), 255]);
    const page = new Uint8Array((tw + 8) * (th + 8) * 4);
    filterChart(source, w + 6, { x: 3, y: 2, w, h, gutter: GUTTER }, page, tw + 8, { x: 4, y: 4, w: tw, h: th, pad: GUTTER }, "color");
    for (let channel = 0; channel < 3; channel++) {
      let before = 0, after = 0;
      for (let y = 0; y < ih; y++) for (let x = 0; x < iw; x++) before += linear(source[((2 + GUTTER + y) * (w + 6) + 3 + GUTTER + x) * 4 + channel]);
      for (let y = 0; y < th; y++) for (let x = 0; x < tw; x++) after += linear(page[((4 + y) * (tw + 8) + 4 + x) * 4 + channel]);
      assert.ok(Math.abs(before / (iw * ih) - after / (tw * th)) < 0.01, `${iw}x${ih} to ${tw}x${th}`);
    }
  }
});

test("a chart at the authored density keeps its baked texels, gutter included", () => {
  const width = 40, height = 24;
  const source = image(width, height, (x, y) => [x * 5, y * 9, (x * y) % 251, 255 - x]);
  const chart = { x: 7, y: 3, w: 21, h: 14, gutter: GUTTER }, page = new Uint8Array(64 * 32 * 4);
  for (const kind of ["color", "data", "normal"]) {
    filterChart(source, width, chart, page, 64, { x: 30, y: 9, w: 17, h: 10, pad: GUTTER }, kind);
    for (let y = 0; y < chart.h; y++) for (let x = 0; x < chart.w; x++) {
      const from = ((chart.y + y) * width + chart.x + x) * 4, to = ((9 - GUTTER + y) * 64 + 30 - GUTTER + x) * 4;
      assert.deepEqual([...page.subarray(to, to + 4)], [...source.subarray(from, from + 4)]);
    }
  }
});

test("a different density, page limit or layout is a different bake", () => {
  const key = settings => bakeKey(["producer"], "<architecture />", { nativeMeshes: [] }, settings);
  const medium = key({ density: tierDensities.medium });
  assert.equal(key({}), medium, "the authored density is the default");
  assert.equal(key({ density: 1, pageLimit: 4096, layout: "charts" }), medium);
  const keys = [medium, key({ density: tierDensities.low }), key({ density: tierDensities["very-low"] }),
    key({ pageLimit: 2048 }), key({ layout: "receivers" }), key({ layout: "receivers", pageLimit: 4096 })];
  assert.equal(new Set(keys).size, keys.length);
  assert.notEqual(bakeKey(["producer"], "<architecture id='other' />", { nativeMeshes: [] }, {}), medium);
  assert.throws(() => bakeSettings({ density: 0 }));
  assert.throws(() => bakeSettings({ density: 2 }));
  assert.throws(() => bakeSettings({ density: 0.25, layout: "receivers" }), "whole atlases cannot hold a reduced gutter");
});

test("a delivery leaves a page baked at its tier alone and scales the rest by what is left", () => {
  const texture = index => ({ index });
  const json = {
    images: Array.from({ length: 7 }, () => ({})),
    textures: [{ source: 0 }, { source: 1 }, { extensions: { EXT_texture_webp: { source: 2 } } }, { source: 3 }, { source: 4 }, { source: 5 }, { source: 6 }],
    materials: [
      { extras: { worldSpecTexelDensity: 0.25, worldSpecSurface: { detailTexture: 2 } }, pbrMetallicRoughness: { baseColorTexture: texture(0) }, normalTexture: texture(1) },
      { extras: { worldSpecTexelDensity: 1 }, pbrMetallicRoughness: { baseColorTexture: texture(3), metallicRoughnessTexture: texture(4) } },
      // A catalogue prop states nothing, and so does every image it shares.
      { pbrMetallicRoughness: { baseColorTexture: texture(5) }, emissiveTexture: texture(4) },
      { extras: { worldSpecTexelDensity: 0.125 }, occlusionTexture: texture(6) },
      { extras: { worldSpecTexelDensity: 0.25 }, occlusionTexture: texture(6) },
    ],
  };
  assert.deepEqual([...bakedTexelDensities(json)].sort(([a], [b]) => a - b), [[0, 0.25], [1, 0.25], [2, 0.25], [3, 1]]);
  // The page the bake sized for Low is Low's page, however large; Very Low keeps half of it.
  assert.deepEqual(bakedPageSize(4096, 3000, tierDensities.low, tierDensities.low), { width: 4096, height: 3000 });
  assert.deepEqual(bakedPageSize(4096, 3000, tierDensities.low, tierDensities["very-low"]), { width: 2048, height: 1500 });
  assert.deepEqual(bakedPageSize(4096, 3000, tierDensities["very-low"], tierDensities.low), { width: 4096, height: 3000 }, "never enlarged");
  // An authored page keeps the tier's share of every edge, so its texel density is the tier's.
  assert.deepEqual(bakedPageSize(4096, 2048, 1, tierDensities.low), { width: 1024, height: 512 });
  assert.deepEqual(bakedPageSize(4094, 30, 1, tierDensities.low, 4), { width: 1024, height: 8 });
});

/** Deterministic chart sizes: many small parts, some long walls. */
function sizes(count, seed = 12345) {
  const next = () => (seed = (Math.imul(seed, 1103515245) + 12345) >>> 0) / 2 ** 32;
  return Array.from({ length: count }, (_, id) => {
    const wall = next() < 0.04;
    return { id, w: 1 + Math.floor(next() * (wall ? 480 : 60)), h: 1 + Math.floor(next() * (wall ? 200 : 24)) };
  });
}

test("packed charts keep their size, stay on their page and never overlap", () => {
  for (const count of [1500, 40]) {
    const charts = sizes(count), limit = 512, pad = GUTTER;
    const pages = packCharts(charts, limit, pad);
    const seen = new Set();
    let placed = 0, texels = 0;
    for (const { width, height, items } of pages) {
      assert.ok(width <= limit && height <= limit && width % 4 === 0 && height % 4 === 0);
      const occupied = new Uint8Array(width * height);
      for (const { chart, x, y } of items) {
        assert.equal(seen.has(chart.id), false);
        seen.add(chart.id);
        assert.deepEqual([chart.w, chart.h], [charts[chart.id].w, charts[chart.id].h]);
        assert.ok(x >= pad && y >= pad && x + chart.w + pad <= width && y + chart.h + pad <= height, "the gutter is on the page");
        for (let row = y - pad; row < y + chart.h + pad; row++) for (let column = x - pad; column < x + chart.w + pad; column++)
          assert.equal(occupied[row * width + column]++, 0, "a texel belongs to one chart");
        placed += (chart.w + 2 * pad) * (chart.h + 2 * pad);
      }
      texels += width * height;
    }
    assert.equal(seen.size, charts.length);
    // Shelves of like heights leave little unused, on full pages and on a last, narrower one.
    assert.ok(placed / texels > (count > 1000 ? 0.8 : 0.5), `fill ${placed / texels}`);
  }
  // A chart longer than the limit still gets a page; nothing else is turned away.
  const [page] = packCharts([{ w: 700, h: 8 }], 512, GUTTER);
  assert.ok(page.width >= 704 && page.height >= 12);
  assert.deepEqual(packCharts([], 512, GUTTER), []);
});

test("a part's charts stay together in one block that packs like a chart", () => {
  const parts = [sizes(300, 7), sizes(40, 11), sizes(1, 3)], pad = GUTTER;
  const blocks = parts.map((charts, part) => {
    const block = packBlock(charts, pad), occupied = new Uint8Array(block.width * block.height);
    assert.deepEqual(block.items.map(({ chart }) => chart.id).sort((a, b) => a - b), charts.map(chart => chart.id));
    for (const { chart, x, y } of block.items) {
      assert.ok(x >= pad && y >= pad && x + chart.w + pad <= block.width && y + chart.h + pad <= block.height);
      for (let row = y - pad; row < y + chart.h + pad; row++) for (let column = x - pad; column < x + chart.w + pad; column++)
        assert.equal(occupied[row * block.width + column]++, 0);
    }
    return { part, w: block.width, h: block.height, block };
  });
  // Blocks carry their charts' gutters, so they pack edge to edge.
  const pages = packCharts(blocks, 1024, 0);
  for (const { width, height, items } of pages) {
    const owner = new Int8Array(width * height).fill(-1);
    for (const { chart: unit, x, y } of items) for (const { chart, x: dx, y: dy } of unit.block.items)
      for (let row = y + dy - pad; row < y + dy + chart.h + pad; row++) for (let column = x + dx - pad; column < x + dx + chart.w + pad; column++) {
        assert.equal(owner[row * width + column], -1, "a texel belongs to one chart of one part");
        owner[row * width + column] = unit.part;
      }
    // Every texel beside a chart of a part is that part's or unused, up to the block's edge.
    for (const { chart: unit, x, y } of items) for (let row = y; row < y + unit.h; row++) for (let column = x; column < x + unit.w; column++)
      assert.ok([-1, unit.part].includes(owner[row * width + column]));
  }
  assert.equal(pages.flatMap(page => page.items).length, parts.length);
});

test("finish roles follow the publisher's own tests", () => {
  for (const finish of ["sign / board", "room / posters", "tube-light / glass", "lamp / lens", "blade / paint", "flat / board"]) assert.ok(keptFinish(finish), finish);
  for (const finish of ["wall / front", "cage / steel", "design / tile"]) assert.equal(keptFinish(finish), false, finish);
  for (const finish of ["wall / front", "balcony / concrete", "window / pane"]) assert.ok(dielectricFinish(finish), finish);
  for (const finish of ["cage / steel", "duct / galvanized"]) assert.equal(dielectricFinish(finish), false, finish);
});

test("every triangle's UVs land inside its own chart on the page at every tier", () => {
  const rectangles = [{ x: 0, y: 0, w: 100, h: 40 }, { x: 100, y: 0, w: 28, h: 12 }, { x: 0, y: 40, w: 9, h: 9 }, { x: 9, y: 40, w: 64, h: 6 }];
  const layout = receiver(rectangles, 128, 64);
  const { charts, owner } = receiverCharts(layout);
  assert.deepEqual(charts, rectangles);
  assert.deepEqual([...owner], [0, 0, 1, 1, 2, 2, 3, 3]);
  for (const density of Object.values(tierDensities)) {
    // Rows of a readback count from the lower edge, as the bake reads them.
    const placed = charts.map((c, id) => ({ id, source: { x: c.x, y: layout.height - c.y - c.h, w: c.w, h: c.h, gutter: GUTTER },
      w: scaledEdge(c.w - 2 * GUTTER, density), h: scaledEdge(c.h - 2 * GUTTER, density) }));
    for (const chart of placed) assert.ok(chart.w >= 1 && chart.h >= 1);
    const pages = packCharts(placed, 64, GUTTER);
    const at = new Map(pages.flatMap((page, index) => page.items.map(({ chart, x, y }) => [chart.id, { index, x, y }])));
    for (let triangle = 0; triangle < layout.triangles; triangle++) {
      const chart = placed[owner[triangle]], place = at.get(chart.id);
      const { sx, sy, ox, oy } = chartPlacement(chart.source, { ...place, w: chart.w, h: chart.h });
      for (let corner = 0; corner < 3; corner++) {
        const u = layout.records[triangle * RECORD + 32 + corner * 2] * layout.width, v = layout.records[triangle * RECORD + 33 + corner * 2] * layout.height;
        const x = u * sx + ox, y = v * sy + oy;
        // Inside the inner rectangle, so bilinear filtering reads this chart and its gutter only.
        assert.ok(x >= place.x - 1e-4 && x <= place.x + chart.w + 1e-4 && y >= place.y - 1e-4 && y <= place.y + chart.h + 1e-4,
          `density ${density} triangle ${triangle}`);
        for (const [other, spot] of at) if (other !== chart.id && spot.index === place.index) {
          const o = placed[other];
          assert.ok(x <= spot.x - GUTTER || x >= spot.x + o.w + GUTTER || y <= spot.y - GUTTER || y >= spot.y + o.h + GUTTER, "outside every other chart");
        }
      }
      if (density === 1) assert.deepEqual([sx, sy], [1, 1], "the authored density moves charts without resampling them");
    }
  }
});

test("a receiver whose triangles leave their chart keeps its atlas whole", () => {
  const layout = receiver([{ x: 0, y: 0, w: 32, h: 16 }, { x: 32, y: 0, w: 32, h: 16 }], 64, 16);
  assert.ok(receiverCharts(layout));
  const reaching = { ...layout, records: Float32Array.from(layout.records) };
  reaching.records[32] = 40 / 64; // The first corner now lies in the second chart.
  assert.equal(receiverCharts(reaching), undefined);
  const resized = { ...layout, records: Float32Array.from(layout.records) };
  resized.records[RECORD + 2] = 30; // Two triangles disagree about their chart's size.
  assert.equal(receiverCharts(resized), undefined);
});
