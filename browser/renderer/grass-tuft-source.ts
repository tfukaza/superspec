import * as THREE from "three";

export interface GrassTuftOptions {
  /** Candidate tufts per square meter. */
  density?: number;
  /**
   * Every tuft kept (default), with no test of where the picture changes; false gathers them at edges, light borders and
   * colour changes over a thin base share. Both cost about the same: a tuft's cost is placing and painting it.
   */
  everywhere?: boolean;
  /** Share kept where nothing changes (0 to 1). */
  base?: number;
  /** Tuft heights (m), from the smallest to the largest (most are small: the size is drawn as a square of a random). */
  size?: [number, number];
  /** Tuft width over its height, a range (each tuft its own). */
  aspect?: [number, number];
  /** Close level: extra chubby tufts a square meter, drawn only near the camera (full to near[0] m, gone at near[1]). */
  closeDensity?: number;
  /** The least a tuft's painted normal faces the camera (its paint follows the slope it is seen against). */
  face?: number;
  /** Tall tips: the share only a little lighter than the ground, and the range (times the ground) of the rest. */
  tip?: [number, number, number];
  /** The tall blades' tip colour (linear RGB), a bright lime, brighter where lit. */
  lime?: [number, number, number];
  /** Tall blades: the ground lightness (luma) below which they thin to a tenth, and above which all are kept. */
  light?: [number, number];
  /** Chubby tufts' colour range: brightness (share either way) and warm/cool hue shift (0 to 1). */
  vary?: [number, number];
  near?: [number, number];
  /** Share of the scattered tufts that are tiny-stroke clusters (the rest are short chubby tufts). */
  clusterShare?: number;
  /** Tall thin blades: candidates a square meter (kept only in patches) and the patches' grid cell and radius range (m). */
  tallDensity?: number;
  tallPatch?: [number, [number, number]];
  /** Share of tall candidates kept between patches. */
  tallFloor?: number;
  /** The thin tufts' brightness over the ground's paint and their hue variation (0 none, 1 full). */
  brightness?: number;
  hue?: number;
  /** Colour factor of the clusters where lit (short grass in the sun). */
  sparkle?: [number, number, number];
  /** Tufts fade out from 60 % of this distance (m) and are gone at it. */
  far?: number;
  seed?: number;
}

