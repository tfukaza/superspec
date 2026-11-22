import { createServer } from "node:http";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { mkdir, open, readFile, stat, writeFile } from "node:fs/promises";
import { basename, dirname, extname, resolve, sep } from "node:path";
import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";
import { PNG } from "pngjs";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { resolveRenderProfile } from "../adapters/three/src/renderProfile.ts";

// Review cameras are glTF Y-up metres. A JSON file can supply other cities' views.
const presets = [
  { name: "market", eye: [18, 1.6, 9], target: [20, 2, 3] },
  { name: "close-shop", eye: [18, 1.6, -1], target: [19.5, 1.4, -4] },
  { name: "spawn-lane", eye: [17.5, 1.6, -15], target: [25, 1.8, -15] },
  { name: "street", eye: [0, 1.6, -25], target: [0, 2, 0] },
  { name: "terrace", eye: [28.5, 5.1, -9.5], target: [24.5, 5.1, -2] },
  { name: "city", eye: [30, 22, 35], target: [5, 6, 0] },
  { name: "city-pan", eye: [30, 22, 35], target: [5, 6, 0],
    endEye: [42, 22, 23], endTarget: [5, 6, 0] },
  { name: "top-down", eye: [0, 65, 2], target: [0, 0, 0] },
  { name: "spawn-walk", eye: [-24, 1.6, -28.5], target: [-20, 1.9, -20.5],
    endEye: [24, 1.6, -28.5], endTarget: [20, 1.9, -20.5] },
];

const help = `Usage: npm run bench:published-city -- ENTRY.glb [options]

Benchmarks the packaged native viewer; build it with npm run build:native-viewer.
Supply the actual published entry GLB; its companion assets stay beside it.

  --out DIR              Results JSON and view PNGs (generated/published-city-bench)
  --resolution WIDTHxHEIGHT  Output resolution (1920x1080). At 100% render scale it matches the drawing buffer.
  --views NAMES|FILE.json Comma-separated review cameras or a JSON array of views
                         Default: market,close-shop,city,city-pan,spawn-walk
  --frames N             Measured frames per view (180; moving views default 720)
  --warmup N             Unmeasured frames at each view's start (90). Ordinary moving routes
                         also get one complete untimed replay before measurement.
  --viewer DIR           Packaged native viewer (generated/native-viewer)
  --renderer BACKEND     auto, webgpu or webgl. Explicit requests must run on that backend.
  --graphics PRESET      medium, high or very-high (default medium)
  --profile FILE         Version 1 JSON render profile with overrides and diagnostics
  --prop-detail PROFILE  default, balanced (previous budgets), or full
  --throughput           Measure completed render frames per second independently of display refresh (default).
  --paced                Measure playback at browser animation-frame cadence, including missed refreshes.
  --passes               Attribute CPU and GPU costs to render passes in a separate diagnostic run
  --gpu-sample-every N   Select paced measurement and time one GPU frame every N frames.
                         Use 0 for query-free pacing or 1 for an every-frame timer.
  --calibrate            Replay each warmed route in six balanced rounds. Compare two identical
                         query-free controls with GPU sampling (every frame unless specified).
                         Defaults to city-pan and spawn-walk, 180 poses per pass.
  --first-traversal      Load at the first view's start, measure startup, then replay each
                         route in the same renderer. Defaults to zero stationary warmup frames.
  --viewer-query PARAMS  Viewer feature switches, e.g. warmup=off, for controlled comparisons
  --compare DIR          Compare each settled view PNG with the same view from a prior run
  --help                 Show this help without launching a browser

Review cameras: ${presets.map(view => view.name).join(", ")}
JSON views: [{"name":"room","eye":[0,2,5],"target":[0,2,0]}]
Add endEye and endTarget for a linear moving route. Coordinates are glTF Y-up metres.
These named cameras target Stoa; use a JSON file for other published cities.

Uses tools/browser.mjs: BROWSER_PATH, BROWSER_ARGS and SOFTWARE_RENDERING apply.
For Windows hardware timing set BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist".
Pass attribution, GPU timer sampling, calibration and first traversal use WebGL diagnostics.
CPU submission, GPU timer queries and delivered rAF intervals are measured separately.
No GPU timer support is reported as unavailable, never estimated from CPU timings.
`;

