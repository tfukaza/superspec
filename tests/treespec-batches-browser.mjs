/**
 * The washroom with foliage beside its slab, twice in the built Material Studio: as flattened meshes (the grass strip
 * and the shrub added with "Add models", as tests/treespec-browser.mjs does) and as instanced batches (the same
 * generated foliage in the project's `batches`: a batch per grass clump variant, the shrub's leaf clusters as one batch,
 * its bark and the ground as batches of one instance). Reports draw calls, triangles, the GPU's frame time and the load
 * time of each, and saves a picture of each beside the inputs. Write the inputs first:
 *   cargo test --release -p superspec-engine --test treespec_verification -- --ignored write_fixture_glbs
 *   cargo test --release -p superspec-engine --test treespec_batches -- --ignored
 * Shadows are lightened as in treespec-browser.mjs (QUERY adds to "shadowdark=0.45&gpuprobe=1").
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve(process.env.TREESPEC_OUTPUT ?? "generated/verification/treespec");
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

const foliage = JSON.parse(await readFile(resolve(out, "washroom-foliage.batches.json"), "utf8"));
const query = `shadowdark=0.45&gpuprobe=1${process.env.QUERY ? `&${process.env.QUERY}` : ""}`;
let browser;
try {
  browser = await launchBrowser();
  const results = {};
  for (const variant of ["flattened", "instanced"]) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    const ready = async message => {
      await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 600000 });
      const state = await page.locator("#status").getAttribute("data-state");
      assert.equal(state, "ready", `${message}: ${await page.locator("#status").textContent()}`);
    };
    const rebuilt = async (action, message) => {
      const before = Number(await page.locator("#viewport").getAttribute("data-input-revision") ?? 0);
      await action();
      await page.waitForFunction(start => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > start, before, { timeout: 600000 });
      await ready(message);
      // Every mesh shows its baked maps before anything is measured (a scene still on its per-pixel looks draws slower).
      await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked", {}, { timeout: 600000 });
      await waitForFrames(page, 4);
    };
    await page.goto(`http://127.0.0.1:${server.address().port}/material.html?${query}`);
    await ready("initial");
    const { project } = buildProject(Number(process.env.RESOLUTION ?? 256));
    if (variant === "instanced") project.batches.push(...foliage);
    const text = JSON.stringify(project);
    const started = Date.now();
    await rebuilt(() => page.locator("#open-project").setInputFiles({ name: "washroom.studio.json", mimeType: "application/json", buffer: Buffer.from(text) }), "washroom");
    if (variant === "flattened") await rebuilt(() => page.locator("#add-models").setInputFiles([resolve(out, "washroom-grass.glb"), resolve(out, "washroom-shrub.glb")]), "added foliage");
    const loaded = Date.now() - started;
    // The view of treespec-browser.mjs's second picture, where the foliage is in front.
    const box = await page.locator("#viewport canvas").boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down(); await page.mouse.move(box.x + box.width / 2 - 220, box.y + box.height / 2 + 40, { steps: 8 }); await page.mouse.up();
    await waitForFrames(page, 6);
    await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked", {}, { timeout: 600000 });
    await page.waitForTimeout(1500);
    const measured = await page.evaluate(async () => {
      const scene = window.__scene, gl = scene.renderer.getContext(), pixel = new Uint8Array(4);
      const timer = gl.getExtension("EXT_disjoint_timer_query_webgl2");
      const gpu = [], draw = [];
      for (let i = 0; i < 40; i++) {
        const query = timer && gl.createQuery();
        if (query) gl.beginQuery(timer.TIME_ELAPSED_EXT, query);
        const start = performance.now();
        scene.draw();
        if (query) gl.endQuery(timer.TIME_ELAPSED_EXT);
        gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
        if (i >= 8) draw.push(performance.now() - start);
        if (query) {
          for (let waited = 0; waited < 500 && !gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE); waited++) await new Promise(done => setTimeout(done, 2));
          if (i >= 8 && gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE) && !gl.getParameter(timer.GPU_DISJOINT_EXT)) gpu.push(gl.getQueryParameter(query, gl.QUERY_RESULT) / 1e6);
          gl.deleteQuery(query);
        }
      }
      const summary = values => { values.sort((a, b) => a - b); return values.length ? { median: +values[values.length >> 1].toFixed(2), min: +values[0].toFixed(2), max: +values.at(-1).toFixed(2) } : null; };
      const host = document.querySelector("#viewport");
      const info = gl.getExtension("WEBGL_debug_renderer_info");
      return { frame: scene.measureFrame(), gpuMs: summary(gpu), drawAndReadMs: summary(draw), memory: { ...scene.renderer.info.memory },
        meshes: JSON.parse(host.dataset.meshes).length, vertices: JSON.parse(host.dataset.meshes).reduce((sum, mesh) => sum + mesh.vertices, 0),
        batches: JSON.parse(host.dataset.batches ?? "[]").map(batch => ({ id: batch.id, instances: batch.instances, vertices: batch.vertices, attributes: batch.attributes })),
        renderer: info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : "unknown" };
    });
    results[variant] = { loadedMs: loaded, projectBytes: text.length, ...measured };
    await writeFile(resolve(out, `washroom-foliage-${variant}.png`), await page.locator("#viewport").screenshot());
    assert.deepEqual(errors.filter(error => !/Source could not be compiled|Material evaluation|MS_/.test(error)), []);
    await page.close();
  }
  const { flattened, instanced } = results;
  // The same foliage: the instanced scene keeps each mesh once and draws the instances the flattened one holds as vertices.
  assert.equal(instanced.batches.length, 1 + foliage.length, "the bolts and every foliage batch are instanced meshes");
  for (const batch of foliage) {
    const drawn = instanced.batches.find(candidate => candidate.id === batch.id);
    assert.equal(drawn?.instances, batch.instances.length, batch.id);
    for (const name of Object.keys(batch.scalars).concat(Object.keys(batch.vectors))) assert.ok(drawn.attributes.includes(`msInstance_${name}`), `${batch.id}: ${name} is bound per instance`);
    for (const name of Object.keys(batch.mesh.scalars).concat(Object.keys(batch.mesh.vectors))) assert.ok(drawn.attributes.includes(`msVertex_${name}`), `${batch.id}: ${name} is bound per vertex`);
  }
  assert.equal(flattened.frame.triangles, instanced.frame.triangles, "both scenes draw the same triangles");
  const stored = instanced.vertices + instanced.batches.reduce((sum, batch) => sum + batch.vertices, 0);
  console.log(`Flattened: ${JSON.stringify(flattened)}`);
  console.log(`Instanced: ${JSON.stringify(instanced)}`);
  console.log(`Foliage as instanced batches: ${instanced.frame.calls} draw calls for ${flattened.frame.calls}, ${stored.toLocaleString()} vertices held for ${flattened.vertices.toLocaleString()}, `
    + `GPU ${instanced.gpuMs?.median} ms for ${flattened.gpuMs?.median} ms a frame, loaded in ${instanced.loadedMs} ms for ${flattened.loadedMs} ms (${flattened.renderer}).`);
} finally {
  await browser?.close();
  server.close();
}
