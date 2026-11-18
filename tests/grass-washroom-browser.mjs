/**
 * Painted grass on the washroom's lawn (roadmap stage 6, TreeSpec phase 4) in the built Material Studio: the washroom
 * (tests/studio/washroom) with its lawn and grass (`buildProject(..., { grass: true })`), drawn by the grass shader
 * families (browser/renderer/foliage.ts) in the painted and the lit profile over the same geometry and instances.
 * Write the lawn first:
 *   cargo test --release -p superspec-engine --test treespec_batches -- --ignored write_washroom_lawn
 *
 * Checks and reports, per profile: the cold compile of its programs (a profile the page has not drawn yet) apart from
 * the steady frame; the GPU time of a frame with and without the grass (timer queries) and of a frame in the
 * time-driven mode with wind; blades, instances, tiles and draw calls; that moving the sun only remakes the boundary
 * field (no re-scatter, no rebuild, no bake) and moves the blades the painted profile keeps. Saves pictures of both
 * profiles, seen as the Studio frames the scene and from close by, to OUT (default generated/verification/grass).
 * RESOLUTION (default 256) is the washroom's texture size; QUERY adds to the page's query ("gpuprobe&ao=off"); AO=1
 * keeps ambient occlusion on (its files end in -ao); the real GPU wants BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist".
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject, loadLawn } from "./studio/washroom/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve(process.env.OUT ?? "generated/verification/grass");
// AO=1 keeps the scene's ambient occlusion (off by default, as the painted checks count exact colors); its files say so.
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

const lawn = loadLawn();
const { project } = buildProject(Number(process.env.RESOLUTION ?? 256), undefined, { grass: lawn });
const grassBatches = project.batches.filter(batch => batch.foliage);
assert.ok(grassBatches.length >= 3, "the lawn's grass is delivered as foliage batches, a level each");
const text = JSON.stringify(project);
// The lawn's look keeps blades only near shade boundaries (a lawn written with LAWN_BOUNDARY=1), or keeps every blade
// (grass everywhere, the default): the boundary field and the painted profile's rejection are checked for the first.
const bounded = (grassBatches[0].foliage.rejection?.boundary ?? 0.22) > 0;

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
const report = { lawn: { candidates: lawn.stats.candidates, clumps: lawn.stats.instances, blades: lawn.stats.blades, tiles: lawn.tiles, batches: grassBatches.length } };
try {
  // Each run starts in one profile and switches to the other, so each profile's programs are compiled cold once. Each
  // has a browser of its own: the GPU process keeps compiled programs across the pages of one browser.
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
    await ready("washroom with grass");
    await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked", {}, { timeout: 900000 });
    await waitForFrames(page, 6);
    // The boundary field is made after the frames that made the shadow map and the ground fields.
    if (bounded) await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.foliage ?? "null")?.boundary?.ready === true, {}, { timeout: 60000 });
    const loadedMs = Date.now() - started;
    const result = report[first] ??= { loadedMs };
    const state = () => page.evaluate(() => {
      const scene = window.__scene, host = document.querySelector("#viewport");
      const { described, blades } = scene.foliageState();
      return { described, blades, input: host.dataset.inputRevision, draft: host.dataset.draftRevision, bake: JSON.parse(host.dataset.sceneBake ?? "null")?.drawn ?? null,
        sceneFields: JSON.parse(host.dataset.sceneFields ?? "null") };
    });
    const loaded = await state();
    assert.equal(loaded.sceneFields?.ground, project.materials.find(material => material.id === "painted-lawn").meshes[0], "the lawn is the ground the grass samples");
    // The Wasm build delivered every clump TreeSpec grew (native), each level the share its keep says.
    const delivered = loaded.described.patches[0].levels;
    assert.equal(delivered[0].instances, lawn.stats.instances, "level 0 holds every clump");
    assert.deepEqual(delivered.map(level => level.instances), [0, 1, 2].map(level => grassBatches.filter(batch => batch.foliage.level === level).reduce((sum, batch) => sum + batch.instances.length, 0)));
    const shot = async name => {
      await waitForFrames(page, 4);
      await writeFile(resolve(out, `washroom-grass-${name}${suffix}.png`), await page.locator("#viewport").screenshot());
    };
    /** One profile's steady numbers at the current view: frames with and without the grass, counts, and wind in the time-driven mode. */
    const profile = async (name, view) => {
      const now = await state();
      const steady = await measure(page);
      // The same frame without the grass: its meshes hidden (no tile changes level while the camera stands still).
      const without = await page.evaluate(() => {
        const meshes = window.__scene.models.children.filter(mesh => mesh.userData.foliage);
        const shown = meshes.map(mesh => mesh.visible);
        for (const mesh of meshes) mesh.visible = false;
        return shown;
      });
      const bare = await measure(page);
      await page.evaluate(shown => window.__scene.models.children.filter(mesh => mesh.userData.foliage).forEach((mesh, index) => { mesh.visible = shown[index]; }), without);
      // Wind: the time-driven mode advances the clock every frame; its readout has the GPU's time per frame.
      await page.evaluate(() => window.__scene.setContinuous(true));
      await page.waitForFunction(() => document.querySelector("#viewport").dataset.frameBudget !== undefined, {}, { timeout: 30000 });
      await page.waitForTimeout(2500);
      const windy = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.frameBudget));
      await page.evaluate(() => window.__scene.setContinuous(false));
      await shot(`${name}-${view}`);
      const levels = now.described.patches[0].levels;
      return {
        view, tiles: now.described.patches[0].tiles, tilesDrawn: now.described.patches[0].tilesDrawn,
        instancesDrawn: levels.reduce((sum, level) => sum + level.drawn, 0), levels: levels.map(level => ({ level: level.level, instances: level.drawn, blades: level.bladesDrawn, vertices: level.verticesDrawn, meshes: level.meshes })),
        bladesSubmitted: now.blades.candidates, bladesDrawn: now.blades.drawn, keptBy: now.blades.byReason,
        frame: steady.frame, gpuMs: steady.gpuMs, drawAndReadMs: steady.drawAndReadMs, withoutGrass: { frame: bare.frame, gpuMs: bare.gpuMs },
        wind: { gpuMs: windy.gpuMs, intervalMs: windy.intervalMs, cpuMs: windy.cpuMs, calls: windy.calls, triangles: windy.triangles, time: windy.time }, renderer: steady.renderer,
      };
    };
    const close = async () => {
      // Closer: toward the lawn in front of the room, as a person standing on it would see it.
      const box = await page.locator("#viewport canvas").boundingBox();
      await page.mouse.move(box.x + box.width * 0.62, box.y + box.height * 0.62);
      for (let i = 0; i < 5; i++) { await page.mouse.wheel(0, -240); await waitForFrames(page, 2); }
      await waitForFrames(page, 6);
    };
    result[first] = { framed: await profile(first, "framed") };
    console.log(`${first} framed: ${JSON.stringify(result[first].framed)}`);
    // Moving the sun (painted only: the lit profile keeps every blade): the boundary field is remade, nothing else.
    if (first === "painted" && bounded) {
      const sun = await page.evaluate(async () => {
        const scene = window.__scene, host = document.querySelector("#viewport");
        const meshes = scene.models.children.filter(mesh => mesh.userData.foliage);
        const versions = () => meshes.map(mesh => mesh.instanceMatrix.version).join(",");
        const record = JSON.parse(host.dataset.lighting);
        const [x, y] = record.lights[0].direction, azimuth = Math.atan2(y, x);
        const moves = [];
        const instancesBefore = versions();
        for (const elevation of [62, 47, 33, 54]) {
          const lift = elevation * Math.PI / 180;
          const lighting = structuredClone(record);
          lighting.lights[0].direction = [Math.cos(lift) * Math.cos(azimuth), Math.cos(lift) * Math.sin(azimuth), Math.sin(lift)];
          const made = scene.foliageState().described.boundary.boundaryMade;
          const started = performance.now();
          scene.setLighting(lighting);
          // The frame that makes the shadow map, after which the boundary is made, and the frame that shows it.
          scene.draw(); scene.draw();
          const ms = performance.now() - started;
          await new Promise(done => setTimeout(done, 60));
          const { described, blades } = scene.foliageState();
          moves.push({ elevation, ms: +ms.toFixed(2), boundaryMs: +described.boundary.boundaryMs.toFixed(2), boundaryGpuMs: described.boundary.gpuMs, remade: described.boundary.boundaryMade - made, bladesDrawn: blades.drawn, keptBy: blades.byReason });
        }
        scene.setLighting(record);
        scene.draw(); scene.draw();
        return { moves, instancesUnchanged: versions() === instancesBefore };
      });
      const after = await state();
      result.sunMoves = sun;
      console.log(`Sun moves: ${JSON.stringify(sun)}`);
      assert.ok(sun.instancesUnchanged, "moving the sun re-packed or re-scattered no instance");
      assert.deepEqual([after.input, after.draft, after.bake], [loaded.input, loaded.draft, loaded.bake], "moving the sun rebuilt or baked nothing");
      assert.ok(sun.moves.every(move => move.remade >= 1), `each sun move remade the boundary field: ${JSON.stringify(sun.moves)}`);
      assert.ok(new Set(sun.moves.map(move => move.bladesDrawn)).size > 1, `the blades drawn follow the boundary: ${JSON.stringify(sun.moves)}`);
      // A high sun lights every slope of the lawn and a low one shades most: a boundary crosses it at the middle suns.
      assert.ok(sun.moves.filter(move => move.bladesDrawn > 0).length >= 2, "middle suns leave blades at a boundary");
      assert.equal(after.described.patches[0].tiles, loaded.described.patches[0].tiles);
    }
    // The other profile over the same geometry and instances: its programs compiled cold, then the same numbers.
    const switched = await page.evaluate(async other => window.__scene.setFoliageProfile(other), other);
    await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.foliage ?? "null")?.boundary?.ready !== undefined);
    result[`${other}ColdCompileMs`] = +switched.compileMs.toFixed(1);
    result[other] = { framed: await profile(other, "framed") };
    await close();
    result[other].close = await profile(other, "close");
    await page.evaluate(async first => window.__scene.setFoliageProfile(first), first);
    result[first].close = await profile(first, "close");
    // Low over the lawn (painted): where the view grazes the ground the grazing term keeps blades away from boundaries.
    if (first === "painted" && bounded) {
      const box = await page.locator("#viewport canvas").boundingBox();
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      // Dragging up turns the view toward the horizon (about 0.4 radians).
      await page.mouse.down(); await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2 - 70, { steps: 10 }); await page.mouse.up();
      result.painted.low = await profile("painted", "low");
      assert.ok(result.painted.low.keptBy.grazing > 0, `the grazing term keeps blades where the view runs along the ground: ${JSON.stringify(result.painted.low.keptBy)}`);
    }
    // Both profiles draw the same instances; the painted one rejects the blades away from boundaries.
    for (const view of ["framed", "close"]) {
      const painted = result.painted?.[view], lit = result.lit?.[view];
      if (painted && lit) {
        assert.equal(painted.instancesDrawn, lit.instancesDrawn, `${view}: both profiles draw the same instances`);
        if (bounded) {
          assert.ok(painted.bladesDrawn < lit.bladesDrawn, `${view}: the painted profile rejects blades away from boundaries`);
          assert.ok(painted.bladesDrawn > 0, `${view}: the painted profile keeps blades at boundaries`);
        } else assert.equal(painted.bladesDrawn, lit.bladesDrawn, `${view}: grass everywhere: both profiles draw every blade`);
      }
    }
    assert.deepEqual(errors.filter(error => !/Source could not be compiled|Material evaluation|MS_/.test(error)), [], "no page errors");
    await page.close();
  }
  console.log(JSON.stringify(report, null, 1));
  await writeFile(resolve(out, `washroom-grass${suffix}.json`), JSON.stringify(report, null, 1) + "\n");
  const p = report.painted, l = report.lit;
  console.log(`Grass on the washroom lawn (${p.painted.framed.renderer}): ${report.lawn.blades.toLocaleString()} blades in ${report.lawn.clumps.toLocaleString()} clumps over ${report.lawn.tiles} tiles.`);
  for (const view of ["framed", "close"]) {
    for (const [name, values] of [["painted", p.painted[view]], ["lit", l.lit[view]]]) {
      console.log(`  ${name} ${view}: GPU ${values.gpuMs?.median} ms (without grass ${values.withoutGrass.gpuMs?.median} ms), wind ${values.wind.gpuMs} ms; `
        + `${values.instancesDrawn} instances in ${values.tilesDrawn}/${values.tiles} tiles, ${values.bladesDrawn}/${values.bladesSubmitted} blades drawn, ${values.frame.calls} draw calls, ${values.frame.triangles.toLocaleString()} triangles submitted`);
    }
  }
  console.log(`  cold compile: lit ${p.litColdCompileMs} ms, painted ${l.paintedColdCompileMs} ms; sun moves ${p.sunMoves ? JSON.stringify(p.sunMoves.moves.map(move => [move.ms, move.boundaryGpuMs])) + " (ms, boundary GPU ms)" : "(grass everywhere: no boundary field)"}`);
} finally {
  await browser?.close();
  server.close();
}
