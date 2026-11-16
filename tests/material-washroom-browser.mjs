/**
 * The MaterialSpec benchmark scene (tests/studio/washroom) in the built Material Studio: opens
 * the project through the production Wasm workers, checks every material landed on its meshes
 * and reports how long the scene took to load. RESOLUTION (default 256) sets the texture size;
 * SCREENSHOT names a PNG of the loaded viewport.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject, materials } from "./studio/washroom/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const resolution = Number(process.env.RESOLUTION ?? 256);
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "index.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".png": "image/png" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

let browser;
try {
  const { project } = buildProject(resolution);
  // ONLY=id,id keeps just those materials, to see what each costs.
  const only = process.env.ONLY?.split(",");
  if (only) project.materials = project.materials.filter(material => only.includes(material.id));
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  const state = () => page.locator("#status").getAttribute("data-state");
  const ready = async () => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 900000 });
    const findings = await state() === "ready" ? "" : (await page.locator("#diagnostics button").allTextContents()).join("\n");
    assert.equal(await state(), "ready", `${await page.locator("#status").textContent()}\n${findings}`);
  };
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html${process.env.QUERY ? `?${process.env.QUERY}` : ""}`);
  await ready();

  const before = Number(await page.locator("#viewport").getAttribute("data-input-revision") ?? 0);
  const started = Date.now();
  await page.locator("#open-project").setInputFiles({ name: "washroom.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
  // The first thing shown is the quick draft; the scene is loaded when the full-size build is ready.
  const revision = name => page.waitForFunction(([name, start]) => Number(document.querySelector("#viewport").dataset[name] ?? 0) > start, [name, before], { timeout: 900000 });
  await Promise.race([revision("draftRevision"), revision("inputRevision")]);
  const firstPreview = Date.now() - started;
  await revision("inputRevision");
  await ready(); await waitForFrames(page);
  const loaded = Date.now() - started;

  const bindings = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.bindings ?? "{}"));
  const expected = Object.fromEntries(project.materials.flatMap(material => material.meshes.map(mesh => [mesh, material.id])));
  assert.deepEqual(bindings, expected, "every mesh shows its material");
  if (!only) assert.equal(Object.keys(expected).length, materials.reduce((sum, material) => sum + material.meshes.length, 0));
  assert.equal(errors.length, 0, errors.join("\n"));
  // Edit latency: one parameter of a light material (tile) and of the heaviest (concrete walls),
  // from the keystroke to the draft in the viewport and to the full-size result.
  const edits = {};
  for (const [material, label, value] of [["tile", "Glaze roughness", "0.3"], ["concrete-walls", "Chip amount", "0.4"]]) {
    if (!project.materials.some(candidate => candidate.id === material)) continue;
    const current = () => page.evaluate(() => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0));
    let start = await current();
    await page.locator(`#material-list li[data-material="${material}"] .scene-choose`).click();
    await page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, start, { timeout: 900000 });
    await ready();
    const control = page.getByRole("spinbutton", { name: label, exact: true });
    start = await current();
    const began = Date.now();
    await control.fill(value); await control.press("Tab");
    const shown = name => page.waitForFunction(([name, from]) => Number(document.querySelector("#viewport").dataset[name] ?? 0) > from, [name, start], { timeout: 900000 });
    await Promise.race([shown("draftRevision"), shown("inputRevision")]);
    const draft = Date.now() - began;
    await shown("inputRevision"); await ready();
    edits[material] = { draft, full: Date.now() - began };
  }
  console.log(`Edit latency (ms): ${JSON.stringify(edits)}`);
  // Frame time: redraw the loaded scene several times and wait for the GPU to finish each frame.
  const frame = await page.evaluate(async () => {
    const exposure = document.querySelector("#exposure");
    const canvas = document.querySelector("#viewport canvas");
    const gl = canvas.getContext("webgl2");
    const times = [];
    for (let i = 0; i < 6; i++) {
      const start = performance.now();
      exposure.value = String(1 + (i % 2) * 0.01); exposure.dispatchEvent(new Event("input"));
      await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
      gl.finish();
      times.push(Math.round(performance.now() - start));
    }
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    return { times, renderer: info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER) };
  });
  console.log(`Frame times (ms): ${JSON.stringify(frame)}`);
  if (process.env.SCREENSHOT) await writeFile(process.env.SCREENSHOT, await page.locator("#viewport").screenshot());
  console.log(`Washroom benchmark passed at ${resolution} px: first preview ${firstPreview} ms, loaded ${loaded} ms (${project.materials.length} materials, ${Object.keys(expected).length} meshes).`);
} finally {
  await browser?.close();
  server.close();
}