function random(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Cells of the atlas: 4 columns, 6 rows, each a different tuft. Rows 0 to 3: short chubby tufts; row 4: tall thin
 * accents; row 5: clusters of tiny strokes (short grass catching the sun).
 */
const CELL = 128, COLUMNS = 4, ROWS = 6;

/**
 * The tuft atlas, painted once as a digital painter would: a chubby tuft is 2 to 4 broad strokes fanning from a root near
 * its bottom, a tall one 2 or 3 long thin strokes, a cluster 6 to 12 tiny strokes spread along the cell's bottom; each
 * stroke its own height, lean, droop, width and tip (rounded or pointed), then lightly blurred. Red: the blade's own tone (0 dark to
 * 1 light); green: how far up its blade a pixel is; alpha: coverage. Row 0 is the bottom of the cell.
 */
export function tuftAtlas(seed = 11, blur = 9): THREE.DataTexture {
  const width = CELL * COLUMNS, height = CELL * ROWS;
  const cover = new Float32Array(width * height), tone = new Float32Array(width * height), along = new Float32Array(width * height);
  const next = random(seed);
  for (let k = 0; k < COLUMNS * ROWS; k++) {
    const ox = (k % COLUMNS) * CELL, oy = Math.floor(k / COLUMNS) * CELL;
    const kind = k < 16 ? "chubby" : k < 20 ? "tall" : "cluster";
    const blades = kind === "chubby" ? 2 + Math.floor(next() * 3) : kind === "tall" ? 2 + Math.floor(next() * 2) : 6 + Math.floor(next() * 7);
    for (let b = 0; b < blades; b++) {
      const long = next() < 0.45, pointed = kind === "chubby" ? next() < 0.55 : next() < 0.85;
      const rootX = kind === "cluster" ? 0.1 + 0.8 * next() : 0.5 + (next() - 0.5) * (kind === "tall" ? 0.2 : 0.28);
      const tall = kind === "tall" ? 0.6 + 0.33 * next() : kind === "cluster" ? 0.25 + 0.4 * next() : long ? 0.62 + 0.3 * next() : 0.3 + 0.32 * next();
      const lean = kind === "cluster" ? (next() - 0.5) * 0.9 : (rootX - 0.5) * 1.6 + (next() - 0.5) * (kind === "tall" ? 1.1 : 0.7);
      const bend = kind === "tall" ? 1.1 + 1.3 * next() : 1.6;
      const droop = kind === "tall" ? next() * 0.85 : long ? next() * 0.5 : next() * 0.2;
      const thick = kind === "tall" ? 0.05 + 0.035 * next() : kind === "cluster" ? 0.035 + 0.035 * next() : 0.08 + 0.08 * next();
      const own = next();
      const points: { x: number; y: number; t: number }[] = [];
      for (let s = 0; s <= 24; s++) {
        const t = s / 24;
        points.push({ x: rootX + lean * tall * Math.pow(t, bend), y: 0.05 + tall * (t - droop * Math.pow(t, 3) * 0.6) * (1 - 0.12 * Math.abs(lean) * t), t });
      }
      for (let py = 0; py < CELL; py++) for (let px = 0; px < CELL; px++) {
        const x = (px + 0.5) / CELL, y = (py + 0.5) / CELL;
        let best = 1e9, bestT = 0;
        for (let s = 0; s < points.length - 1; s++) {
          const a = points[s], c = points[s + 1];
          const dx = c.x - a.x, dy = c.y - a.y, len2 = dx * dx + dy * dy;
          const u = Math.max(0, Math.min(1, ((x - a.x) * dx + (y - a.y) * dy) / len2));
          const ex = a.x + dx * u - x, ey = a.y + dy * u - y, d = Math.hypot(ex, ey);
          if (d < best) { best = d; bestT = a.t + (c.t - a.t) * u; }
        }
        // Half width along the stroke: swelling a little above its root, then to a point or a rounded tip.
        const swell = 0.75 + 0.25 * Math.sin(Math.min(bestT * 2.2, 1) * Math.PI / 2);
        const half = 0.5 * thick * swell * (pointed ? 1 - Math.pow(bestT, 1.2) : Math.sqrt(Math.max(1 - Math.pow(bestT, 3), 0))) + 0.002;
        const pixel = 1 / CELL;
        const c = Math.max(0, Math.min(1, (half - best) / pixel + 0.5));
        if (c <= 0) continue;
        const i = (oy + py) * width + ox + px;
        // Over: this blade on top of what is there.
        tone[i] = own * c + tone[i] * (1 - c);
        along[i] = bestT * c + along[i] * (1 - c);
        cover[i] = c + cover[i] * (1 - c);
      }
    }
  }
  // Pre-blurred: a separable Gaussian over premultiplied values, within each cell.
  const pre = [0, 1, 2].map(() => new Float32Array(width * height));
  for (let i = 0; i < cover.length; i++) { pre[0][i] = tone[i] * cover[i]; pre[1][i] = along[i] * cover[i]; pre[2][i] = cover[i]; }
  // The chubby tufts and the clusters are blurred; the tall thin ones (row 4) keep crisp edges.
  // The clusters (row 5) are tiny on screen, so they take a much wider blur to read as soft.
  const kernel = (sigma: number) => { const reach = Math.ceil(sigma * 3), weights: number[] = []; for (let k = -reach; k <= reach; k++) weights.push(Math.exp(-0.5 * k * k / (sigma * sigma))); return { reach, weights }; };
  // The tall thin blades (row 4) take only a slight blur.
  // Chubby rows 0 to 3 each their own blur (from crisp to soft, the softest lur), so tufts vary in softness.
  const kernels = [kernel(3), kernel(0.4), kernel(blur * 0.22), kernel(blur * 0.45), kernel(blur * 0.7), kernel(blur)];
  const pass = (source: Float32Array, horizontal: boolean): Float32Array => {
    const out = new Float32Array(source.length);
    for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
      const cx = Math.floor(x / CELL) * CELL, cy = Math.floor(y / CELL) * CELL;
      if (blur <= 0) { out[y * width + x] = source[y * width + x]; continue; }
      let sum = 0, total = 0;
      const row = Math.floor(cy / CELL);
      const { reach, weights } = kernels[row >= 5 ? 0 : row === 4 ? 1 : 2 + row];
      for (let k = -reach; k <= reach; k++) {
        const sx = horizontal ? x + k : x, sy = horizontal ? y : y + k;
        if (sx < cx || sx >= cx + CELL || sy < cy || sy >= cy + CELL) continue;
        const w = weights[k + reach];
        sum += source[sy * width + sx] * w;
        total += w;
      }
      out[y * width + x] = sum / total;
    }
    return out;
  };
  const blurred = pre.map(channel => pass(pass(channel, true), false));
  const data = new Uint8Array(width * height * 4);
  for (let i = 0; i < cover.length; i++) {
    const a = blurred[2][i];
    data[i * 4] = Math.round(255 * (a > 1e-4 ? blurred[0][i] / a : 0.5));
    data[i * 4 + 1] = Math.round(255 * (a > 1e-4 ? blurred[1][i] / a : 0));
    data[i * 4 + 2] = 0;
    data[i * 4 + 3] = Math.round(255 * Math.min(1, a));
  }
  const texture = new THREE.DataTexture(data, width, height, THREE.RGBAFormat, THREE.UnsignedByteType);
  texture.colorSpace = THREE.NoColorSpace;
  texture.magFilter = THREE.LinearFilter;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.generateMipmaps = true;
  texture.needsUpdate = true;
  return texture;
}

