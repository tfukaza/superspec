/**
 * Frame cost of the Studio scene as it stands in a page (`window.__scene`), for tests/item-facade-browser.mjs (FRAMES=1).
 *
 * `measureFrames(page, { frames })` returns, for the current view:
 * - steady: the time-driven mode's own account (scene.ts `setContinuous`), over windows of about half a second until at
 *   least `frames` frames were drawn: the page's milliseconds to issue a frame (mean of the windows) and the GPU's
 *   milliseconds for one (timer queries round the whole frame; the median of the windows' medians), the interval between
 *   frames (the display's refresh caps it), draw calls and triangles;
 * - passes: one frame drawn with every top-level pass instrumented: draw calls and triangles by pass (the sun's shadow
 *   map, the scene into the multisampled target, each occlusion pass with its depth/normal draw and its foliage mask,
 *   the contour lines, the output pass; or the scene straight to the canvas when the occlusion is off), and in the
 *   scene's own pass by category (the first part of the drawn mesh's name, foliage, and whether it is transparent);
 * - timing: the same frame drawn `syncFrames` times with the GPU finished before and after each pass (gl.finish), the
 *   median milliseconds per pass. Finishing stalls the pipeline, so these sum to more than the steady GPU time; they say
 *   where the time goes, not what a frame costs. The shadow map is timed both as the frame leaves it (it is drawn only
 *   when something asks for it) and forced to redraw;
 * - memory: the renderer's count of textures and geometries, and the bake's maps on screen and pass graph (MiB).
 */
