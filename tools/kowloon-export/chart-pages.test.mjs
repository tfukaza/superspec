import assert from "node:assert/strict";
import test from "node:test";
import { chartPages } from "./chart-pages.mjs";

test("opaque charts share shelves with intact gutters, texel density and JPEG block phase", () => {
  const charts = Array.from({ length: 90 }, (_, id) => ({
    id, w: [4, 10, 18, 12, 28][id % 5], h: [10, 8, 6][Math.floor(id / 30)],
    repack: true, emitter: id % 17 === 0, hasEdge: id % 3 === 0,
  }));
  const limit = 64, pad = 2;
  const control = chartPages(charts.map(chart => ({ ...chart, repack: false })), limit, pad);
  const original = new Map(control.flatMap(page => page.items.map(item => [item.id, item])));
  const optimized = chartPages(charts, limit, pad);
  assert.ok(optimized.length <= control.length);
  const channelTexels = pages => pages.reduce((n, p) => n + p.width * p.height *
    (3 + Number(p.items.some(i => i.emitter)) + Number(p.items.some(i => i.hasEdge))), 0);
  assert.ok(channelTexels(optimized) < channelTexels(control));
  const seen = [];
  for (const { items, width, height } of optimized) {
    assert.ok(width <= limit && height <= limit);
    const occupied = new Uint8Array(width * height);
    for (const item of items) {
      const old = original.get(item.id);
      assert.deepEqual([item.w, item.h, item.x & 7, item.y & 7], [old.w, old.h, old.x & 7, old.y & 7]);
      assert.ok(item.x >= pad && item.y >= pad && item.x + item.w + pad <= width && item.y + item.h + pad <= height);
      for (let y = item.y - pad; y < item.y + item.h + pad; y++)
        for (let x = item.x - pad; x < item.x + item.w + pad; x++)
          assert.equal(occupied[y * width + x]++, 0);
      seen.push(item.id);
    }
  }
  assert.deepEqual(seen.sort((a, b) => a - b), charts.map(item => item.id));
  // A page with a transparent receiver or out-of-range UVs retains every chart.
  const pinned = control[1].items;
  const id = pinned[0].id;
  const protectedPages = chartPages(charts.map(c => ({ ...c, repack: c.id !== id })), limit, pad);
  assert.deepEqual(protectedPages.find(p => p.items.some(c => c.id === id)).items.map(c => [c.id, c.x, c.y]),
    pinned.map(c => [c.id, c.x, c.y]));
});
