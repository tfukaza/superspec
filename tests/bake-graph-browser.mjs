/**
 * The GPU bake's pass graph on a real WebGL context (tests/browser/bake-graph.fixture.ts): the separable blur node on a
 * synthetic target, checked against a Gaussian computed here, and the intrinsic-plus-mapping shape, where receivers
 * share one intrinsic node and each draws only its own mapping. The graph's scheduling itself is tested without a GPU
 * in tests/browser/bake-graph.test.ts.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { build } from "vite";
import { launchBrowser } from "../tools/browser.mjs";

const root = resolve("generated/bake-graph-browser");
await build({ configFile: false, root: "tests/browser", publicDir: false, logLevel: "warn",
  build: { outDir: root, emptyOutDir: true, rollupOptions: { input: resolve("tests/browser/bake-graph.html") } } });
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "bake-graph.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

/** One direction of the blur as the shader defines it: Gaussian weights over `radius` taps each side, the edge repeated. */
function blurAlong(texels, width, height, radius, sigma, dx, dy) {
  const out = new Float64Array(texels.length);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const sum = [0, 0, 0, 0];
    let weights = 0;
    for (let tap = -radius; tap <= radius; tap++) {
      const weight = Math.exp(-0.5 * tap * tap / (sigma * sigma));
      const sx = Math.min(Math.max(x + tap * dx, 0), width - 1), sy = Math.min(Math.max(y + tap * dy, 0), height - 1);
      for (let channel = 0; channel < 4; channel++) sum[channel] += weight * texels[(sy * width + sx) * 4 + channel];
      weights += weight;
    }
    for (let channel = 0; channel < 4; channel++) out[(y * width + x) * 4 + channel] = sum[channel] / weights;
  }
  return out;
}

function close(actual, expected, tolerance, what) {
  assert.equal(actual.length, expected.length, what);
  let worst = 0, at = 0;
  for (let index = 0; index < actual.length; index++) {
    // Relative for values above one: the texels are 32-bit floats.
    const difference = Math.abs(actual[index] - expected[index]) / Math.max(1, Math.abs(expected[index]));
    if (difference > worst) { worst = difference; at = index; }
  }
  assert.ok(worst <= tolerance, `${what}: texel ${at >> 2} channel ${at & 3} is ${actual[at]}, expected ${expected[at]}`);
  return worst;
}