/** The seeded candidate roots, up vectors and [height,width,atlas cell,rank]
 * shared by the live grass shader and the static game export. */
export function grassTuftSamples(ground: THREE.Mesh, options: GrassTuftOptions = {}) {
  const next = random(options.seed ?? 5);
  ground.updateWorldMatrix(true, false);
  const source = ground.geometry.index ? ground.geometry.toNonIndexed() : ground.geometry;
  const position = source.getAttribute("position"), normal = source.getAttribute("normal");
  const turn = new THREE.Matrix3().getNormalMatrix(ground.matrixWorld);
  const triangles: { p: THREE.Vector3[]; n: THREE.Vector3[] }[] = [];
  const cumulative: number[] = [];
  let total = 0;
  for (let i = 0; i + 2 < position.count; i += 3) {
    const p = [0, 1, 2].map(k => new THREE.Vector3().fromBufferAttribute(position, i + k).applyMatrix4(ground.matrixWorld));
    const area = new THREE.Vector3().subVectors(p[1], p[0]).cross(new THREE.Vector3().subVectors(p[2], p[0])).length() / 2;
    if (area <= 0) continue;
    const n = [0, 1, 2].map(k => {
      const v = normal ? new THREE.Vector3().fromBufferAttribute(normal, i + k).applyMatrix3(turn).normalize() : new THREE.Vector3(0, 0, 1);
      return v.z < 0 ? v.negate() : v;
    });
    total += area;
    triangles.push({ p, n });
    cumulative.push(total);
  }
  const [s0, s1] = options.size ?? [0.05, 0.26], [a0, a1] = options.aspect ?? [0.75, 1.4];
  const clusterShare = options.clusterShare ?? 0.07;
  const point = (): { at: THREE.Vector3; up: THREE.Vector3 } => {
    const pick = next() * total;
    let lo = 0, hi = cumulative.length - 1;
    while (lo < hi) { const mid = (lo + hi) >> 1; if (cumulative[mid] < pick) lo = mid + 1; else hi = mid; }
    const { p, n } = triangles[lo];
    let u = next(), v = next();
    if (u + v > 1) { u = 1 - u; v = 1 - v; }
    const w = 1 - u - v;
    return {
      at: p[0].clone().multiplyScalar(w).addScaledVector(p[1], u).addScaledVector(p[2], v),
      up: n[0].clone().multiplyScalar(w).addScaledVector(n[1], u).addScaledVector(n[2], v).normalize(),
    };
  };
  const rootList: number[] = [], upList: number[] = [], dataList: number[] = [];
  const add = (at: THREE.Vector3, up: THREE.Vector3, tall: number, aspect: number, cell: number) => {
    rootList.push(at.x, at.y, at.z);
    upList.push(up.x, up.y, up.z);
    // A cell is square; each tuft is stretched to its own width.
    dataList.push(tall, tall * aspect, cell, next());
  };
  // Chubby tufts and clusters, scattered evenly.
  const scattered = Math.max(1, Math.round(total * (options.density ?? 400)));
  for (let k = 0; k < scattered; k++) {
    const { at, up } = point();
    if (next() < clusterShare) add(at, up, 0.04 + 0.04 * next(), 1.6 + 0.8 * next(), 20 + Math.floor(next() * 4));
    else add(at, up, s0 + (s1 - s0) * next() ** 2, a0 + (a1 - a0) * next(), Math.floor(next() * 16));
  }
  // The close level: more chubby tufts, kept only near the camera (their random plus one marks them).
  const close = Math.round(total * (options.closeDensity ?? 1200));
  for (let k = 0; k < close; k++) {
    const { at, up } = point();
    add(at, up, s0 + (s1 - s0) * next() ** 2, a0 + (a1 - a0) * next(), Math.floor(next() * 16));
    dataList[dataList.length - 1] += 1;
  }
  // Tall thin blades gathered in patches: candidates scattered evenly, kept only near the centre of a patch. Patches are
  // blobs on a jittered 3D grid (one a cell, its radius its own), so they lie on slopes as on flat ground.
  const [patchCell, patchRadius] = options.tallPatch ?? [0.6, [0.22, 0.4]] as [number, [number, number]];
  const hash = (x: number, y: number, z: number, s: number) => { const h = Math.sin(x * 127.1 + y * 311.7 + z * 74.7 + s * 19.3) * 43758.5453; return h - Math.floor(h); };
  const nearPatch = (at: THREE.Vector3): number => {
    const q = at.clone().divideScalar(patchCell), base = new THREE.Vector3(Math.floor(q.x), Math.floor(q.y), Math.floor(q.z));
    let best = 0;
    for (let x = -1; x <= 1; x++) for (let y = -1; y <= 1; y++) for (let z = -1; z <= 1; z++) {
      const cx = base.x + x, cy = base.y + y, cz = base.z + z;
      const centre = new THREE.Vector3(cx + 0.15 + 0.7 * hash(cx, cy, cz, 1), cy + 0.15 + 0.7 * hash(cx, cy, cz, 2), cz + 0.15 + 0.7 * hash(cx, cy, cz, 3)).multiplyScalar(patchCell);
      const radius = patchRadius[0] + (patchRadius[1] - patchRadius[0]) * hash(cx, cy, cz, 4);
      const d = at.distanceTo(centre) / radius;
      if (d < 1) best = Math.max(best, 1 - d);
    }
    return best;
  };
  const tallCandidates = Math.round(total * (options.tallDensity ?? 1100));
  for (let k = 0; k < tallCandidates; k++) {
    const { at, up } = point();
    // A thin sprinkling between patches too.
    if (next() > Math.max(nearPatch(at), options.tallFloor ?? 0.05)) continue;
    add(at, up, 0.22 + 0.16 * next(), 0.5 + 0.2 * next(), 16 + Math.floor(next() * 4));
  }
  return { roots: new Float32Array(rootList), ups: new Float32Array(upList), data: new Float32Array(dataList) };
}
