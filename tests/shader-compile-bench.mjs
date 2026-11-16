/**
 * Shader compile microbench: how long the browser's driver takes to compile and link each MaterialSpec program of the
 * washroom scene (tests/studio/washroom), alone, in a bare WebGL2 page (tests/browser/shader-compile-bench.html).
 * Timings inside the viewer are confounded by its CPU build, uploads and frames; this isolates the compile.
 *
 * The sources are the real ones: the built Studio (STUDIO_UI_ROOT, default generated/rust-editor) opens the washroom
 * with every `linkProgram` recorded, once as shipped and once with `?preview=shader`, and each distinct program that
 * evaluates a MaterialSpec program is then compiled in a fresh browser. Kinds: `atlas` is the GPU bake's evaluate pass,
 * `quad` the per-pixel material with derivative relief, `pixel` the per-pixel material with per-pixel relief; `inline`
 * marks the form with every noise where it is used, which the viewer compiles last (see glsl.rs).
 *
 * Environment: RUNS (default 3) repeats, each with sources made unique so no cache answers; RESOLUTION (default 256);
 * SAVE names a JSON file the captured sources are written to; SOURCES names such a file to measure instead of
 * capturing ({ programs: [{ name, vertex, fragment }] }); TOGETHER=1 also links all programs at once; COLD_DRAW=1 makes
 * the first draw a cold one for the graphics driver too (the driver caches compiled shaders across browsers; defeating
 * that slows the compile being measured, so read compile times from a run without it); PROFILE names a
 * persistent browser profile directory and measures the sources unsalted, once per launch for LAUNCHES (default 2)
 * launches, which shows whether the browser's program cache answers a second load. Set
 * BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist" to measure the real GPU's compiler.
 */
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { chromium } from "playwright";
import { launchBrowser } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom/build.mjs";

const here = import.meta.dirname;
const runs = Number(process.env.RUNS ?? 3);

function serve(root) {
  const server = createServer(async (request, response) => {
    try {
      const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
      const path = resolve(root, `.${name}`);
      if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
      response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm" }[extname(path)] ?? "application/octet-stream");
      response.end(await readFile(path));
    } catch { response.writeHead(404).end(); }
  });
  return new Promise(done => server.listen(0, "127.0.0.1", () => done(server)));
}

/** What a fragment shader is to the viewer, or undefined when it evaluates no MaterialSpec program. */
function kind(fragment) {
  if (!fragment?.includes("ms_evaluate();")) return undefined;
  if (fragment.includes("msAtlasTexel")) return "atlas";
  if (fragment.includes("msPlace")) return fragment.includes("ms_evaluate_height();") ? "pixel" : "quad";
  return "swatch";
}

/** A short name for the MaterialSpec program inside a fragment shader: the washroom's recipes by what they declare. */
function recipe(fragment) {
  const count = pattern => new Set(fragment.match(pattern) ?? []).size;
  const parameters = count(/uniform \w+ ms_p\d+;/g), seeds = count(/uniform uint ms_seed\d+;/g);
  const known = { "24/26": "concrete", "15/8": "steel", "6/4": "tile", "6/5": "wood" }[`${parameters}/${seeds}`];
  return known ?? `${parameters}p${seeds}s`;
}

/** Opens the washroom in the built Studio and returns every distinct MaterialSpec program it linked. */
async function capture() {
  const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
  const server = await serve(root);
  const { project } = buildProject(Number(process.env.RESOLUTION ?? 256));
  const found = new Map();
  try {
    for (const query of ["", "?preview=shader"]) {
      const browser = await launchBrowser();
      try {
        const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
        await page.addInitScript(() => {
          const prototype = WebGL2RenderingContext.prototype, sources = new WeakMap(), attached = new WeakMap();
          window.__linked = [];
          const { shaderSource, attachShader, linkProgram } = prototype;
          prototype.shaderSource = function (shader, source) { sources.set(shader, source); return shaderSource.call(this, shader, source); };
          prototype.attachShader = function (program, shader) { attached.set(program, [...attached.get(program) ?? [], shader]); return attachShader.call(this, program, shader); };
          prototype.linkProgram = function (program) {
            const [vertex, fragment] = (attached.get(program) ?? []).map(shader => sources.get(shader));
            window.__linked.push({ vertex, fragment });
            return linkProgram.call(this, program);
          };
        });
        await page.goto(`http://127.0.0.1:${server.address().port}/material.html${query}`);
        const ready = () => page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 900000 });
        await ready();
        const before = Number(await page.locator("#viewport").getAttribute("data-input-revision") ?? 0);
        await page.locator("#open-project").setInputFiles({ name: "washroom.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
        await page.waitForFunction(start => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > start, before, { timeout: 900000 });
        await ready();
        // Everything the viewer compiles in the background has been linked once the per-pixel relief and the baked maps are in.
        if (await page.evaluate(() => document.querySelector("#viewport").dataset.relief !== undefined))
          await page.waitForFunction(() => document.querySelector("#viewport").dataset.relief === "pixel", {}, { timeout: 900000 });
        if (await page.evaluate(() => document.querySelector("#viewport").dataset.sceneBake !== undefined))
          await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake).state === "baked", {}, { timeout: 900000 });
        // The inline programs are the last: linked one after another, each when the one before it has compiled (as shipped,
        // they are the per-pixel looks shown while a value changes, settled behind the baked scene).
        await page.waitForFunction(() => document.querySelector("#viewport").dataset.noise === "inline", {}, { timeout: 120000 }).catch(() => {});
        await page.waitForTimeout(2000);
        for (const { vertex, fragment } of await page.evaluate(() => window.__linked)) {
          const what = kind(fragment);
          if (what && !found.has(fragment)) found.set(fragment, { name: `${what} ${recipe(fragment)}${fragment.includes("#define MS_INLINE_NOISE") ? " inline" : ""}`, vertex, fragment });
        }
      } finally { await browser.close(); }
    }
  } finally { server.close(); }
  return [...found.values()].sort((a, b) => a.name.localeCompare(b.name));
}

