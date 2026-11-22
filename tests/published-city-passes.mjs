// Serialized by Playwright and executed in the packaged viewer. Keep all helpers inside the function.
export async function measureCityPasses({ view, frames, warmup }) {
  const { renderer, scene, camera, render, settle } = window.__worldSpecView;
  const gl = renderer.getContext(), timer = gl.getExtension("EXT_disjoint_timer_query_webgl2");
  const debug = gl.getExtension("WEBGL_debug_renderer_info");
  const gpu = gl.getParameter(debug ? debug.UNMASKED_RENDERER_WEBGL : gl.RENDERER);
  const names = ["prepare", "mainColor", "shadows", "outlineDepth", "bloom", "composite", "displayAA", "target", "other"];
  // Each block covers the route with two whole-frame controls, one sample of each common
  // pass and open slots for rare shadow updates. Rotate the order between blocks.
  const strata = ["whole", "mainColor", null, "outlineDepth", null, "composite", "whole",
    "displayAA", null, "bloom", null, "target", null, null];
  const samples = [], pending = [], originals = [], materialCaptures = [];
  const autoReset = renderer.info.autoReset;
  const displayLayers = camera.layers.mask;
  let sample, phase, started, startCalls, startTriangles, activeQuery, gpuEnabled;
  let mainSeen = false, disjointSamples = 0, unresolvedGpuSamples = 0;
  let querySegments = 0, resolvedSegments = 0;
  const clock = () => performance.now();
  function hook(owner, key, wrap) {
    const method = owner[key];
    originals.push(() => { owner[key] = method; });
    owner[key] = wrap(method);
  }
  function moveCamera(t) {
    const pos = view.eye.map((v, j) => v + ((view.endEye?.[j] ?? v) - v) * t);
    const target = view.target.map((v, j) => v + ((view.endTarget?.[j] ?? v) - v) * t);
    camera.position.fromArray(pos);
    camera.lookAt(...target);
    camera.updateMatrixWorld();
  }
  function stopCpu() {
    const p = sample.phases[phase], info = renderer.info.render;
    p.cpuMs += clock() - started;
    p.calls += info.calls - startCalls;
    p.triangles += info.triangles - startTriangles;
  }
  function startCpu(name) {
    phase = name;
    startCalls = renderer.info.render.calls;
    startTriangles = renderer.info.render.triangles;
    started = clock();
  }
  function stopGpu() {
    if (activeQuery) gl.endQuery(timer.TIME_ELAPSED_EXT);
    activeQuery = undefined;
  }
  function startGpu(name) {
    if (!gpuEnabled || name !== sample.gpuCategory) return false;
    const query = gl.createQuery();
    if (!query) { sample.validGpu = false; return false; }
    gl.beginQuery(timer.TIME_ELAPSED_EXT, query);
    activeQuery = query;
    pending.push({ query, sample, name });
    sample.segments++;
    querySegments++;
    return true;
  }
  function scope(name, action) {
    const parent = phase;
    // Whole-frame controls run across every CPU subsection with a single query. A
    // measured colour pass only has to suspend its query when a shadow redraw interrupts it.
    const resumeGpu = !!activeQuery && sample.gpuCategory !== "whole" && name !== sample.gpuCategory;
    stopCpu();
    if (resumeGpu) stopGpu();
    startCpu(name);
    const began = !activeQuery && startGpu(name);
    try { return action(); }
    finally {
      if (began) stopGpu();
      stopCpu();
      // The shadow pass can interrupt mainColor. A new segment resumes its timer.
      startCpu(parent === "prepare" && mainSeen ? "other" : parent);
      if (resumeGpu) startGpu(parent);
    }
  }
  function poll() {
    if (!timer) return false;
    const disjoint = gl.getParameter(timer.GPU_DISJOINT_EXT);
    while (pending.length && (disjoint || gl.getQueryParameter(pending[0].query, gl.QUERY_RESULT_AVAILABLE))) {
      const { query, sample: owner, name } = pending.shift();
      if (disjoint) { owner.validGpu = false; disjointSamples++; }
      else {
        const ms = gl.getQueryParameter(query, gl.QUERY_RESULT) / 1e6;
        if (name === "whole") owner.gpuMs = ms;
        else owner.phases[name].gpuMs = (owner.phases[name].gpuMs ?? 0) + ms;
        resolvedSegments++;
      }
      gl.deleteQuery(query);
    }
    return disjoint;
  }
  function classify(root, passCamera) {
    if (root === scene && passCamera === camera) {
      if (passCamera.layers.mask !== displayLayers) return "outlineDepth";
      mainSeen = true;
      return "mainColor";
    }
    const u = root.material?.uniforms;
    if (u?.aoDepth && u?.highlightMap) return "composite";
    if (u?.frame && u?.aoTexel) return "bloom";
    if (u?.tDiffuse && u?.resolution || u?.texel && u?.frame) return "displayAA";
    return "other";
  }
  function installHooks() {
    hook(renderer, "render", original => function(root, passCamera) {
      if (!sample) return original.call(this, root, passCamera);
      return scope(classify(root, passCamera), () => original.call(this, root, passCamera));
    });
    hook(renderer.shadowMap, "render", original => function(lights, root, passCamera) {
      if (sample && phase === "mainColor" && this.enabled && (this.autoUpdate || this.needsUpdate)) {
        const dirty = lights.filter(l => l.shadow && (l.shadow.autoUpdate || l.shadow.needsUpdate));
        if (dirty.length) {
          sample.shadowLights.push(...dirty.map(l => l.name || l.type));
          if (sample.gpuCategory !== "whole" && !sample.segments) sample.gpuCategory = "shadows";
          return scope("shadows", () => original.call(this, lights, root, passCamera));
        }
      }
      return original.call(this, lights, root, passCamera);
    });
    hook(renderer, "setRenderTarget", original => function(...args) {
      if (!sample || (phase !== "prepare" && phase !== "other") || args[0] === this.getRenderTarget())
        return original.apply(this, args);
      return scope("target", () => original.apply(this, args));
    });
    hook(renderer, "copyFramebufferToTexture", original => function(...args) {
      if (!sample) return original.apply(this, args);
      return scope("displayAA", () => original.apply(this, args));
    });
  }
  // Count actual submitted colour draws in separate frames. renderBufferDirect also runs for
  // shadow and excluded-depth meshes, so explicitly require the original camera and layers.
  function countMaterials(t) {
    const rows = new Map(), original = renderer.renderBufferDirect;
    let inColor = false;
    const originalRender = renderer.render;
    renderer.render = function(root, passCamera) {
      const previous = inColor;
      inColor = root === scene && passCamera === camera && camera.layers.mask === displayLayers;
      try { return originalRender.call(this, root, passCamera); }
      finally { inColor = previous; }
    };
    renderer.renderBufferDirect = function(passCamera, passScene, geometry, material, object, group) {
      if (!inColor || passCamera !== camera) return original.apply(this, arguments);
      const { calls, triangles } = this.info.render;
      const value = original.apply(this, arguments);
      const draws = this.info.render.calls - calls;
      if (draws) {
        const key = material.name || material.type;
        const row = rows.get(key) ?? { material: key, calls: 0, triangles: 0 };
        row.calls += draws;
        row.triangles += this.info.render.triangles - triangles;
        rows.set(key, row);
      }
      return value;
    };
    try { moveCamera(t); render(); }
    finally { renderer.render = originalRender; renderer.renderBufferDirect = original; }
    return { position: t, materials: [...rows.values()].sort((a, b) => b.calls - a.calls) };
  }
  const stats = values => {
    const v = values.filter(Number.isFinite).sort((a, b) => a - b);
    if (!v.length) return null;
    return { mean: v.reduce((a, b) => a + b, 0) / v.length, median: v[Math.floor(v.length / 2)],
      p95: v[Math.ceil(.95 * v.length) - 1], max: v.at(-1) };
  };
  try {
    camera.fov = 70; camera.updateProjectionMatrix();
    renderer.info.autoReset = false;
    installHooks();
    moveCamera(0); render(); await settle?.();
    let last;
    for (let i = -warmup; i <= frames; i++) {
      const timestamp = await new Promise(requestAnimationFrame);
      if (i > 0) samples.at(-1).frameMs = timestamp - last;
      last = timestamp;
      const disjoint = poll();
      if (i === frames) break;
      moveCamera(Math.max(0, i) / (frames - 1));
      if (i < 0) { render(); continue; }
      renderer.info.reset();
      gpuEnabled = !!timer && !disjoint;
      sample = { frame: i, validGpu: gpuEnabled, gpuMs: null, segments: 0,
        gpuCategory: strata[(i + Math.floor(i / strata.length) * 5) % strata.length],
        phases: Object.fromEntries(names.map(name => [name, { cpuMs: 0, gpuMs: null, calls: 0, triangles: 0 }])), shadowLights: [] };
      const beforePrograms = renderer.info.programs?.length ?? 0, beforeTextures = renderer.info.memory.textures;
      mainSeen = false;
      const frameStart = clock();
      startCpu("prepare");
      startGpu("whole");
      try { render(); }
      finally { stopGpu(); stopCpu(); }
      sample.cpuMs = clock() - frameStart;
      sample.calls = renderer.info.render.calls; sample.triangles = renderer.info.render.triangles;
      sample.programDelta = (renderer.info.programs?.length ?? 0) - beforePrograms;
      sample.textureDelta = renderer.info.memory.textures - beforeTextures;
      samples.push(sample);
      sample = undefined;
    }
    gl.flush();
    const deadline = clock() + 10000;
    while (pending.length && clock() < deadline) { await new Promise(requestAnimationFrame); poll(); }
    unresolvedGpuSamples = pending.length;
    for (const job of pending) job.sample.validGpu = false;
    for (const row of samples) if (!row.validGpu) {
      for (const p of Object.values(row.phases)) p.gpuMs = null;
      row.gpuMs = null;
    }
    for (const t of view.endEye ? [.25, .75] : [0]) materialCaptures.push(countMaterials(t));
    moveCamera(view.endEye ? 1 : 0); render();
    const phases = Object.fromEntries(names.map(name => [name, Object.fromEntries(["cpuMs", "gpuMs", "calls", "triangles"]
      .map(key => [key, stats(samples.map(s => s.phases[name][key]))]))]));
    const phaseGpuSamples = Object.fromEntries(names.map(name =>
      [name, samples.filter(s => Number.isFinite(s.phases[name].gpuMs)).length]));
    const totals = Object.fromEntries(["cpuMs", "gpuMs", "frameMs", "calls", "triangles"].map(key => [key, stats(samples.map(s => s[key]))]));
    if (gl.getError() !== gl.NO_ERROR) throw Error("WebGL error after pass profiling " + view.name);
    return { name: view.name, moving: !!view.endEye, gpu, width: renderer.domElement.width, height: renderer.domElement.height,
      frames: samples.length, ...totals, phases, phaseGpuSamples, gpuTimerAvailable: !!timer,
      gpuSamples: samples.filter(s => Number.isFinite(s.gpuMs)).length,
      disjointSamples, unresolvedGpuSamples, querySegments, resolvedSegments,
      shadowRedrawFrames: samples.filter(s => s.shadowLights.length).length, materialCaptures, samples,
      instrumentation: { cpu: "Exclusive wall time. prepare covers WorldSpec visibility/LOD/lighting and setup before the first colour draw. other includes inter-pass JS.",
        gpu: "Sparse, one category per frame. gpuMs is a separately sampled whole-frame control. Missing phase measurements are null. Sampled GPU pass ranges include their MSAA resolve and mip generation. Driver waits and starvation may appear in elapsed time.",
        shadows: "Dirty light projections inside the Three shadow pass. Shadow sampling cost is included in mainColor. Idle shadow checks remain in mainColor.",
        displayAA: "Includes framebuffer copy and screen draw.", materials: "Submitted colour draw counts by named material at additional unmeasured positions. These counts do not measure per-material time." } };
  } finally {
    sample = undefined;
    for (const restore of originals.reverse()) restore();
    for (const job of pending) gl.deleteQuery(job.query);
    renderer.info.autoReset = autoReset;
  }
}
