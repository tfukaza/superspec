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
  // Which GL implementation ran this: headless Playwright defaults to SwiftShader (software); BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist" picks a GPU.
  const glInfo = await page.evaluate(() => {
    const gl = document.createElement("canvas").getContext("webgl2"); const ext = gl.getExtension("WEBGL_debug_renderer_info");
    return { renderer: ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER), textureUnits: gl.getParameter(gl.MAX_TEXTURE_IMAGE_UNITS), drawBuffers: gl.getParameter(gl.MAX_DRAW_BUFFERS), parallelCompile: !!gl.getExtension("KHR_parallel_shader_compile") };
  });
  const software = /SwiftShader|llvmpipe|Software/i.test(glInfo.renderer);
  console.log(`WebGL renderer: ${glInfo.renderer} (${software ? "software" : "GPU"}); texture units ${glInfo.textureUnits}, draw buffers ${glInfo.drawBuffers}, parallel shader compile ${glInfo.parallelCompile}`);
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
  // The CPU draft is faster than it was and runs in parallel with the GPU lane, so which one a viewer sees first is a race. To test
  // the GPU lane deterministically the 64 px CPU draft request is held until the lane has published (or given a reason).
  await page.evaluate(() => {
    const original = Worker.prototype.postMessage;
    window.__hold = false; window.__held = [];
    Worker.prototype.postMessage = function(message, ...options) {
      if (message?.kind === "material" && ((window.__hold && message.resolution === 64) || (window.__holdFull && message.resolution > 64))) {
        window.__held.push(() => original.call(this, message, ...options)); return;
      }
      return original.call(this, message, ...options);
    };
  });
  /** Runs an edit with the CPU draft held until the GPU lane has finished its attempt, then lets the build end. */
  const held = async edit => {
    await take();
    await page.evaluate(() => { delete document.querySelector("#viewport").dataset.gpuPreviewFallback; window.__hold = true; });
    const started = await page.evaluate(() => performance.now());
    await edit();
    await page.waitForFunction(() => {
      const d = document.querySelector("#viewport").dataset;
      if (d.gpuPreviewFallback) return true;
      if (!d.gpuPreview) return false;
      const preview = JSON.parse(d.gpuPreview);
      return preview.stage === "full" && preview.fieldsFresh !== false;
    }, {}, { timeout: 8000 }).catch(() => {});
    await page.evaluate(() => { window.__hold = window.__holdFull = false; for (const send of window.__held.splice(0)) send(); });
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 180000 });
    await waitForFrames(page);
    return { state: await page.locator("#status").getAttribute("data-state"), events: await take(), started };
  };
  const editAndWait = async edit => {
    const result = await held(edit);
    assert.equal(result.state, "ready", `edit ${await page.locator("#status").textContent()} ${(await page.locator("#diagnostics button").allTextContents()).join(" | ")} for ${edit}`);
    return result;
  };
  // Cold start of the most expensive program, in a page of its own so its shaders are new to the GL context. The CPU result
  // must not wait for the GPU compile, which continues in the background; once it has drawn once, a parameter drag shows its
  // GPU preview within a few frames of the edit, with no compile left.
  {
    const cold = await browser.newPage({ viewport: { width: 1200, height: 900 } });
    await cold.goto(`${address}/material.html?gpuprobe=1`);
    await cold.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 180000 });
    await cold.evaluate(() => {
      window.__events = [];
      const v = document.querySelector("#viewport");
      new MutationObserver(() => { const d = v.dataset; window.__events.push({ at: performance.now(), preview: d.gpuPreview && JSON.parse(d.gpuPreview), fallback: d.gpuPreviewFallback, state: document.querySelector("#status").dataset.state }); })
        .observe(v, { attributes: true, attributeFilter: ["data-gpu-preview", "data-gpu-preview-fallback"] });
    });
    const t0 = await cold.evaluate(() => performance.now());
    await cold.locator("#starter").selectOption("kowloon-concrete.material.ts");
    await cold.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready" && Number(document.querySelector("#viewport").dataset.inputRevision) > 0, {}, { timeout: 180000 });
    const cpuReady = await cold.evaluate(t => performance.now() - t, t0);
    const firstGpu = await cold.evaluate(() => window.__events.find(event => event.preview));
    // The GPU program is warm once it has been drawn (a late first preview is dropped, but the program is kept and drawn once).
    await cold.waitForFunction(() => { const s = window.__scene, source = s.gpuUniforms?.material?.shader?.source; return Boolean(source && s.bakerInstance?.drawn.has(source)); }, {}, { timeout: 120000 });
    const warmAt = await cold.evaluate(t => performance.now() - t, t0);
    await cold.waitForTimeout(300);
    await cold.evaluate(() => { window.__events.length = 0; });
    const edited = await cold.evaluate(async () => {
      const input = document.querySelector("#parameters input[type=number]");
      const started = performance.now();
      input.value = String((Number(input.min || 0) + Number(input.max || 1)) / 2);
      input.dispatchEvent(new Event("change", { bubbles: true }));
      for (let waited = 0; waited < 8000; waited += 4) {
        const shown = window.__events.find(event => event.preview);
        if (shown) return { at: shown.at - started, state: shown.state, compile: shown.preview.timings.compile, stage: shown.preview.stage };
        await new Promise(done => setTimeout(done, 4));
      }
      return null;
    });
    console.log(`Cold concrete: CPU result after ${cpuReady.toFixed(0)} ms; GPU first look ${firstGpu ? "shown" : "not shown (CPU came first)"}; GPU program warm after ${warmAt.toFixed(0)} ms; parameter edit shows the GPU preview after ${edited?.at.toFixed(0)} ms (${JSON.stringify(edited)})`);
    assert.ok(edited, "a parameter edit shows the GPU preview once the program is warm");
    assert.equal(edited.compile, 0, "no shader compile is left for the edit");
    assert.ok(edited.at < (software ? 3000 : 400), `the preview follows the edit within a few frames (${edited.at.toFixed(0)} ms)`);
    await cold.close();
  }

  const report = {};
  const firstUse = {};
  const starterStats = {};
  const unitsUsed = {};
  let steelEvents = [];
  const starters = await page.locator("#starter option").evaluateAll(options => options.map(option => option.value).filter(value => !/yard/.test(value)));
  for (const starter of starters) {
    const { events } = await editAndWait(() => page.locator("#starter").selectOption(starter));
    const shown = events.find(event => event.preview);
    const fallback = events.findLast(event => event.fallback)?.fallback;
    report[starter] = shown ? "gpu" : `cpu (${fallback ?? "no event"})`;
    if (starter === "kowloon-rusted-steel.material.ts") steelEvents = events;
    if (shown) { firstUse[starter] = shown.preview.timings; starterStats[starter] = events.findLast(event => event.stats)?.stats ?? shown.stats; }
    if (shown) {
      assert.equal(shown.preview.profile, "gpu-preview", `${starter}: own profile`);
      // Texture units: the baking program and the viewer material (maps, environment, live argument textures) fit the device.
      assert.ok(shown.preview.units.bake <= shown.preview.units.limit && shown.preview.units.viewer <= shown.preview.units.limit, `${starter}: texture units ${JSON.stringify(shown.preview.units)}`);
      assert.ok(shown.preview.units.limit <= glInfo.textureUnits, `${starter}: the limit is the device limit`);
      // The viewer figure is read from the drawn program: its maps (roughness and metalness bind the packed texture
      // once each), the environment, the sun's shadow map and any live argument textures.
      assert.ok(shown.preview.units.viewer >= 7, `${starter}: the viewer material binds at least seven units (${shown.preview.units.viewer})`);
      unitsUsed[starter] = shown.preview.units;
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
  console.log("Texture units per GPU starter:", JSON.stringify(unitsUsed));
  console.log("First preview of each GPU starter (ms; compile is the shader's first compile):", JSON.stringify(firstUse));
  assert.ok(Object.values(report).some(value => value === "gpu"), "at least one starter previews on the GPU");
  // Receiver-normal outputs, rim-edge receiver features and live operations are all handled on the GPU.
  for (const starter of ["kowloon-concrete.material.ts", "kowloon-rusted-steel.material.ts"]) {
    assert.equal(report[starter], "gpu", `${starter} previews on the GPU`);
  }
  // Live renderer operations follow the edit: their arguments are baked on the GPU too, not kept from the last CPU build.
  const steelFull = steelEvents.find(event => event.preview?.stage === "full");
  assert.ok(steelFull.preview.timings.live > 0, "the steel's live arguments were evaluated on the GPU");
  assert.doesNotMatch(steelFull.label, /last build/, "no stale lighting effects");
  // The concrete's relief is strong; the steel study's only varies near rims, which a swatch has little of.
  assert.ok(starterStats["kowloon-concrete.material.ts"].normalXDistinct > 3, "the concrete's relief varies its normals");

  // Parameter edits and structural edits, with per-stage latency.
  await editAndWait(() => page.locator("#starter").selectOption("painted-metal.ms"));
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
  // While a 64 px draft is what the viewport shows (its full build still running, as during a drag), the next edit's
  // preview is drawn at the requested size on the draft's layout, not at 64 px.
  await page.evaluate(() => { window.__holdFull = true; });
  await input.fill(String(low512 + (high512 - low512) * 0.6)); await input.press("Tab");
  await page.waitForFunction(() => /Draft shown/.test(document.querySelector("#status").textContent), {}, { timeout: 60000 });
  await page.evaluate(() => { window.__holdFull = true; });
  const overDraft = await editAndWait(async () => { await page.evaluate(() => { window.__holdFull = true; }); await input.fill(String(low512 + (high512 - low512) * 0.8)); await input.press("Tab"); });
  const overDraftFull = overDraft.events.find(event => event.preview?.stage === "full");
  assert.ok(overDraftFull, "an edit made while a draft is displayed previews");
  assert.equal(overDraftFull.preview.width, 512, "at the requested size, not the draft's 64 px");
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
  await editAndWait(() => page.locator("#starter").selectOption("chipped-concrete.ms"));
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
  const refused = await held(() => page.locator("#code").fill(unpartitionable));
  assert.equal(refused.state, "ready", "the CPU build alone follows");
  assert.match(await page.locator("#viewport").getAttribute("data-gpu-preview-fallback"), /not partitionable: .*output uses runoff directly/);
  assert.equal(await page.locator("#viewport").getAttribute("data-gpu-preview"), null, "no GPU preview for a program that cannot be partitioned");
  // Synthetic programs for the paths the starters do not reach. Each is a small source typed into the editor.
  const tryProgram = code => held(() => page.locator("#code").fill(code));
  const fullStage = events => events.findLast(event => event.preview?.stage === "full");
  const levelInput = page.locator("#parameters input[type=number]").first();
  const setLevel = value => held(async () => { await levelInput.fill(String(value)); await levelInput.press("Tab"); });

  // A program with no height output: its normal is flat, drawn once per target size and then skipped.
  const flat = await tryProgram(`material 1;\nparam level: scalar = 0.5 range(0, 1);\nlet grain = noise(surface * 8.0, seed("grain"));\noutput base_color = vec3(grain * level);\noutput roughness = 0.5;\noutput metalness = 0;\n`);
  assert.equal(flat.state, "ready");
  assert.ok(fullStage(flat.events), "a program with no height previews");
  assert.ok(fullStage(flat.events).stats.normalXDistinct <= 2, "its normal map is flat");
  const flatAgain = await setLevel(0.9);
  assert.equal(fullStage(flatAgain.events).preview.timings.pass2, 0, "pass 2 is skipped once the flat normal exists");
  assert.ok(fullStage(flatAgain.events).stats.colorMean > fullStage(flat.events).stats.colorMean, "the color still follows the parameter");

  // A receiver_normal facing away from the receiver is a CPU bake error: the GPU preview gives way to the CPU's report.
  const away = await tryProgram(`material 1;\noutput base_color = vec3(0.5);\noutput receiver_normal = vec3(0.0, 1.0, 0.0);\noutput roughness = 0.5;\noutput metalness = 0;\n`);
  assert.equal(away.state, "error", "the CPU build reports the bake error");
  assert.ok(!away.events.some(event => event.preview), "no GPU preview of a normal facing away");
  assert.match(await page.locator("#viewport").getAttribute("data-gpu-preview-fallback"), /face the same side/, "the fallback names the reason");

  // A swatch's four rim edges reach creaseDistance: a rim-distance gradient shows on the GPU, which an empty feature list would not give.
  await editAndWait(() => page.locator("#starter").selectOption("painted-metal.material.ts"));
  const rim = await tryProgram(`import { defineMaterial, vec3, creaseDistance, smoothstep } from "@superspec/material/v1";\nconst rim = creaseDistance("boundary", 10, 0.3);\nexport default defineMaterial({\n  base: { baseColor: vec3(0.5) },\n  channels: { baseColor: vec3(smoothstep(0.0, 0.3, rim)) },\n});\n`);
  if (rim.state === "ready" && fullStage(rim.events)) {
    const rimStats = fullStage(rim.events).stats;
    assert.ok(rimStats.colorHigh - rimStats.colorLow > 60, `rim distance varies across the swatch (${rimStats.colorLow}..${rimStats.colorHigh})`);
  } else timings.rimFeatures = `not exercised: ${rim.state} ${await page.locator("#viewport").getAttribute("data-gpu-preview-fallback")}`;

  // A hybrid CPU field that reads a parameter: editing it bakes the field again, with the old one showing first.
  // (The top edge set above is still in the project, so the new field has edges to read.)
  await page.locator("#starter").selectOption("chipped-concrete.ms");
  const reach = await tryProgram(`material 1;\nparam reach: scalar = 0.03 range(0.01, 0.4);\nlet wet = runoff(reach, 0.7);\noutput base_color = vec3(0.8 * (1.0 - wet * 0.6));\noutput roughness = 0.5;\noutput metalness = 0;\n`);
  assert.equal(reach.state, "ready");
  assert.equal(fullStage(reach.events)?.preview.lane, "hybrid");
  // A short edge, so the runoff band's width matters at its sides (a full-width edge saturates the swatch either way).
  const shortEdge = await held(() => page.locator("#environment").fill(JSON.stringify({ edges: [{ start: [0.4, 0, 1], end: [0.6, 0, 1] }], gravity: [0, 0, -1] })));
  assert.equal(shortEdge.state, "ready");
  const reachBakes = await page.evaluate(() => window.__fieldBakes);
  const reachEdit = await setLevel(0.3);
  const reachStages = reachEdit.events.filter(event => event.preview?.stage === "full");
  assert.deepEqual(reachStages.map(event => event.preview.fieldsFresh), [false, true], "the field the parameter changes shows stale, then fresh");
  assert.equal(await page.evaluate(() => window.__fieldBakes), reachBakes + 1, "one field bake for the parameter edit");
  assert.ok(reachStages[1].stats.colorMean < reachStages[0].stats.colorMean, "a wider runoff band darkens more of the swatch");

  // A hybrid CPU field that reads an image: the CPU bakes it with the image, the shader never needs the image for it.
  const imaged = await tryProgram(`material 1;\nparam reach: scalar = 0.1 range(0.01, 0.4);\nlet rusty = dot(sample_data("superspec-material:rust.png", surface), vec3(1.0, 0.0, 0.0));\nlet wet = runoff(reach, 0.7) * rusty;\noutput base_color = vec3(0.8 * (1.0 - wet * 0.6));\noutput roughness = 0.5;\noutput metalness = 0;\n`);
  assert.equal(imaged.state, "ready");
  const imagedFull = fullStage(imaged.events);
  assert.equal(imagedFull?.preview.lane, "hybrid", "a field that reads an image is a CPU field");
  assert.ok(imagedFull.stats.colorHigh > imagedFull.stats.colorLow, "its texture carries the image-modulated runoff");

  // A program the driver rejects (here: text that does not compile, appended through the baker's test seam) is caught
  // when it is compiled, not shown as whatever the targets held: the CPU build alone follows, with the reason.
  const grain = scale => `material 1;\nparam level: scalar = 0.5 range(0, 1);\nlet grain = noise(surface * ${scale}, seed("grain"));\noutput base_color = vec3(grain * level);\noutput roughness = 0.5;\noutput metalness = 0;\n`;
  await page.evaluate(() => {
    const scene = window.__scene;
    scene.bakerInstance.inject = "\n#error injected failure\n";
    window.__compiles = 0;
    const compile = scene.renderer.compileAsync;
    // Bake programs only (shader materials): the viewer also compiles each build's own materials before showing them.
    scene.renderer.compileAsync = function(...values) {
      if (values[0]?.children?.some(child => child.material?.isShaderMaterial)) window.__compiles++;
      return compile.apply(this, values);
    };
  });
  const rejected = await tryProgram(grain("11.0"));
  assert.equal(rejected.state, "ready", "the CPU build alone follows a rejected shader");
  assert.ok(!rejected.events.some(event => event.preview), "no GPU preview of a program that did not link");
  assert.match(await page.locator("#viewport").getAttribute("data-gpu-preview-fallback"), /did not compile/, "the fallback names the reason");
  assert.equal(await page.locator("#viewport").getAttribute("data-gpu-preview"), null);
  // The same source is not compiled again on the next edit: the failure is remembered.
  const compilesAfterFailure = await page.evaluate(() => window.__compiles);
  assert.ok(compilesAfterFailure >= 1, "the rejected program was compiled once");
  const rejectedAgain = await setLevel(0.8);
  assert.equal(rejectedAgain.state, "ready");
  assert.ok(!rejectedAgain.events.some(event => event.preview), "still no GPU preview for the rejected source");
  assert.match(await page.locator("#viewport").getAttribute("data-gpu-preview-fallback"), /did not compile/);
  assert.equal(await page.evaluate(() => window.__compiles), compilesAfterFailure, "a source that failed is not compiled again");
  // Another source compiles and previews as usual once the seam is clear.
  await page.evaluate(() => { window.__scene.bakerInstance.inject = ""; });
  const mended = await tryProgram(grain("13.0"));
  assert.ok(fullStage(mended.events), "a later program previews again");

  // Context loss and restore: nothing compiled or drawn is remembered, the shadow map is asked for again, and the next
  // edit previews on the new context.
  const restored = await page.evaluate(async () => {
    const scene = window.__scene, canvas = scene.renderer.domElement;
    const lose = scene.renderer.getContext().getExtension("WEBGL_lose_context");
    if (!lose) return null;
    const seen = {};
    canvas.addEventListener("webglcontextrestored", () => { seen.shadowMap = scene.renderer.shadowMap.needsUpdate; }, { once: true });
    const lost = new Promise(done => canvas.addEventListener("webglcontextlost", done, { once: true }));
    const back = new Promise(done => canvas.addEventListener("webglcontextrestored", done, { once: true }));
    lose.loseContext(); await lost;
    seen.during = scene.gpuPreviewUnavailable();
    await new Promise(done => setTimeout(done, 50));
    lose.restoreContext(); await back;
    await new Promise(done => setTimeout(done, 200));
    return { ...seen, after: scene.gpuPreviewUnavailable(), drawn: scene.bakerInstance.drawn.size, passes: scene.bakerInstance.passes.size };
  });
  if (restored) {
    assert.match(restored.during, /context is lost/, "no baking while the context is lost");
    assert.equal(restored.after, null, "baking is available again");
    assert.deepEqual([restored.drawn, restored.passes], [0, 0], "no program of the lost context is taken for compiled or drawn");
    assert.equal(restored.shadowMap, true, "the shadow map is drawn again");
    const afterLoss = await setLevel(0.3);
    assert.equal(afterLoss.state, "ready", "the build follows a restored context");
    assert.ok(fullStage(afterLoss.events), "the GPU preview works on the restored context");
    assert.ok(fullStage(afterLoss.events).stats.colorHigh > fullStage(afterLoss.events).stats.colorLow, "and draws real texels");
  } else console.log("WEBGL_lose_context is not available: context restore not exercised.");
  await editAndWait(() => page.locator("#starter").selectOption("painted-metal.ms"));
  // The label tells the viewer these textures are a preview, and it is gone once the CPU build is shown.
  assert.equal(await page.locator("#preview-label").isHidden(), true, "no preview label on a CPU build");
  console.log("GPU preview timings (ms, software rendering unless BROWSER_ARGS selects a GPU):", JSON.stringify(timings, null, 2));

  assert.deepEqual(errors, [], "no page errors");
  console.log("GPU preview bake acceptance passed.");
} finally {
  await browser?.close();
  server.close();
}
