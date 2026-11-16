/**
 * Pictures of the TreeSpec fixtures in the built Material Studio. Write the GLBs first:
 *   cargo test --release -p superspec-engine --test treespec_verification -- --ignored
 * Each GLB under generated/verification/treespec is opened with "Open GLB" and its viewport saved
 * beside it as a PNG. Then the washroom project is opened and the grass strip and the shrub are
 * added beside its slab with "Add models" (washroom-scene.png). ONLY=name,name limits the GLBs.
 * Shadows are lightened (QUERY, default "shadowdark=0.45") so shaded leaves stay readable: the
 * vertex colours are lit by the viewer's standard material, not by a painted one.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve(process.env.TREESPEC_OUTPUT ?? "generated/verification/treespec");
const names = (process.env.ONLY ?? "grass,grass-lods,shrub,shrub-lods,broadleaf,broadleaf-lods,conifer,conifer-lods").split(",").filter(Boolean);
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
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  const ready = async message => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 600000 });
    const state = await page.locator("#status").getAttribute("data-state");
    const findings = state === "ready" ? "" : (await page.locator("#diagnostics button").allTextContents()).join("\n");
    assert.equal(state, "ready", `${message}: ${await page.locator("#status").textContent()}\n${findings}`);
  };
  /** Waits for a build that starts after `action`, then for it to finish. */
  const rebuilt = async (action, message) => {
    const before = Number(await page.locator("#viewport").getAttribute("data-input-revision") ?? 0);
    await action();
    await page.waitForFunction(start => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > start, before, { timeout: 600000 });
    await ready(message); await waitForFrames(page, 4);
  };
  const shot = async name => {
    await waitForFrames(page, 4);
    await writeFile(resolve(out, name), await page.locator("#viewport").screenshot());
    console.log(`Wrote ${resolve(out, name)}`);
  };
  /** Turns the view by dragging and moves in or out with the wheel. */
  const view = async ({ drag = [0, 0], wheel = 0 }) => {
    const box = await page.locator("#viewport canvas").boundingBox();
    const [x, y] = [box.x + box.width / 2, box.y + box.height / 2];
    await page.mouse.move(x, y);
    if (drag[0] || drag[1]) { await page.mouse.down(); await page.mouse.move(x + drag[0], y + drag[1], { steps: 8 }); await page.mouse.up(); }
    for (let i = 0; i < Math.abs(wheel); i++) { await page.mouse.wheel(0, Math.sign(wheel) * 240); await waitForFrames(page, 2); }
  };

  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?${process.env.QUERY ?? "shadowdark=0.45"}`);
  await ready("initial");
  const renderer = await page.evaluate(() => {
    const gl = document.querySelector("#viewport canvas").getContext("webgl2");
    const info = gl?.getExtension("WEBGL_debug_renderer_info");
    return info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : "unknown";
  });
  console.log(`WebGL renderer: ${renderer}`);

  for (const name of names) {
    const path = resolve(out, `${name}.glb`);
    assert.ok(existsSync(path), `${path} is missing; run the treespec_verification test first`);
    const started = Date.now();
    await rebuilt(() => page.locator("#open-glb").setInputFiles(path), name);
    console.log(`${name}: opened in ${Date.now() - started} ms`);
    await shot(`${name}.png`);
    if (!name.endsWith("-lods")) {
      // A closer look from a lower angle, where single blades and leaves can be judged.
      await view({ drag: [120, name === "grass" ? -45 : -90], wheel: -3 });
      await shot(`${name}-close.png`);
    }
    await page.reload(); await ready("reloaded");
  }

  if (!process.env.ONLY || process.env.WASHROOM === "1") {
    // The washroom project, then the foliage beside its slab through the scene's "Add models".
    const { project } = buildProject(Number(process.env.RESOLUTION ?? 256));
    await writeFile(resolve(out, "washroom.studio.json"), JSON.stringify(project));
    await rebuilt(() => page.locator("#open-project").setInputFiles({ name: "washroom.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) }), "washroom");
    await rebuilt(() => page.locator("#add-models").setInputFiles([resolve(out, "washroom-grass.glb"), resolve(out, "washroom-shrub.glb")]), "added foliage");
    const meshes = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.meshes).map(({ id }) => id));
    assert.ok(meshes.some(id => id.startsWith("washroom-grass/")) && meshes.some(id => id.startsWith("washroom-shrub/")), `${meshes}`);
    // The scene bakes its maps after the first look; wait for them where the page reports it.
    await page.waitForFunction(() => { const bake = JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null"); return !bake || bake.state === "baked"; }, {}, { timeout: 600000 });
    await shot("washroom-scene.png");
    await view({ drag: [-220, 40], wheel: 0 });
    await shot("washroom-scene-side.png");
  }
  const unexpected = errors.filter(error => !/Source could not be compiled|Material evaluation|MS_/.test(error));
  assert.equal(unexpected.length, 0, unexpected.join("\n"));
  console.log("TreeSpec pictures written.");
} finally {
  await browser?.close();
  server.close();
}
