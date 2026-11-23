import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { measure, options } from "./published-city-bench.mjs";
import { calibrateView } from "./published-city-calibration.mjs";
import { lightingOverridesQuery, resolveRenderProfile, writeRenderProfile } from "../adapters/three/src/renderProfile.ts";
import { graphicsPackUrl } from "../adapters/three/src/graphicsSettings.ts";

test("mobile profiles lower runtime budgets and request their own map packs", () => {
  const medium = resolveRenderProfile();
  const low = resolveRenderProfile({ version: 1, preset: "low" });
  const tiny = resolveRenderProfile({ version: 1, preset: "very-low" });
  // Every preset renders at the display's resolution. The lighter tiers lower geometry and material budgets.
  assert.deepEqual([tiny, low, medium].map(tier => tier.settings.renderScale), [1, 1, 1]);
  assert.ok(tiny.settings.geometryLodBias < low.settings.geometryLodBias && low.settings.geometryLodBias < medium.settings.geometryLodBias);
  // Rebuilding fixture shadows redraws the street in one frame, so the lighter tiers go without them.
  assert.deepEqual([tiny, low].map(tier => tier.settings.fixtureShadowMapSize), [0, 0]);
  assert.ok(medium.settings.fixtureShadowMapSize !== 0);
  assert.equal(tiny.diagnostics.materials.bounce, medium.diagnostics.materials.bounce);
  assert.equal(tiny.diagnostics.materials.reflectionProbes, false);
  assert.equal(tiny.diagnostics.loading.warmup, false);
  const overrides = resolveRenderProfile({ version: 1, preset: "very-low", overrides: { materials: "high" },
    diagnostics: { lighting: { ambientOcclusion: "on" } } });
  assert.equal(overrides.diagnostics.materials.reflectionProbes, medium.diagnostics.materials.reflectionProbes);
  assert.equal(overrides.diagnostics.lighting.ambientOcclusion, "on");
  assert.equal(graphicsPackUrl(new URL("https://example.invalid/maps/test.glb?v=2"), tiny.variant).pathname,
    "/maps/test.very-low.glb");
  assert.equal(graphicsPackUrl(new URL("https://example.invalid/maps/test.very-low.glb"), "low"), undefined);
});

