/** Diagnostic only. Capture each unique published mip-zero texture once per settled scene;
 * keep compressed reference pixels in the page so Playwright never serializes a city of pixels. */
export async function beginMapTrace(page) {
  await page.evaluate(async () => {
    const scene = window.__scene, renderer = scene.renderer, gl = renderer.getContext();
    const reference = new Map(), contents = new Map(), work = new WeakMap();
    const digest = async data => Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", data)),
      byte => byte.toString(16).padStart(2, "0")).join("");
    const collectLayout = async layout => {
      if (layout.records.length !== layout.triangles * 38) throw Error("Unknown triangle chart record layout");
      const placement = new Float32Array(14 * layout.triangles), frames = new Float32Array(8 * layout.triangles);
      let latticeTriangles = 0;
      for (let i = 0; i < layout.triangles; i++) {
        // Rectangle, quarter turn, sampling dimensions, chart role and three signed atlas UV pairs.
        placement.set(layout.records.subarray(i * 38, i * 38 + 8), i * 14);
        frames.set(placement.subarray(i * 14, i * 14 + 8), i * 8);
        if (layout.records[i * 38 + 7] === 3) latticeTriangles++;
        placement.set(layout.records.subarray(i * 38 + 32, (i + 1) * 38), i * 14 + 8);
      }
      const [records, chart, grid, frame] = await Promise.all([layout.records, placement, layout.surface, frames].map(digest));
      return { width: layout.width, height: layout.height, gutter: layout.gutter, triangles: layout.triangles,
        density: layout.density, records, chart, grid, frame, latticeTriangles };
    };
    const read = node => {
      const target = node.target, { width, height } = target, original = renderer.getRenderTarget();
      try {
        renderer.setRenderTarget(target);
        const type = gl.getParameter(gl.IMPLEMENTATION_COLOR_READ_TYPE);
        if (gl.getParameter(gl.IMPLEMENTATION_COLOR_READ_FORMAT) !== gl.RGBA) throw Error("Expected RGBA readback");
        const ArrayType = type === gl.FLOAT ? Float32Array : type === gl.HALF_FLOAT ? Uint16Array : type === gl.UNSIGNED_BYTE ? Uint8Array : undefined;
        if (!ArrayType) throw Error("Unsupported GPU readback type " + type);
        const pixels = new ArrayType(width * height * 4);
        gl.readPixels(0, 0, width, height, gl.RGBA, type, pixels);
        if (gl.getError() !== gl.NO_ERROR) throw Error("Failed GPU readback");
        const tex = target.texture;
        const format = JSON.stringify([width, height, type, tex.format, tex.type, tex.internalFormat, tex.colorSpace,
          tex.flipY, tex.wrapS, tex.wrapT, tex.minFilter, tex.magFilter, tex.generateMipmaps]);
        return { width, height, format, pixels };
      } finally { renderer.setRenderTarget(original); }
    };
    const half = n => {
      const sign = n & 32768 ? -1 : 1, exponent = (n >> 10) & 31, mantissa = n & 1023;
      return exponent === 31 ? (mantissa ? NaN : sign * Infinity)
        : exponent ? sign * 2 ** (exponent - 15) * (1 + mantissa / 1024) : sign * mantissa * 2 ** -24;
    };
    const compare = async (old, pixels) => {
      const raw = await new Response(contents.get(old).stream().pipeThrough(new DecompressionStream("deflate-raw"))).arrayBuffer();
      const before = new pixels.constructor(raw);
      if (before.length !== pixels.length) throw Error("Cannot compare unequal pixel buffers");
      const decode = pixels instanceof Uint16Array ? half : pixels instanceof Uint8Array ? n => n / 255 : n => n;
      const oldBits = pixels instanceof Float32Array ? new Uint32Array(raw) : before;
      const newBits = pixels instanceof Float32Array ? new Uint32Array(pixels.buffer, pixels.byteOffset, pixels.length) : pixels;
      let storageChanged = 0, changed = 0, over1 = 0, over8 = 0, peak = 0, nonFinite = 0;
      for (let i = 0; i < pixels.length; i += 4) {
        let delta = 0, differentBits = false;
        for (let k = 0; k < 4; k++) {
          if (oldBits[i+k] === newBits[i+k]) continue;
          differentBits = true;
          const d = Math.abs(decode(before[i+k]) - decode(pixels[i+k]));
          if (Number.isFinite(d)) delta = Math.max(delta, d); else nonFinite++;
        }
        if (differentBits) storageChanged++;
        if (delta) changed++;
        if (delta > 1 / 255 + 1e-12) over1++;
        if (delta > 8 / 255 + 1e-12) over8++;
        peak = Math.max(peak, delta);
      }
      return { storageChanged, changed, over1, over8, peak, nonFinite };
    };
    async function snapshot(initial) {
      await scene.gpuFinished();
      const entries = scene.sceneBaker.current.entries, textures = new Map(), differences = new Map(), rows = [];
      const names = new Set();
      for (const { target, maps } of entries) {
        const name = target.mesh.name;
        if (!maps || names.has(name)) throw Error("Missing or ambiguous receiving surface " + name);
        names.add(name);
        const layout = await collectLayout(target.layout);
        const measured = work.get(maps.nodes);
        const outputs = {}, old = reference.get(name);
        for (const [channel, node] of maps.nodes.nodes) {
          const previous = old?.outputs[channel];
          let image = textures.get(node), pixels;
          if (!image) {
            const sample = read(node);
            pixels = sample.pixels;
            const hash = await digest(pixels);
            image = { format: sample.format, width: sample.width, height: sample.height, hash };
            textures.set(node, image);
            if (initial && !contents.has(hash)) {
              const packed = new Blob([pixels.buffer]).stream().pipeThrough(new CompressionStream("deflate-raw"));
              contents.set(hash, await new Response(packed).blob());
            }
          }
          if (initial) { outputs[channel] = image; continue; }
          const sameFormat = previous?.format === image.format;
          const exact = !!sameFormat && previous.hash === image.hash;
          let delta;
          if (sameFormat && !exact) {
            const pair = previous.hash + image.hash + image.format;
            if (!differences.has(pair)) differences.set(pair, await compare(previous.hash, pixels ?? read(node).pixels));
            delta = differences.get(pair);
          }
          outputs[channel] = { ...image, previousHash: previous?.hash, previousFormat: previous?.format,
            redrawn: measured?.drawn.some(output => output.id === channel) ?? false, sameFormat, exact, delta };
        }
        if (initial) reference.set(name, { layout, outputs });
        else rows.push({ name, material: target.material, layout, previousLayout: old?.layout, work: measured, outputs,
          removed: Object.keys(old?.outputs ?? {}).filter(channel => !Object.hasOwn(outputs, channel)) });
      }
      return { rows, removed: [...reference.keys()].filter(name => !names.has(name)) };
    }
    await snapshot(true);
    const graph = scene.bakerInstance.graph, realize = graph.realize;
    graph.realize = async function(specs, ...args) {
      const result = await realize.call(this, specs, ...args);
      if (result) {
        const specsById = new Map(specs.map(spec => [spec.id, spec]));
        const drawn = result.drawn.map(id => {
          const spec = specsById.get(id);
          return { id, op: spec.op.label, texels: spec.target.width * spec.target.height };
        });
        work.set(result.instance, { drawn, reused: result.reused, ms: result.timings.total,
          prepareMs: result.timings.prepare, submitMs: result.timings.draw, mipsMs: result.timings.mips });
      }
      return result;
    };
    window.__finishMapTrace = async () => {
      graph.realize = realize;
      try { return await snapshot(false); }
      finally { contents.clear(); reference.clear(); delete window.__finishMapTrace; }
    };
  });
}

