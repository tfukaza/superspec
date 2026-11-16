/**
 * The MaterialSpec benchmark scene (tests/studio/washroom) in the built Material Studio: opens
 * the project through the production Wasm workers, checks every material landed on its meshes
 * and reports how long the scene took to load. RESOLUTION (default 256) sets the texture size
 * (above 512 the project is given the production profile, the only one that allows it);
 * SCREENSHOT names a PNG of the loaded viewport. LIGHTING names a JSON lighting record the project
 * then carries (see docs/plans/lighting-settings.md); without it the scene has the shipped look.
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
  // A preview project holds at most 512 texels a side; above that only the production profile is a valid project.
  if (resolution > 512) project.profile = "production";
  if (process.env.LIGHTING) project.lighting = JSON.parse(await readFile(process.env.LIGHTING, "utf8"));
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  // Main-thread long tasks (over 50 ms), so jank on load and on an edit is recorded with the timings.
  await page.addInitScript(() => {
    window.__longTasks = [];
    try {
      new PerformanceObserver(list => { for (const entry of list.getEntries()) window.__longTasks.push([Math.round(entry.startTime), Math.round(entry.duration)]); })
        .observe({ entryTypes: ["longtask"] });
    } catch { /* not offered by this browser */ }
  });
  const pageNow = () => page.evaluate(() => performance.now());
  /** Long tasks that began in a span of page time: how many, their sum and the longest, milliseconds. */
  const longTasks = async (from, to = Infinity) => {
    const tasks = (await page.evaluate(() => window.__longTasks ?? [])).filter(([start]) => start >= from && start < to);
    return { count: tasks.length, total: tasks.reduce((sum, [, duration]) => sum + duration, 0), longest: Math.max(0, ...tasks.map(([, duration]) => duration)),
      tasks: tasks.map(([start, duration]) => `${Math.round(start - from)}+${duration}`).join(" ") };
  };
  const state = () => page.locator("#status").getAttribute("data-state");
  const ready = async () => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 900000 });
    const findings = await state() === "ready" ? "" : (await page.locator("#diagnostics button").allTextContents()).join("\n");
    assert.equal(await state(), "ready", `${await page.locator("#status").textContent()}\n${findings}`);
  };
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html${process.env.QUERY ? `?${process.env.QUERY}` : ""}`);
  await ready();

  const before = Number(await page.locator("#viewport").getAttribute("data-input-revision") ?? 0);
  // PROFILE names a .cpuprofile of the page's main thread from here to the end of the edits (open it in DevTools), for
  // saying what a long task was.
  const profiler = process.env.PROFILE ? await page.context().newCDPSession(page) : null;
  if (profiler) { await profiler.send("Profiler.enable"); await profiler.send("Profiler.setSamplingInterval", { interval: 500 }); await profiler.send("Profiler.start"); }
  const started = Date.now(), loadFrom = await pageNow();
  await page.locator("#open-project").setInputFiles({ name: "washroom.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
  // The first thing shown is the quick draft; the scene is loaded when the full-size build is ready.
  const revision = name => page.waitForFunction(([name, start]) => Number(document.querySelector("#viewport").dataset[name] ?? 0) > start, [name, before], { timeout: 900000 });
  // A project the Studio refuses (or fails to build) shows no revision at all: that ends the run at once, with the reason.
  const refused = page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "error", {}, { timeout: 900000 })
    .then(async () => { throw new Error(`The Studio did not open the project: ${await page.locator("#status").textContent()}`); });
  refused.catch(() => { /* reported through the races below, or the page closed first */ });
  await Promise.race([revision("draftRevision"), revision("inputRevision"), refused]);
  const firstPreview = Date.now() - started;
  await Promise.race([revision("inputRevision"), refused]);
  await ready(); await waitForFrames(page);
  const loaded = Date.now() - started;
  // The shader preview shows the fast-compiling relief first and swaps to the per-pixel one in the background.
  let perPixel = "";
  if (await page.evaluate(() => document.querySelector("#viewport").dataset.relief !== undefined) && !/relief=quad/.test(process.env.QUERY ?? "")) {
    await page.waitForFunction(() => document.querySelector("#viewport").dataset.relief === "pixel", {}, { timeout: 900000 });
    perPixel = `, per-pixel relief ${Date.now() - started} ms`;
    // The programs then settle on the form that draws fastest and compiles slowest; SETTLED=1 waits for it.
    if (process.env.SETTLED === "1" && !/noise=/.test(process.env.QUERY ?? "")) {
      await page.waitForFunction(() => document.querySelector("#viewport").dataset.noise === "inline", {}, { timeout: 900000 });
      perPixel += `, settled programs ${Date.now() - started} ms`;
    }
  }
  // By default GPU-baked maps follow the first look (scene-bake.ts): the scene is baked when every mesh shows its maps.
  const sceneBake = () => page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null"));
  const baked = async () => {
    if (!await sceneBake()) return null;
    await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake).state === "baked", {}, { timeout: 900000 });
    await waitForFrames(page);
    return sceneBake();
  };
  let bakedMaps = "";
  const firstBake = await baked();
  if (firstBake) {
    bakedMaps = `, GPU-baked maps ${Date.now() - started} ms`;
    console.log(`Scene bake: ${JSON.stringify(firstBake)}`);    if (!/preview=/.test(process.env.QUERY ?? "")) {
      assert.deepEqual(firstBake.unsupported, {}, "every mesh of the washroom shows GPU-baked maps");
      assert.equal(firstBake.baked, firstBake.meshes);
      // The per-pixel looks shown while a value changes then settle on the form that draws fastest, behind the scene; SETTLED=1 waits for it.
      if (process.env.SETTLED === "1" && !/noise=/.test(process.env.QUERY ?? "")) {
        await page.waitForFunction(() => document.querySelector("#viewport").dataset.noise === "inline", {}, { timeout: 900000 });
        bakedMaps += `, settled edit programs ${Date.now() - started} ms (${await page.evaluate(() => document.querySelector("#viewport").dataset.noiseMs)} ms after the scene was complete)`;
      }
    }
  }

  console.log(`Long tasks on load (ms after opening the project, start+duration): ${JSON.stringify(await longTasks(loadFrom))}`);
  const bindings = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.bindings ?? "{}"));
  const expected = Object.fromEntries(project.materials.flatMap(material => material.meshes.map(mesh => [mesh, material.id])));
  assert.deepEqual(bindings, expected, "every mesh shows its material");
  if (!only) assert.equal(Object.keys(expected).length, materials.reduce((sum, material) => sum + material.meshes.length, 0));
  assert.equal(errors.length, 0, errors.join("\n"));
  // The view is lit by the project's record (the shipped look without one), under the page's overrides.
  {
    const lighting = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.lighting ?? "null"));
    assert.ok(lighting, "the viewport states the lighting record in effect");
    const record = project.lighting ?? {}, query = new URLSearchParams(process.env.QUERY ?? "");
    assert.equal(lighting.version, 1);
    assert.equal(lighting.exposure, record.exposure ?? 1);
    assert.equal(lighting.ambient.intensity, record.ambient?.intensity ?? 0.65);
    assert.deepEqual(lighting.lights[0]?.direction.map(axis => +axis.toFixed(3)), (record.lights?.[0]?.direction ?? [-0.303, -0.505, 0.808]).map(axis => +axis.toFixed(3)));
    if (query.get("ao") === "off") assert.equal(lighting.ambientOcclusion.enabled, false);
    if (query.get("shadowstyle") === "plain") assert.equal(lighting.lights[0].shadow.style, "plain");
  }
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
    const began = Date.now(), editFrom = await pageNow();
    await control.fill(value); await control.press("Tab");
    const shown = name => page.waitForFunction(([name, from]) => Number(document.querySelector("#viewport").dataset[name] ?? 0) > from, [name, start], { timeout: 900000 });
    await Promise.race([shown("draftRevision"), shown("inputRevision")]);
    const draft = Date.now() - began;
    await shown("inputRevision"); await ready();
    edits[material] = { draft, full: Date.now() - began };
    // With GPU-baked maps the edit shows per pixel at once and is baked again when it settles.
    const rebaked = await baked();
    if (rebaked) Object.assign(edits[material], { baked: Date.now() - began, drawn: rebaked.drawn, reused: rebaked.reused, drawMs: rebaked.drawMs, fieldMs: rebaked.fieldMs,
      nodesDrawn: rebaked.nodesDrawn, nodesReused: rebaked.nodesReused, details: rebaked.details.join("; ") });
    edits[material].longTasks = await longTasks(editFrom);
  }
  console.log(`Edit latency (ms): ${JSON.stringify(edits)}`);
  if (profiler) await writeFile(process.env.PROFILE, JSON.stringify((await profiler.send("Profiler.stop")).profile));
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
  // Draw time without the display's frame pacing: with QUERY containing gpuprobe, redraw directly and read one pixel
  // so the GPU has finished. (The frame times above wait two animation frames, so a GPU reads as about two refreshes.)
  const draws = await page.evaluate(async () => {
    const scene = window.__scene;
    if (!scene) return null;
    const gl = scene.renderer.getContext(), pixel = new Uint8Array(4), times = [];
    for (let i = 0; i < 24; i++) {
      const start = performance.now();
      scene.draw();
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
      times.push(performance.now() - start);
    }
    times.sort((a, b) => a - b);
    // The GPU's own time for a frame, where the browser offers timer queries: no read-back and no CPU time in it.
    const timer = gl.getExtension("EXT_disjoint_timer_query_webgl2");
    let gpu = "timer queries unavailable";
    if (timer) {
      const frames = [];
      for (let i = 0; i < 12; i++) {
        const query = gl.createQuery();
        gl.beginQuery(timer.TIME_ELAPSED_EXT, query);
        scene.draw();
        gl.endQuery(timer.TIME_ELAPSED_EXT);
        for (let waited = 0; waited < 500 && !gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE); waited++) await new Promise(done => setTimeout(done, 2));
        if (gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE) && !gl.getParameter(timer.GPU_DISJOINT_EXT)) frames.push(gl.getQueryParameter(query, gl.QUERY_RESULT) / 1e6);
        gl.deleteQuery(query);
      }
      frames.sort((a, b) => a - b);
      gpu = frames.length ? { median: +frames[frames.length >> 1].toFixed(2), min: +frames[0].toFixed(2), max: +frames.at(-1).toFixed(2), frames: frames.length } : "no timer query completed";
    }
    return { median: +times[12].toFixed(2), min: +times[0].toFixed(2), max: +times[23].toFixed(2), gpuTimerQuery: gpu, shadowExtentMeters: scene.shadowStyle?.uniforms.msShadowExtent.value };
  });
  if (draws) console.log(`Draw time (ms; draw and one-pixel read, 24 draws; gpuTimerQuery is the GPU's time alone): ${JSON.stringify(draws)}`);
  if (process.env.SCREENSHOT) await writeFile(process.env.SCREENSHOT, await page.locator("#viewport").screenshot());
  // Frame time while a value is dragged (with QUERY containing gpuprobe): a scalar parameter of DRAG (default the
  // concrete walls) changes before every frame, as a slider does, so its meshes show their per-pixel look and no bake
  // starts. `form` says which form of the per-pixel programs was drawn (`looped` until they settle on `inline`). The
  // parameter gets its value back afterwards and the maps are baked again.
  const drag = await page.evaluate(async material => {
    const scene = window.__scene, entry = scene?.shading.get(material);
    if (!entry) return null;
    const host = document.querySelector("#viewport");
    const gl = scene.renderer.getContext(), timer = gl.getExtension("EXT_disjoint_timer_query_webgl2"), pixel = new Uint8Array(4);
    const original = JSON.parse(entry.values);
    const parameter = entry.material.shader.parameters.find(candidate => candidate.type === "scalar");
    if (!parameter) return null;
    const base = Number(original[parameter.name] ?? entry.material.parameters.find(candidate => candidate.name === parameter.name)?.default ?? 0);
    const frames = [], reads = [];
    let shown = "";
    for (let i = 0; i < 40; i++) {
      scene.setShadingParameters(material, { ...original, [parameter.name]: base + 0.0005 * (1 + i % 3) });
      shown = scene.models.children.filter(mesh => mesh.userData.shaded === material).map(mesh => mesh.material.userData.programUniforms ? "shader" : "maps").join(",");
      const start = performance.now();
      const query = timer && gl.createQuery();
      if (query) gl.beginQuery(timer.TIME_ELAPSED_EXT, query);
      scene.draw();
      if (query) gl.endQuery(timer.TIME_ELAPSED_EXT);
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
      // The first frames pay for uniform state and are left out.
      if (i >= 8) reads.push(performance.now() - start);
      if (query) {
        for (let waited = 0; waited < 60 && !gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE); waited++) await new Promise(done => setTimeout(done, 1));
        if (i >= 8 && gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE) && !gl.getParameter(timer.GPU_DISJOINT_EXT)) frames.push(gl.getQueryParameter(query, gl.QUERY_RESULT) / 1e6);
        gl.deleteQuery(query);
      }
    }
    const form = host.dataset.noise ?? (new URLSearchParams(location.search).get("noise") === "inline" ? "inline" : "looped");
    scene.setShadingParameters(material, original);
    const summary = values => { values.sort((a, b) => a - b); return values.length ? { median: +values[values.length >> 1].toFixed(2), min: +values[0].toFixed(2), max: +values.at(-1).toFixed(2), frames: values.length } : null; };
    return { material, parameter: parameter.name, form, shown, gpuTimerQuery: summary(frames) ?? "timer queries unavailable", drawAndRead: summary(reads) };
  }, process.env.DRAG ?? "concrete-walls");
  if (drag) {
    console.log(`Drag frame time (ms; the GPU's time for a frame while a parameter changes every frame): ${JSON.stringify(drag)}`);
    await baked();
  }
  // CLOSEUP names a PNG of the tall wall seen close, for judging relief and texture detail.
  if (process.env.CLOSEUP) {
    const box = await page.locator("#viewport canvas").boundingBox();
    await page.mouse.move(box.x + box.width * 0.55, box.y + box.height * 0.4);
    for (let i = 0; i < 8; i++) { await page.mouse.wheel(0, -240); await waitForFrames(page, 2); }
    await waitForFrames(page, 6);
    await writeFile(process.env.CLOSEUP, await page.locator("#viewport canvas").screenshot());
  }
  // Source edits (after the screenshots: they change the scene). A source edit rebuilds the scene; what the GPU bake
  // then draws again says how much of the baked maps a rebuild reuses. First an edit that leaves every program as it
  // was (a trailing line), then one that changes the tile's program (a constant).
  if (firstBake && project.materials.some(material => material.id === "tile")) {
    const current = () => page.evaluate(() => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0));
    const sourceEdit = async text => {
      const start = await current(), began = Date.now(), from = await pageNow();
      await page.locator("#code").fill(text);
      await page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, start, { timeout: 900000 });
      await ready();
      const status = await baked();
      return { baked: Date.now() - began, drawn: status.drawn, reused: status.reused, nodesDrawn: status.nodesDrawn, nodesReused: status.nodesReused,
        compileMs: status.compileMs, drawMs: status.drawMs, longTasks: await longTasks(from),
        // The form of the per-pixel programs for edits right after the rebuild: programs the scene before held settled stay `inline`.
        editPrograms: await page.evaluate(() => document.querySelector("#viewport").dataset.noise ?? "looped") };
    };
    let start = await current();
    await page.locator('#material-list li[data-material="tile"] .scene-choose').click();
    await page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, start, { timeout: 900000 });
    await ready(); await baked();
    const code = await page.locator("#code").inputValue();
    const sourceEdits = { sameProgram: await sourceEdit(`${code}\n`) };
    if (code.includes("roughness: 0.8")) sourceEdits.tileProgram = await sourceEdit(code.replace("roughness: 0.8", "roughness: 0.7"));
    console.log(`Source edits (ms; bakes and graph nodes drawn and reused): ${JSON.stringify(sourceEdits)}`);
    assert.equal(errors.length, 0, errors.join("\n"));
  }
  console.log(`Washroom benchmark passed at ${resolution} px: first preview ${firstPreview} ms, loaded ${loaded} ms${perPixel}${bakedMaps} (${project.materials.length} materials, ${Object.keys(expected).length} meshes).`);
} finally {
  await browser?.close();
  server.close();
}