export async function measureFrames(page, { frames = 60, syncFrames = 15 } = {}) {
  // Steady state: the time-driven mode, its windows' accounts read until enough frames were drawn.
  await page.evaluate(() => { delete document.querySelector("#viewport").dataset.frameBudget; window.__scene.setContinuous(true); });
  const windows = [];
  // The first window warms up (programs compiled, shadow map drawn): left out.
  for (let drawn = 0, k = 0; drawn < frames || k < 3; k++) {
    await page.evaluate(() => { delete document.querySelector("#viewport").dataset.frameBudget; });
    await page.waitForFunction(() => document.querySelector("#viewport").dataset.frameBudget !== undefined, {}, { timeout: 60000 });
    const budget = JSON.parse(await page.evaluate(() => document.querySelector("#viewport").dataset.frameBudget));
    if (k > 0) { windows.push(budget); drawn += budget.frames; }
  }
  await page.evaluate(() => window.__scene.setContinuous(false));
  const median = values => { const sorted = values.filter(v => v !== null).sort((a, b) => a - b); return sorted.length ? sorted[sorted.length >> 1] : null; };
  const framesDrawn = windows.reduce((sum, w) => sum + w.frames, 0);
  const steady = {
    frames: framesDrawn,
    gpuMs: median(windows.map(w => w.gpuMs)),
    cpuMs: +(windows.reduce((sum, w) => sum + w.cpuMs * w.frames, 0) / framesDrawn).toFixed(2),
    intervalMs: +(windows.reduce((sum, w) => sum + w.intervalMs * w.frames, 0) / framesDrawn).toFixed(2),
    calls: windows.at(-1).calls, triangles: windows.at(-1).triangles,
  };
  const detail = await page.evaluate(syncFrames => {
    const s = window.__scene, renderer = s.renderer, gl = renderer.getContext(), info = renderer.info.render;
    const ao = s.lights?.ambientOcclusion;
    const shadowMap = renderer.shadowMap;
    const top = new Set([...s.models.children]);
    const rootOf = object => { let o = object; while (o && !top.has(o) && o.parent) o = o.parent; return o ?? object; };
    // The pass being drawn: set by the wrapped composer passes and the shadow map; refined by what renderer.render draws.
    let pass = "scene", passes = {}, timing = null;
    const entry = name => passes[name] ??= { calls: 0, triangles: 0, categories: {} };
    const originalDraw = renderer.renderBufferDirect.bind(renderer);
    renderer.renderBufferDirect = (camera, scene, geometry, material, object, group) => {
      const calls = info.calls, triangles = info.triangles;
      originalDraw(camera, scene, geometry, material, object, group);
      const e = entry(pass), drawnCalls = info.calls - calls, drawnTriangles = info.triangles - triangles;
      e.calls += drawnCalls; e.triangles += drawnTriangles;
      const root = rootOf(object), foliage = object.userData?.foliage || root.userData?.foliage;
      const head = (root.name || object.name || "(unnamed)").split("/")[0];
      const category = foliage ? `foliage:${object.name?.startsWith("anime/") ? "illustrated" : object.name?.startsWith("painted/") ? "painted" : (foliage.family ?? "parts")}`
        : `${head}${material?.transparent ? " (transparent)" : ""}`;
      const c = e.categories[category] ??= { calls: 0, triangles: 0 };
      c.calls += drawnCalls; c.triangles += drawnTriangles;
    };
    const finish = () => { if (timing) gl.finish(); return performance.now(); };
    const timed = (name, run) => {
      const started = finish();
      const result = run();
      const ms = finish() - started;
      if (timing) (timing[name] ??= []).push(ms);
      return result;
    };
    // The shadow map draws inside the scene's pass (WebGLShadowMap.render): its own pass here, timed alone.
    const originalShadow = shadowMap.render.bind(shadowMap);
    let shadowDrawn = 0, shadowMs = 0;
    shadowMap.render = (...args) => {
      if (!shadowMap.enabled || (!shadowMap.autoUpdate && !shadowMap.needsUpdate)) return originalShadow(...args);
      const outer = pass; pass = "shadow map"; shadowDrawn++;
      const started = finish();
      try { return originalShadow(...args); } finally { const ms = finish() - started; shadowMs += ms; if (timing) (timing["shadow map"] ??= []).push(ms); pass = outer; }
    };
    // Inside an occlusion pass: its depth/normal draw (the scene with an override material), its foliage mask (the
    // scene's leaves into the mask target) and its full-screen passes.
    const originalRender = renderer.render.bind(renderer);
    renderer.render = (scene, camera) => {
      const outer = pass;
      if (outer.startsWith("ao") || outer.startsWith("contact")) {
        pass = scene === s.scene ? `${outer.split(":")[0]}: ${scene.overrideMaterial ? "depth/normal" : "foliage mask"}` : `${outer.split(":")[0]}: full-screen`;
      }
      try { return originalRender(scene, camera); } finally { pass = outer; }
    };
    const wrapped = [];
    if (ao) {
      // By instance: the bundle's class names are minified.
      for (const [index, p] of ao.composer.passes.entries()) {
        const name = p === ao.pass ? "ao" : p === ao.contact ? "contact ao" : p === ao.contours ? "contours" : p === ao.output ? "output" : index === 0 ? "scene" : `pass ${index}`;
        const original = p.render;
        p.render = function (...args) {
          if (!p.enabled) return original.apply(this, args);
          const outer = pass; pass = name;
          // The shadow map's time is inside the scene pass's: taken out after.
          const before = shadowMs;
          try { return timed(name, () => original.apply(this, args)); } finally {
            pass = outer;
            if (timing && name === "scene") timing.scene[timing.scene.length - 1] -= shadowMs - before;
          }
        };
        wrapped.push([p, original]);
      }
    }
    const frame = () => { if (ao) s.measureFrame(); else timed("scene", () => s.measureFrame()); };
    try {
      // Counts: one frame as the view draws it, and one with the shadow map redrawn.
      passes = {}; shadowDrawn = 0; frame();
      const counted = passes, shadowInFrame = shadowDrawn;
      passes = {}; shadowMap.needsUpdate = true; frame();
      const shadowRedraw = passes["shadow map"] ?? null;
      // Timing: frames with every pass finished, as the view draws them, then with the shadow map forced each frame.
      timing = {};
      for (let k = 0; k < syncFrames; k++) { passes = {}; frame(); }
      const asDrawn = timing; timing = {};
      for (let k = 0; k < syncFrames; k++) { passes = {}; shadowMap.needsUpdate = true; frame(); }
      const forced = timing; timing = null;
      const med = values => { const v = [...values].sort((a, b) => a - b); return +v[v.length >> 1].toFixed(2); };
      const summary = t => Object.fromEntries(Object.entries(t).map(([k, v]) => [k, med(v)]));
      return { counted, shadowInFrame, shadowRedraw, msFinished: summary(asDrawn), shadowForcedMs: forced["shadow map"] ? med(forced["shadow map"]) : null,
        canvas: [renderer.domElement.width, renderer.domElement.height], ao: !!ao && ao.pass.enabled, contours: !!ao?.contours?.enabled,
        memory: { textures: renderer.info.memory.textures, geometries: renderer.info.memory.geometries, programs: renderer.info.programs?.length ?? null,
          ...(bake => bake ? { mapsMiB: +(bake.bytes / 1048576).toFixed(1), graphMiB: +((bake.graph?.bytes ?? 0) / 1048576).toFixed(1) } : {})(JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")) } };
    } finally {
      renderer.renderBufferDirect = originalDraw; shadowMap.render = originalShadow; renderer.render = originalRender;
      for (const [p, original] of wrapped) p.render = original;
    }
  }, syncFrames);
  return { steady, ...detail };
}
