// Playwright serializes this function into the published viewer page.
export async function measureCityThroughput({ view, frames, warmup = 0 }) {
  if (!Number.isSafeInteger(frames) || frames < 1 || !Number.isSafeInteger(warmup) || warmup < 0)
    throw Error("Throughput requires a positive frame count and a nonnegative warmup.");
  const probe = window.__worldSpecView;
  const { renderer, camera, render, settle, textureStats, declaredLods, backend = "webgl" } = probe;
  const webgpu = backend === "webgpu";
  const device = webgpu && renderer.backend.device;
  const gl = webgpu ? undefined : renderer.getContext();
  if (webgpu ? !device?.queue?.onSubmittedWorkDone : typeof gl.fenceSync !== "function")
    throw Error("The renderer does not expose GPU submission completion.");
  const debug = gl?.getExtension("WEBGL_debug_renderer_info");
  const gpu = webgpu ? (device.adapterInfo?.description || device.adapterInfo?.vendor || "WebGPU adapter") :
    gl.getParameter(debug ? debug.UNMASKED_RENDERER_WEBGL : gl.RENDERER);
  const bufferWidth = () => gl?.drawingBufferWidth ?? renderer.domElement.width;
  const bufferHeight = () => gl?.drawingBufferHeight ?? renderer.domElement.height;
  const width = bufferWidth(), height = bufferHeight();
  const info = renderer.info;
  const autoReset = info.autoReset, oldFov = camera.fov;
  const oldPosition = camera.position.clone(), oldRotation = camera.quaternion.clone();
  const channel = new MessageChannel();
  const nextTask = () => new Promise(resolve => {
    channel.port1.onmessage = resolve;
    channel.port2.postMessage(null);
  });
  const queueDepth = 3, pending = [];
  function fence() {
    if (webgpu) {
      pending.push(device.queue.onSubmittedWorkDone().then(() => null, error => error ?? Error("WebGPU queue failed.")));
      return;
    }
    const sync = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
    if (!sync) throw Error("WebGL could not create a completion fence for " + view.name);
    pending.push(sync);
    gl.flush();
  }
  async function waitOldest() {
    if (webgpu) {
      let timer;
      try {
        const timeout = new Promise(resolve => { timer = setTimeout(() => resolve(Error("WebGPU queue completion timed out.")), 30000); });
        const error = await Promise.race([pending.shift(), timeout]);
        if (error) throw error;
        if (window.__worldSpecView !== probe) throw Error("The WebGPU device changed during the benchmark.");
      } finally { clearTimeout(timer); }
      return;
    }
    const sync = pending[0], deadline = performance.now() + 30000;
    for (;;) {
      const status = gl.clientWaitSync(sync, 0, 0);
      if (status === gl.ALREADY_SIGNALED || status === gl.CONDITION_SATISFIED) break;
      if (status !== gl.TIMEOUT_EXPIRED || gl.isContextLost()) throw Error("WebGL completion fence failed for " + view.name);
      if (performance.now() >= deadline) throw Error("Timed out waiting for WebGL completion for " + view.name);
      // WebGL sync status can advance only after control returns to the event loop.
      await nextTask();
    }
    gl.deleteSync(sync);
    pending.shift();
  }
  function setPose(t) {
    const eye = view.eye.map((v, axis) => v + ((view.endEye?.[axis] ?? v) - v) * t);
    const target = view.target.map((v, axis) => v + ((view.endTarget?.[axis] ?? v) - v) * t);
    camera.position.fromArray(eye);
    camera.lookAt(...target);
    camera.updateMatrixWorld();
  }
  try {
    info.autoReset = false;
    camera.fov = 70;
    camera.updateProjectionMatrix();
    setPose(0);
    render();
    await settle?.();
    fence();
    for (let i = 0; i < warmup; i++) {
      if (pending.length === queueDepth) await waitOldest();
      render();
      fence();
    }
    while (pending.length) await waitOldest();

    const samples = [];
    const start = performance.now();
    for (let i = 0; i < frames; i++) {
      if (pending.length === queueDepth) await waitOldest();
      setPose(frames > 1 ? i / (frames - 1) : 0);
      info.reset();
      const cpuStart = performance.now();
      render();
      const cpuMs = performance.now() - cpuStart;
      fence();
      samples.push({ cpuMs, calls: webgpu ? info.render.drawCalls : info.render.calls, triangles: info.render.triangles });
    }
    while (pending.length) await waitOldest();
    const elapsedMs = performance.now() - start;
    if (gl && (gl.isContextLost() || gl.getError() !== gl.NO_ERROR)) throw Error("WebGL error measuring throughput for " + view.name);
    if (width !== bufferWidth() || height !== bufferHeight()) throw Error("Drawing-buffer size changed during throughput measurement.");
    function summary(key) {
      const values = samples.map(sample => sample[key]).sort((a, b) => a - b);
      return { median: values[Math.floor(frames / 2)], p95: values[Math.ceil(frames * .95) - 1],
        max: values.at(-1), mean: values.reduce((sum, value) => sum + value, 0) / frames };
    }
    return { name: view.name, moving: !!view.endEye, width, height, gpu, backend, frames, warmup, queueDepth,
      elapsedMs, completedFps: 1000 * frames / elapsedMs, cpuMs: summary("cpuMs"),
      calls: summary("calls"), triangles: summary("triangles"), textures: info.memory?.textures,
      textureResidency: textureStats?.(), declaredLods: declaredLods?.(), samples };
  } finally {
    if (gl) for (const sync of pending) gl.deleteSync(sync);
    channel.port1.close();
    channel.port2.close();
    info.autoReset = autoReset;
    camera.position.copy(oldPosition);
    camera.quaternion.copy(oldRotation);
    camera.fov = oldFov;
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();
  }
}
