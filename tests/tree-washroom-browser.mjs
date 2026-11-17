/**
 * Shrubs and trees on the washroom's lawn (roadmap stage 7, TreeSpec phase 5) in the built Material Studio: the washroom
 * (tests/studio/washroom) with its lawn, grass and plants (`buildProject(..., { trees: true })`), the plants drawn by the
 * plant shader families (browser/renderer/foliage-plants.ts) in the painted and the lit profile over the same geometry
 * and instances. Write the lawn and the plants first:
 *   cargo test --release -p superspec-engine --test treespec_batches -- --ignored write_washroom_lawn write_washroom_trees
 *
 * Checks and reports, per profile: the cold compile of its programs apart from the steady frame; the GPU time of a frame
 * with and without the plants (timer queries) at the Studio's framing, close to the tree and far away (where both plants
 * are impostors), and of a frame in the time-driven mode with wind; draw calls, instances and triangles per family; that
 * the Wasm build delivered every batch TreeSpec wrote (native) with its instances; that moving the sun rewrites no
 * instance data and rebuilds or bakes nothing while the painted canopy follows it. Saves pictures to OUT (default
 * generated/verification/trees). RESOLUTION (default 256) is the washroom's texture size; AO=1 keeps ambient occlusion
 * on (files end in -ao); the real GPU wants BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist".
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject, loadTrees } from "./studio/washroom/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve(process.env.OUT ?? "generated/verification/trees");
const occlusion = process.env.AO === "1", suffix = occlusion ? "-ao" : "";
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

const plants = loadTrees();
const { project } = buildProject(Number(process.env.RESOLUTION ?? 256), undefined, { trees: plants });
const plantBatches = project.batches.filter(batch => batch.foliage && batch.foliage.family !== "grass");
assert.ok(plantBatches.length >= 8, "the plants are delivered as foliage batches, a part and level each");
const text = JSON.stringify(project);
/** What the native document holds, per plant, level and family: instances. */
const native = {};
for (const batch of plantBatches) {
  const { patch, level, family } = batch.foliage;
  ((native[patch] ??= {})[family === "shadow" ? "shadow" : level] ??= {})[family] = batch.instances.length;
}
const tree = plants.plants.find(plant => plant.id === "broadleaf"), shrub = plants.plants.find(plant => plant.id === "shrub");
const views = {
  // Close to the tree, with the shrub behind it: level 0 of both.
  close: { position: [tree.base[0] + 3.0, tree.base[1] - 3.3, 1.9], target: [tree.base[0] - 0.2, tree.base[1] + 0.3, 2.0] },
  // Far: both plants are impostors.
  far: { position: [tree.base[0] + 19, tree.base[1] - 19, 8], target: [(tree.base[0] + shrub.base[0]) / 2, (tree.base[1] + shrub.base[1]) / 2, 1.2] },
};

/** In the page: the GPU's time for `frames` draws (timer queries), and the time to draw and read one pixel. */
async function measure(page, frames = 32) {
  return page.evaluate(async frames => {
    const scene = window.__scene, gl = scene.renderer.getContext(), pixel = new Uint8Array(4);
    const timer = gl.getExtension("EXT_disjoint_timer_query_webgl2");
    const gpu = [], draw = [];
    for (let i = 0; i < frames + 6; i++) {
      const query = timer && gl.createQuery();
      if (query) gl.beginQuery(timer.TIME_ELAPSED_EXT, query);
      const start = performance.now();
      scene.draw();
      if (query) gl.endQuery(timer.TIME_ELAPSED_EXT);
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
      if (i >= 6) draw.push(performance.now() - start);
      if (query) {
        for (let waited = 0; waited < 500 && !gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE); waited++) await new Promise(done => setTimeout(done, 2));
        if (i >= 6 && gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE) && !gl.getParameter(timer.GPU_DISJOINT_EXT)) gpu.push(gl.getQueryParameter(query, gl.QUERY_RESULT) / 1e6);
        gl.deleteQuery(query);
      }
    }
    const summary = values => { values.sort((a, b) => a - b); return values.length ? { median: +values[values.length >> 1].toFixed(3), min: +values[0].toFixed(3), max: +values.at(-1).toFixed(3) } : null; };
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    return { gpuMs: summary(gpu), drawAndReadMs: summary(draw), frame: scene.measureFrame(), renderer: info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : "unknown" };
  }, frames);
}

