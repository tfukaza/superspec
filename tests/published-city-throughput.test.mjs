import assert from "node:assert/strict";
import test from "node:test";
import { PerspectiveCamera } from "three";
import { measureCityThroughput } from "./published-city-throughput.mjs";

const view = { name: "pan", eye: [0, 1, 1], endEye: [4, 1, 1], target: [0, 1, 0], endTarget: [4, 1, 0] };

function installProbe(t, failFence = false) {
  const camera = new PerspectiveCamera(55);
  camera.position.set(8, 9, 10);
  camera.lookAt(3, 2, 1);
  const rotation = camera.quaternion.toArray();
  let now = 0, gpuReady = 0, maxPending = 0, deleted = 0;
  const pending = new Set(), poses = [];
  const info = { autoReset: true, render: { calls: 0, triangles: 0 }, reset() {} };
  const gl = {
    SYNC_GPU_COMMANDS_COMPLETE: 10, ALREADY_SIGNALED: 11, TIMEOUT_EXPIRED: 12, WAIT_FAILED: 13, NO_ERROR: 0,
    drawingBufferWidth: 1920, drawingBufferHeight: 1080,
    getExtension: () => null, getParameter: () => "test GPU", isContextLost: () => false, getError: () => 0,
    fenceSync(type, flags) {
      assert.deepEqual([type, flags], [10, 0]);
      gpuReady = Math.max(gpuReady, now) + 5;
      const sync = { at: gpuReady, flushed: false };
      pending.add(sync);
      maxPending = Math.max(maxPending, pending.size);
      return sync;
    },
    flush() { for (const sync of pending) sync.flushed = true; },
    clientWaitSync(sync, flags, timeout) {
      assert.deepEqual([sync.flushed, flags, timeout], [true, 0, 0]);
      if (failFence && poses.length >= 4) return this.WAIT_FAILED;
      if (now >= sync.at) return this.ALREADY_SIGNALED;
      now = sync.at;
      return this.TIMEOUT_EXPIRED;
    },
    deleteSync(sync) { assert.ok(pending.delete(sync)); deleted++; },
  };
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "window");
  t.after(() => {
    if (descriptor) Object.defineProperty(globalThis, "window", descriptor);
    else delete globalThis.window;
  });
  t.mock.method(performance, "now", () => now);
  globalThis.window = { __worldSpecView: {
    camera, renderer: { info, getContext: () => gl }, settle: () => { now += 23; },
    render() {
      now++;
      poses.push(camera.position.x);
      info.render.calls = camera.position.x + 1;
      info.render.triangles = 10 * info.render.calls;
    },
  } };
  return { poses, verify() {
    assert.equal(maxPending, 3);
    assert.equal(deleted, poses.length);
    assert.equal(pending.size, 0);
    assert.equal(info.autoReset, true);
    assert.equal(camera.fov, 55);
    assert.deepEqual(camera.position.toArray(), [8, 9, 10]);
    assert.deepEqual(camera.quaternion.toArray(), rotation);
  } };
}

test("throughput times the full GPU-completed route, excluding startup and bounding outstanding frames", async t => {
  const probe = installProbe(t);
  const run = await measureCityThroughput({ view, frames: 5, warmup: 2 });
  assert.deepEqual(probe.poses, [0, 0, 0, 0, 1, 2, 3, 4]);
  assert.deepEqual(run.samples.map(s => s.triangles), [10, 20, 30, 40, 50]);
  assert.equal(run.calls.median, 3);
  assert.equal(run.cpuMs.mean, 1);
  // First submit costs 1 ms, followed by 5 x 5 ms of serialized GPU work.
  assert.equal(run.elapsedMs, 26);
  assert.equal(run.completedFps, 5000 / 26);
  assert.equal(run.queueDepth, 3);
  probe.verify();
});

test("a failed completion fence rejects the run and releases renderer, camera and fence state", async t => {
  const probe = installProbe(t, true);
  await assert.rejects(measureCityThroughput({ view, frames: 4, warmup: 0 }), /completion fence failed/);
  probe.verify();
});

test("WebGPU counts actual draws and waits for all submitted frames before reporting", async t => {
  installProbe(t);
  const probe = globalThis.window.__worldSpecView;
  let inFlight = 0, peak = 0, completed = 0;
  probe.backend = "webgpu";
  probe.renderer.domElement = { width: 1920, height: 1080 };
  probe.renderer.backend = { device: { adapterInfo: { vendor: "test adapter" }, queue: {
    onSubmittedWorkDone() {
      peak = Math.max(peak, ++inFlight);
      return new Promise(resolve => setImmediate(() => { inFlight--; completed++; resolve(); }));
    },
  } } };
  probe.render = () => { probe.renderer.info.render.drawCalls = 11; };
  const run = await measureCityThroughput({ view, frames: 5 });
  assert.deepEqual([run.backend, run.gpu, run.calls.mean], ["webgpu", "test adapter", 11]);
  assert.deepEqual([peak, inFlight, completed], [3, 0, 6]);
  probe.renderer.backend.device.queue.onSubmittedWorkDone = () => Promise.reject(Error("device lost"));
  await assert.rejects(measureCityThroughput({ view, frames: 5 }), /device lost/);
  assert.equal(probe.renderer.info.autoReset, true);
});
