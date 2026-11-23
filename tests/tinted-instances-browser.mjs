/**
 * Tinted instanced part prototypes on a real GPU, through the published path of `@superspec/three`:
 * the loader, scene binding, instance merging, camera selection, the sun's shadow pass and first-frame
 * preparation. The pack is synthetic (`tests/browser/tinted-instances.fixture.ts`); no published pack
 * carries tinted instances yet. Every expectation is derived here from the authored colours.
 *
 * Renders on SwiftShader by default. For the machine's GPU:
 *   BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist" node tests/tinted-instances-browser.mjs
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, sep, extname } from "node:path";
import { build } from "vite";
import { launchBrowser } from "../tools/browser.mjs";

const root = resolve("generated/tinted-instances-browser");
await build({ configFile: false, root: "tests/browser", publicDir: false, logLevel: "warn",
  build: { outDir: root, emptyOutDir: true, rollupOptions: { input: resolve("tests/browser/tinted-instances.html") } } });
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "tinted-instances.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

// The part, as the fixture authors it (glTF Y-up metres): a "case" quad of role 1 left of the placement's
// origin and a "bracket" quad of role 0 right of it, each half a metre square, facing the camera.
const CASE_SHADE = [0.8, 0.8, 0.8], BRACKET = [0.1, 0.6, 0.9];
const caseAt = ([x, y, z]) => [x - 0.25, y + 0.25, z], bracketAt = ([x, y, z]) => [x + 0.25, y + 0.25, z];
// The first tinted placement stands 40 m to the left, outside the first view, so the camera's selection
// is a subset whose slots do not line up with the batch's. Two nodes share the prototype and merge.
const parts = [
  { node: "tinted-a", translations: [[-40, 0, -8], [-3, 0, -8], [0, 1, -8]], tints: [[0.2, 1, 0.2], [1, 0.2, 0.2], [0.2, 0.2, 1]] },
  { node: "tinted-b", translations: [[3, 0, -8]], tints: [[0.9, 0.6, 0.3]] },
  { node: "plain-a", translations: [[0, -1.5, -8]] },
  { node: "plain-b", translations: [[-3, -1.5, -8]] },
];
const panes = [{ node: "glass-a", translations: [[-6, 3, -8]] }, { node: "glass-b", translations: [[6, 3, -8]] }];
const placements = parts.flatMap(batch => batch.translations.map((at, i) => ({ at, tint: batch.tints?.[i] })));

const toLinear = byte => { const c = byte / 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
const toByte = linear => { const c = Math.min(1, Math.max(0, linear)); return 255 * (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055); };
const close = (found, expected, message, tolerance = 4) => {
  assert.ok(found, `${message}: the point is off screen`);
  found.forEach((value, channel) => assert.ok(Math.abs(value - expected[channel]) <= tolerance,
    `${message}: drew ${found}, expected about ${expected.map(Math.round)}`));
};

let browser;
try {
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 960, height: 640 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => window.driver);

  for (const roles of [true, false]) {
    const opened = await page.evaluate(([parts, panes, roles]) => window.driver.open(parts, panes, roles), [parts, panes, roles]);
    // Preparation needs KHR_parallel_shader_compile. A driver without it links while drawing, as it always has.
    const prepares = opened.prepared !== null;
    if (prepares) assert.ok(opened.prepared > 0, "first-frame preparation compiled the scene's programs");

    // Matte surfaces under ambient light alone show their own colour times one scalar the lighting model fixes.
    // An untinted case measures that scalar; every other expectation follows from the authored colours.
    // The second view sees only the placement 40 m to the left, and uses the first view's scalar.
    let scalar;
    for (const eye of [[0, 0, 0], [-40, 0, 0]]) {
      const seen = placements.filter(({ at }) => Math.abs(at[0] - eye[0]) < 5);
      const points = seen.flatMap(({ at }) => [caseAt(at), bracketAt(at)]);
      // The first frame also draws the sun's shadow map, which later frames reuse.
      const first = eye[0] === 0;
      const frame = await page.evaluate(([eye, points, first]) => window.driver.draw(eye, 0, points, first), [eye, points, first]);
      if (!scalar) {
        const pixel = frame.samples[2 * seen.findIndex(({ tint }) => !tint)];
        assert.ok(pixel && pixel[0] > 100 && pixel[0] < 250, `the untinted case is drawn in mid range: ${pixel}`);
        scalar = pixel.map((byte, channel) => toLinear(byte) / CASE_SHADE[channel]);
      }
      seen.forEach(({ at, tint }, i) => {
        const [casePixel, bracketPixel] = frame.samples.slice(2 * i, 2 * i + 2);
        const label = `${roles ? "roles" : "no roles"}, ${tint ? "tinted" : "untinted"} placement at ${at} from ${eye}`;
        // Without the attribute Three's stock behaviour holds: the instance colour multiplies every vertex.
        const caseTint = tint ?? [1, 1, 1], bracketTint = roles || !tint ? [1, 1, 1] : tint;
        close(casePixel, CASE_SHADE.map((shade, channel) => toByte(shade * caseTint[channel] * scalar[channel])), `${label}: the case takes the instance tint`);
        close(bracketPixel, BRACKET.map((own, channel) => toByte(own * bracketTint[channel] * scalar[channel])),
          `${label}: ${roles ? "role-0 vertices keep their own colour" : "every vertex is tinted"}`);
      });
      const tintedDraws = frame.batches.filter(batch => batch.tinted);
      assert.deepEqual(tintedDraws.map(batch => batch.count), [seen.filter(({ tint }) => tint).length],
        "the tinted placements in view are one draw: a subset of the merged batch");
      if (first) {
        // The light covers the three tinted placements near the origin, not the one 40 m away.
        assert.deepEqual(frame.tintedDepthDraws, [3], "the sun's shadow pass draws the tinted batch, culled to the light");
      } else assert.deepEqual(frame.tintedDepthDraws, []);
      if (prepares) assert.deepEqual(frame.latePrograms, [], "no program was first linked after preparation");
    }

    // The sun shines along -Z onto a wall one metre behind the parts. From the camera, the strip of each shadow
    // nearest the view axis shows beside its caster. A tinted and an untinted caster shade the wall alike.
    const lit = [2.6, 2, -9], behindTinted = [2.6, 0.25, -9], behindPlain = [-2.6, -1.25, -9];
    const sunlit = await page.evaluate(([points]) => window.driver.draw([0, 0, 0], 2, points), [[lit, behindTinted, behindPlain]]);
    const [open, tintedShade, plainShade] = sunlit.samples;
    assert.ok(open && tintedShade && plainShade);
    assert.ok(open[0] > 200, `the open wall is sunlit: ${open}`);
    assert.ok(tintedShade[0] < 0.7 * open[0], `the tinted batch shades the wall: ${tintedShade} against ${open}`);
    close(tintedShade, plainShade, "tinted and untinted casters cast the same shadow", 2);
    if (prepares) assert.deepEqual(sunlit.latePrograms, [], "the shadow and colour passes linked no program after preparation");
    console.log(`${roles ? "With" : "Without"} _MS_ROLE: ${placements.length} placements match their authored colours on ${opened.gpu}; `
      + (prepares ? `${opened.preparedPrograms} programs, none linked after preparation.` : "this driver cannot prepare programs ahead of the first frame."));
  }
  assert.deepEqual(errors, []);
} finally {
  await browser?.close();
  await new Promise(done => server.close(done));
}
