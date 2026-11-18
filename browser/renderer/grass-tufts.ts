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
  /** Heights (m) of the short chubby tufts and the long ones, each a range. */
  short?: [number, number];
  long?: [number, number];
  /** Share of long tufts. */
  longShare?: number;
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

/** Cells of the atlas: 4 columns, 2 rows; the lower row short chubby tufts, the upper long ones. */
const CELL = 128, COLUMNS = 4, ROWS = 2;

/**
 * The tuft atlas, painted once: each cell 4 or 5 blades fanning from a root near its bottom, each a curved stroke
 * tapering to its tip (chubby ones wide and rounded, long ones thin), then blurred. Red: the blade's own tone (0 dark to
 * 1 light); green: how far up its blade a pixel is; alpha: coverage. Row 0 is the bottom of the cell.
 */
export function tuftAtlas(seed = 11, blur = 2.2): THREE.DataTexture {
  const width = CELL * COLUMNS, height = CELL * ROWS;
  const cover = new Float32Array(width * height), tone = new Float32Array(width * height), along = new Float32Array(width * height);
  const next = random(seed);
  for (let k = 0; k < COLUMNS * ROWS; k++) {
    const long = k >= COLUMNS, ox = (k % COLUMNS) * CELL, oy = Math.floor(k / COLUMNS) * CELL;
    const blades = 4 + (next() < 0.5 ? 1 : 0);
    for (let b = 0; b < blades; b++) {
      const rootX = 0.5 + (next() - 0.5) * 0.3;
      const tall = long ? 0.68 + 0.22 * next() : 0.42 + 0.25 * next();
      const lean = (rootX - 0.5) * 1.4 + (next() - 0.5) * (long ? 0.55 : 0.45);
      const thick = long ? 0.045 + 0.02 * next() : 0.085 + 0.035 * next();
      const own = next();
      const points: { x: number; y: number; t: number }[] = [];
      for (let s = 0; s <= 24; s++) {
        const t = s / 24;
        points.push({ x: rootX + lean * tall * Math.pow(t, 1.6), y: 0.06 + tall * t * (1 - 0.12 * Math.abs(lean) * t), t });
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
        // Half width along the blade: chubby blades rounded to their tips, long ones tapering to a point.
        const half = long ? 0.5 * thick * (1 - Math.pow(bestT, 1.4)) + 0.002 : 0.5 * thick * Math.sqrt(Math.max(1 - bestT * bestT, 0)) + 0.002;
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
  const reach = Math.ceil(blur * 3), weights: number[] = [];
  for (let k = -reach; k <= reach; k++) weights.push(Math.exp(-0.5 * k * k / (blur * blur)));
  const pass = (source: Float32Array, horizontal: boolean): Float32Array => {
    const out = new Float32Array(source.length);
    for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
      const cx = Math.floor(x / CELL) * CELL, cy = Math.floor(y / CELL) * CELL;
      let sum = 0, total = 0;
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
  // Upright along the relief's normal, turned about it to face the camera.
  vec3 side = cross(rn, v);
  side = length(side) < 1e-4 ? t1 : normalize(side);
  float mirror = fract(tuftData.w * 91.7 + tuftKeep.z) < 0.5 ? -1.0 : 1.0;
  vec3 world = root - rn * 0.08 * tuftData.x + side * position.x * tuftData.y * mirror + rn * position.y * tuftData.x;
  float cell = tuftData.z;
  vUv = (vec2(mod(cell, 4.0), floor(cell / 4.0)) + uv) / vec2(4.0, 2.0);
  vColor = reliefPaint(root, rn, h, tone);
  vLit = band;
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}`;

const FRAGMENT = /* glsl */`
#include <common>
uniform sampler2D tuftAtlas;
varying vec2 vUv;
varying vec3 vColor;
varying float vLit;
void main() {
  vec4 t = texture(tuftAtlas, vUv);
  if (t.a < 0.02) discard;
  // Each blade lighter or darker by its own tone; its lit tip a little lighter still.
  vec3 c = vColor * mix(0.84, 1.14, t.r) * mix(1.0, 1.18, t.g * t.g * vLit);
  gl_FragColor = vec4(c, t.a);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}`;

/** Tufts over `ground` (the lawn, before relief), painted and placed by the relief mesh `relief` (grass-relief.ts). */
export function grassTufts(ground: THREE.Mesh, relief: THREE.Mesh, options: GrassTuftOptions = {}): THREE.Mesh {
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
  const count = Math.max(1, Math.round(total * (options.density ?? 600)));
  const [s0, s1] = options.short ?? [0.06, 0.1], [l0, l1] = options.long ?? [0.13, 0.2];
  const roots = new Float32Array(count * 3), ups = new Float32Array(count * 3), data = new Float32Array(count * 4);
  for (let k = 0; k < count; k++) {
    const pick = next() * total;
    let lo = 0, hi = cumulative.length - 1;
    while (lo < hi) { const mid = (lo + hi) >> 1; if (cumulative[mid] < pick) lo = mid + 1; else hi = mid; }
    const { p, n } = triangles[lo];
    let u = next(), v = next();
    if (u + v > 1) { u = 1 - u; v = 1 - v; }
    const w = 1 - u - v;
    const at = p[0].clone().multiplyScalar(w).addScaledVector(p[1], u).addScaledVector(p[2], v);
    const up = n[0].clone().multiplyScalar(w).addScaledVector(n[1], u).addScaledVector(n[2], v).normalize();
    const long = next() < (options.longShare ?? 0.45);
    const tall = long ? l0 + (l1 - l0) * next() : s0 + (s1 - s0) * next();
    roots.set([at.x, at.y, at.z], k * 3);
    ups.set([up.x, up.y, up.z], k * 3);
    // A cell's width equals its height, so a tuft's quad is square.
    data.set([tall, tall, (long ? 4 : 0) + Math.floor(next() * 4), next()], k * 4);
  }
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([-0.5, 0, 0, 0.5, 0, 0, 0.5, 1, 0, -0.5, 1, 0], 3));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute([0, 0, 1, 0, 1, 1, 0, 1], 2));
  geometry.setIndex([0, 1, 2, 0, 2, 3]);
  geometry.setAttribute("tuftRoot", new THREE.InstancedBufferAttribute(roots, 3));
  geometry.setAttribute("tuftUp", new THREE.InstancedBufferAttribute(ups, 3));
  geometry.setAttribute("tuftData", new THREE.InstancedBufferAttribute(data, 4));
  geometry.instanceCount = count;
  const shared = (relief.material as THREE.ShaderMaterial);
  const material = new THREE.ShaderMaterial({
    vertexShader: VERTEX, fragmentShader: FRAGMENT, transparent: true, depthWrite: false,
    defines: { ...shared.defines, ...(options.everywhere ?? true ? { TUFT_EVERYWHERE: "" } : {}) },
    uniforms: {
      ...shared.uniforms,
      tuftAtlas: { value: tuftAtlas() },
      tuftKeep: { value: new THREE.Vector3(options.base ?? 0.25, options.far ?? 30, 0.37) },
    },
  });
  followPaintedFinishing(material);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = "grass/tufts";
  mesh.frustumCulled = false;
  mesh.renderOrder = 3;
  mesh.userData.clumpBlur = true;
  mesh.userData.grassTufts = { candidates: count, area: total };
  return mesh;
}