async function options(args = process.argv.slice(2)) {
  const { values, positionals } = parseArgs({ args, allowPositionals: true, options: {
    help: { type: "boolean", short: "h" }, out: { type: "string" }, resolution: { type: "string" },
    views: { type: "string" }, frames: { type: "string" }, warmup: { type: "string" }, viewer: { type: "string" },
    graphics: { type: "string" }, profile: { type: "string" },
    "prop-detail": { type: "string", default: "default" }, passes: { type: "boolean", default: false },
    paced: { type: "boolean" }, throughput: { type: "boolean" }, renderer: { type: "string" },
    calibrate: { type: "boolean", default: false }, "gpu-sample-every": { type: "string" },
    "first-traversal": { type: "boolean", default: false }, "viewer-query": { type: "string" }, compare: { type: "string" },
  } });
  if (values.help) { console.log(help); return; }
  const profile = values["prop-detail"];
  if (!["default", "balanced", "full"].includes(profile)) throw Error("--prop-detail expects default, balanced or full.");
  if (positionals.length !== 1) throw Error("Supply one published entry GLB path. Use --help for usage.");
  const integer = (value, name, min, max) => {
    const number = Number(value);
    if (!/^\d+$/.test(value) || !Number.isSafeInteger(number) || number < min || number > max)
      throw Error(`${name} must be an integer from ${min} to ${max}.`);
    return number;
  };
  const dimensions = (values.resolution ?? "1920x1080").match(/^(\d+)x(\d+)$/);
  if (!dimensions) throw Error("--resolution must have the form WIDTHxHEIGHT, for example 1920x1080.");
  const width = integer(dimensions[1], "width", 64, 8192), height = integer(dimensions[2], "height", 64, 8192);
  const frames = values.frames === undefined ? undefined : integer(values.frames, "--frames", 2, 100000);
  const firstTraversal = values["first-traversal"];
  const calibrate = values.calibrate;
  const paced = values.paced || calibrate || firstTraversal || values.passes || values["gpu-sample-every"] !== undefined;
  if (values.throughput && paced)
    throw Error("Run --throughput separately from --paced, --gpu-sample-every, --passes, --calibrate and --first-traversal.");
  const timingMode = paced ? "paced" : "throughput";
  if (calibrate && (firstTraversal || values.passes)) throw Error("--calibrate measures warmed whole routes.");
  if (values.passes && values["gpu-sample-every"] !== undefined) throw Error("--passes uses its own sparse pass sampling schedule.");
  const gpuSampleEvery = integer(values["gpu-sample-every"] ?? (calibrate ? "1" : "0"), "--gpu-sample-every", calibrate ? 1 : 0, 100000);
  const warmup = integer(values.warmup ?? (firstTraversal ? "0" : "90"), "--warmup", 0, 10000);
  const viewerQuery = new URLSearchParams(values["viewer-query"]);
  if (values.graphics !== undefined) {
    if (viewerQuery.has("graphics") && viewerQuery.get("graphics") !== values.graphics)
      throw Error("--graphics conflicts with the graphics preset in --viewer-query.");
    viewerQuery.set("graphics", values.graphics);
  }
  if (values.profile !== undefined) {
    if (viewerQuery.has("profile")) throw Error("--profile conflicts with a render profile in --viewer-query.");
    const profileSource = JSON.parse(await readFile(resolve(values.profile), "utf8"));
    resolveRenderProfile(profileSource, viewerQuery);
    viewerQuery.set("profile", JSON.stringify(profileSource));
  } else if (viewerQuery.has("graphics")) resolveRenderProfile(undefined, viewerQuery);
  if (values.renderer && viewerQuery.has("renderer") && viewerQuery.get("renderer") !== values.renderer)
    throw Error("--renderer conflicts with the renderer in --viewer-query.");
  const diagnostics = calibrate || firstTraversal || values.passes || gpuSampleEvery > 0;
  const requestedRenderer = values.renderer ?? viewerQuery.get("renderer") ?? (diagnostics ? "webgl" : "auto");
  if (!["auto", "webgpu", "webgl"].includes(requestedRenderer)) throw Error("--renderer expects auto, webgpu or webgl.");
  if (diagnostics && requestedRenderer === "webgpu") throw Error("These diagnostics require --renderer webgl.");
  viewerQuery.set("renderer", diagnostics ? "webgl" : requestedRenderer);
  for (const key of ["gltf", "gpuprobe", "camera", "target", "fov", "propdetail"])
    if (viewerQuery.has(key)) throw Error(`The benchmark owns the ${key} parameter.`);
  if (firstTraversal && values.passes) throw Error("Measure the first traversal and detailed passes in separate runs.");
  const choice = values.views ?? (calibrate ? "city-pan,spawn-walk" : "market,close-shop,city,city-pan,spawn-walk");
  const views = choice.endsWith(".json") ? JSON.parse(await readFile(resolve(choice), "utf8"))
    : choice.split(",").map(name => {
      const view = presets.find(view => view.name === name);
      if (!view) throw Error(`Unknown view '${name}'. Use --help for available names or a JSON view file.`);
      return view;
    });
  const point = value => Array.isArray(value) && value.length === 3 && value.every(Number.isFinite);
  if (!Array.isArray(views) || !views.length || views.some(view => !view || typeof view.name !== "string" || !/^[a-zA-Z0-9_-]+$/.test(view.name)
    || !point(view.eye) || !point(view.target) || (view.endEye !== undefined || view.endTarget !== undefined)
      && (!point(view.endEye) || !point(view.endTarget))) || new Set(views.map(view => view.name)).size !== views.length)
    throw Error("Views need unique filename-safe names, eye/target triples, and both endEye/endTarget for a moving route.");
  if (firstTraversal && views.some(view => views.some(other => other.name === view.name + "-repeat")))
    throw Error("View names must leave the -repeat suffix available for replay results.");
  const entry = resolve(positionals[0]), viewer = resolve(values.viewer ?? process.env.STUDIO_UI_ROOT ?? "generated/native-viewer");
  if (extname(entry).toLowerCase() !== ".glb") throw Error("The entry must be a published .glb file.");
  const file = await open(entry, "r").catch(() => { throw Error(`Published entry GLB does not exist: ${entry}`); });
  try {
    const header = Buffer.alloc(12), info = await file.stat();
    const { bytesRead } = await file.read(header, 0, 12, 0);
    if (bytesRead !== 12 || header.toString("ascii", 0, 4) !== "glTF" || header.readUInt32LE(4) !== 2
      || header.readUInt32LE(8) !== info.size) throw Error(`Invalid glTF 2 binary header: ${entry}`);
  } finally { await file.close(); }
  if (!await stat(resolve(viewer, "native.html")).then(info => info.isFile(), () => false))
    throw Error(`Packaged native.html is missing from ${viewer}. Run npm run build:native-viewer or pass --viewer DIR.`);
  const output = resolve(values.out ?? "generated/published-city-bench");
  const comparison = values.compare && resolve(values.compare);
  if (comparison === output) throw Error("The comparison images and new results require different directories.");
  return { entry, viewer, profile, timingMode, firstTraversal, viewerQuery, requestedRenderer, passes: values.passes, calibrate, gpuSampleEvery,
    output, comparison, width, height, frames, warmup, views };
}

