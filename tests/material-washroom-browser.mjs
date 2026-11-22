/**
 * The MaterialSpec benchmark scene (tests/studio/washroom) in the built Material Studio: opens
 * the project through the selected Studio host, checks every material landed on its meshes
 * and reports how long the scene took to load. RESOLUTION (default 256) sets the texture size
 * (above 512 the project is given the production profile, the only one that allows it);
 * SCREENSHOT names a PNG of the loaded viewport. LIGHTING names a JSON lighting record the project
 * then carries (see docs/plans/lighting-settings.md). DAYLIGHT=1 adds the clerestory and a low-angle sun. BATCHES=0 builds the
 * scene without its instanced batch (the bolts), as it was before it had one; with QUERY containing gpuprobe the frame's
 * draw calls and triangles are reported, and with `animate=1` the time-driven mode's frame budget.
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
    if (name === "/favicon.ico") { response.writeHead(204).end(); return; }
    const path = resolve(root, name === "/" ? "index.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".png": "image/png" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

let browser;
try {
  const withBatches = process.env.BATCHES !== "0";
  // PAINTED=1 gives the slab the painted ground (roadmap stage 5) and checks that its shade boundary follows the sun.
  const painted = process.env.PAINTED === "1";
  const daylight = process.env.DAYLIGHT === "1";
  const { project } = buildProject(resolution, undefined, { batches: withBatches, painted, daylight });
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
  // The painted check reads the viewer's scene (`gpuprobe`) and counts exact paint colors, so ambient occlusion is off.
  const query = [process.env.QUERY, ...(painted ? ["gpuprobe", "ao=off"] : []),
    ...(process.env.CHECK_LIT_EDGES === "1" ? ["gpuprobe", "edgeboost=on"] : [])].filter(Boolean).join("&");
  if (painted) process.env.QUERY = query;
  const host = process.env.STUDIO_URL ?? ("http://127.0.0.1:" + server.address().port);
  await page.goto(`${host}/material.html${query ? `?${query}` : ""}`);
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
  if (!only) assert.equal(Object.keys(expected).length, materials.reduce((sum, material) => sum + material.meshes.length, 0) + Number(daylight) - Number(!withBatches));
  // The scene's instanced batch: one instanced mesh for every bolt, drawn from the template's one bake.
  const drawnBatches = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.batches ?? "[]"));
  if (withBatches && !only) {
    const stress = Number(new URLSearchParams(process.env.QUERY ?? "").get("batchstress")) || 0;
    assert.equal(drawnBatches.length, 1, "the bolts are one instanced batch");
    assert.equal(drawnBatches[0].id, "bolts");
    assert.equal(drawnBatches[0].instances, stress || project.batches[0].instances.length);
    assert.ok(drawnBatches[0].tints && drawnBatches[0].values, "per-instance tints and values are bound");
    if (firstBake && !/preview=/.test(process.env.QUERY ?? "")) {
      // A painted mesh is drawn by its program and gets no baked maps.
      assert.equal(firstBake.meshes, project.materials.reduce((sum, material) => sum + material.meshes.length, 0) - (painted ? 1 : 0), "the batch is one mesh of the scene bake, as its template");
      assert.ok(firstBake.atlases.bolts, "the batch shows maps baked over its template's atlas");
    }
    console.log(`Instanced batches: ${JSON.stringify(drawnBatches)}`);
  } else assert.equal(drawnBatches.length, 0);
  assert.equal(errors.length, 0, errors.join("\n"));
  // The view is lit by the project's record (the shipped look without one), under the page's overrides.
  {
    const lighting = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.lighting ?? "null"));
    assert.ok(lighting, "the viewport states the lighting record in effect");
    const record = project.lighting ?? {}, query = new URLSearchParams(process.env.QUERY ?? "");
    assert.equal(lighting.version, 1);
    assert.equal(lighting.exposure, record.exposure ?? 1);
    assert.equal(lighting.ambient.intensity, record.ambient?.intensity ?? 0.65);
    // A record's empty `lights` list is no direct light; a record without the list has the shipped sun.
    assert.equal(lighting.lights.length, record.lights?.length ?? 1);
    if (lighting.lights.length) assert.deepEqual(lighting.lights[0].direction.map(axis => +axis.toFixed(3)), (record.lights?.[0]?.direction ?? [-0.303, -0.505, 0.808]).map(axis => +axis.toFixed(3)));
    assert.equal(lighting.toneMapping, record.toneMapping ?? "aces");
    assert.equal(lighting.environment.source, record.environment?.source ?? "studio");
    assert.equal(lighting.environment.rotation, record.environment?.rotation ?? 0);
    if (query.get("ao") === "off") assert.equal(lighting.ambientOcclusion.enabled, false);
    if (query.get("shadowstyle") === "plain") assert.equal(lighting.lights[0].shadow.style, "plain");
  }
  // The painted slab (PAINTED=1): its program draws the authored paints untouched by the light loop and the tone curve,
  // and moving the sun of the lighting record moves its shade boundary with no rebuild and no bake.
  if (painted) {
    const srgb = linear => linear.map(value => Math.round(255 * (value <= 0.0031308 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055)));
    const paints = { lit: srgb([0.62, 0.55, 0.4]), shade: srgb([0.18, 0.2, 0.32]) };
    const result = await page.evaluate(async paints => {
      const scene = window.__scene, host = document.querySelector("#viewport");
      const slab = scene.models.children.find(mesh => mesh.userData.shaded === "painted-ground");
      if (!slab) return { error: "no painted slab" };
      const material = slab.material;
      const state = () => ({ input: host.dataset.inputRevision, draft: host.dataset.draftRevision, bake: JSON.parse(host.dataset.sceneBake ?? "null")?.drawn ?? null, status: document.querySelector("#status").dataset.state });
      const before = state();
      const gl = scene.renderer.getContext();
      const width = gl.drawingBufferWidth, height = gl.drawingBufferHeight, pixels = new Uint8Array(width * height * 4);
      const count = () => {
        scene.draw();
        gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        const near = (at, color) => Math.abs(pixels[at] - color[0]) <= 2 && Math.abs(pixels[at + 1] - color[1]) <= 2 && Math.abs(pixels[at + 2] - color[2]) <= 2;
        let lit = 0, shade = 0, between = 0;
        for (let at = 0; at < pixels.length; at += 4) {
          if (near(at, paints.lit)) lit++;
          else if (near(at, paints.shade)) shade++;
          // Between the paints: the band's gradient, on the line from one to the other.
          else if ([0, 1, 2].every(c => pixels[at + c] >= Math.min(paints.lit[c], paints.shade[c]) - 2 && pixels[at + c] <= Math.max(paints.lit[c], paints.shade[c]) + 2)) between++;
        }
        return { lit, shade, between };
      };
      const record = JSON.parse(host.dataset.lighting);
      const [x, y] = record.lights[0].direction, azimuth = Math.atan2(y, x);
      const sun = elevation => {
        const lift = elevation * Math.PI / 180;
        const lighting = structuredClone(record);
        lighting.lights[0].direction = [Math.cos(lift) * Math.cos(azimuth), Math.cos(lift) * Math.sin(azimuth), Math.sin(lift)];
        scene.setLighting(lighting);
        return count();
      };
      const started = performance.now();
      const counts = { 70: sun(70), 33: sun(33), 15: sun(15) };
      const moveMs = performance.now() - started;
      scene.setLighting(record);
      await new Promise(done => setTimeout(done, 500));
      return { painted: material.defines?.MS_PAINTED !== undefined, toneMapped: material.toneMapped, counts, moveMs, before, after: state() };
    }, paints);
    assert.ok(!result.error, result.error);
    console.log(`Painted slab: ${JSON.stringify(result)}`);
    assert.ok(result.painted && result.toneMapped === false, "the slab is drawn by its painted program without the tone curve");
    const { 70: high, 33: mid, 15: low } = result.counts;
    // High sun: the slab is lit but where the room casts its shadow; low sun: the whole slab is in shade; between, the
    // boundary crosses the floor. The paint is exact (no light loop, no tone curve), so the pixels are counted by color.
    assert.ok(high.lit > 10000 && high.lit > mid.lit && mid.lit > low.lit, JSON.stringify(result.counts));
    assert.ok(low.shade > high.shade && low.shade > 10000, JSON.stringify(result.counts));
    assert.ok(mid.lit > 1000 && mid.shade > 1000, `a boundary crosses the slab at a middle sun: ${JSON.stringify(mid)}`);
    assert.deepEqual(result.after, result.before, "moving the sun rebuilt or baked nothing");
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
    const rejected = await control.evaluate(field => {
      const host = document.querySelector("#viewport"), before = host.dataset.inputRevision, initial = field.value;
      for (const value of ["", String(Number(field.max || 1000) + 1)]) {
        field.value = value;
        field.dispatchEvent(new Event("input", { bubbles: true }));
        field.dispatchEvent(new Event("change", { bubbles: true }));
      }
      field.value = initial;
      return host.dataset.inputRevision === before;
    });
    assert.ok(rejected, "blank and out-of-range material values leave the accepted revision visible");
    start = await current();
    if (Number(process.env.BENCH_DRAG_FRAMES) > 1) await control.evaluate((field, material) => {
      const scene = window.__scene, view = document.querySelector("#viewport");
      if (!scene) throw Error("BENCH_DRAG_FRAMES requires QUERY=gpuprobe.");
      const receiver = scene.models.children.find(mesh => mesh.userData.shaded === material);
      const geometry = receiver.geometry, draw = scene.draw, before = view.dataset.inputRevision;
      const reading = window.__materialGesture = {};
      field.addEventListener("input", () => { reading.start = performance.now(); }, { once: true, capture: true });
      scene.draw = function (...args) {
        draw.apply(this, args);
        if (reading.start === undefined) return;
        const bake = JSON.parse(view.dataset.sceneBake ?? "null");
        const changed = receiver.geometry !== geometry || (view.dataset.inputRevision !== before && bake?.state === "baked" && bake.drawn > 0);
        if (changed) reading.first ??= performance.now() - reading.start;
        if (changed && bake?.state === "baked" && view.dataset.inputRevision !== before) {
          reading.finished = performance.now() - reading.start;
          scene.draw = draw;
        }
      };
      reading.restore = () => { scene.draw = draw; };
    }, material);
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
    // Drive the same numeric control as a continuous gesture. Count only renderer draws showing
    // the new uniforms (or the finished maps); input events and generic animation ticks aren't draws.
    const frames = Number(process.env.BENCH_DRAG_FRAMES ?? 0);
    if (frames > 1) {
      edits[material].singleFrame = await page.evaluate(() => {
        const reading = window.__materialGesture;
        reading.restore();
        return { firstDrawMs: reading.first, finishedDrawMs: reading.finished };
      });
      // A freshly opened scene can still be compiling its per-pixel shaders after its maps
      // appear. The discrete edit above includes that case. Begin the warm drag once ready.
      await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake).shaderReady,
        undefined, { timeout: 120000 });
      edits[material].gesture = await page.evaluate(async ({ material, label, frames }) => {
        const scene = window.__scene, host = document.querySelector("#viewport");
        if (!scene) throw Error("BENCH_DRAG_FRAMES requires QUERY=gpuprobe.");
        const mesh = scene.models.children.find(object => object.userData.shaded === material);
        if (!mesh) throw Error("Missing material receiver " + material);
        const field = [...document.querySelectorAll('input[type="number"]')].find(input => input.getAttribute("aria-label") === label);
        const draw = scene.draw, initialGeometry = mesh.geometry, before = Number(host.dataset.inputRevision);
        const inputs = new Map(), draws = new Map();
        const frame = () => new Promise(resolve => requestAnimationFrame(resolve));
        const bake = () => JSON.parse(host.dataset.sceneBake ?? "null");
        scene.draw = function (...args) {
          draw.apply(this, args);
          const rev = Number(host.dataset.inputRevision);
          if (inputs.has(rev) && !draws.has(rev) && (mesh.geometry !== initialGeometry || bake()?.state === "baked")) {
            draws.set(rev, performance.now());
          }
        };
        try {
          for (let i = 0; i < frames; i++) {
            if (i) await frame();
            inputs.set(before + i + 1, performance.now());
            field.value = (0.11 + (i % 70) * 0.01).toFixed(2);
            field.dispatchEvent(new Event("input", { bubbles: true }));
          }
          field.dispatchEvent(new Event("change", { bubbles: true }));
          const last = inputs.get(before + frames);
          do {
            await frame();
            if (performance.now() - last > 30000) throw Error("Material drag failed to settle.");
          } while (Number(host.dataset.inputRevision) <= before || bake()?.state !== "baked");
          await frame();
          const times = [...draws.values()];
          const stats = values => {
            values.sort((a, b) => a - b);
            const pct = p => values.length ? +values[Math.ceil(p * values.length) - 1].toFixed(1) : null;
            return { p50: pct(.5), p95: pct(.95), max: pct(1) };
          };
          return { inputs: frames, drawn: times.length, acceptedRevisions: Number(host.dataset.inputRevision) - before,
            inputToDrawMs: stats([...draws].map(([rev, time]) => time - inputs.get(rev))),
            intervalMs: stats(times.slice(1).map((time, i) => time - times[i])), fps: times.length > 1 ? +(1000 * (times.length - 1) / (times.at(-1) - times[0])).toFixed(1) : null,
            settledAfterLastInputMs: Math.round(performance.now() - last) };
        } finally { scene.draw = draw; }
      }, { material, label, frames });
    }
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
    return { median: +times[12].toFixed(2), min: +times[0].toFixed(2), max: +times[23].toFixed(2), gpuTimerQuery: gpu, shadowExtentMeters: scene.shadowStyle?.uniforms.msShadowExtent.value,
      // Draw calls and triangles of one frame, every pass counted (shadow map, scene, ambient occlusion).
      frame: scene.measureFrame() };
  });
  if (draws) console.log(`Draw time (ms; draw and one-pixel read, 24 draws; gpuTimerQuery is the GPU's time alone): ${JSON.stringify(draws)}`);
  // The time-driven mode (`?animate=1`): frames keep coming and its readout states them against the budget.
  if (/animate=1/.test(process.env.QUERY ?? "")) {
    await page.waitForFunction(() => document.querySelector("#viewport").dataset.frameBudget !== undefined, {}, { timeout: 30000 });
    await page.waitForTimeout(1500);
    console.log(`Frame budget (time-driven mode): ${await page.evaluate(() => document.querySelector("#viewport").dataset.frameBudget)}`);
  }
  if (process.env.SCREENSHOT) await writeFile(process.env.SCREENSHOT, await page.locator("#viewport").screenshot());
  if (process.env.CHECK_LIT_EDGES === "1") {
    const difference = await page.evaluate(() => {
      const s = window.__scene, effect = s.lights.ambientOcclusion.litEdges;
      const gl = s.renderer.getContext();
      function capture(enabled) {
        effect.enabled = enabled; s.draw();
        const bytes = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4);
        gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, bytes);
        return bytes;
      }
      const off = capture(false), on = capture(true);
      // Use the same sun depth map but place every edge's visibility probe behind
      // the washroom at the center of that map. All ordinary lighting stays live.
      const matrix = effect.uniforms.toShadow.value;
      const project = matrix.multiplyMatrices;
      let covered;
      try {
        matrix.multiplyMatrices = () => matrix.set(0,0,0,.5, 0,0,0,.5, 0,0,0,.99999, 0,0,0,1);
        covered = capture(true);
      } finally { matrix.multiplyMatrices = project; }
      let brighter = 0, darker = 0, peak = 0;
      for (let i = 0; i < on.length; i += 4) {
        const r = on[i] - off[i], g = on[i + 1] - off[i + 1], b = on[i + 2] - off[i + 2];
        if (Math.max(r, g, b) > 0) brighter++;
        if (Math.min(r, g, b) < 0) darker++;
        peak = Math.max(peak, r, g, b);
      }
      return { brighter, darker, peak, pixels: on.length / 4, directSun: effect.uniforms.usable.value,
        shadowLeak: covered.some((value, i) => value !== off[i]) };
    });
    assert(difference.directSun && difference.brighter > 0 && difference.brighter < 0.03 * difference.pixels, JSON.stringify(difference));
    assert.equal(difference.darker, 0, JSON.stringify(difference));
    assert(!difference.shadowLeak && difference.peak <= 6, JSON.stringify(difference));
    console.log("Washroom geometric edge highlights:", difference);
  }
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
