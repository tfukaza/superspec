/**
 * Live programs (roadmap stage 5) in a browser: the generated GLSL of a painted program and of one that reads every
 * other render input agrees with the CPU evaluation on a sample grid, and the viewer's program material binds each
 * render input (instance root, tint and random, the placed position, a vertex attribute and its fallback, a scene
 * field and the sun) and draws the ground fields from a ground's own color.
 *
 * The CPU's values come from the engine; write them first:
 *   MATERIAL_LIVE_GLSL_OUTPUT=$PWD/generated/verification/material-live-glsl.json \
 *     cargo test -p superspec-engine --test material_live the_cpu_reference_fixture_is_written
 * Use the real GPU with BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist".
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { resolve, sep, extname } from "node:path";
import { build } from "vite";
import { launchBrowser } from "../tools/browser.mjs";

const fixturePath = resolve(process.env.LIVE_FIXTURE ?? "generated/verification/material-live-glsl.json");
let fixture;
try { fixture = JSON.parse(await readFile(fixturePath, "utf8")); }
catch { throw new Error(`Missing ${fixturePath}: run the engine's material_live fixture test with MATERIAL_LIVE_GLSL_OUTPUT set to it.`); }
const root = resolve("generated/live-program-browser");
await build({ configFile: false, root: "tests/browser", publicDir: false, logLevel: "warn",
  build: { outDir: root, emptyOutDir: true, rollupOptions: { input: resolve("tests/browser/live-program.html") } } });
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "live-program.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

let browser;
try {
  browser = await launchBrowser();
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => window.liveProgram !== undefined);
  const renderer = await page.evaluate(() => {
    const gl = document.createElement("canvas").getContext("webgl2"), info = gl.getExtension("WEBGL_debug_renderer_info");
    return info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
  });
  const grid = await page.evaluate(fixture => window.liveProgram.grid(fixture), fixture);
  for (const [name, result] of Object.entries(grid)) {
    // 32-bit floats against the CPU's 64: the band's steepest slope (1 / 0.05) times float error stays far below this.
    assert.ok(result.worst <= 1e-3, `${name}: GLSL and CPU differ by ${result.worst} at sample ${result.at}`);
  }
  const bound = await page.evaluate(fixture => window.liveProgram.bindings(fixture), fixture);
  for (const result of bound) {
    result.expected.forEach((value, index) => assert.ok(Math.abs(result.found[index] - value) <= result.tolerance,
      `${result.name} of instance ${result.instance}: found ${result.found}, expected ${result.expected}`));
  }
  assert.deepEqual(errors, []);
  const summary = { renderer, grid, inputs: [...new Set(bound.map(result => result.name))] };
  await writeFile(resolve("generated/verification/live-program-browser.json"), JSON.stringify({ ...summary, bound }, null, 2) + "\n");
  console.log(`Live programs passed on ${renderer}: GLSL against CPU on ${Object.values(grid)[0].samples} samples (largest difference ${JSON.stringify(Object.fromEntries(Object.entries(grid).map(([name, result]) => [name, result.worst])))}); inputs bound: ${summary.inputs.join(", ")}.`);
} finally {
  await browser?.close();
  server.close();
}