async function sha256(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

function compareScreenshot(reference, current) {
  const before = PNG.sync.read(reference), after = PNG.sync.read(current);
  if (before.width !== after.width || before.height !== after.height) throw Error("Comparison screenshots have different dimensions.");
  let absolute = 0, changed = 0, over8 = 0, over32 = 0, largest = 0;
  for (let at = 0; at < before.data.length; at += 4) {
    const red = Math.abs(before.data[at] - after.data[at]);
    const green = Math.abs(before.data[at + 1] - after.data[at + 1]);
    const blue = Math.abs(before.data[at + 2] - after.data[at + 2]);
    const max = Math.max(red, green, blue);
    absolute += red + green + blue;
    if (max > 0) changed++;
    if (max > 8) over8++;
    if (max > 32) over32++;
    largest = Math.max(largest, max);
  }
  const pixels = before.width * before.height;
  return { pixels, identicalPixels: pixels - changed, pixelsOver8: over8, pixelsOver32: over32,
    meanAbsoluteRgb: absolute / (pixels * 3), maxChannelDelta: largest };
}

async function serve(viewer, artifacts) {
  const server = createServer(async (request, response) => {
    try {
      const pathname = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
      if (pathname === "/favicon.ico") { response.writeHead(204).end(); return; }
      const asset = pathname.startsWith("/artifacts/"), root = asset ? artifacts : viewer;
      const path = resolve(root, "." + (asset ? pathname.slice("/artifacts".length) : pathname));
      if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
      const info = await stat(path);
      if (!info.isFile()) { response.writeHead(404).end(); return; }
      response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css",
        ".wasm": "application/wasm", ".png": "image/png", ".jpg": "image/jpeg", ".json": "application/json",
        ".glb": "model/gltf-binary", ".ktx2": "image/ktx2" }[extname(path)] ?? "application/octet-stream");
      response.setHeader("Content-Length", info.size);
      createReadStream(path).on("error", () => response.destroy()).pipe(response);
    } catch { response.writeHead(404).end(); }
  });
  await new Promise((done, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", done); });
  return server;
}

// Playwright serializes this function into the packaged page.
async function measure({ view, frames, warmup, trackResources = false, gpuSampleEvery = 0 }) {
  const probe = window.__worldSpecView;
  const { renderer, camera, render, settle, backend = "webgl" } = probe;
  const webgpu = backend === "webgpu";
  const gl = webgpu ? undefined : renderer.getContext(), ext = gl?.getExtension("EXT_disjoint_timer_query_webgl2");
  const debug = gl?.getExtension("WEBGL_debug_renderer_info");
  const gpu = webgpu ? (renderer.backend.device.adapterInfo?.description || renderer.backend.device.adapterInfo?.vendor || "WebGPU adapter") :
    gl.getParameter(debug ? debug.UNMASKED_RENDERER_WEBGL : gl.RENDERER);
  const autoReset = renderer.info.autoReset;
  renderer.info.autoReset = false;
  camera.fov = 70;
  camera.updateProjectionMatrix();
  const samples = [], pending = [], raf = [], cpu = [], gpuTimes = [], calls = [], triangles = [];
  const resources = () => ({ programs: renderer.info.programs.length, textures: renderer.info.memory.textures });
  const entryResources = trackResources && resources();
  const changes = [];
  let startingResources, previousResources, knownPrograms;
  let last, lastMeasured = false, disjointSamples = 0;
  const poll = () => {
    const disjoint = ext && gl.getParameter(ext.GPU_DISJOINT_EXT);
    while (pending.length && (disjoint || gl.getQueryParameter(pending[0].query, gl.QUERY_RESULT_AVAILABLE))) {
      const { query, sample } = pending.shift();
      if (disjoint) disjointSamples++;
      else { sample.gpuMs = gl.getQueryParameter(query, gl.QUERY_RESULT) / 1e6; gpuTimes.push(sample.gpuMs); }
      gl.deleteQuery(query);
    }
  };
  try {
    camera.position.fromArray(view.eye);
    camera.lookAt(...view.target);
    camera.updateMatrixWorld();
    render();
    await settle?.();
    for (let i = -warmup; i <= frames; i++) {
      const now = await new Promise(requestAnimationFrame);
      if (lastMeasured) { samples.at(-1).frameMs = now - last; raf.push(now - last); }
      if (pending.length) poll();
      if (i === frames) break;
      const t = Math.max(0, i) / (frames - 1);
      const position = view.eye.map((v, axis) => v + ((view.endEye?.[axis] ?? v) - v) * t);
      const target = view.target.map((v, axis) => v + ((view.endTarget?.[axis] ?? v) - v) * t);
      camera.position.fromArray(position);
      camera.lookAt(...target);
      camera.updateMatrixWorld();
      renderer.info.reset();
      const sample = { cpuMs: 0, calls: 0, triangles: 0, gpuMs: null };
      const scheduled = gpuSampleEvery > 0 && i >= 0 && i % gpuSampleEvery === 0;
      const query = scheduled && ext && !gl.getParameter(ext.GPU_DISJOINT_EXT) ? gl.createQuery() : null;
      if (query) pending.push({ query, sample });
      if (i === 0 && trackResources) {
        startingResources = previousResources = resources();
        knownPrograms = new Set(renderer.info.programs);
      }
      if (query) gl.beginQuery(ext.TIME_ELAPSED_EXT, query);
      const start = performance.now();
      try { render(); sample.cpuMs = performance.now() - start; } finally { if (query) gl.endQuery(ext.TIME_ELAPSED_EXT); }
      if (i >= 0) {
        sample.calls = webgpu ? renderer.info.render.drawCalls : renderer.info.render.calls; sample.triangles = renderer.info.render.triangles;
        if (trackResources && (renderer.info.programs.length !== previousResources.programs
          || renderer.info.memory.textures !== previousResources.textures)) {
          const state = resources();
          const addedPrograms = state.programs === previousResources.programs ? undefined :
            renderer.info.programs.filter(program => !knownPrograms.has(program))
              .map(program => ({ id: program.id, name: program.name, cacheKey: program.cacheKey }));
          if (addedPrograms) knownPrograms = new Set(renderer.info.programs);
          previousResources = state;
          changes.push({ frame: i, ...state, addedPrograms });
        }
        samples.push(sample); cpu.push(sample.cpuMs); calls.push(sample.calls); triangles.push(sample.triangles);
      }
      last = now; lastMeasured = i >= 0;
    }
    // Drain asynchronous GPU timings after measured rAF intervals, without gl.finish or blocking readback.
    if (pending.length) gl.flush();
    const deadline = performance.now() + 5000;
    while (pending.length && performance.now() < deadline) { await new Promise(requestAnimationFrame); poll(); }
    const summary = values => {
      if (!values.length) return null;
      const sorted = [...values].sort((a, b) => a - b);
      return { median: sorted[Math.floor(sorted.length * .5)], p95: sorted[Math.ceil(sorted.length * .95) - 1],
        max: sorted.at(-1), mean: values.reduce((sum, value) => sum + value, 0) / values.length };
    };
    if (window.__worldSpecView !== probe) throw Error("The renderer changed during the benchmark.");
    if (gl && gl.getError() !== gl.NO_ERROR) throw Error(`WebGL error after rendering ${view.name}`);
    return { name: view.name, moving: !!view.endEye, gpu, backend, width: renderer.domElement.width, height: renderer.domElement.height,
      frames: samples.length, frameMs: summary(raf), cpuMs: summary(cpu), gpuMs: summary(gpuTimes),
      deliveredFps: 1000 * raf.length / raf.reduce((sum, duration) => sum + duration, 0),
      framesOver25ms: raf.filter(ms => ms > 25).length, framesOver50ms: raf.filter(ms => ms > 50).length,
      resources: trackResources ? { atViewEntry: entryResources, beforeMovement: startingResources, changes, atEnd: resources() } : undefined,
      gpuSampleEvery, gpuTimerAvailable: !!ext, scheduledGpuSamples: gpuSampleEvery ? Math.ceil(frames / gpuSampleEvery) : 0,
      gpuSamples: gpuTimes.length, disjointSamples, unresolvedGpuSamples: pending.length,
      calls: summary(calls), triangles: summary(triangles), textures: renderer.info.memory.textures,
      textureResidency: window.__worldSpecView.textureStats?.(), declaredLods: window.__worldSpecView.declaredLods?.(), samples };
  } finally {
    for (const { query } of pending) gl.deleteQuery(query);
    renderer.info.autoReset = autoReset;
  }
}

async function main() {
  const config = await options();
  if (!config) return;
  await mkdir(config.output, { recursive: true });
  const [entrySha256, nativeHtmlSha256] = await Promise.all([sha256(config.entry), sha256(resolve(config.viewer, "native.html"))]);
  const server = await serve(config.viewer, dirname(config.entry));
  let browser;
  const report = { entry: config.entry, viewer: config.viewer, entrySha256, nativeHtmlSha256,
    profile: config.profile, timingMode: config.timingMode, passProfile: config.passes, timestamp: new Date().toISOString(),
    routeWarmup: config.calibrate ? "full-query-free-and-instrumented" : config.firstTraversal ? "first-traversal" : "full-moving-route",
    firstTraversal: config.firstTraversal, gpuSampleEvery: config.passes ? undefined : config.gpuSampleEvery,
    calibration: config.calibrate, requestedRenderer: config.requestedRenderer, viewerQuery: config.viewerQuery.toString(), comparison: config.comparison,
    outputResolution: [config.width, config.height], warmupFrames: config.warmup, views: config.views,
    browserArgs: process.env.BROWSER_ARGS ?? "", softwareRendering: process.env.SOFTWARE_RENDERING === "1", results: [], errors: [] };
  const save = () => writeFile(resolve(config.output, "results.json"), JSON.stringify(report, null, 2));
  try {
    browser = await launchBrowser();
    report.browserVersion = browser.version();
    const page = await browser.newPage({ viewport: { width: config.width, height: config.height }, deviceScaleFactor: 1 });
    page.on("pageerror", error => report.errors.push(error.message));
    page.on("console", message => {
      if (message.type() === "error" || message.text().includes("Using full-detail city geometry:")) report.errors.push(message.text().slice(0, 1000));
    });
    page.on("response", response => { if (!response.ok()) report.errors.push(`${response.status()} ${response.url()}`); });
    const query = new URLSearchParams(config.viewerQuery);
    query.set("gltf", "/artifacts/" + encodeURIComponent(basename(config.entry)));
    query.set("gpuprobe", "");
    if (config.profile !== "default") query.set("propdetail", config.profile);
    const viewportStyle = "#viewport { position:fixed; inset:0; width:100vw; height:100vh; z-index:99; }";
    if (config.firstTraversal) {
      // The viewer's URL takes domain Z-up metres, while the route cameras use glTF Y-up.
      const domain = ([x, y, z]) => [x, -z, y].join(",");
      query.set("camera", domain(config.views[0].eye));
      query.set("target", domain(config.views[0].target));
      query.set("fov", "70");
      await page.addInitScript(css => {
        const observer = new MutationObserver(() => {
          if (!document.head) return;
          const style = document.createElement("style");
          style.textContent = css; document.head.append(style);
          observer.disconnect();
        });
        observer.observe(document, { childList: true, subtree: true });
        const ready = new MutationObserver(() => {
          if (document.querySelector("#status")?.dataset.state === "ready") {
            window.__cityViewerReadyMs = performance.now();
            ready.disconnect();
          }
        });
        ready.observe(document, { attributes: true, attributeFilter: ["data-state"], subtree: true });
      }, viewportStyle);
    }
    await page.goto(`http://127.0.0.1:${server.address().port}/native.html?${query}`, { timeout: 120000 });
    await page.waitForFunction(() => {
      const state = document.querySelector("#status")?.dataset.state;
      return state === "error" || state === "ready" && !!window.__worldSpecView;
    }, undefined, { timeout: 240000 });
    if (await page.locator("#status").getAttribute("data-state") === "error") throw Error(await page.locator("#status").textContent());
    report.backend = await page.evaluate(() => window.__worldSpecView.backend ?? "webgl");
    report.graphics = await page.evaluate(() => window.__worldSpecView.graphics ?? null);
    if (!report.graphics && (query.has("graphics") || query.has("profile")))
      throw Error("The selected viewer does not report graphics profiles. Rebuild the packaged viewer.");
    const scale = report.graphics?.renderScale ?? 1;
    report.resolution = [Math.floor(config.width * scale), Math.floor(config.height * scale)];
    report.appearance = await page.locator("#status").getAttribute("data-appearance");
    report.rendererStatus = await page.locator("#status").textContent();
    if (config.requestedRenderer !== "auto" && config.requestedRenderer !== report.backend)
      throw Error("Requested " + config.requestedRenderer + " but the viewer reports " + report.rendererStatus);
    if (config.firstTraversal) {
      await page.waitForFunction(() => Number.isFinite(window.__cityViewerReadyMs), undefined, { timeout: 240000 });
      report.startup = await page.evaluate(async () => {
        const probe = window.__worldSpecView;
        await probe.settle?.();
        probe.render();
        await new Promise(requestAnimationFrame);
        return { navigationToViewerReadyMs: window.__cityViewerReadyMs, navigationToSettledStartMs: performance.now(),
          shaderWarmup: probe.warmup ?? null, programs: probe.renderer.info.programs.length,
          textures: probe.renderer.info.memory.textures };
      });
      console.log(JSON.stringify(report.startup));
      await save();
    } else await page.addStyleTag({ content: viewportStyle });
    await waitForFrames(page, 3);
    // Style and ResizeObserver delivery can land on different frames. Check
    // the actual drawing buffer before the first route or calibration primer.
    await page.waitForFunction(([width, height]) => {
      const { renderer, backend } = window.__worldSpecView;
      if (backend === "webgpu") return renderer.domElement.width === width && renderer.domElement.height === height;
      const gl = renderer.getContext();
      return gl.drawingBufferWidth === width && gl.drawingBufferHeight === height;
    }, report.resolution);
    let sampleView = measure;
    if (config.passes) sampleView = (await import("./published-city-passes.mjs")).measureCityPasses;
    if (config.timingMode === "throughput") sampleView = (await import("./published-city-throughput.mjs")).measureCityThroughput;
    const calibrateView = config.calibrate && (await import("./published-city-calibration.mjs")).calibrateView;
    const views = config.firstTraversal ? config.views.flatMap(view => [view, { ...view, name: view.name + "-repeat" }]) : config.views;
    for (const view of views) {
      const frames = config.frames ?? (view.endEye && !config.calibrate ? 720 : 180);
      if (view.endEye && !config.calibrate && !config.firstTraversal) {
        const prime = config.timingMode === "throughput" ? sampleView : measure;
        await page.evaluate(prime, { view, frames, warmup: 0, gpuSampleEvery: 0 });
      }
      const measureRoute = async gpuSampleEvery => {
        const result = await page.evaluate(sampleView, { view, warmup: config.warmup,
          frames, trackResources: config.firstTraversal, gpuSampleEvery });
        if (result.backend && result.backend !== report.backend) throw Error("The renderer changed between benchmark routes.");
        if (result.width !== report.resolution[0] || result.height !== report.resolution[1])
          throw Error("Drawing-buffer resolution differs from the render profile's expected resolution.");
        return result;
      };
      const result = calibrateView ? await calibrateView(view.name, config.gpuSampleEvery, measureRoute) : await measureRoute(config.gpuSampleEvery);
      report.results.push(result);
      const { samples, materialCaptures, resources, rounds, ...summary } = result;
      console.log(JSON.stringify({ timingMode: config.timingMode, ...summary }));
      if (config.firstTraversal || config.timingMode === "throughput") await page.evaluate(async endpoint => {
        const probe = window.__worldSpecView;
        if (endpoint) {
          // The throughput measurement restores its caller's camera when it drains.
          probe.camera.fov = 70;
          probe.camera.updateProjectionMatrix();
          probe.camera.position.fromArray(endpoint.endEye ?? endpoint.eye);
          probe.camera.lookAt(...(endpoint.endTarget ?? endpoint.target));
          probe.camera.updateMatrixWorld();
        }
        await probe.settle?.();
        probe.render();
        await new Promise(requestAnimationFrame);
      }, config.timingMode === "throughput" ? view : null);
      const filename = `${view.name}.png`;
      const screenshot = await page.locator("#viewport").screenshot({ path: resolve(config.output, filename) });
      const sameRenderer = await page.evaluate(backend => document.querySelector("#status")?.dataset.state === "ready"
        && !!window.__worldSpecView && (window.__worldSpecView.backend ?? "webgl") === backend, report.backend);
      if (!sameRenderer) throw Error("The renderer changed before the benchmark screenshot completed.");
      if (config.comparison) result.imageDifference = compareScreenshot(await readFile(resolve(config.comparison, filename)), screenshot);
      await save();
      if (report.errors.length) throw Error(report.errors.join("\n"));
    }
    console.log(`Saved ${resolve(config.output, "results.json")}`);
  } catch (error) {
    report.failure = error.message;
    await save();
    throw error;
  } finally {
    await browser?.close();
    await new Promise(done => server.close(done));
  }
}

export { measure, options, presets, serve };

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href)
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
