/**
 * A layered slot keeps its base's opacity in the GPU bake: glass under dust stays blended, with the glass's alpha where
 * the dust gives no opacity and the clouding layer's where it covers; a cutout base under a layer keeps its cutout. The
 * same recipes as crates/engine/tests/material_layer_opacity.rs, on two 1 m panes of an item, in the built Material
 * Studio's GPU-baked preview: the baked looks' materials (blended, cutout) and the alphas of their base color maps.
 * STUDIO_UI_ROOT names the built Studio (default generated/rust-editor).
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const recipes = await readFile(resolve("crates/engine/tests/material_layer_opacity.rs"), "utf8");
const recipe = name => new RegExp(`const ${name}: &str = r#"([\\s\\S]*?)"#;`).exec(recipes)[1];
const pane = (id, x) => ({
  id, kind: "item", transform: [[1, 0, 0], [0, 1, 0], [0, 0, 1], [x, 0, 0]], slots: { front: id },
  source: JSON.stringify({ format: "itemspec", version: 2, id: `${id}-pane`,
    parts: [{ id: "pane", generator: { kind: "panel", width: 1, height: 1, thickness: 0.01 }, slots: { front: "front" } }] }),
});
const project = {
  schemaVersion: 2, kind: "material", entry: "glass.material.ts", parameters: {}, seed: 0, resolution: 64, mapping: "world",
  inspection: "final", domain: [1, 1], base: { kind: "swatch" }, target: { scope: "mesh", enabled: false }, assets: {},
  modules: { "glass.material.ts": recipe("GLASS"), "cutout.material.ts": recipe("CUTOUT"), "dust.material.ts": recipe("DUST"), "clouding.material.ts": recipe("CLOUDING") },
  // The cutout is opaque where world x < 0.5, so its pane stands at the origin; the glass's layers read world z alone.
  models: [pane("glass", 2), pane("cut", 0)],
  materials: [
    { id: "glass", entry: "glass.material.ts", parameters: {}, seed: 0, meshes: [], layers: [{ entry: "dust.material.ts", parameters: {}, seed: 1 }, { entry: "clouding.material.ts", parameters: {}, seed: 1 }] },
    { id: "cut", entry: "cutout.material.ts", parameters: {}, seed: 0, meshes: [], layers: [{ entry: "dust.material.ts", parameters: {}, seed: 1 }] },
  ],
};

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
const browser = await launchBrowser();
try {
  const page = await browser.newPage({ viewport: { width: 900, height: 700 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?ao=off&shadows=off&gpuprobe`);
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 600000 });
  await page.locator("#open-project").setInputFiles({ name: "opacity.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
  await page.waitForFunction(() => document.querySelector("#viewport").dataset.items !== undefined || document.querySelector("#status")?.dataset.state === "error", {}, { timeout: 600000 });
  await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 600000 });
  assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", await page.locator("#status").textContent());
  await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "{}").state === "baked", {}, { timeout: 600000 });
  await waitForFrames(page, 4);
  const looks = await page.evaluate(() => {
    const scene = window.__scene, renderer = scene.renderer;
    const result = {};
    for (const entry of scene.sceneBaker.current.entries) {
      const target = entry.maps.nodes.node("base").target, { width, height } = target;
      const texels = new Uint8Array(width * height * 4);
      renderer.readRenderTargetPixels(target, 0, 0, width, height, texels);
      const counts = {};
      for (let i = 3; i < texels.length; i += 4) counts[texels[i]] = (counts[texels[i]] ?? 0) + 1;
      result[entry.target.material] = { layered: !!entry.target.stack, transparent: entry.material.transparent, alphaTest: entry.material.alphaTest, counts, texels: width * height };
    }
    return result;
  });
  const share = (look, value) => Object.entries(look.counts).filter(([alpha]) => Math.abs(Number(alpha) - Math.round(value * 255)) <= 1).reduce((sum, [, n]) => sum + n, 0) / look.texels;
  console.log(JSON.stringify(Object.fromEntries(Object.entries(looks).map(([id, look]) => [id, { ...look, counts: undefined, at04: share(look, 0.4), at09: share(look, 0.9), at0: share(look, 0), at1: share(look, 1) }]))));
  const glass = Object.entries(looks).find(([id]) => id.startsWith("glass"))[1], cut = Object.entries(looks).find(([id]) => id.startsWith("cut"))[1];
  assert.ok(glass.layered && cut.layered, "both slots are composed from their layers");
  assert.equal(glass.transparent, true, "glass under layers stays blended");
  assert.ok(share(glass, 0.4) > 0.3 && share(glass, 0.9) > 0.3, "the glass's alpha where the clouding gives none, and the clouding's where it covers");
  assert.equal(cut.alphaTest, 0.5, "a cutout under a layer keeps its cutoff");
  assert.ok(share(cut, 1) > 0.3 && share(cut, 0) > 0.3, "and its opacity");
  assert.equal(errors.length, 0, errors.join("\n"));
  console.log("Layer opacity passed: layered glass is blended with its alphas, and a layered cutout keeps its cutout.");
} finally {
  await browser.close();
  server.close();
}
