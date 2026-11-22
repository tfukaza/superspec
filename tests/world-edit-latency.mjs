import { spawn } from "node:child_process";
import { access, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { basename, resolve } from "node:path";
import { createInterface } from "node:readline";
import { launchBrowser } from "../tools/browser.mjs";

// A file is a settled edit. --drag FILE interpolates one existing metric attribute to the
// supplied endpoint. --drag DIR replays sorted AML files for arbitrary gesture recordings.
// For a project with companions, AML_FOLDER opens the initial scene via the normal folder picker.
const args = process.argv.slice(2);
const traceIndex = args.indexOf("--trace-maps");
const traceFile = traceIndex < 0 ? undefined : args.splice(traceIndex, 2)[1];
if (traceIndex >= 0 && !traceFile) throw Error("--trace-maps requires an output JSON path.");
const [firstArg, ...drafts] = args;
const millimeter = firstArg === "--millimeter";
const initial = millimeter ? undefined : firstArg;
const usage = "Run npm run bench:world-edits for the Stoa building suite. Pass --millimeter for a 9.800 to 9.801 m resize, or initial.aml [--drag] edit.aml ... for your own scene. Add --trace-maps FILE to --millimeter for diagnostic GPU readbacks and per-surface work; run without it for latency.";
if (initial === "--help" || initial === "-h") { console.log(usage); process.exit(0); }
if ((initial && !drafts.length) || (millimeter && drafts.length)) throw Error(usage);
if (traceFile && !millimeter) throw Error("--trace-maps requires --millimeter.");
const normalized = text => text.replace(/\r\n?/g, "\n");
// The same seeded Kowloon prototype used by the Stoa game export. No external maps or companions are needed.
const stoa = '<architecture version="1" id="building-3"><city><building id="prototype" position="0 0 0" floors="4" seed="4" /></city></architecture>';
const widthEdit = width => stoa.replace(" />", ` width="${width}" />`);
function interpolate(from, to) {
  for (const match of from.matchAll(/\s(?:position|width|depth|footprint)\s*=\s*(["'])([\d.eE+;\s-]+)\1/g)) {
    const end = match.index + match[0].length - 1, start = end - match[2].length;
    if (!to.startsWith(from.slice(0, start)) || !to.endsWith(from.slice(end))) continue;
    const target = to.slice(start, end + to.length - from.length);
    const number = /[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/g;
    if (target.replace(number, "#") !== match[2].replace(number, "#")) continue;
    const a = match[2].match(number).map(Number), b = target.match(number).map(Number);
    if (![...a, ...b].every(Number.isFinite)) continue;
    const drafts = [];
    for (let step = 1; step <= 60; step++) {
      let i = 0;
      const value = step === 60 ? target : match[2].replace(number, () => {
        const axis = i++;
        return String(+(a[axis] + (b[axis] - a[axis]) * step / 60).toFixed(3));
      });
      const text = from.slice(0, start) + value + from.slice(end);
      if (text !== (drafts.at(-1) ?? from)) drafts.push(text);
    }
    if (drafts.length) return drafts;
  }
  throw Error("--drag FILE needs one changed position, width, depth or footprint attribute with the same separators. Use --drag DIR for other edits.");
}

function* stoaGestures() {
  const moved = stoa.replace('position="0 0 0"', 'position="2 0 0"');
  const withWidth = width => moved.replace(" />", ' width="' + width + '" />');
  const lot = rearX => moved.replace(" />", ' footprint="0 0; 10.8 0; ' + rearX + ' 7; 0 7" />');
  const rectangle = lot(10.8), trapezoid = lot(9.3);
  yield { name: "move", texts: interpolate(stoa, moved) };
  // The original lot uses the shipped default width. The first drag input introduces the explicit attribute.
  yield { name: "width", texts: interpolate(withWidth(9.8), withWidth(10.8)) };
  yield { name: "explicit-lot.aml", texts: [rectangle] };
  yield { name: "taper", texts: interpolate(rectangle, trapezoid) };
  yield { name: "object-rename-taper.aml", texts: [trapezoid.replace('id="prototype"', 'id="teahouse"')] };
}

async function* suppliedGestures(prior) {
  for (let i = 0; i < drafts.length; i++) {
    const drag = drafts[i] === "--drag", file = drag ? drafts[++i] : drafts[i];
    if (!file) throw Error("Missing --drag AML file or directory.");
    const directory = drag && (await stat(file)).isDirectory();
    const files = directory ? (await readdir(file)).filter(name => name.endsWith(".aml")).sort().map(name => resolve(file, name)) : [file];
    if (!files.length) throw Error("Empty drag sequence " + file);
    const loaded = await Promise.all(files.map(async path => normalized(await readFile(path, "utf8"))));
    const texts = drag && !directory ? interpolate(prior, loaded[0]) : loaded;
    yield { name: basename(file), texts };
    prior = texts.at(-1);
  }
}
let server, browser;
try {
  let address = process.env.STUDIO_URL;
  if (!address) {
    const ui = resolve(process.env.STUDIO_UI_ROOT ?? "generated/native-viewer");
    try { await access(resolve(ui, "index.html")); } catch {
      throw Error("Missing native viewer in " + ui + ". Run npm run build:native-viewer or set STUDIO_UI_ROOT to the built viewer.");
    }
    const cli = process.env.RUST_STUDIO || process.env.STUDIO_CLI || resolve(process.env.CARGO_TARGET_DIR ?? "target", "release", "superspec-studio" + (process.platform === "win32" ? ".exe" : ""));
    server = spawn(cli, ["serve", "--ui", ui], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
    address = await new Promise((ok, fail) => {
      server.once("error", error => fail(error.code === "ENOENT" ? Error("Missing native Studio at " + cli +
        ". Run cargo build --release --locked -p superspec-cli --bin superspec-studio, set STUDIO_CLI to the executable, or use STUDIO_URL for a running Studio.") : error));
      server.once("exit", code => fail(Error("Studio exited with code " + code)));
      createInterface({ input: server.stderr }).on("line", line => {
        const match = line.match(/listening at (http:\/\/[^/]+)/);
        if (match) ok(match[1]);
      });
    });
  }
  browser = await launchBrowser();
  const page = await browser.newPage();
  await page.goto(new URL("index.html?gpuprobe", address).href);
  async function ready() {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status").dataset.state), undefined, { timeout: 180000 });
    if (await page.locator("#status").getAttribute("data-state") === "error") throw Error(await page.locator("#diagnostics").innerText());
  }
  await ready();
  const revision = await page.locator("#viewport").getAttribute("data-revision");
  const initialText = initial ? normalized(await readFile(initial, "utf8")) : millimeter ? widthEdit(9.8) : stoa;
  const openedAt = await page.evaluate(() => performance.now());
  if (initial && process.env.AML_FOLDER) await page.locator("#open-folder").setInputFiles(resolve(process.env.AML_FOLDER));
  else await page.locator("#open-aml").setInputFiles(initial ? resolve(initial)
    : { name: "building-3.aml", mimeType: "application/xml", buffer: Buffer.from(initialText) });
  await page.waitForFunction(before => document.querySelector("#viewport").dataset.revision !== before || document.querySelector("#status").dataset.state === "error", revision, { timeout: 180000 });
  await ready();
  if (await page.locator("#aml").inputValue() !== initialText) throw Error("Opened AML differs from the specified initial source.");
  await page.waitForFunction(() => {
    const bake = document.querySelector("#viewport").dataset.sceneBake;
    return !bake || JSON.parse(bake).state === "baked";
  }, undefined, { timeout: 180000 });
  console.log(JSON.stringify(await page.evaluate(openedAt => {
    const gl = window.__scene.renderer.getContext(), probe = gl.getExtension("WEBGL_debug_renderer_info");
    const bake = JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null");
    return { renderer: probe && gl.getParameter(probe.UNMASKED_RENDERER_WEBGL), viewport: [gl.drawingBufferWidth, gl.drawingBufferHeight],
      initialOpenToSettledMs: Math.round(performance.now() - openedAt), initialMaterialMs: bake?.ms, displayedMapBytes: bake?.bytes };
  }, openedAt)));
  const trace = traceFile && await import("./world-edit-map-trace.mjs");
  if (trace) await trace.beginMapTrace(page);
  const selection = process.env.BENCH_SELECT ?? (initial || millimeter ? undefined : "city/prototype");
  if (selection) await page.evaluate(owner => {
    const scene = window.__scene, mesh = scene.models.children.find(child => child.userData.source.objectId === owner);
    if (!mesh) throw Error("Missing selected placement " + owner);
    scene.select(mesh.name);
  }, selection);
  const edits = initial ? suppliedGestures(initialText) : millimeter ? [{ name: "width-plus-1mm", texts: [widthEdit(9.801)] }] : stoaGestures();
  for await (const { name, texts } of edits) {
    const timing = await page.evaluate(async texts => {
      const input = document.querySelector("#aml"), status = document.querySelector("#status"), viewport = document.querySelector("#viewport");
      const scene = window.__scene, selection = document.querySelector("#selection"), priorLabel = selection.textContent;
      const stockBefore = { ...scene.bakerInstance?.graph.totals };
      const previous = Number(viewport.dataset.revision), inputs = new Map(), draws = new Map();
      let regeneratedFrame, regeneratedMaterialCoverage;
      const frame = () => new Promise(done => requestAnimationFrame(done));
      const draw = scene.draw;
      scene.draw = function (...args) {
        draw.apply(this, args);
        const revision = Number(viewport.dataset.placementFrame);
        if (inputs.has(revision) && !draws.has(revision)) draws.set(revision, performance.now());
        if (regeneratedFrame === undefined && Number(viewport.dataset.footprintGeometryFrame) === previous + texts.length) {
          regeneratedFrame = performance.now();
          regeneratedMaterialCoverage = JSON.parse(viewport.dataset.footprintMaterialCoverage ?? "null");
        }
      };
      try {
        let lastInput, labelFrame, authoritativeFrame, bake, worstBackgroundGap = 0;
        for (const [index, text] of texts.entries()) {
          // Give the first input an arbitrary refresh phase like a discrete pointer/key event.
          if (index) await frame();
          lastInput = performance.now();
          inputs.set(previous + index + 1, lastInput);
          input.value = text;
          input.dispatchEvent(new Event("input", { bubbles: true }));
        }
        let lastTick = lastInput;
        while (performance.now() - lastInput < 180000) {
          await frame();
          const now = performance.now();
          worstBackgroundGap = Math.max(worstBackgroundGap, now - lastTick);
          lastTick = now;
          if (selection.textContent !== priorLabel && selection.textContent !== "Click a model to inspect its source.") labelFrame ??= now;
          if (status.dataset.state === "error") throw Error(document.querySelector("#diagnostics").textContent);
          if (status.dataset.state !== "ready" || Number(viewport.dataset.revision) !== previous + texts.length) continue;
          authoritativeFrame ??= now;
          bake = JSON.parse(viewport.dataset.sceneBake ?? "null");
          if (!(bake && bake.state !== "baked")) break;
        }
        if (!authoritativeFrame || (bake && bake.state !== "baked")) throw Error("Timed out waiting for " + status.textContent);
        await frame();
        const settled = performance.now(), start = inputs.get(previous + 1);
        // Record physical GPU completion separately from the first fully shaded editor frame.
        const gl = scene.renderer.getContext(), fence = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
        if (!fence) throw Error("Could not fence the settled editor frame.");
        let gpuSettled;
        try {
          gl.flush();
          let state = gl.clientWaitSync(fence, 0, 0);
          while (state === gl.TIMEOUT_EXPIRED && performance.now() - settled < 60_000) {
            await new Promise(done => setTimeout(done, 1));
            state = gl.clientWaitSync(fence, 0, 0);
          }
          if (state !== gl.ALREADY_SIGNALED && state !== gl.CONDITION_SATISFIED) throw Error("The settled editor frame did not finish on the GPU.");
          gpuSettled = performance.now();
        } finally { gl.deleteSync(fence); }
        const entries = scene.sceneBaker?.current?.entries;
        if (bake && entries) {
          const heldBytes = scene.bakerInstance.atlasBytes(entries.map(entry => entry.maps));
          if (heldBytes !== bake.bytes) throw Error(`Scene reports ${bake.bytes} bytes but holds ${heldBytes} bytes of atlas maps.`);
        }
        const ms = n => n === undefined ? undefined : Math.round(n * 10) / 10;
        const samples = values => {
          if (!values.length) return;
          values.sort((a, b) => a - b);
          const pct = p => ms(values[Math.ceil(p * values.length) - 1]);
          return { p50: pct(.5), p95: pct(.95), max: pct(1) };
        };
        const frames = [...draws.values()];
        const intervals = frames.slice(1).map((time, i) => time - frames[i]);
        const first = Math.min(frames[0] ?? Infinity, labelFrame ?? Infinity, regeneratedFrame ?? Infinity, authoritativeFrame);
        const requests = performance.getEntriesByType("resource")
          .filter(e => e.startTime >= start && /\/api\/session\/.+\/(aml-update|shade-layouts|project-fields)$/.test(e.name))
          .map(e => ({ action: new URL(e.name).pathname.split("/").pop(), bytes: e.decodedBodySize,
            startMs: ms(e.startTime - start), endMs: ms(e.responseEnd - start), ms: Math.round(e.duration) }));
        const counts = scene.bakerInstance?.graph.totals;
        return { inputs: inputs.size, previewDraws: draws.size, firstFeedbackMs: ms(first - start),
          placementFrameMs: frames.length ? ms(frames[0] - start) : undefined, labelFrameMs: ms(labelFrame === undefined ? undefined : labelFrame - start),
          inputToDrawMs: samples([...draws].map(([rev, time]) => time - inputs.get(rev))),
          drawIntervalMs: samples(intervals), drawnFps: intervals.length ? ms(1000 * intervals.length / (frames.at(-1) - frames[0])) : undefined,
          regeneratedAfterLastInputMs: regeneratedFrame === undefined ? undefined : ms(regeneratedFrame - lastInput),
          regeneratedMaterialCoverage,
          authoritativeAfterLastInputMs: ms(authoritativeFrame - lastInput), finishedAfterLastInputMs: ms(settled - lastInput),
          gpuSettledAfterLastInputMs: ms(gpuSettled - lastInput),
          worstBackgroundRafGapMs: ms(worstBackgroundGap), finishedFrameMs: ms(settled - start), unsupported: bake?.unsupported,
          materialBake: bake && { drawn: bake.drawn, reused: bake.reused, nodesDrawn: bake.nodesDrawn, nodesReused: bake.nodesReused,
            stockTexelsDrawn: counts && counts.stockTexelsDrawn - (stockBefore.stockTexelsDrawn ?? 0),
            stockTexelsReused: counts && counts.stockTexelsReused - (stockBefore.stockTexelsReused ?? 0) }, requests };
      } finally {
        scene.draw = draw;
      }
    }, texts);
    console.log(JSON.stringify({ draft: name, ...timing }));
    if (trace) {
      const report = await trace.finishMapTrace(page);
      await writeFile(traceFile, JSON.stringify(report));
      console.log(JSON.stringify({ trace: traceFile, ...report.summary }));
    }
  }
} finally {
  await browser?.close();
  server?.kill();
}
