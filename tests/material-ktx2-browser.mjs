/**
 * KTX2 delivery in the built Material Studio through the production Wasm worker:
 * - Download GLB stores KTX2 (KHR_texture_basisu) by default and PNG when unchecked, and the
 *   independent Khronos validator accepts both;
 * - a KTX2 GLB over the former 16 MB limit opens in Studio and renders compressed textures;
 * - with NATIVE_STUDIO set (a native superspec-studio command), the saved project rebuilt natively
 *   gives byte-identical GLBs, so browser and native share one encoder semantics.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { resolve, extname, sep } from "node:path";
import validator from "gltf-validator";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve("generated/material-ktx2-browser");
await mkdir(out, { recursive: true });
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

/** The JSON document of a GLB. */
function document(bytes) {
  const length = bytes.readUInt32LE(12);
  return JSON.parse(bytes.subarray(20, 20 + length).toString("utf8"));
}
async function validate(bytes, name) {
  const report = await validator.validateBytes(new Uint8Array(bytes), { uri: name, maxIssues: 50 });
  assert.equal(report.issues.numErrors, 0, `${name}: ${JSON.stringify(report.issues.messages.filter(m => m.severity === 0), null, 1)}`);
  return report;
}

const timings = {};
let browser;
try {
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, acceptDownloads: true });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  const ready = async () => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 300000 });
    assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", await page.locator("#status").textContent());
  };
  const rebuilt = async action => {
    const before = Number(await page.locator("#viewport").getAttribute("data-input-revision") ?? 0);
    await action();
    await page.waitForFunction(start => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > start, before, { timeout: 300000 });
    await ready(); await waitForFrames(page);
  };
  const download = async button => {
    const pending = page.waitForEvent("download", { timeout: 300000 });
    await page.locator(button).click();
    return readFile(await (await pending).path());
  };

  await page.goto(`http://127.0.0.1:${server.address().port}/material.html`);
  await ready();
  await rebuilt(() => page.locator("#starter").selectOption("kowloon-yard"));
  // Wait out the draft refinement so the export uses the full-size bake.
  await page.waitForTimeout(4000); await ready();

  // 1. KTX2 by default, PNG when unchecked; both valid glTF.
  assert.equal(await page.locator("#export-ktx2").isChecked(), true);
  let started = Date.now();
  const ktx2 = await download("#export-glb");
  timings.ktx2Export = Date.now() - started;
  await ready();
  await page.locator("#export-ktx2").uncheck();
  started = Date.now();
  const png = await download("#export-glb");
  timings.pngExport = Date.now() - started;
  await ready();
  await page.locator("#export-ktx2").check();
  const ktx2Document = document(ktx2);
  assert.deepEqual(ktx2Document.extensionsRequired, ["KHR_texture_basisu"]);
  assert.ok(ktx2Document.images.every(image => image.mimeType === "image/ktx2"));
  assert.ok(document(png).images.every(image => image.mimeType === "image/png"));
  await validate(ktx2, "studio-ktx2.glb");
  await validate(png, "studio-png.glb");
  timings.ktx2Bytes = ktx2.length; timings.pngBytes = png.length;
  assert.ok(ktx2.length < png.length, `KTX2 ${ktx2.length} bytes, PNG ${png.length}`);
  await writeFile(resolve(out, "studio-ktx2.glb"), ktx2);
  await writeFile(resolve(out, "studio-png.glb"), png);
  const project = await download("#save-project");
  await writeFile(resolve(out, "yard.studio.json"), project);

  // 2. Native parity: the same saved project, built natively, gives the same bytes.
  if (process.env.NATIVE_STUDIO) {
    const native = textures => execFileSync(process.env.NATIVE_STUDIO_SHELL ?? process.env.NATIVE_STUDIO,
      [...(process.env.NATIVE_STUDIO_SHELL ? [process.env.NATIVE_STUDIO] : []), "glb", process.env.NATIVE_PROJECT ?? resolve(out, "yard.studio.json"), "--textures", textures],
      { maxBuffer: 1024 * 1024 * 1024 });
    assert.ok(Buffer.compare(native("png"), png) === 0, "native PNG GLB differs from the browser's");
    assert.ok(Buffer.compare(native("ktx2"), ktx2) === 0, "native KTX2 GLB differs from the browser's");
    timings.nativeParity = true;
  }

  // 3. A KTX2 GLB larger than the former 16 MB limit opens and renders compressed textures.
  const large = process.env.LARGE_GLB;
  if (large) {
    // From a fresh page: a scene project composes its own models and ignores an opened base.
    await page.goto(`http://127.0.0.1:${server.address().port}/material.html`);
    await ready();
    started = Date.now();
    await rebuilt(() => page.locator("#open-glb").setInputFiles(large));
    timings.largeOpen = Date.now() - started;
    const kinds = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.textureKinds ?? "{}"));
    // KTX2 maps upload compressed; PNG maps as images.
    const compressed = (document(await readFile(large)).extensionsRequired ?? []).includes("KHR_texture_basisu");
    assert.ok(compressed ? (kinds.compressed ?? 0) > 0 && !kinds.image : (kinds.image ?? 0) > 0 && !kinds.compressed, `textures ${JSON.stringify(kinds)}`);
    timings.largeTextures = kinds;
  }
  assert.deepEqual(errors.filter(error => !/Source could not be compiled|MS_/.test(error)), [], errors.join("\n"));
} finally {
  await browser?.close();
  server.close();
}
console.log(`Material KTX2 browser acceptance passed: ${JSON.stringify(timings)}`);