export async function finishMapTrace(page) {
  const result = await page.evaluate(() => window.__finishMapTrace());
  return { ...result, ...summarizeMapTrace(result) };
}

/** Costs below are wall time in graph realization (including waits), and actually drawn base-level texels. */
export function summarizeMapTrace({ rows, removed }) {
  const summary = { receivers: rows.length, removedReceivers: removed.length, publishedMaps: 0,
    redrawnReceivers: 0, allMapsByteIdentical: 0, redrawnReceiversAllMapsByteIdentical: 0,
    redrawnPublishedMaps: 0, redrawnExact: 0, redrawnExactFullLayout: 0,
    exact: 0, exactFullLayout: 0, sameFormatChanged: 0, otherFormatOrNew: 0,
    changedWithinOne255: 0, changedBeyondOne255: 0, changedBeyondEight255: 0 };
  const families = {}, surfaceGroups = {}, channels = {}, operations = {};
  const surfaceCosts = [];
  const mapTally = () => ({ maps: 0, exact: 0, redrawnMaps: 0, redrawnExact: 0, redrawnExactFullLayout: 0,
    comparableTexels: 0, changedPixels: 0, beyondOne255: 0, beyondEight255: 0, differentFormatOrNew: 0 });
  const tally = () => ({ ...mapTally(), receivers: 0, redrawnTexels: 0, graphMs: 0, layerTexels: 0 });
  const groupOf = name => {
    if (name.includes("/roof/roof/")) return "roof slab";
    if (/\/(?:parapet|plinth(?:-\d+)?|fascia|band(?:-\d+)?|ledge|coping)\//.test(name)) return "rims and trim";
    if (/\/(?:ground|groundFloor)\/.*\/wall\/wall\//.test(name)) return "ground walls";
    if (/\/floor-\d+\/.*\/(?:wall|tiles-\d+|tileEnd|tileStart)\//.test(name) || /\/cornerRound-\d+\/.*\/tiles-\d+\//.test(name)) return "upper walls and tiles";
    return "remaining surfaces and accessories";
  };
  for (const s of rows) {
    const a = s.previousLayout, b = s.layout;
    const dims = a && a.width === b.width && a.height === b.height && a.gutter === b.gutter && a.density === b.density;
    const full = !!dims && a.records === b.records && a.grid === b.grid;
    s.alignment = !dims ? "new-or-different-extents" : full ? "identical-layout" :
      a.chart === b.chart && a.grid === b.grid ? "same-chart-and-grid-changed-surface" :
      a.frame && a.frame === b.frame && a.grid === b.grid && b.latticeTriangles === b.triangles
        ? "same-lattice-rectangles-different-crop" : "different-chart-or-grid";
    const family = s.material.split(".")[0], groups = [
      families[family] ??= tally(), surfaceGroups[groupOf(s.name)] ??= tally(),
    ];
    const drawn = s.work?.drawn ?? [];
    if (drawn.length) summary.redrawnReceivers++;
    if (s.previousLayout && !s.removed.length && Object.values(s.outputs).every(x => x.exact)) {
      summary.allMapsByteIdentical++;
      if (drawn.length) summary.redrawnReceiversAllMapsByteIdentical++;
    }
    let texels = 0;
    for (const n of drawn) {
      texels += n.texels;
      const op = operations[n.op] ??= { nodes: 0, drawnTexels: 0 };
      op.nodes++; op.drawnTexels += n.texels;
    }
    for (const group of groups) {
      group.receivers++;
      group.redrawnTexels += texels;
      group.graphMs += s.work?.ms ?? 0;
      group.layerTexels += drawn.filter(n => n.op === "layer").reduce((sum, n) => sum + n.texels, 0);
    }
    if (texels) surfaceCosts.push({ name: s.name, family, ms: s.work.ms, drawnTexels: texels,
      alignment: s.alignment, changed: Object.keys(s.outputs).filter(id => !s.outputs[id].exact) });
    for (const [id, v] of Object.entries(s.outputs)) {
      const c = channels[id] ??= mapTally();
      for (const g of [c, ...groups]) {
        g.maps++; g.exact += +v.exact; g.redrawnMaps += +v.redrawn;
        g.redrawnExact += +(v.redrawn && v.exact); g.redrawnExactFullLayout += +(v.redrawn && v.exact && full);
        g.changedPixels += v.delta?.changed ?? 0; g.beyondOne255 += v.delta?.over1 ?? 0; g.beyondEight255 += v.delta?.over8 ?? 0;
        if (v.sameFormat) g.comparableTexels += v.width * v.height; else g.differentFormatOrNew++;
      }
      summary.publishedMaps++; summary.redrawnPublishedMaps += +v.redrawn;
      summary.redrawnExact += +(v.redrawn && v.exact);
      summary.redrawnExactFullLayout += +(v.redrawn && v.exact && full);
      if (v.exact) { summary.exact++; summary.exactFullLayout += +full; }
      else if (v.delta) {
        summary.sameFormatChanged++;
        if (!v.delta.over1 && !v.delta.nonFinite) summary.changedWithinOne255++;
        if (v.delta.over1) summary.changedBeyondOne255++;
        if (v.delta.over8) summary.changedBeyondEight255++;
      } else summary.otherFormatOrNew++;
    }
  }
  return { summary, families, surfaceGroups, channels, operations, topSurfacesByTexels: surfaceCosts.sort((a,b) => b.drawnTexels - a.drawnTexels).slice(0, 20),
    topSurfacesByGraphMs: surfaceCosts.sort((a,b) => b.ms - a.ms).slice(0, 20) };
}