const programs = process.env.SOURCES ? JSON.parse(await readFile(process.env.SOURCES, "utf8")).programs : await capture();
if (process.env.SAVE) await writeFile(process.env.SAVE, JSON.stringify({ programs }));
if (!programs.length) throw new Error("No MaterialSpec program was captured.");

const server = await serve(resolve(here, "browser"));
const url = `http://127.0.0.1:${server.address().port}/shader-compile-bench.html`;
const range = values => { const sorted = values.map(Math.round).sort((a, b) => a - b); return sorted[0] === sorted.at(-1) ? `${sorted[0]}` : `${sorted[0]}-${sorted.at(-1)}`; };
try {
  if (process.env.PROFILE) {
    // The same sources in the same profile, a browser launch each: a later launch is faster when a cache answers.
    for (let launch = 0; launch < Number(process.env.LAUNCHES ?? 2); launch++) {
      const context = await chromium.launchPersistentContext(resolve(process.env.PROFILE), { headless: true, args: process.env.BROWSER_ARGS?.split(" ").filter(Boolean) });
      try {
        const page = await context.newPage();
        await page.goto(url);
        const report = await page.evaluate(programs => window.runBench(programs, { salt: false }), programs);
        console.log(`Launch ${launch + 1} (${report.renderer}): ${report.results.map(result => `${result.name} ${Math.round(result.completeMs)}+${Math.round(result.firstDrawMs)} ms`).join(", ")}; all ${Math.round(report.wallMs)} ms`);
        // The cache is written behind the page; give it a moment before the browser closes.
        await page.waitForTimeout(3000);
      } finally { await context.close(); }
    }
  } else {
    const rows = new Map(programs.map(program => [program.name, { complete: [], draw: [], bytes: program.fragment.length }]));
    const walls = [], together = [];
    let renderer = "";
    for (let run = 0; run < runs; run++) {
      // A browser per run: nothing of an earlier run is in memory.
      const browser = await launchBrowser();
      try {
        const page = await browser.newPage();
        await page.goto(url);
        const report = await page.evaluate(([programs, cold]) => window.runBench(programs, { cold }), [programs, process.env.COLD_DRAW === "1"]);
        renderer = `${report.renderer}${report.parallel ? "" : " (no KHR_parallel_shader_compile: times are of the blocking status read)"}`;
        for (const result of report.results) {
          if (!result.linked) throw new Error(`${result.name} did not link:\n${result.log}`);
          rows.get(result.name).complete.push(result.completeMs + result.statusMs);
          rows.get(result.name).draw.push(result.firstDrawMs);
        }
        walls.push(report.wallMs);
        if (process.env.TOGETHER === "1") together.push((await page.evaluate(programs => window.runBench(programs, { together: true }), programs)).wallMs);
      } finally { await browser.close(); }
    }
    console.log(`Shader compile microbench on ${renderer}, ${runs} runs (ms, min-max):`);
    console.log(`${"program".padEnd(22)}${"fragment bytes".padStart(15)}${"compile+link".padStart(14)}${"first draw".padStart(12)}`);
    for (const [name, row] of rows) console.log(`${name.padEnd(22)}${String(row.bytes).padStart(15)}${range(row.complete).padStart(14)}${range(row.draw).padStart(12)}`);
    console.log(`One after another: ${range(walls)} ms${together.length ? `; all linked at once: ${range(together)} ms` : ""}.`);
  }
} finally { server.close(); }
