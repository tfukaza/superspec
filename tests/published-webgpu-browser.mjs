/** Run after npm run build:native-viewer, on a browser with WebGPU. WEBGPU_WASHROOM can point
 * to a baked washroom GLB for an additional end-to-end appearance and texture decode check. */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { PNG } from "pngjs";
import { launchBrowser } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/native-viewer");
// The same washroom meshes supply the ordinary and styled manifests. Serving the buffer
// externally lets us count whether fidelity preflight decodes a city's sidecars twice.
const { glb } = buildProject(64);
const binary = Buffer.from(glb), jsonLength = binary.readUInt32LE(12);
const plain = JSON.parse(binary.toString("utf8", 20, 20 + jsonLength));
plain.buffers[0].uri = "washroom.bin";
const styled = structuredClone(plain);
styled.extras = { superspec: { lighting: { version: 1, ambientOcclusion: { enabled: true } } } };
const world = structuredClone(styled);
world.scenes[world.scene ?? 0].extras = {
  worldSpec: true, shopLightPositions: [[0, 2, 0]],
  worldSpecRender: { version: 1, effects: [], bloom: false, outlines: false },
};
const graphicsProfile = {
  version: 1, preset: "high", overrides: { renderScale: .75, geometry: "medium", materials: "very-high", shadows: "medium" },
  diagnostics: { lighting: { fixtures: false }, effects: { displayAA: false } },
};
const fixtures = new Map([
  ["/fixtures/plain.gltf", JSON.stringify(plain)], ["/fixtures/styled.gltf", JSON.stringify(styled)],
  ["/fixtures/world.gltf", JSON.stringify(world)], ["/fixtures/graphics.json", JSON.stringify(graphicsProfile)],
  ["/fixtures/washroom.bin", binary.subarray(28 + jsonLength)], ["/fixtures/malformed.gltf", "{ invalid JSON"],
]);
if (process.env.WEBGPU_WASHROOM) fixtures.set("/fixtures/baked.glb", await readFile(process.env.WEBGPU_WASHROOM));
const counts = new Map();
const server = createServer(async (request, response) => {
  try {
    const name = new URL(request.url, "http://localhost").pathname;
    const type = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".gltf": "model/gltf+json",
      ".glb": "model/gltf-binary", ".wasm": "application/wasm", ".json": "application/json" }[extname(name)] ?? "application/octet-stream";
    const path = resolve(root, "." + name);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    const bytes = fixtures.get(name) ?? await readFile(path);
    if (fixtures.has(name)) counts.set(name, (counts.get(name) ?? 0) + 1);
    response.writeHead(200, { "Content-Type": type, "Cache-Control": "no-store" }).end(bytes);
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));
let browser;
try {
  browser = await launchBrowser();
  const origin = "http://127.0.0.1:" + server.address().port;
  const page = await browser.newPage({ viewport: { width: 960, height: 640 } });
  const errors = [];
  let expectGpuValidation = false;
  const validationMessages = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", event => {
    if (expectGpuValidation) validationMessages.push(event.text());
    if (event.type() === "error" && !event.text().includes("Device Lost") &&
      !(expectGpuValidation && /GPUValidationError|usage.*zero/i.test(event.text()))) errors.push(event.text());
  });
  const ready = async (target, verdict) => {
    await target.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status").dataset.state), undefined, { timeout: 120000 });
    const status = await target.locator("#status").evaluate(el => ({ ...el.dataset, text: el.textContent }));
    assert.equal(status.state === "error" ? "error" : status.renderer, verdict, status.text);
    if (verdict !== "error") assert.equal(await target.locator("#viewport canvas").count(), 1);
    return status;
  };
  const open = async (target, asset, renderer, verdict, settings = {}) => {
    const query = new URLSearchParams({ gltf: "/fixtures/" + asset, gpuprobe: "", renderer, ...settings });
    await target.goto(origin + "/native.html?" + query);
    return ready(target, verdict);
  };
  const graphics = () => page.evaluate(() => {
    const view = window.__worldSpecView;
    const { renderer, scene, camera, controls } = view;
    const canvas = renderer.domElement;
    const gl = view.backend === "webgl" ? renderer.getContext() : undefined;
    const shadowMaps = {};
    scene.traverse(light => {
      if (!light.castShadow || (!light.isDirectionalLight && !light.isSpotLight)) return;
      const kind = light.isDirectionalLight ? "sun" : "fixture";
      shadowMaps[kind] = light.shadow.mapSize.x;
    });
    return { settings: view.graphics, css: [canvas.clientWidth, canvas.clientHeight], shadowMaps,
      buffer: gl ? [gl.drawingBufferWidth, gl.drawingBufferHeight] : [canvas.width, canvas.height],
      pose: [...camera.position.toArray(), ...controls.target.toArray()], shadows: renderer.shadowMap.enabled };
  });
  const assertScale = (view, scale) => {
    assert.equal(view.settings.renderScale, scale);
    assert.deepEqual(view.buffer, view.css.map(size => Math.floor(size * scale)));
  };
  const assertPose = (after, before) => {
    assert.ok(after.every((v, i) => Math.abs(v - before[i]) < 1e-7), `Camera shifted from ${before} to ${after}`);
  };
  const recovered = async () => {
    try {
      await page.waitForFunction(() => {
        const { state, renderer } = document.querySelector("#status").dataset;
        return state === "error" || (state === "ready" && renderer === "webgl");
      }, undefined, { timeout: 20000 });
      assert.equal(await page.locator("#status").getAttribute("data-renderer"), "webgl");
    } catch (cause) {
      throw Error(`Failed to recover with WebGL: ${await page.locator("#status").textContent()}; ${[...errors, ...validationMessages].join("\n")}`, { cause });
    }
  };
  const assertImage = async () => {
    const { data } = PNG.sync.read(await page.locator("#viewport canvas").screenshot());
    const colors = new Set();
    for (let i = 0; i < data.length; i += 4 * 17) colors.add((data[i] << 16) | (data[i + 1] << 8) | data[i + 2]);
    assert.ok(colors.size > 25, `Expected lit washroom geometry in the viewport, found ${colors.size} colors.`);
  };
  const noGPU = await browser.newPage();
  await noGPU.addInitScript(() => { Object.defineProperty(navigator, "gpu", { value: undefined, configurable: true }); });
  assert.match((await open(noGPU, "plain.gltf", "auto", "webgl")).text, /unavailable/i);
  await noGPU.close();
  const noAdapter = await browser.newPage();
  await noAdapter.addInitScript(() => { Object.defineProperty(navigator.gpu, "requestAdapter", { value: async () => null }); });
  assert.match((await open(noAdapter, "plain.gltf", "webgpu", "webgl")).text, /fallback|initialize/i);
  await noAdapter.close();
  await open(page, "world.gltf", "webgl", "webgl");
  assert.equal(await page.evaluate(() => performance.getEntriesByType("resource").some(r => r.name.includes("/native-webgpu-"))), false);
  const preset = page.getByLabel("Graphics", { exact: true });
  assert.equal(await preset.inputValue(), "medium");
  const medium = await graphics();
  assert.equal(medium.settings.preset, "medium");
  assertScale(medium, 1);
  assert.deepEqual(medium.shadowMaps, { sun: 2048, fixture: 512 });
  const viewerPose = await page.evaluate(() => {
    const { camera, controls } = window.__worldSpecView;
    camera.position.set(5.3, 4.1, 6.2);
    controls.target.set(.8, 1.4, -.75);
    controls.update();
    return [...camera.position.toArray(), ...controls.target.toArray()];
  });
  await Promise.all([page.waitForURL(url => JSON.parse(url.searchParams.get("profile") ?? "{}").preset === "very-high"),
    preset.selectOption("very-high")]);
  await ready(page, "webgl");
  const veryHigh = await graphics();
  assert.equal(await preset.inputValue(), "very-high");
  assert.equal(veryHigh.settings.preset, "very-high");
  assertScale(veryHigh, 1.25);
  assert.deepEqual(veryHigh.css, medium.css);
  assert.deepEqual(veryHigh.shadowMaps, { sun: 4096, fixture: 1024 });
  assertPose(veryHigh.pose, viewerPose);
  await page.getByText("Graphics options", { exact: true }).click();
  await page.getByLabel("Render scale (%)", { exact: true }).fill("80");
  await page.getByLabel("Geometry LOD", { exact: true }).selectOption("medium");
  await page.getByLabel("Material detail", { exact: true }).selectOption("high");
  await page.getByLabel("Shadows", { exact: true }).selectOption("medium");
  await Promise.all([page.waitForURL(url => JSON.parse(url.searchParams.get("profile") ?? "{}").overrides?.renderScale === .8),
    page.getByRole("button", { name: "Apply", exact: true }).click()]);
  await ready(page, "webgl");
  const custom = await graphics();
  assertScale(custom, .8);
  assert.deepEqual(custom.css, medium.css);
  assert.deepEqual(custom.shadowMaps, medium.shadowMaps);
  assert.equal(custom.settings.geometryLodBias, medium.settings.geometryLodBias);
  assert.equal(custom.settings.materialAnisotropy, 8);
  assertPose(custom.pose, viewerPose);
  await open(page, "world.gltf", "webgl", "webgl", { profile: "/fixtures/graphics.json" });
  const fromFile = await graphics();
  assert.equal(fromFile.settings.preset, "high");
  assertScale(fromFile, .75);
  assert.equal(fromFile.settings.materialAnisotropy, 16);
  assert.deepEqual(fromFile.shadowMaps, { sun: 2048 });
  assert.equal(counts.get("/fixtures/graphics.json"), 1);
  await open(page, "world.gltf", "webgl", "webgl", { profile: "/fixtures/graphics.json", fixtures: "on", shadows: "off" });
  const legacy = await graphics();
  assertScale(legacy, .75);
  assert.equal(legacy.shadows, false);
  assert.equal(await page.evaluate(() => !!window.__worldSpecView.scene.getObjectByName("shop-fixture")), true);
  await page.evaluate(() => window.dispatchEvent(new Event("pagehide")));
  assert.equal(await page.locator("#viewport canvas").count(), 0);
  assert.equal(await page.evaluate(() => window.__worldSpecView), undefined);
  const ordinary = await open(page, "plain.gltf", "auto", "webgpu", { profile: "/fixtures/graphics.json" });
  assert.equal(ordinary.appearance, "standard");
  assertScale(await graphics(), .75);
  const adapter = await page.evaluate(async () => {
    const view = window.__worldSpecView;
    if (!view.renderer.backend.isWebGPUBackend) throw Error("The selected backend is not WebGPU");
    await view.settle();
    const info = view.renderer.backend.device.adapterInfo;
    return info?.description || info?.vendor;
  });
  await assertImage();
  const pose = await page.evaluate(() => {
    const { renderer, controls, camera } = window.__worldSpecView;
    controls.target.set(.8, 1.4, -.75);
    camera.position.set(5.3, 4.1, 6.2); controls.update();
    const before = [...camera.position.toArray(), ...controls.target.toArray()];
    renderer.backend.device.destroy();
    return before;
  });
  await recovered();
  assert.equal(await page.locator("#viewport canvas").count(), 1, "A lost device's canvas must be removed");
  const fallback = await graphics();
  assertPose(fallback.pose, pose);
  assertScale(fallback, .75);
  const buffers = counts.get("/fixtures/washroom.bin");
  assert.match((await open(page, "styled.gltf", "auto", "webgl")).text, /lighting|shadow/i);
  assert.equal(counts.get("/fixtures/washroom.bin") - buffers, 1, "Fallback should load external buffers once");
  const styledPreview = await open(page, "styled.gltf", "webgpu", "webgpu");
  assert.equal(styledPreview.appearance, "portable");
  await assertImage();
  expectGpuValidation = true;
  await page.evaluate(async () => {
    // A read-mappable buffer may only combine MAP_READ with COPY_DST. Combining it
    // with STORAGE must go through the device's uncaptured-error handler.
    const device = window.__worldSpecView.renderer.backend.device;
    device.createBuffer({ size: 4, usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.STORAGE });
    device.queue.submit([]);
    await device.queue.onSubmittedWorkDone();
  });
  await recovered();
  assert.match(await page.locator("#status").textContent(), /WebGPU rendering failed/);
  assert.equal(await page.locator("#viewport canvas").count(), 1);
  expectGpuValidation = false;
  if (fixtures.has("/fixtures/baked.glb")) {
    await open(page, "baked.glb", "auto", "webgl");
    assert.equal((await open(page, "baked.glb", "webgpu", "webgpu")).appearance, "portable");
    await page.evaluate(() => window.__worldSpecView.settle());
    await assertImage();
  }
  await open(page, "malformed.gltf", "auto", "error");
  assert.equal(counts.get("/fixtures/malformed.gltf"), 1, "Invalid assets should report their error without a renderer retry");
  assert.equal(await page.locator("#viewport canvas").count(), 0, "Failed open should release its canvas");
  await open(page, "malformed.gltf", "webgl", "error");
  assert.equal(await page.locator("#viewport canvas").count(), 0, "A WebGL parse failure should release its canvas");
  assert.deepEqual(errors, [], "Unexpected browser or GPU errors");
  console.log(JSON.stringify({ passed: true, adapter, bakedWashroom: fixtures.has("/fixtures/baked.glb") }));
} finally { await browser?.close(); await new Promise(done => server.close(done)); }
