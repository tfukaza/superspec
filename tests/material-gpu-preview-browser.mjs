/**
 * GPU preview bake in the built Material Studio. A smoke test, not a parity test: GPU pixels are an
 * approximation and are never compared with the CPU baker. It checks that
 * - every starter either previews on the GPU with non-empty, plausibly varying texels or names why
 *   it falls back to the CPU path,
 * - the preview is never a ready revision, is replaced by the CPU result, and carries its own profile,
 * - a parameter edit reuses the compiled shader.
 * Edit-to-visible latency is printed per stage; under software rendering it is order-of-magnitude only.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "index.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

let browser;
try {
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const address = `http://127.0.0.1:${server.address().port}`;
  const ready = async message => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 180000 });
    assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", `${message} ${await page.locator("#status").textContent()}`);
  };
  await page.goto(`${address}/material.html?gpuprobe=1`);
  await ready("initial");
  // Record every GPU preview publication, which the CPU draft replaces within a second.
  await page.evaluate(() => {
    window.__gpu = [];
    const viewport = document.querySelector("#viewport");
    new MutationObserver(() => {
      const d = viewport.dataset;
      window.__gpu.push({ preview: d.gpuPreview && JSON.parse(d.gpuPreview), stats: d.gpuPreviewStats && JSON.parse(d.gpuPreviewStats),
        fallback: d.gpuPreviewFallback, label: document.querySelector("#preview-label")?.textContent, status: document.querySelector("#status").dataset.state, accepted: d.inputRevision, at: performance.now() });
    }).observe(viewport, { attributes: true, attributeFilter: ["data-gpu-preview", "data-gpu-preview-fallback", "data-gpu-preview-stats"] });
  });
  const take = () => page.evaluate(() => { const taken = window.__gpu; window.__gpu = []; return taken; });
  const editAndWait = async edit => {
    await take();
    const started = await page.evaluate(() => performance.now());
    await edit();
    await ready("edit");
    await waitForFrames(page);
    return { events: await take(), started };
  };

  const report = {};
  const firstUse = {};
  const starterStats = {};
  const starters = await page.locator("#starter option").evaluateAll(options => options.map(option => option.value).filter(value => !/yard/.test(value)));
  for (const starter of starters) {
    const { events } = await editAndWait(() => page.locator("#starter").selectOption(starter));
    const shown = events.find(event => event.preview);
    const fallback = events.findLast(event => event.fallback)?.fallback;
    report[starter] = shown ? "gpu" : `cpu (${fallback ?? "no event"})`;
    if (shown) { firstUse[starter] = shown.preview.timings; starterStats[starter] = events.findLast(event => event.stats)?.stats ?? shown.stats; }
    if (shown) {
      assert.equal(shown.preview.profile, "gpu-preview", `${starter}: own profile`);
      assert.match(shown.preview.key, /^gpu-preview:\d+:/, `${starter}: own cache key`);
      assert.equal(shown.status, "compiling", `${starter}: a GPU preview is never a ready revision`);
      const stats = events.findLast(event => event.stats)?.stats ?? shown.stats;
      assert.ok(stats.colorHigh > stats.colorLow, `${starter}: base color varies (${stats.colorLow}..${stats.colorHigh})`);
      assert.ok(stats.colorDistinct > 8, `${starter}: base color has many levels`);
      assert.ok(Number.isFinite(stats.heightLow) && Number.isFinite(stats.heightHigh), `${starter}: finite heights`);
      assert.ok(stats.normalXLow >= 0 && stats.normalXHigh <= 255, `${starter}: normals are bytes ${JSON.stringify(stats)}`);
    }
    // The CPU result always replaces it and the build ends ready, with no GPU label left behind.
    assert.equal(await page.locator("#viewport").getAttribute("data-gpu-preview"), null, `${starter}: the CPU result replaced the preview`);
    assert.equal(await page.locator("#export-glb").isDisabled(), false, `${starter}: a ready build can export`);
  }
  console.log("Per starter:", JSON.stringify(report, null, 2));
  console.log("First preview of each GPU starter (ms; compile is the shader's first compile):", JSON.stringify(firstUse));
  assert.ok(Object.values(report).some(value => value === "gpu"), "at least one starter previews on the GPU");
  // Receiver-normal outputs, rim-edge receiver features and live operations are all handled on the GPU.
  for (const starter of ["kowloon-concrete.material.ts", "kowloon-rusted-steel.material.ts"]) {
    assert.equal(report[starter], "gpu", `${starter} previews on the GPU`);
  }
  // The concrete's relief is strong; the steel study's only varies near rims, which a swatch has little of.
  assert.ok(starterStats["kowloon-concrete.material.ts"].normalXDistinct > 3, "the concrete's relief varies its normals");

  // Parameter edits and structural edits, with per-stage latency.
  await page.locator("#starter").selectOption("painted-metal.ms");
  await ready("painted metal");
  const input = page.locator("#parameters input[type=number]").first();
  const timings = {};
  if (await input.count()) {
    const [min, max] = await input.evaluate(node => [Number(node.min || 0), Number(node.max || 1)]);
    const parameter = await editAndWait(async () => { await input.fill(String((min + max) / 2)); await input.press("Tab"); });
    const stages = parameter.events.filter(event => event.preview);
    const low = stages.find(event => event.preview.stage === "low"), full = stages.find(event => event.preview.stage === "full");
    assert.ok(low && full, "a 256 px preview shows a quick stage, then the requested size");
    assert.ok(low.preview.width < full.preview.width && full.preview.width === 256, "the quick stage is smaller");
    assert.equal(low.preview.shader, "reused", "a parameter edit reuses the bound shader");
    assert.equal(full.preview.timings.compile, 0, "a parameter edit does not recompile");
    assert.ok(low.at <= full.at, "quick stage first");
    for (const [name, event] of [["parameterLow", low], ["parameterFull", full]]) timings[name] = { ...event.preview.timings, sinceEditMs: event.at - parameter.started };
    // A burst of input events in one task costs one bake per frame at most, and only the newest refines.
    const burst = await editAndWait(() => input.evaluate((node, [min, max]) => {
      for (let step = 1; step <= 20; step++) {
        node.value = String(min + (max - min) * step / 21);
        node.dispatchEvent(new Event("change", { bubbles: true }));
      }
    }, [min, max]));
    const revisions = new Set(burst.events.filter(event => event.preview?.stage === "full").map(event => event.accepted + ":" + event.preview.key));
    timings.burstFullBakes = revisions.size;
    assert.ok(revisions.size <= 2, `20 edits in one task make at most 2 full GPU bakes, got ${revisions.size}`);
  }
  // A 512 px target exceeds the band budget: it is drawn in bands, with a quarter-size look first.
  await page.locator("#resolution").selectOption("512");
  await ready("512 px");
  const [low512, high512] = await input.evaluate(node => [Number(node.min || 0), Number(node.max || 1)]);
  const big = await editAndWait(async () => { await input.fill(String(low512 + (high512 - low512) * 0.35)); await input.press("Tab"); });
  const bigFull = big.events.find(event => event.preview?.stage === "full");
  assert.ok(bigFull, "a 512 px swatch previews");
  assert.equal(bigFull.preview.width, 512);
  assert.equal(bigFull.preview.timings.bands, 2, "512 px is drawn in two bands");
  assert.ok(bigFull.stats.colorDistinct > 8 && bigFull.stats.colorHigh > bigFull.stats.colorLow, "a banded bake has no empty band");
  timings.bands512 = { ...bigFull.preview.timings, sinceEditMs: bigFull.at - big.started };
  await page.locator("#resolution").selectOption("256");
  await ready("256 px again");
  const code = await page.locator("#code").inputValue();
  const structural = await editAndWait(() => page.locator("#code").fill(`${code}\n`));
  const structuralShown = structural.events.find(event => event.preview?.stage === "full");
  if (structuralShown) timings.sourceEdit = { ...structuralShown.preview.timings, sinceEditMs: structuralShown.at - structural.started };
  // Lane H: chipped concrete's runoff is a CPU-baked field the GPU shader reads as a texture.
  for (const starter of ["chipped-concrete.ms", "chipped-concrete.material.ts"]) {
    assert.equal(report[starter], "gpu", `${starter} previews on the GPU through the hybrid lane`);
  }
  await page.locator("#starter").selectOption("chipped-concrete.ms");
  await ready("chipped concrete");
  await page.evaluate(() => {
    window.__fieldBakes = 0;
    const original = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function(message, ...options) {
      if (message?.action === "bake-fields") window.__fieldBakes++;
      return original.call(this, message, ...options);
    };
  });
  const hybridLow = await input.evaluate(node => [Number(node.min || 0), Number(node.max || 1)]);
  const gpuSide = await editAndWait(async () => { await input.fill(String((hybridLow[0] + hybridLow[1]) / 2)); await input.press("Tab"); });
  const gpuSideFull = gpuSide.events.find(event => event.preview?.stage === "full");
  assert.equal(gpuSideFull.preview.lane, "hybrid", "the runoff program previews as hybrid");
  assert.deepEqual(gpuSideFull.preview.cpuChannels, ["base_color"], "only the color rests on the CPU field");
  assert.ok(gpuSideFull.preview.gpuChannels.includes("roughness") && gpuSideFull.preview.gpuChannels.includes("height"), "the other channels are GPU only");
  assert.match(gpuSideFull.label, /hybrid: base color use CPU-baked runoff_mask.*other channels GPU only/, "the caption names the CPU-supplied channels");
  assert.equal(gpuSideFull.preview.fieldsFresh, true);
  assert.equal(await page.evaluate(() => window.__fieldBakes), 0, "a GPU-side parameter edit reuses the cached CPU field");
  assert.equal(gpuSideFull.status, "compiling", "a hybrid preview is not a ready revision");
  // New source edges change what the runoff field reads: the old field shows at once, the new one replaces it.
  const colorMean = events => events.filter(event => event.preview?.stage === "full" && event.stats).map(event => [event.preview.fieldsFresh, event.stats.colorMean]);
  const before = (colorMean(gpuSide.events).at(-1) ?? [])[1];
  const edge = await editAndWait(() => page.locator("#swatch-edge").click());
  const stages = edge.events.filter(event => event.preview?.stage === "full");
  assert.deepEqual(stages.map(event => event.preview.fieldsFresh), [false, true], "stale field first, then the refreshed one");
  assert.equal(await page.evaluate(() => window.__fieldBakes), 1, "a field-affecting edit bakes the CPU field once");
  assert.ok(stages[1].preview.fieldMs > 0);
  assert.ok(stages[1].stats.colorMean < stages[0].stats.colorMean, "runoff under the new edge darkens the color");
  assert.ok(before === undefined || Math.abs(stages[0].stats.colorMean - before) < 1e-6 * 255 + 0.5, "the stale stage still shows the previous field");
  timings.hybridFieldEdit = { fieldMs: stages[1].preview.fieldMs, ...stages[1].preview.timings, sinceEditMs: stages[1].at - edge.started };
  // What the shader cannot place is refused with its reason, and the CPU build alone follows.
  const unpartitionable = "material 1;\nlet shade = noise(surface * 4.0, seed(\"shade\"));\noutput base_color = vec3(shade * (1.0 - runoff(0.1, 0.5)));\noutput roughness = 0.5;\noutput metalness = 0;\n";
  await take();
  await page.locator("#code").fill(unpartitionable);
  await ready("runoff used directly in an output");
  assert.match(await page.locator("#viewport").getAttribute("data-gpu-preview-fallback"), /not partitionable: .*output uses runoff directly/);
  assert.equal(await page.locator("#viewport").getAttribute("data-gpu-preview"), null, "no GPU preview for a program that cannot be partitioned");
  await page.locator("#starter").selectOption("painted-metal.ms");
  await ready("painted metal again");
  // The label tells the viewer these textures are a preview, and it is gone once the CPU build is shown.
  assert.equal(await page.locator("#preview-label").isHidden(), true, "no preview label on a CPU build");
  console.log("GPU preview timings (ms, software rendering unless BROWSER_ARGS selects a GPU):", JSON.stringify(timings, null, 2));

  assert.deepEqual(errors, [], "no page errors");
  console.log("GPU preview bake acceptance passed.");
} finally {
  await browser?.close();
  server.close();
}
