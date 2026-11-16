/**
 * Per-texture bake keys on the washroom's real programs (tests/studio/washroom), in the built Material Studio. A GPU
 * bake keys each texture by the inputs it reads (`attachmentReads` in browser/renderer/atlas.ts), so a changed parameter
 * draws only the textures it reaches. That is only right if nothing else read the parameter: for every parameter of
 * every material this changes it, lets the scene bake with per-texture keys, reads every texture of the material's
 * meshes back, bakes everything again keyed by every value (`GpuBaker.nodeKeys = false`) and reads back again. A
 * difference is a texture that read the parameter and was not drawn. The expectation is independent of how the reads
 * are found (entry points today, the engine's stated dependencies later): only texels are compared.
 * RESOLUTION (default 128) sets the texture size; ONLY=id,id keeps just those materials.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
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
  const { project } = buildProject(Number(process.env.RESOLUTION ?? 128));
  const only = process.env.ONLY?.split(",");
  if (only) project.materials = project.materials.filter(material => only.includes(material.id));
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1024, height: 768 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  // `gpuprobe` exposes the viewer (`window.__scene`); ambient occlusion only costs frames here.
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?gpuprobe&ao=off`);
  const ready = () => page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 900000 });
  await ready();
  const before = Number(await page.locator("#viewport").getAttribute("data-input-revision") ?? 0);
  await page.locator("#open-project").setInputFiles({ name: "washroom.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
  await page.waitForFunction(start => {
    const state = document.querySelector("#status")?.dataset.state;
    return state === "error" || (state === "ready" && Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > start);
  }, before, { timeout: 900000 });
  assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", await page.locator("#status").textContent());
  await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "{}").state === "baked", {}, { timeout: 900000 });
  await waitForFrames(page);
  const first = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null"));
  assert.ok(first, "the scene shows GPU-baked maps");
  assert.deepEqual(first.unsupported, {}, "every mesh of the washroom shows GPU-baked maps");

  const report = await page.evaluate(async () => {
    const scene = window.__scene, baker = scene.bakerInstance, sceneBaker = scene.sceneBaker, renderer = scene.renderer, gl = renderer.getContext();
    const pause = ms => new Promise(done => setTimeout(done, ms));
    // A changed value is baked when it has been at rest for a moment (scene-bake.ts).
    const settled = async () => {
      await pause(200);
      for (let tries = 0; tries < 3000 && !sceneBaker.settled(); tries++) await pause(20);
      if (!sceneBaker.settled()) throw new Error("The scene did not finish baking.");
    };
    const entriesOf = material => sceneBaker.current.entries.filter(entry => entry.target.material === material && entry.maps);
    const hashOf = bytes => { let value = 0x811c9dc5; for (let index = 0; index < bytes.length; index++) value = Math.imul(value ^ bytes[index], 0x01000193); return (value >>> 0).toString(16); };
    /** A hash of every texture of the material's bakes, by mesh and node. */
    const read = material => {
      const found = {};
      for (const entry of entriesOf(material)) {
        for (const [id, node] of entry.maps.nodes.nodes) {
          const { width, height } = node.target, bytes = node.target.texture.type === window.__three.UnsignedByteType;
          const texels = bytes ? new Uint8Array(width * height * 4) : new Float32Array(width * height * 4);
          renderer.setRenderTarget(node.target);
          gl.readPixels(0, 0, width, height, gl.RGBA, bytes ? gl.UNSIGNED_BYTE : gl.FLOAT, texels);
          renderer.setRenderTarget(null);
          found[`${entry.target.mesh.name}:${id}`] = hashOf(new Uint8Array(texels.buffer));
        }
      }
      return found;
    };
    const keysOf = material => entriesOf(material).map(entry => new Map(entry.maps.nodes.keys));
    const report = { materials: {}, failures: [], parameters: 0, textures: 0, rekeyed: 0, kept: 0 };
    await settled();
    for (const [id, entry] of scene.shading) {
      const original = JSON.parse(entry.values);
      const defaults = new Map(entry.material.parameters.map(parameter => [parameter.name, parameter.default]));
      const summary = report.materials[id] = {};
      for (const parameter of entry.material.shader.parameters) {
        const base = original[parameter.name] ?? defaults.get(parameter.name);
        const changed = typeof base === "boolean" ? !base : Array.isArray(base) ? base.map((value, index) => value + 0.11 + 0.03 * index) : Number(base) + 0.137;
        baker.nodeKeys = true;
        const before = keysOf(id);
        scene.setShadingParameters(id, { ...original, [parameter.name]: changed });
        await settled();
        // The textures the change reached under per-texture keys: those whose key changed.
        const drawn = new Set();
        keysOf(id).forEach((keys, index) => { for (const [node, key] of keys) { if (before[index].get(node) !== key) { drawn.add(node); report.rekeyed++; } else report.kept++; } });
        const what = [...drawn].sort().join(",") || "(none)";
        summary[what] = (summary[what] ?? 0) + 1;
        const selective = read(id);
        // Everything again, keyed by every value.
        baker.nodeKeys = false;
        sceneBaker.rebake();
        await settled();
        const whole = read(id);
        report.parameters++;
        for (const key of Object.keys(whole)) {
          report.textures++;
          if (selective[key] !== whole[key]) report.failures.push(`${id}.${parameter.name}: ${key} was not drawn again (textures drawn for it: ${what})`);
        }
        baker.nodeKeys = true;
        scene.setShadingParameters(id, original);
        await settled();
      }
    }
    return report;
  });
  console.log(`Textures drawn again per changed parameter, by material (set of textures: parameters): ${JSON.stringify(report.materials)}`);
  assert.deepEqual(report.failures, [], "a texture kept under per-texture keys holds what a bake of everything gives it");
  assert.ok(report.parameters > 0 && report.textures > 0, "parameters were changed and textures compared");
  // The keys do narrow: were every texture keyed by every value, none would be kept.
  assert.ok(report.kept > 0, "some textures are kept when a parameter they do not read changes");
  assert.equal(errors.length, 0, errors.join("\n"));
  console.log(`Per-texture bake keys passed: ${report.parameters} parameters, ${report.textures} textures compared with a bake of everything; ${report.kept} of ${report.kept + report.rekeyed} textures were kept across the changes.`);
} finally {
  await browser?.close();
  server.close();
}