let browser;
try {
  browser = await launchBrowser();
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => window.driver);
  assert.equal(await page.evaluate(() => window.driver.unavailable()), null);

  // ---- Blur ----
  const blur = (gain, radius, bandTexels) => page.evaluate(([gain, radius, bandTexels]) => window.driver.blur(gain, radius, bandTexels), [gain, radius, bandTexels]);
  const first = await blur(1, 4);
  assert.deepEqual(first.drawn, ["source", "blurred:rows", "blurred"]);
  const { width, height } = first;
  // The synthetic source is what the test says it is: an impulse, a step and a ramp.
  const at = (texels, x, y, channel) => texels[(y * width + x) * 4 + channel];
  assert.equal(at(first.source, 20, 12, 0), 1); assert.equal(at(first.source, 21, 12, 0), 0);
  assert.equal(at(first.source, 15, 3, 1), 0); assert.equal(at(first.source, 16, 3, 1), 1);
  const rows = blurAlong(first.source, width, height, 4, 2, 1, 0);
  const worstRows = close(first.rows, rows, 1e-6, "the rows pass");
  const worst = close(first.blurred, blurAlong(rows, width, height, 4, 2, 0, 1), 1e-6, "the blurred node");
  // The blur spreads the impulse and keeps its sum; along the ramp's edge the repeated edge texel shows.
  let sum = 0;
  for (let index = 0; index < first.blurred.length; index += 4) sum += first.blurred[index];
  assert.ok(Math.abs(sum - 1) < 1e-5, `the impulse's energy is kept (${sum})`);
  assert.ok(at(first.blurred, 20, 12, 0) < 0.1 && at(first.blurred, 22, 13, 0) > 0, "the impulse is spread");

  // Nothing changed: nothing is drawn. A wider blur draws the blur's two nodes and reuses the source; a changed source draws all three.
  assert.deepEqual((await blur(1, 4)).drawn, []);
  const wider = await blur(1, 7);
  assert.deepEqual(wider.drawn, ["blurred:rows", "blurred"]);
  assert.equal(wider.reused, 1);
  close(wider.blurred, blurAlong(blurAlong(wider.source, width, height, 7, 3.5, 1, 0), width, height, 7, 3.5, 0, 1), 1e-6, "the wider blur");
  const brighter = await blur(2, 7);
  assert.deepEqual(brighter.drawn, ["source", "blurred:rows", "blurred"]);
  assert.equal(at(brighter.source, 20, 12, 0), 2);
  // Drawn in bands (with the browser given turns between them), the texels are the same.
  const banded = await blur(3, 7, width * 5);
  assert.equal(banded.bands, Math.ceil(height / 5));
  close(banded.blurred, brighter.blurred.map(value => value * 1.5), 1e-6, "the banded blur");

  // ---- Intrinsic + mapping ----
  const mapping = (receiver, gain, x, size) => page.evaluate(([receiver, gain, x, size]) => window.driver.mapping(receiver, gain, x, size), [receiver, gain, x, size]);
  /** What every inner texel of the receiver's chart must read: the intrinsic value at its surface point. */
  const expected = (gain, x, size) => {
    const texels = [];
    for (let ty = 2; ty < 10; ty++) for (let tx = 2; tx < 10; tx++) {
      texels.push({ tx, ty, value: [gain * (x + size * (tx - 1.5) / 8) / 4, gain * (size * (ty - 1.5) / 8) / 4] });
    }
    return texels;
  };
  const check = (made, gain, x, size, what) => {
    let worst = 0;
    for (const { tx, ty, value } of expected(gain, x, size)) {
      // Atlas rows count from the top; the read-back's from the bottom.
      const index = ((15 - ty) * 16 + tx) * 4;
      for (let channel = 0; channel < 2; channel++) worst = Math.max(worst, Math.abs(made.mapped[index + channel] - value[channel]));
    }
    assert.ok(worst < 2e-3, `${what}: the mapped texels are the intrinsic values at their surface points (worst ${worst})`);
    return worst;
  };
  const wall = await mapping("wall", 1, 0, 1);
  assert.deepEqual(wall.drawn, ["intrinsic", "mapped"]);
  const worstMapping = check(wall, 1, 0, 1, "wall");
  // A second receiver elsewhere on the surface: the intrinsic node is shared (one texture, two users), only its mapping is drawn.
  const slab = await mapping("slab", 1, 2, 2);
  assert.deepEqual(slab.drawn, ["mapped"]);
  assert.equal(slab.intrinsic, wall.intrinsic);
  assert.equal(slab.intrinsicUsers, 2);
  check(slab, 1, 2, 2, "slab");
  // The wall moves and grows: its mapping is drawn again, the intrinsic bake is not.
  const moved = await mapping("wall", 1, 1, 2);
  assert.deepEqual(moved.drawn, ["mapped"]);
  assert.equal(moved.intrinsic, wall.intrinsic);
  check(moved, 1, 1, 2, "moved wall");
  // The appearance changes: the intrinsic bake is drawn once, and each receiver's mapping once.
  const wall2 = await mapping("wall", 0.5, 1, 2), slab2 = await mapping("slab", 0.5, 2, 2);
  assert.deepEqual(wall2.drawn, ["intrinsic", "mapped"]);
  assert.deepEqual(slab2.drawn, ["mapped"]);
  assert.equal(slab2.intrinsic, wall2.intrinsic);
  assert.notEqual(wall2.intrinsic, wall.intrinsic);
  check(wall2, 0.5, 1, 2, "wall after the change"); check(slab2, 0.5, 2, 2, "slab after the change");

  const mixedMaps = (...args) => page.evaluate(args => window.driver.mixedWallMaps(...args), args);
  const full = await mixedMaps(false, 2), mixed = await mixedMaps(true, 2);
  assert.deepEqual(mixed.base, full.base);
  assert.deepEqual(mixed.properties, full.properties);
  assert.ok(mixed.drawn.includes("tile/0,0/base_color"));
  // The left wall reveals one more column of its reserved surface. The ordinary return
  // beside it stays in place. Compare every byte, including the exposed strip and gutters.
  const cropped = await mixedMaps(true, 2, 2, 0, true, 24, 0.75);
  const revealed = await mixedMaps(true, 2, 2, 0, true, 24, 0.875);
  const revealedDirect = await mixedMaps(false, 2, 2, 0, true, 24, 0.875);
  assert.deepEqual(revealed.base, cropped.base);
  assert.deepEqual(revealed.base, revealedDirect.base);
  assert.deepEqual(revealed.properties, revealedDirect.properties);
  assert.ok(!revealed.drawn.some(id => id.startsWith("tile/") || id === "stack.base/base_color" || id === "base"),
    "changing the UV crop keeps the stock tile, remapped plane and composed color");
  // The right chart's material and grime both change when its physical sample position changes.
  const changed = await mixedMaps(true, 3), direct = await mixedMaps(false, 3);
  assert.deepEqual(changed.base, direct.base);
  assert.deepEqual(changed.properties, direct.properties);
  assert.ok(!changed.drawn.some(id => id.startsWith("tile/")), "resizing the return reuses the stock tile");
  assert.ok(!changed.drawn.includes("emissive") && !changed.drawn.includes("stack.base/emissive"), "a spatially constant stack channel reuses its plane and final map");
  const pixel = (x, channel) => (6 * 24 + x) * 4 + channel;
  assert.ok(changed.base[pixel(18, 0)] > mixed.base[pixel(18, 0)]);
  assert.ok(changed.properties[pixel(18, 1)] > mixed.properties[pixel(18, 1)]);
  assert.equal(changed.base[pixel(6, 0)], mixed.base[pixel(6, 0)]);

  // The same single texel feeds the emissive output and a weathering layer's below.emissive.
  // Widen the destination atlas to force new maps of both. The reference evaluates the same
  // expression over a full-size plane, including the uncovered area behind the charts.
  const expanded = await mixedMaps(true, 3, 2, 0, true, 32);
  const fullPlane = await mixedMaps(true, 3, 2, 0, false, 32);
  assert.ok(!expanded.drawn.includes("stack.base/emissive"));
  assert.ok(fullPlane.drawn.includes("stack.base/emissive"));
  assert.deepEqual(expanded.emissive, fullPlane.emissive);
  assert.deepEqual(expanded.properties, fullPlane.properties);
  assert.ok(expanded.emissive[(6 * 32 + 6) * 4] > 0);
  assert.equal(expanded.emissive[(6 * 32 + 26) * 4], 0);

  // Fittings receive new part anchors during footprint edits even when their charts stay fixed.
  for (const mode of [0, 1, 2, 3]) {
    const before = await mixedMaps(false, 3, mode), shifted = await mixedMaps(false, 3, mode, 0.5);
    if (mode === 3) {
      assert.notDeepEqual(shifted.base, before.base);
      assert.ok(shifted.drawn.includes("base"));
    } else {
      assert.deepEqual(shifted.base, before.base);
      assert.deepEqual(shifted.properties, before.properties);
      assert.deepEqual(shifted.drawn, []);
    }
  }

  // Two independent zone layers can pass through a preceding layer, the base, or a default
  // channel (normal_offset). Read the actual composed textures through every combination.
  for (const counts of [[0, 0], [1, 0], [0, 1], [1, 1], [0, 0]]) {
    const run = shortcut => mixedMaps(true, 2, 2, 0, true, 32, 1, { shortcut, counts });
    const optimized = await run(true), reference = await run(false);
    for (const channel of ["base", "properties", "normal"]) assert.deepEqual(optimized[channel], reference[channel]);
    counts.forEach((count, index) => {
      if (!count) assert.ok(!optimized.drawn.some(id => id.startsWith("stack." + (index + 2) + "/")), "an empty layer aliases its inputs");
    });
  }
  for (const count of [0, 1]) {
    assert.equal(await mixedMaps(true, 41 + count, 2, 0, true, 32, 1,
      { shortcut: true, counts: [count], changeDuringBake: true }), null);
  }

  assert.equal(errors.length, 0, errors.join("\n"));
  const renderer = await page.evaluate(() => { const gl = document.querySelector("canvas").getContext("webgl2"); const info = gl.getExtension("WEBGL_debug_renderer_info"); return info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER); });
  console.log(`Bake graph nodes passed on ${renderer}: blur within ${Math.max(worstRows, worst).toExponential(1)} of the Gaussian, mapping within ${worstMapping.toExponential(1)}; ${JSON.stringify(await page.evaluate(() => window.driver.stats()))}.`);
} finally {
  await browser?.close();
  server.close();
}
