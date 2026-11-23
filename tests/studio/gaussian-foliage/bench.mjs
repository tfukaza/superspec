import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";
import { preview } from "vite";
import { launchBrowser } from "../../../tools/browser.mjs";

const out = resolve(process.env.OUT ?? "generated/treespec-gaussian-foliage");
await mkdir(out, { recursive: true });
const server = await preview({
  configFile: fileURLToPath(new URL("./vite.config.mjs", import.meta.url)),
  preview: { host: "127.0.0.1", port: 0 },
});
const base = `http://127.0.0.1:${server.httpServer.address().port}/`;
const rows = [];
const frames = 32;
let browser;
try {
  browser = await launchBrowser();
  for (const side of [1, 4, 8]) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    await page.goto(`${base}?field=${side}&bench`);
    await page.waitForFunction(() => document.querySelector("#metrics").value.includes("dabs"));
    await page.evaluate(() => window.foliageBenchmark.renderer.setAnimationLoop(null));
    const cases = side === 1
      ? [["overview", "lod", "off"], ["overview", "lod", "cached"], ["overview", "lod", "moving"]]
      : [["overview", "lod", "cached"], ["overview", "all", "cached"], ["walk", "lod", "cached"],
         ["walk", "cull", "cached"], ["walk", "all", "cached"], ["walk", "lod", "moving"]];
    if (side === 1) await checkLighting(page);
    for (const [view, policy, shadows] of cases) {
      await page.locator("#detail").selectOption(policy);
      await page.locator("#shadows").setChecked(shadows !== "off");
      const samples = await page.evaluate(async ({ view, side, shadows, frames }) => {
        const { camera, renderer, focus, draw, resetView, setSun } = window.foliageBenchmark;
        const gl = renderer.getContext();
        const ext = gl.getExtension("EXT_disjoint_timer_query_webgl2");
        const frame = () => new Promise(requestAnimationFrame);
        resetView();
        setSun(-38, 50);
        const position = camera.position.clone().sub(focus);
        const rows = [], intervals = [], timings = [];
        let previous;
        for (let i = -8; i < frames; i++) {
          const now = await frame();
          const turn = i / frames;
          if (view === "overview") {
            const angle = (turn - 0.5) * 0.28;
            camera.position.set(focus.x + position.x * Math.cos(angle) - position.y * Math.sin(angle),
              focus.y + position.x * Math.sin(angle) + position.y * Math.cos(angle), focus.z + position.z);
            camera.lookAt(focus);
          } else {
            camera.position.set(focus.x + (turn - 0.5) * 4.5, focus.y - side * 1.65, 1.85 + turn * 0.4);
            camera.lookAt(focus.x + 3 * Math.sin(turn * 2.1), focus.y + 5.5, 0.62);
          }
          if (shadows === "moving") setSun(-48 + turn * 30, 48 - turn * 5);
          const query = ext && i >= 0 ? gl.createQuery() : null;
          const result = draw(query ? { query, target: ext.TIME_ELAPSED_EXT } : undefined);
          if (i >= 0) {
            rows.push(result);
            if (previous !== undefined) intervals.push(now - previous);
            previous = now;
          }
          if (query) timings.push(query);
        }
        const gpuTimes = [];
        if (ext) {
          for (let attempt = 0; attempt < 30 && !gl.getQueryParameter(timings.at(-1), gl.QUERY_RESULT_AVAILABLE); attempt++) await frame();
          if (!gl.getParameter(ext.GPU_DISJOINT_EXT)) {
            for (const query of timings) {
              if (gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE)) gpuTimes.push(gl.getQueryParameter(query, gl.QUERY_RESULT) / 1e6);
            }
          }
          timings.forEach(query => gl.deleteQuery(query));
        }
        const debug = gl.getExtension("WEBGL_debug_renderer_info");
        return {
          rows, intervals, gpuTimes, resolution: [gl.drawingBufferWidth, gl.drawingBufferHeight],
          gpu: debug && gl.getParameter(debug.UNMASKED_RENDERER_WEBGL),
        };
      }, { view, side, shadows, frames });
      assert.ok(samples.rows.every(row => row.redrawShadow === (shadows === "moving")), "shadow updates match the workload");
      assert.ok(samples.rows.every(row => row.activeCount > 0 && row.activeCount <= row.capacity), "instances stay within the field capacity");
      const result = summarize(samples, { patches: side * side, view, policy, shadows });
      rows.push(result);
      console.log(JSON.stringify(result));
      if (side === 8 && policy === "lod" && shadows === "cached") {
        await page.locator("canvas").screenshot({ path: resolve(out, `field-${view}.png`) });
      }
    }
    assert.deepEqual(errors, []);
    await page.close();
  }
  const row = (view, policy) => rows.find(r => r.patches === 64 && r.view === view && r.policy === policy && r.shadows === "cached");
  assert.ok(row("overview", "lod").activeDabs < row("overview", "all").activeDabs / 4);
  assert.ok(row("walk", "lod").activeDabs < row("walk", "cull").activeDabs);
  assert.ok(row("walk", "cull").activeDabs < row("walk", "all").activeDabs);
  await writeFile(resolve(out, "scale-and-lighting.json"), JSON.stringify(rows, null, 2) + "\n");
} finally {
  await browser?.close();
  await new Promise((done, fail) => server.httpServer.close(error => error ? fail(error) : done()));
}