let browser;
const report = { plants: plants.plants, batches: plantBatches.length };
try {
  for (const first of ["painted", "lit"]) {
    await browser?.close();
    browser = await launchBrowser();
    const other = first === "painted" ? "lit" : "painted";
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    const ready = async message => {
      await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 900000 });
      assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", `${message}: ${await page.locator("#status").textContent()}`);
    };
    const query = ["gpuprobe", ...(occlusion ? [] : ["ao=off"]), ...(first === "lit" ? ["foliage=lit"] : []), process.env.QUERY].filter(Boolean).join("&");
    await page.goto(`http://127.0.0.1:${server.address().port}/material.html?${query}`);
    await ready("initial");
    const before = Number(await page.locator("#viewport").getAttribute("data-input-revision") ?? 0);
    const started = Date.now();
    await page.locator("#open-project").setInputFiles({ name: "washroom.studio.json", mimeType: "application/json", buffer: Buffer.from(text) });
    await page.waitForFunction(start => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > start, before, { timeout: 900000 });
    await ready("washroom with plants");
    await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked", {}, { timeout: 900000 });
    await waitForFrames(page, 6);
    const loadedMs = Date.now() - started;
    const result = report[first] ??= { loadedMs };
    const state = () => page.evaluate(() => {
      const scene = window.__scene, host = document.querySelector("#viewport");
      return { described: scene.foliageState().described, input: host.dataset.inputRevision, draft: host.dataset.draftRevision, bake: JSON.parse(host.dataset.sceneBake ?? "null")?.drawn ?? null };
    });
    const loaded = await state();
    // The Wasm build delivered what TreeSpec wrote natively: every plant, level and family with its instances.
    const delivered = {};
    for (const patch of loaded.described.patches.filter(patch => patch.kind === "plant")) {
      for (const level of patch.levels) for (const [family, counts] of Object.entries(level.families)) {
        ((delivered[patch.id] ??= {})[family === "shadow" ? "shadow" : level.level] ??= {})[family] = counts.instances;
      }
    }
    assert.deepEqual(delivered, native, "the delivered plant batches hold the native document's instances");
    const plantMeshes = "window.__scene.models.children.filter(mesh => mesh.userData.foliage && mesh.userData.foliage.family !== 'grass')";
    const shot = async name => {
      await waitForFrames(page, 4);
      await writeFile(resolve(out, `washroom-trees-${name}${suffix}.png`), await page.locator("#viewport").screenshot());
    };
    const setView = async view => page.evaluate(view => {
      const scene = window.__scene;
      scene.camera.position.fromArray(view.position);
      scene.controls.target.fromArray(view.target);
      scene.controls.update();
      scene.draw();
    }, view);
    /** One profile's steady numbers at the current view: frames with and without the plants, counts and wind. */
    const profile = async (name, view) => {
      await waitForFrames(page, 4);
      const steady = await measure(page);
      const now = await state();
      const hidden = await page.evaluate(`(() => { const meshes = ${plantMeshes}; const shown = meshes.map(mesh => mesh.visible); for (const mesh of meshes) mesh.visible = false; return shown; })()`);
      const bare = await measure(page);
      await page.evaluate(`((shown) => ${plantMeshes}.forEach((mesh, index) => { mesh.visible = shown[index]; }))(${JSON.stringify(hidden)})`);
      await page.evaluate(() => window.__scene.setContinuous(true));
      await page.waitForFunction(() => document.querySelector("#viewport").dataset.frameBudget !== undefined, {}, { timeout: 30000 });
      await page.waitForTimeout(2500);
      const windy = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.frameBudget));
      await page.evaluate(() => window.__scene.setContinuous(false));
      await shot(`${name}-${view}`);
      const plantsNow = now.described.patches.filter(patch => patch.kind === "plant").map(patch => ({
        id: patch.id, levels: patch.levels.filter(level => level.drawn).map(level => ({ level: level.level, families: Object.fromEntries(Object.entries(level.families).filter(([, f]) => f.drawn)) })),
      }));
      return {
        view, plants: plantsNow, frame: steady.frame, gpuMs: steady.gpuMs, drawAndReadMs: steady.drawAndReadMs, withoutPlants: { frame: bare.frame, gpuMs: bare.gpuMs },
        wind: { gpuMs: windy.gpuMs, intervalMs: windy.intervalMs, cpuMs: windy.cpuMs, calls: windy.calls, triangles: windy.triangles }, renderer: steady.renderer,
      };
    };
    const framed = await page.evaluate(() => ({ position: window.__scene.camera.position.toArray(), target: window.__scene.controls.target.toArray() }));
    result[first] = { framed: await profile(first, "framed") };
    console.log(`${first} framed: ${JSON.stringify(result[first].framed)}`);
    if (first === "painted") {
      // Moving the sun: no instance is rewritten, nothing rebuilt or baked, and the painted canopy follows the sun.
      await setView(views.close);
      const record = await page.evaluate(plantMeshes => {
        window.__plantVersions = () => eval(plantMeshes).map(mesh => `${mesh.instanceMatrix.version}:${Object.values(mesh.geometry.attributes).map(a => a.version).join("/")}`).join(",");
        window.__plantBefore = window.__plantVersions();
        return JSON.parse(document.querySelector("#viewport").dataset.lighting);
      }, plantMeshes);
      const [x, y] = record.lights[0].direction, azimuth = Math.atan2(y, x);
      const moves = [], pictures = [];
      for (const elevation of [62, 47, 33, 20]) {
        const lift = elevation * Math.PI / 180;
        const lighting = structuredClone(record);
        lighting.lights[0].direction = [Math.cos(lift) * Math.cos(azimuth), Math.cos(lift) * Math.sin(azimuth), Math.sin(lift)];
        const ms = await page.evaluate(lighting => {
          const scene = window.__scene, gl = scene.renderer.getContext();
          const started = performance.now();
          // The frame that makes the shadow map (with the proxies) and the frame after it.
          scene.setLighting(lighting);
          scene.draw(); scene.draw();
          gl.finish();
          return performance.now() - started;
        }, lighting);
        const picture = await page.locator("#viewport").screenshot();
        await writeFile(resolve(out, `washroom-trees-painted-sun-${elevation}${suffix}.png`), picture);
        pictures.push(picture.toString("base64"));
        moves.push({ elevation, ms: +ms.toFixed(2) });
      }
      await page.evaluate(record => { window.__scene.setLighting(record); window.__scene.draw(); window.__scene.draw(); }, record);
      const sun = { moves, instancesUnchanged: await page.evaluate(() => window.__plantVersions() === window.__plantBefore), distinctPictures: new Set(pictures).size };
      const after = await state();
      result.sunMoves = sun;
      console.log(`Sun moves: ${JSON.stringify(sun)}`);
      assert.ok(sun.instancesUnchanged, "moving the sun rewrote no plant instance or attribute");
      assert.deepEqual([after.input, after.draft, after.bake], [loaded.input, loaded.draft, loaded.bake], "moving the sun rebuilt or baked nothing");
      assert.equal(sun.distinctPictures, moves.length, "the painted plants follow the sun");
      await setView(framed);
    }
    const switched = await page.evaluate(async other => window.__scene.setFoliageProfile(other), other);
    result[`${other}ColdCompileMs`] = +switched.compileMs.toFixed(1);
    result[other] = { framed: await profile(other, "framed") };
    for (const view of ["close", "far"]) {
      await setView(views[view]);
      result[other][view] = await profile(other, view);
    }
    await page.evaluate(async first => window.__scene.setFoliageProfile(first), first);
    for (const view of ["close", "far"]) {
      await setView(views[view]);
      result[first][view] = await profile(first, view);
    }
    // Levels follow distance: level 0 close, the impostors (no leaves) far.
    for (const name of [first, other]) {
      const close = result[name].close.plants, far = result[name].far.plants;
      assert.ok(close.every(plant => plant.levels.some(level => level.level === 0 && level.families.leaves)), `${name}: leaves close: ${JSON.stringify(close)}`);
      assert.ok(far.every(plant => plant.levels.every(level => !level.families.leaves) && plant.levels.some(level => level.families.hull)), `${name}: impostors far: ${JSON.stringify(far)}`);
    }
    assert.deepEqual(errors.filter(error => !/Source could not be compiled|Material evaluation|MS_/.test(error)), [], "no page errors");
    await page.close();
  }
  console.log(JSON.stringify(report, null, 1));
  await writeFile(resolve(out, `washroom-trees${suffix}.json`), JSON.stringify(report, null, 1) + "\n");
  const p = report.painted, l = report.lit;
  console.log(`Plants on the washroom lawn (${p.painted.framed.renderer}):`);
  for (const view of ["framed", "close", "far"]) {
    for (const [name, values] of [["painted", p.painted[view]], ["lit", l.lit[view]]]) {
      console.log(`  ${name} ${view}: GPU ${values.gpuMs?.median} ms (without plants ${values.withoutPlants.gpuMs?.median} ms), wind ${values.wind.gpuMs} ms; `
        + `${values.frame.calls} draw calls (without ${values.withoutPlants.frame.calls}), ${values.frame.triangles.toLocaleString()} triangles; ${JSON.stringify(values.plants)}`);
    }
  }
  console.log(`  cold compile: lit ${p.litColdCompileMs} ms, painted ${l.paintedColdCompileMs} ms; sun moves ${JSON.stringify(p.sunMoves.moves.map(move => move.ms))} ms`);
} finally {
  await browser?.close();
  server.close();
}
