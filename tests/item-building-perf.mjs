/**
 * Measures the procedural building (the scene of tests/item-facade-browser.mjs: `kowloon-building` across the lane) in
 * the built Material Studio and writes OUT/perf.json (default generated/verification/item-building-perf) with two
 * pictures to compare runs by. It separates the stages a change may move:
 *
 * - cold: open the project in a fresh page (the engine prepares the items, lays out the atlases; the page compiles the
 *   bake programs and draws every receiver's maps);
 * - warm: open the same project again in the same page (programs compiled, maps answered from the pass graph);
 * - edit: one item parameter changed (EDIT, JSON merged over the building's parameters; default the seed), which
 *   rebuilds the item and bakes what changed.
 *
 * For each: milliseconds to the first look and to the last map, the bake's own sums (compile, draw, CPU fields), nodes
 * drawn and reused, the memory of the maps on screen and of everything the pass graph holds, by kind of texture.
 * Then draw calls, triangles and the frame's CPU and GPU time from two cameras.
 *
 * MODEL, LANE, RESOLUTION, TEXELS, BUILDING_PARAMETERS, BAKE_MEMORY and BROWSER_ARGS are as in item-facade-browser.mjs.
 * QUERY adds page parameters (for example QUERY="bakekeep=all").
 */
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom-items/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve(process.env.OUT ?? "generated/verification/item-building-perf");
await mkdir(out, { recursive: true });
const choice = process.env.MODEL ?? "building";
const lane = process.env.LANE === "1" ? JSON.parse(await readFile(resolve("generated/verification/treespec/lane-plants.json"), "utf8")) : null;
const { project } = buildProject(Number(process.env.RESOLUTION ?? 1024), Number(process.env.TEXELS ?? 160), { facade: choice === "side" ? true : choice, lane, room: process.env.ROOM === "1", grass: process.env.ROOM === "1" });
const facade = project.models.find(model => model.id === (choice === "side" ? "facade" : "building"));
const overrides = process.env.BUILDING_PARAMETERS?.endsWith(".json") ? await readFile(resolve(process.env.BUILDING_PARAMETERS), "utf8") : process.env.BUILDING_PARAMETERS;
Object.assign(facade.parameters, Object.fromEntries(Object.entries(JSON.parse(overrides ?? "{}")).filter(([key]) => !key.startsWith("/"))));
if (project.resolution > 512) project.profile = "production";
const text = JSON.stringify(project);
const edited = JSON.parse(text);
Object.assign(edited.models.find(model => model.id === facade.id).parameters, JSON.parse(process.env.EDIT ?? JSON.stringify({ seed: (facade.parameters.seed ?? 0) + 1 })));

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