function summarize(samples, setup) {
  const { rows, intervals, gpuTimes, resolution, gpu } = samples;
  const measure = (values, percentile = 0.5) => {
    const sorted = values.sort((a, b) => a - b);
    return sorted.length ? Number(sorted[Math.ceil(percentile * sorted.length) - 1].toFixed(3)) : null;
  };
  const cpu = () => rows.map(({ selectMs, sortMs, packMs, submitMs }) => selectMs + sortMs + packMs + submitMs);
  return {
    ...setup, gpu, resolution, frameCount: rows.length, gpuSamples: gpuTimes.length,
    activeDabs: measure(rows.map(row => row.activeCount)),
    visiblePatches: measure(rows.map(row => row.visiblePatches)),
    instanceUploadBytes: measure(rows.map(row => row.instanceBytes)),
    selectMs: measure(rows.map(row => row.selectMs)),
    sortMs: measure(rows.map(row => row.sortMs)),
    packMs: measure(rows.map(row => row.packMs)),
    cpuMs: measure(cpu()), cpuP95Ms: measure(cpu(), 0.95),
    gpuMs: measure(gpuTimes), frameIntervalMs: measure(intervals),
    frameIntervalP95Ms: measure(intervals.slice(), 0.95),
    observedFps: Number((1000 * intervals.length / intervals.reduce((sum, value) => sum + value, 0)).toFixed(1)),
  };
}

async function checkLighting(page) {
  const shot = async name => PNG.sync.read(await page.locator("canvas").screenshot({ path: resolve(out, name + ".png") }));
  const draw = () => page.evaluate(() => window.foliageBenchmark.draw());
  await draw();
  const shadow = await shot("live-shadow");
  await page.locator("#shadows").uncheck();
  await draw();
  const noShadow = await shot("shadow-disabled");
  assert.ok(differentPixels(noShadow, shadow) > 1000, "the scene receives the proxy shadows");
  // Hide the opaque scene so only changes to foliage pigments can affect the comparison.
  await page.evaluate(() => {
    const { scene } = window.foliageBenchmark;
    const opaque = scene.children.filter(child => (child.isMesh || child.isGroup) && !child.material?.isShaderMaterial);
    const visibility = opaque.map(child => child.visible);
    window.restoreOpaque = () => opaque.forEach((child, index) => { child.visible = visibility[index]; });
    opaque.forEach(child => { child.visible = false; });
  });
  await draw();
  const a = await shot("splat-light-a");
  await page.locator("#sun").fill("129");
  const movedSun = await draw();
  assert.equal(movedSun.sortMs + movedSun.packMs, 0, "sun changes reuse the sorted instances at a stationary camera");
  const b = await shot("splat-light-b");
  assert.ok(differentPixels(a, b) > 3000, "a different sun changes the splats without a shadow map or opaque surfaces");
  await page.locator("#relight").uncheck();
  await draw();
  const fixed = await shot("splat-fixed");
  await page.locator("#sun").fill("-38");
  await draw();
  assert.equal(differentPixels(fixed, await shot("splat-fixed-sun-moved")), 0, "fixed pigment is the control");
  await page.evaluate(() => window.restoreOpaque());
  await page.locator("#relight").check();
  await page.locator("#shadows").check();
}

function differentPixels(a, b) {
  assert.equal(a.data.length, b.data.length);
  let count = 0;
  for (let i = 0; i < a.data.length; i += 4) {
    if (Math.abs(a.data[i] - b.data[i]) + Math.abs(a.data[i + 1] - b.data[i + 1]) + Math.abs(a.data[i + 2] - b.data[i + 2]) > 15) count++;
  }
  return count;
}