test("the CLI defaults to completed throughput and retains paced diagnostics", async t => {
  const dir = await mkdtemp(join(tmpdir(), "superspec-city-bench-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const entry = join(dir, "scene.glb");
  const json = Buffer.from('{"asset":{"version":"2.0"}} ');
  const header = Buffer.alloc(20);
  header.write("glTF"); header.writeUInt32LE(2, 4); header.writeUInt32LE(20 + json.length, 8);
  header.writeUInt32LE(json.length, 12); header.write("JSON", 16);
  await Promise.all([writeFile(entry, Buffer.concat([header, json])), writeFile(join(dir, "native.html"), "")]);
  const args = [entry, "--viewer", dir];
  assert.equal((await options(args)).timingMode, "throughput");
  assert.equal((await options(args)).requestedRenderer, "auto");
  assert.equal((await options([...args, "--renderer", "webgpu"])).viewerQuery.get("renderer"), "webgpu");
  assert.equal((await options([...args, "--passes"])).viewerQuery.get("renderer"), "webgl");
  await assert.rejects(options([...args, "--renderer", "webgpu", "--calibrate"]), /require --renderer webgl/);
  await assert.rejects(options([...args, "--renderer", "webgl", "--viewer-query", "renderer=webgpu"]), /conflicts/);
  assert.equal((await options([...args, "--throughput"])).timingMode, "throughput");
  for (const diagnostic of [["--paced"], ["--gpu-sample-every", "0"], ["--gpu-sample-every", "12"],
    ["--passes"], ["--calibrate"], ["--first-traversal"]]) {
    assert.equal((await options([...args, ...diagnostic])).timingMode, "paced");
    await assert.rejects(options([...args, "--throughput", ...diagnostic]), /Run --throughput separately/);
  }
  const profileFile = join(dir, "graphics.json");
  const profile = { version: 1, preset: "very-high", overrides: { renderScale: 1 }, diagnostics: { lighting: { shadowCache: false } } };
  await writeFile(profileFile, JSON.stringify(profile));
  const configured = await options([...args, "--profile", profileFile, "--graphics", "high"]);
  assert.deepEqual(JSON.parse(configured.viewerQuery.get("profile")), profile);
  assert.equal(resolveRenderProfile(profile, configured.viewerQuery).preset, "high");
  await assert.rejects(options([...args, "--graphics", "ultra"]), /Unknown graphics preset/);
});

test("a render profile preserves independent overrides and converts legacy flags to a shareable record", () => {
  const query = new URLSearchParams("gltf=/town.glb&camera=1,2,3&gpuprobe&paintedspots=off&citycull=off&shadowstyle=none&shadowblur=.06");
  const named = resolveRenderProfile();
  assert.deepEqual(resolveRenderProfile(named.profile), named);
  assert.deepEqual([named.preset, named.settings.renderScale, named.settings.geometryLodBias, named.settings.materialLodBias], ["medium", 1, 1, 1]);
  const input = { version: 1, preset: "very-high", overrides: { renderScale: 0.8, materials: "medium" },
    diagnostics: { lighting: { paintedSpots: true, ambientOcclusion: "off" }, effects: { shopArt: "palette" } } };
  const active = resolveRenderProfile(input, query);
  assert.deepEqual([active.settings.renderScale, active.settings.geometryLodBias, active.settings.materialLodBias], [0.8, 1.4, 1]);
  assert.equal(active.diagnostics.lighting.paintedSpots, false);
  assert.equal(active.diagnostics.visibility.cityCulling, false);
  assert.equal(active.diagnostics.instrumentation.gpuProbe, true);
  assert.equal(active.diagnostics.effects.shopArt, "palette");
  const lighting = lightingOverridesQuery(active.diagnostics);
  assert.equal(lighting.get("ao"), "off");
  assert.equal(lighting.get("shadowstyle"), "plain");
  assert.equal(lighting.get("shadowblur"), "0.06");
  writeRenderProfile(query, active.profile);
  assert.deepEqual([...query.keys()], ["gltf", "camera", "profile"]);
  assert.deepEqual(resolveRenderProfile(JSON.parse(query.get("profile"))), active);
  assert.throws(() => resolveRenderProfile({ version: 3 }), /version/);
  assert.throws(() => resolveRenderProfile({ version: 1, overrides: { renderScale: 0.1 } }), /overrides.renderScale/);
  assert.throws(() => resolveRenderProfile({ version: 1, diagnostics: { visibility: { culling: false } } }), /diagnostics.visibility.culling/);
  assert.throws(() => resolveRenderProfile({ version: 1, diagnostics: { lighting: { paintedShadows: "on" } } }), /diagnostics.lighting.paintedShadows/);
});

test("sampling changes only which poses receive GPU queries and preserves the final frame interval", async () => {
  const ext = { GPU_DISJOINT_EXT: 1001, TIME_ELAPSED_EXT: 1002 };
  const events = [];
  let pose = 0, time = 0;
  const gl = {
    QUERY_RESULT_AVAILABLE: 2001, QUERY_RESULT: 2002, NO_ERROR: 0,
    getExtension: name => name.startsWith("EXT_disjoint") ? ext : null,
    getParameter(param) {
      if (param === ext.GPU_DISJOINT_EXT) events.push("poll");
      return param === ext.GPU_DISJOINT_EXT ? false : "test";
    },
    createQuery: () => ({}), beginQuery: () => events.push(pose), endQuery() {},
    getQueryParameter: (_q, what) => what === gl.QUERY_RESULT_AVAILABLE ? true : 2e6,
    deleteQuery() {}, getError: () => 0, flush() {},
  };
  const renderer = { getContext: () => gl, info: { autoReset: true, reset() {}, memory: { textures: 0 },
    programs: [], render: { calls: 0, triangles: 0 } }, domElement: { width: 1920, height: 1080 } };
  const camera = { position: { fromArray: ([x]) => { pose = x; } }, lookAt() {}, updateMatrixWorld() {}, updateProjectionMatrix() {} };
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  const originalRaf = Object.getOwnPropertyDescriptor(globalThis, "requestAnimationFrame");
  try {
    globalThis.window = { __worldSpecView: { renderer, camera, render() { renderer.info.render.triangles = pose + 1; } } };
    globalThis.requestAnimationFrame = done => done(time += pose === 4 ? 40 : 10);
    const view = { name: "pan", eye: [0,0,0], endEye: [4,0,0], target: [1,1,0], endTarget: [5,1,0] };
    for (const stride of [undefined, 0, 2, 1]) {
      events.length = 0;
      const run = await measure({ view, frames: 5, warmup: 2, gpuSampleEvery: stride });
      assert.deepEqual(run.samples.map(s => s.triangles), [1,2,3,4,5]);
      assert.deepEqual(run.samples.map(s => s.frameMs), [10,10,10,10,40]);
      const selected = [0,1,2,3,4].filter(i => stride > 0 && i % stride === 0);
      assert.deepEqual(events.filter(Number.isInteger), selected);
      if (!stride) assert.equal(events.length, 0);
      assert.equal(run.gpuSamples, selected.length);
      assert.equal(run.scheduledGpuSamples, selected.length);
      assert.deepEqual(run.samples.map(s => s.gpuMs), [0,1,2,3,4].map(i => selected.includes(i) ? 2 : null));
      assert.equal(renderer.info.autoReset, true);
    }
  } finally {
    if (originalWindow) Object.defineProperty(globalThis, "window", originalWindow); else delete globalThis.window;
    if (originalRaf) Object.defineProperty(globalThis, "requestAnimationFrame", originalRaf); else delete globalThis.requestAnimationFrame;
  }
});

test("calibration pairs complete routes, gives both controls identical work and balances every run position", async () => {
  const strides = [];
  const calibration = await calibrateView("pan", 1, async stride => {
    strides.push(stride);
    return { gpuTimerAvailable: true, gpuSamples: stride, scheduledGpuSamples: stride, disjointSamples: 0,
      unresolvedGpuSamples: 0, frames: 1, frameMs: { mean: 10 + stride },
      cpuMs: { median: 5 }, samples: [{ calls: 2, triangles: 6 }], framesOver25ms: 0 };
  });
  assert.deepEqual(strides.slice(0, 2), [0,1]);
  const rounds = calibration.rounds;
  assert.equal(new Set(rounds.map(r => r.sequence.join())).size, 6);
  for (let i = 0; i < rounds.length; i++) {
    assert.deepEqual(strides.slice(2 + 3*i, 5 + 3*i), rounds[i].sequence.map(label => Number(label === "timed")));
    assert.equal(rounds[i].repeatMinusControl.meanFrameMs, 0);
    assert.equal(rounds[i].timedMinusControl.meanFrameMs, 1);
  }
  assert.equal(calibration.totals.control.deliveredFps, 100);
  assert.deepEqual(calibration.incompleteGpuRounds, []);
  assert.equal(calibration.totals.timed.scheduledGpuSamples, 6);
  assert.equal(calibration.totals.timed.deliveredFps, 1000/11);
});

test("calibration flags measured rounds missing GPU results after a successful warmup", async () => {
  let timedRuns = 0;
  const calibration = await calibrateView("pan", 1, async stride => {
    const dropped = stride && ++timedRuns === 2 ? 1 : 0;
    return { gpuTimerAvailable: true, gpuSamples: stride - dropped, scheduledGpuSamples: stride,
      disjointSamples: dropped, unresolvedGpuSamples: 0, frames: 1, frameMs: { mean: 10 },
      cpuMs: { median: 5 }, samples: [{ calls: 2, triangles: 6 }], framesOver25ms: 0 };
  });
  assert.deepEqual(calibration.incompleteGpuRounds, [{ round: 1, scheduled: 1, valid: 0, disjoint: 1, unresolved: 0 }]);
  assert.equal(calibration.totals.timed.disjointGpuSamples, 1);
});