const MiB = bytes => +(bytes / 1048576).toFixed(1);
const browser = await launchBrowser();
const report = { project: { bytes: text.length, resolution: project.resolution, texels: project.texelDensity, models: project.models.length, materials: project.materials.length }, runs: {} };
try {
  const page = await browser.newPage({ viewport: { width: 1400, height: 1000 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  const loading = Date.now();
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?gpuprobe&ao=off&bakememory=${process.env.BAKE_MEMORY ?? 6144}${process.env.QUERY ? `&${process.env.QUERY}` : ""}`);
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 600000 });
  report.studioReadyMs = Date.now() - loading;

  /** Opens a project and waits for its last map; what the page says of the bake and of the pass graph's textures. */
  const open = async (name, body) => {
    const started = Date.now();
    // The viewport states the revision it shows once the scene is committed (the maps may follow).
    const before = await page.evaluate(() => document.querySelector("#viewport").dataset.inputRevision ?? "");
    await page.locator("#open-project").setInputFiles({ name: "washroom-facade.studio.json", mimeType: "application/json", buffer: Buffer.from(body) });
    await page.waitForFunction(before => (document.querySelector("#viewport").dataset.inputRevision ?? "") !== before || document.querySelector("#status")?.dataset.state === "error", before, { timeout: 900000, polling: 100 });
    const shown = Date.now() - started;
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state)
      && (document.querySelector("#status")?.dataset.state === "error" || JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked"), {}, { timeout: Number(process.env.BAKE_TIMEOUT ?? 900000), polling: 100 });
    const baked = Date.now() - started;
    const state = await page.evaluate(() => {
      const viewport = document.querySelector("#viewport"), status = document.querySelector("#status");
      const bake = JSON.parse(viewport.dataset.sceneBake ?? "null");
      // Every texture the pass graph holds, by what it is (its name without the channel) and whether a mesh uses it.
      const kinds = {};
      const graph = window.__scene.bakerInstance?.graph;
      for (const node of graph?.complete?.values() ?? []) {
        const kind = `${node.texture.name.replace(/ \(rows\)$/, "")}${node.users ? "" : " (idle)"}`;
        const entry = kinds[kind] ??= { nodes: 0, bytes: 0 };
        entry.nodes++; entry.bytes += node.bytes;
      }
      // Atlases by width (a receiver takes the smallest halving of the resolution that holds it at the texel density).
      const sizes = {};
      for (const size of Object.values(bake?.atlases ?? {})) { const width = size.split("x")[0]; sizes[width] = (sizes[width] ?? 0) + 1; }
      // The bakes drawn, by material: how many, and the milliseconds the page spent issuing each pass.
      const materials = {};
      for (const line of bake?.details ?? []) {
        const [, material, c, e, n, m, d] = /^[^:]*:(\S+) c(\S+) e(\S+) n(\S+) m(\S+) d(\d+)/.exec(line) ?? [];
        if (!material) continue;
        const entry = materials[material] ??= { bakes: 0, nodes: 0, compileMs: 0, evaluateMs: 0, normalsMs: 0, mipsMs: 0 };
        entry.bakes++; entry.nodes += Number(d); entry.compileMs += Number(c); entry.evaluateMs += Number(e); entry.normalsMs += Number(n); entry.mipsMs += Number(m);
      }
      for (const entry of Object.values(materials)) for (const key of ["evaluateMs", "normalsMs", "mipsMs"]) entry[key] = Math.round(entry[key]);
      // The textures of each material's bakes and how many live argument channels vary over its surface.
      const layouts = {};
      for (const entry of window.__scene.sceneBaker?.current?.entries ?? []) {
        const layout = layouts[entry.target.material] ??= { meshes: 0, texels: 0, attachments: entry.program.attachments.join("+"), liveChannels: entry.program.liveChannels.length, layers: entry.target.stack?.layers.length ?? 0 };
        layout.meshes++; layout.texels += entry.target.layout.width * entry.target.layout.height;
      }
      return { status: status?.textContent, state: status?.dataset.state, firstLook: viewport.dataset.firstLook, firstLookMs: Number(viewport.dataset.firstLookMs),
        bake: bake && { ...bake, details: undefined, atlases: undefined, unsupported: Object.keys(bake.unsupported).length, reasons: [...new Set(Object.values(bake.unsupported))] },
        // The programs the GPU was timed on between bands, and the texels a millisecond it showed for the slowest and fastest.
        bands: (rates => ({ programs: rates.length, slowest: Math.round(Math.min(...rates)), fastest: Math.round(Math.max(...rates)) }))([...(window.__scene.bakerInstance?.bandRates?.values() ?? [])]),
        open: JSON.parse(viewport.dataset.sceneOpen ?? "null"), fields: JSON.parse(viewport.dataset.sceneFieldsBake ?? "null"),
        kinds, sizes, materials, layouts, texels: Object.values(bake?.atlases ?? {}).reduce((sum, size) => sum + size.split("x").reduce((a, b) => a * b, 1), 0) };
    });
    if (state.state !== "ready") throw new Error(`Not ready: ${JSON.stringify(state)}\n${errors.slice(0, 8).join("\n")}`);
    const kinds = Object.fromEntries(Object.entries(state.kinds).sort((a, b) => b[1].bytes - a[1].bytes).map(([kind, entry]) => [kind, { nodes: entry.nodes, MiB: MiB(entry.bytes) }]));
    report.runs[name] = { sceneShownMs: shown, bakedMs: baked, firstLook: state.firstLook, firstLookMs: state.firstLookMs, open: state.open, fieldsBake: state.fields, bands: state.bands,
      bake: { ...state.bake, MiB: MiB(state.bake.bytes), graphMiB: MiB(state.bake.graph.bytes), idleMiB: MiB(state.bake.graph.idleBytes) }, atlasTexels: state.texels, atlasWidths: state.sizes, materials: state.materials, layouts: state.layouts, textures: kinds };
    const run = report.runs[name];
    if (state.open) console.log(`${name}: opening ${JSON.stringify(state.open)}; fields ${JSON.stringify(state.fields)}; bands ${JSON.stringify(state.bands)}`);
    console.log(`${name}: scene ${shown} ms, baked ${baked} ms (bake run ${run.bake.ms} ms: compile ${run.bake.compileMs}, draw ${run.bake.drawMs}, fields ${run.bake.fieldMs}); `
      + `${run.bake.baked}/${run.bake.meshes} meshes, ${run.bake.unsupported} unbaked; nodes drawn ${run.bake.nodesDrawn}, reused ${run.bake.nodesReused}; on screen ${run.bake.MiB} MiB, graph ${run.bake.graphMiB} MiB (${run.bake.graph.nodes} textures)`);
  };

  // PROFILE=1 samples the page's thread while the cold scene is opened and baked (OUT/cold.cpuprofile, for the
  // browser's performance panel) and lists the functions it spent the most time in. Workers are not sampled.
  const profiler = process.env.PROFILE === "1" ? await page.context().newCDPSession(page) : null;
  if (profiler) { await profiler.send("Profiler.enable"); await profiler.send("Profiler.setSamplingInterval", { interval: 500 }); await profiler.send("Profiler.start"); }
  await open("cold", text);
  if (profiler) {
    const { profile } = await profiler.send("Profiler.stop");
    await writeFile(resolve(out, "cold.cpuprofile"), JSON.stringify(profile));
    const self = new Map(), byId = new Map(profile.nodes.map(node => [node.id, node]));
    const step = profile.timeDeltas.reduce((sum, delta) => sum + delta, 0) / profile.samples.length / 1000;
    for (const id of profile.samples) {
      const frame = byId.get(id).callFrame, name = `${frame.functionName || "(anonymous)"} ${frame.url.split("/").pop()}:${frame.lineNumber + 1}:${frame.columnNumber + 1}`;
      self.set(name, (self.get(name) ?? 0) + step);
    }
    report.profile = [...self].sort((a, b) => b[1] - a[1]).slice(0, 40).map(([name, ms]) => `${Math.round(ms)} ms ${name}`);
    console.log(`page thread, self time:\n  ${report.profile.slice(0, 30).join("\n  ")}`);
  }
  await waitForFrames(page, 6);

  // The reference picture's sun and two of its cameras (item-facade-browser.mjs), for comparing runs.
  await page.evaluate(() => {
    const s = window.__scene, record = JSON.parse(document.querySelector("#viewport").dataset.lighting), light = record.lights[0];
    const [az, el] = [-25, 55].map(v => v * Math.PI / 180);
    light.direction = [Math.cos(el) * Math.cos(az), Math.cos(el) * Math.sin(az), Math.sin(el)];
    s.setLighting(record); s.renderer.shadowMap.needsUpdate = true; s.invalidate?.();
  });
  await waitForFrames(page, 6);
  const info = await page.evaluate(id => {
    const s = window.__scene, T = window.__three, box = new T.Box3();
    let meshes = 0, vertices = 0, all = 0;
    for (const m of s.models.children) { all++; if (m.name.startsWith(`${id}/`)) { box.expandByObject(m); meshes++; vertices += m.geometry.getAttribute("position").count; } }
    return { meshes, vertices, sceneObjects: all, min: box.min.toArray(), max: box.max.toArray() };
  }, facade.id);
  report.building = { meshes: info.meshes, vertices: info.vertices, sceneObjects: info.sceneObjects };
  const [x0, y0] = info.min, [x1, y1] = info.max;
  const cy = (y0 + y1) / 2, front = x1;
  const azimuth = 22 * Math.PI / 180, elevation = 8 * Math.PI / 180, distance = 62;
  const views = {
    reference: { position: [(x0 + front) / 2 + distance * Math.cos(elevation) * Math.cos(azimuth), cy + distance * Math.cos(elevation) * Math.sin(azimuth), 7.4 + distance * Math.sin(elevation)], target: [(x0 + front) / 2, cy, 7.4], fov: 26 },
    close: { position: [front + 4, y1 + 1.5, 6], target: [front, y1 - 2, 5.5] },
  };
  report.frames = {};
  const look = async view => {
    await page.evaluate(view => { const s = window.__scene; for (const m of s.models.children) if (m.userData.foliage) m.visible = false; s.defaultFov ??= s.camera.fov; s.camera.fov = view.fov ?? s.defaultFov; s.camera.updateProjectionMatrix(); s.camera.position.fromArray(view.position); s.controls.target.fromArray(view.target); s.controls.update(); s.draw(); }, view);
    await waitForFrames(page, 6);
  };
  // Every picture before any frame is timed: the time-driven mode advances the time some materials read, by as much
  // as its frames took, and the pictures are for comparing runs.
  for (const [name, view] of Object.entries(views)) {
    await look(view);
    await writeFile(resolve(out, `${name}.png`), await page.locator("#viewport canvas").screenshot({ timeout: 300000 }));
  }
  for (const [name, view] of Object.entries(views)) {
    await look(view);
    // The time-driven mode draws every frame and says what one costs (three windows; the last is the steady one).
    await page.evaluate(() => { delete document.querySelector("#viewport").dataset.frameBudget; window.__scene.setContinuous(true); });
    let budget = null;
    for (let window = 0; window < 3; window++) {
      await page.evaluate(() => { delete document.querySelector("#viewport").dataset.frameBudget; });
      await page.waitForFunction(() => document.querySelector("#viewport").dataset.frameBudget !== undefined, {}, { timeout: 60000 });
      budget = JSON.parse(await page.evaluate(() => document.querySelector("#viewport").dataset.frameBudget));
    }
    await page.evaluate(() => window.__scene.setContinuous(false));
    report.frames[name] = budget;
    console.log(`frame ${name}: ${budget.intervalMs} ms interval, page ${budget.cpuMs} ms, GPU ${budget.gpuMs} ms, ${budget.calls} draws, ${budget.triangles} triangles`);
  }
  await page.evaluate(() => { for (const m of window.__scene.models.children) if (m.userData.foliage) m.visible = true; });

  if (process.env.WARM !== "0") {
    await open("warm", text);
    await open("edit", JSON.stringify(edited));
  }
  if (errors.length) report.errors = errors.slice(0, 8);
  await writeFile(resolve(out, "perf.json"), JSON.stringify(report, null, 1));
  console.log(`Wrote ${resolve(out, "perf.json")}${errors.length ? `; page errors:\n${errors.slice(0, 8).join("\n")}` : ""}`);
} finally {
  await browser.close();
  server.close();
}
