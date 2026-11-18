import * as THREE from "three";
import { followPaintedFinishing } from "./shade.ts";
import { PAINT_GLSL, RELIEF_GLSL } from "./grass-relief.ts";

/**
 * Grass tufts over the grass relief (an experiment): camera-facing billboards of a few painted grass blades, pre-blurred
 * into an atlas, scattered densely over the lawn and kept most where the picture changes:
 *
 * - **edges**: where the relief's normal turns away from the view (the outline of a clump, a crest, the bank's lip);
 * - **light borders**: where the shade band is between lit and shaded, from the relief's own normal or from a cast
 *   shadow (read with a wide kernel, so its edge is a band, not a line);
 * - **colour changes**: where the ground's own colour changes around the root.
 *
 * Elsewhere a thin share remains. A tuft stands on the relief (raised by it when the relief is displaced), upright along
 * the relief's normal and turned about it to face the camera, and takes the relief's paint at its root (ground colour,
 * clump tone, crown lift, shade band and cast shadow), each blade lighter or darker by its own tone in the atlas and its
 * lit tip a little lighter. Tufts share the relief's uniforms (`relief`), so both always agree, and blend over it; with
 * the scene's `?clumpblur=` they are blurred with it.
 */
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
export function tuftAtlas(seed = 11, blur = 2.0): THREE.DataTexture {
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
  const kernels = [kernel(blur), kernel(blur * 3), kernel(blur * 0.4)];
  const pass = (source: Float32Array, horizontal: boolean): Float32Array => {
    const out = new Float32Array(source.length);
    for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
      const cx = Math.floor(x / CELL) * CELL, cy = Math.floor(y / CELL) * CELL;
      if (blur <= 0) { out[y * width + x] = source[y * width + x]; continue; }
      let sum = 0, total = 0;
      const { reach, weights } = kernels[cy >= 5 * CELL ? 1 : cy >= 4 * CELL ? 2 : 0];
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

const VERTEX = /* glsl */`
attribute vec3 tuftRoot;
attribute vec3 tuftUp;
// x: height (m), y: width (m), z: atlas cell, w: the tuft's stable random (kept while below its share).
attribute vec4 tuftData;
uniform float lift;
uniform vec3 tuftKeep; // x: base share, y: far (m), z: mirror seed
varying float vKind;
varying float vHue;
varying vec2 vUv;
varying vec3 vColor;
varying float vLit;
${RELIEF_GLSL}
${PAINT_GLSL}
float luma(vec3 c) { return dot(c, vec3(0.2126, 0.7152, 0.0722)); }
// The cast shadow read over a wider kernel (four times the relief's), so a shadow's edge is a band tufts can gather in.
float castWide(vec3 world, vec3 normal) {
  if (msFoliageShadowParams.x < 0.5) return 1.0;
  vec4 c = msFoliageShadowMatrix * vec4(world + normal * msFoliageShadowParams.z, 1.0);
  c.xyz /= c.w;
  if (c.x < 0.0 || c.x > 1.0 || c.y < 0.0 || c.y > 1.0 || c.z > 1.0) return 1.0;
  float depth = c.z + msFoliageShadowParams.y, lit = 0.0;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) lit += texture(msFoliageShadow, vec3(c.xy + vec2(float(x), float(y)) * msFoliageShadowParams.w * 6.0, depth));
  return lit / 9.0;
}
void main() {
  vec3 n = normalize(tuftUp);
  float h = reliefHeight(tuftRoot, n);
  float tone = reliefTone;
  vec3 rn = reliefNormal(tuftRoot, n);
  vec3 root = tuftRoot + n * h * lift;
  vec3 toCamera = cameraPosition - root;
  float away = length(toCamera);
  vec3 v = toCamera / away;
  float band = smoothstep(-shadeBand.y, shadeBand.y, dot(rn, normalize(msSunDirection)) - shadeBand.x);
  vec3 t1, t2;
  reliefFrame(n, t1, t2);
#ifdef TUFT_EVERYWHERE
  float keep = 1.0 - smoothstep(0.6 * tuftKeep.y, tuftKeep.y, away);
#else
  // Where the picture changes: the outline, a light border (shade band or cast shadow), a change of ground colour.
  float edge = 1.0 - smoothstep(0.12, 0.45, abs(dot(rn, v)));
  float light = min(band, castWide(root, rn));
  float border = 1.0 - abs(2.0 * light - 1.0);
  vec2 span = groundRect.zw - groundRect.xy;
  float g0 = luma(texture(groundColor, clamp((root.xy - groundRect.xy) / span, 0.0, 1.0)).rgb);
  float g1 = luma(texture(groundColor, clamp((root.xy + t1.xy * 0.05 - groundRect.xy) / span, 0.0, 1.0)).rgb);
  float g2 = luma(texture(groundColor, clamp((root.xy + t2.xy * 0.05 - groundRect.xy) / span, 0.0, 1.0)).rgb);
  float colour = smoothstep(0.02, 0.08, (abs(g1 - g0) + abs(g2 - g0)) / max(g0, 1e-3));
  float score = max(edge, max(border, colour));
  float keep = mix(tuftKeep.x, 1.0, score) * (1.0 - smoothstep(0.6 * tuftKeep.y, tuftKeep.y, away));
#endif
  if (tuftData.w > keep) { gl_Position = vec4(0.0, 0.0, 2.0, 1.0); return; }
  // Grass grows mostly upward: the tuft stands between the relief's normal and straight up, turned to face the camera.
  vec3 grow = normalize(mix(rn, vec3(0.0, 0.0, 1.0), 0.6));
  vec3 side = cross(grow, v);
  side = length(side) < 1e-4 ? t1 : normalize(side);
  // Each tuft tilted its own way across the view (the tall blades most), so they do not all stand straight up.
  float tilt = (fract(tuftData.w * 17.31 + 0.5) - 0.5) * (tuftData.z > 15.5 && tuftData.z < 19.5 ? 0.9 : 0.35);
  grow = normalize(grow + side * tilt);
  float mirror = fract(tuftData.w * 91.7 + tuftKeep.z) < 0.5 ? -1.0 : 1.0;
  vec3 world = root - grow * 0.08 * tuftData.x + side * position.x * tuftData.y * mirror + grow * position.y * tuftData.x;
  float cell = tuftData.z;
  vUv = (vec2(mod(cell, 4.0), floor(cell / 4.0)) + uv) / vec2(4.0, 6.0);
  vKind = cell < 15.5 ? 0.0 : cell < 19.5 ? 1.0 : 2.0;
  vHue = fract(tuftData.w * 53.7 + 0.21);
  vColor = reliefPaint(root, rn, h, tone);
  vLit = min(band, reliefCast(root, rn));
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}`;

const FRAGMENT = /* glsl */`
#include <common>
uniform sampler2D tuftAtlas;
varying vec2 vUv;
varying vec3 vColor;
varying float vLit;
varying float vKind;
varying float vHue;
uniform vec2 tuftDetail; // x: brightness of the thin tufts, y: their hue variation
uniform vec3 tuftSpark;
void main() {
  vec4 t = texture(tuftAtlas, vUv);
  if (t.a < 0.02) discard;
  vec3 c;
  float a;
  // The thin tufts' hues: warm straw, yellow-green, lime and cool teal, each tuft its own place among them.
  float hs = vHue * 3.0;
  vec3 hue = hs < 1.0 ? mix(vec3(1.3, 1.12, 0.5), vec3(1.12, 1.2, 0.55), hs) : hs < 2.0 ? mix(vec3(1.12, 1.2, 0.55), vec3(0.95, 1.25, 0.45), hs - 1.0) : mix(vec3(0.95, 1.25, 0.45), vec3(0.78, 1.08, 1.3), hs - 2.0);
  vec3 bright = vColor * tuftDetail.x * mix(vec3(1.0), hue, tuftDetail.y) * mix(0.88, 1.12, t.r);
  if (vKind < 0.5) {
    // Chubby: the ground's own paint, barely a tone apart per stroke; soft edged.
    c = vColor * mix(0.95, 1.05, t.r);
    a = smoothstep(0.02, 0.6, t.a);
  } else if (vKind < 1.5) {
    // Tall thin blades: the ground's paint at the base, turning brighter and to their own hue (warm yellow-green to
    // cool blue-green) up the blade; crisp edged.
    c = mix(vColor * mix(0.95, 1.05, t.r), bright * 1.2, smoothstep(0.15, 0.85, t.g));
    a = smoothstep(0.2, 0.6, t.a);
  } else {
    // Clusters: short grass in the sun, bright and brighter where lit; soft edged like the chubby tufts.
    c = bright * mix(vec3(1.0), tuftSpark, vLit);
    a = smoothstep(0.02, 0.6, t.a);
  }
  gl_FragColor = vec4(c, a);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}`;

/** Tufts over `ground` (the lawn, before relief), painted and placed by the relief mesh `relief` (grass-relief.ts). */
export function grassTufts(ground: THREE.Mesh, relief: THREE.Mesh, options: GrassTuftOptions = {}): THREE.Mesh[] {
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
  const clusterShare = options.clusterShare ?? 0.18;
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
  const scattered = Math.max(1, Math.round(total * (options.density ?? 150)));
  for (let k = 0; k < scattered; k++) {
    const { at, up } = point();
    if (next() < clusterShare) add(at, up, 0.04 + 0.04 * next(), 1.6 + 0.8 * next(), 20 + Math.floor(next() * 4));
    else add(at, up, s0 + (s1 - s0) * next() ** 2, a0 + (a1 - a0) * next(), Math.floor(next() * 16));
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
  const tallCandidates = Math.round(total * (options.tallDensity ?? 800));
  for (let k = 0; k < tallCandidates; k++) {
    const { at, up } = point();
    // A thin sprinkling between patches too.
    if (next() > Math.max(nearPatch(at), options.tallFloor ?? 0.05)) continue;
    add(at, up, 0.22 + 0.16 * next(), 0.5 + 0.2 * next(), 16 + Math.floor(next() * 4));
  }
  const count = dataList.length / 4;
  const roots = new Float32Array(rootList), ups = new Float32Array(upList), data = new Float32Array(dataList);
  // Two meshes: the chubby tufts, blurred with the relief, and the thin ones, drawn after it unblurred.
  const atlas = tuftAtlas();
  const shared = (relief.material as THREE.ShaderMaterial);
  const build = (detail: boolean): THREE.Mesh => {
  const picked: number[] = [];
  for (let k = 0; k < count; k++) if ((data[k * 4 + 2] >= 16 && data[k * 4 + 2] < 20) === detail) picked.push(k);
  const pick = (source: Float32Array, size: number) => { const out = new Float32Array(picked.length * size); picked.forEach((k, i) => out.set(source.subarray(k * size, k * size + size), i * size)); return out; };
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([-0.5, 0, 0, 0.5, 0, 0, 0.5, 1, 0, -0.5, 1, 0], 3));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute([0, 0, 1, 0, 1, 1, 0, 1], 2));
  geometry.setIndex([0, 1, 2, 0, 2, 3]);
  geometry.setAttribute("tuftRoot", new THREE.InstancedBufferAttribute(pick(roots, 3), 3));
  geometry.setAttribute("tuftUp", new THREE.InstancedBufferAttribute(pick(ups, 3), 3));
  geometry.setAttribute("tuftData", new THREE.InstancedBufferAttribute(pick(data, 4), 4));
  geometry.instanceCount = picked.length;
  const material = new THREE.ShaderMaterial({
    vertexShader: VERTEX, fragmentShader: FRAGMENT, transparent: true, depthWrite: false,
    defines: { ...shared.defines, ...(options.everywhere ?? true ? { TUFT_EVERYWHERE: "" } : {}) },
    uniforms: {
      ...shared.uniforms,
      tuftAtlas: { value: atlas },
      tuftKeep: { value: new THREE.Vector3(options.base ?? 0.25, options.far ?? 30, 0.37) },
      tuftDetail: { value: new THREE.Vector2(options.brightness ?? 1.4, options.hue ?? 1.0) },
      tuftSpark: { value: new THREE.Vector3(...(options.sparkle ?? [1.75, 1.62, 0.72])) },
    },
  });
  followPaintedFinishing(material);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = detail ? "grass/tufts-detail" : "grass/tufts";
  mesh.frustumCulled = false;
  mesh.renderOrder = 3;
  if (detail) mesh.userData.clumpDetail = true; else mesh.userData.clumpBlur = true;
  mesh.userData.grassTufts = { candidates: picked.length, area: total, detail };
  return mesh;
  };
  return [build(false), build(true)];
}
