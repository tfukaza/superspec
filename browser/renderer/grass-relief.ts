import * as THREE from "three";
import { followPaintedFinishing, programSun } from "./shade.ts";
import { sceneFieldUniforms } from "./scene-fields.ts";

/**
 * Grass relief (an experiment): the lawn as a field of grass clumps given by one height function, drawn three ways so
 * their look and cost can be compared:
 *
 * - `normal`: the lawn's own mesh, its shading normal tipped by the height's slope (a procedural normal map);
 * - `displace`: the lawn's triangles divided finely and each vertex raised along the ground's normal by the height;
 * - `parallax`: the lawn's mesh lifted by the relief's full height, each pixel marching its view ray down through the
 *   height field to where it meets the clumps (parallax occlusion mapping), its depth written at the hit.
 *
 * The height (meters, 0 to `height`): clumps on two jittered cell grids, each a soft dome whose crown is ridged by
 * blades fanning out from its centre (a cosine of the angle around it, sharpened, stronger toward the rim); the taller
 * clump wins where two overlap (a smooth maximum). A slow fractal scales the clumps up and down over the meadow and a fine
 * one roughens everything. Every method paints the same way: the ground's own colour under the point (`ground.color`),
 * a little darker in the hollows, under the lawn's shade band on the relief's normal with the sun's cast shadow.
 */
export type ReliefMode = "normal" | "displace" | "parallax";

export interface GrassReliefOptions {
  /** Highest relief (m). */
  height?: number;
  /** Clump cell size (m). */
  cell?: number;
  /** Blade ridges around a clump, and how much they cut its crown (0 none, 1 deep). */
  blades?: number;
  ridge?: number;
  /** How far fractal noise warps the clumps' outlines (m), the mounds' frequency (1/m) and weight, fine roughness. */
  warp?: number;
  moundScale?: number;
  moundWeight?: number;
  fine?: number;
  /** Darkening in the hollows (share). */
  cavity?: number;
  /** Divisions of each lawn triangle's edge (displace). */
  divisions?: number;
  /** March steps (parallax). */
  steps?: number;
  shadeTint?: [number, number, number];
  threshold?: number;
  range?: number;
  /** The foliage's shadow-map uniforms, for the sun's cast shadow. */
  shadow?: Record<string, THREE.IUniform>;
}

export const RELIEF_GLSL = /* glsl */`
uniform vec4 reliefShape; // x: height (m), y: cell (m), z: blades, w: ridge
uniform vec4 reliefMore;  // x: warp (m), y: mound frequency (1/m), z: mound weight, w: fine roughness
float reliefHash(vec3 c) { return fract(sin(dot(c, vec3(127.1, 311.7, 74.7))) * 43758.5453); }
vec3 reliefHash3(vec3 c) {
  return fract(sin(vec3(dot(c, vec3(127.1, 311.7, 74.7)), dot(c, vec3(269.5, 183.3, 246.1)), dot(c, vec3(113.5, 271.9, 124.6)))) * 43758.5453);
}
// Smooth value noise in 3D, 0 to 1.
float reliefNoise(vec3 p) {
  vec3 i = floor(p), f = fract(p), s = f * f * (3.0 - 2.0 * f);
  float a = mix(mix(reliefHash(i), reliefHash(i + vec3(1, 0, 0)), s.x), mix(reliefHash(i + vec3(0, 1, 0)), reliefHash(i + vec3(1, 1, 0)), s.x), s.y);
  float b = mix(mix(reliefHash(i + vec3(0, 0, 1)), reliefHash(i + vec3(1, 0, 1)), s.x), mix(reliefHash(i + vec3(0, 1, 1)), reliefHash(i + vec3(1, 1, 1)), s.x), s.y);
  return mix(a, b, s.z);
}
float reliefFractal(vec3 p) { return 0.5 * reliefNoise(p) + 0.3 * reliefNoise(p * 2.03 + 7.1) + 0.2 * reliefNoise(p * 4.07 + 3.3); }
// Two directions along the surface at a point with this normal.
void reliefFrame(vec3 n, out vec3 t1, out vec3 t2) {
  t1 = normalize(abs(n.z) < 0.9 ? cross(n, vec3(0.0, 0.0, 1.0)) : cross(n, vec3(1.0, 0.0, 0.0)));
  t2 = cross(n, t1);
}
// Clumps as blobs in space on a jittered 3D grid: where the surface passes through one, a dome rises, so the pattern
// is the same on a slope, a bank or flat ground. Blades ridge each crown unevenly, fanning out from its centre along the surface.
float reliefClumps(vec3 p, vec3 n, float cell) {
  vec3 q = p / cell, base = floor(q);
  vec3 t1, t2;
  reliefFrame(n, t1, t2);
  float best = 0.0;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) for (int z = -1; z <= 1; z++) {
    vec3 c = base + vec3(float(x), float(y), float(z));
    vec3 r = reliefHash3(c);
    vec3 d = (q - (c + 0.1 + 0.8 * r)) / (0.6 + 0.45 * reliefHash(c + 3.7));
    float d2 = dot(d, d);
    if (d2 >= 1.0) continue;
    float dome = pow(1.0 - d2, 1.5);
    float angle = atan(dot(d, t2), dot(d, t1));
    // Irregular blade ridges: noise read around a circle about the clump's centre (so it closes on itself), drifting
    // outward, so blades are uneven in width and spacing rather than a regular star.
    float ring = reliefShape.z / 6.2831853;
    float ridges = reliefNoise(vec3(cos(angle) * ring, sin(angle) * ring, sqrt(d2) * 1.5 + r.z * 40.0));
    ridges = smoothstep(0.25, 0.85, ridges);
    float crown = dome * (1.0 - reliefShape.w * sqrt(d2) * (1.0 - ridges));
    float k = 0.12;
    float h = clamp(0.5 + 0.5 * (crown - best) / k, 0.0, 1.0);
    best = mix(best, crown, h) + k * h * (1.0 - h);
  }
  return best;
}
// The relief at a surface point p with ground normal n, 0 to height. Clumps (their outlines warped by fractal noise,
// so they are ragged lobes, not round pebbles) are gathered into mounds by a smooth low-frequency noise: in a mound they
// stand full and run together, between mounds they are low and sparse; fine fractal roughness goes over all.
float reliefHeight(vec3 p, vec3 n) {
  vec3 warp = vec3(reliefFractal(p * 3.5), reliefFractal(p * 3.5 + 5.2), reliefFractal(p * 3.5 + 9.7)) - 0.5;
  float clumps = reliefClumps(p + warp * reliefMore.x, n, reliefShape.y);
  float mound = smoothstep(0.3, 0.72, reliefNoise(p * reliefMore.y + 11.3) * 0.7 + reliefFractal(p * reliefMore.y * 2.6 + 4.1) * 0.3);
  float h = clumps * mix(0.3, 1.0, mound) + reliefMore.z * mound * 0.6 + reliefMore.w * (reliefFractal(p * 12.0) - 0.5);
  return reliefShape.x * clamp(h / (1.0 + 0.6 * reliefMore.z), 0.0, 1.0);
}
// The ground normal tipped by the relief's slope along the surface at p.
vec3 reliefNormal(vec3 p, vec3 n) {
  vec3 t1, t2;
  reliefFrame(n, t1, t2);
  float e = 0.006;
  float g1 = (reliefHeight(p + t1 * e, n) - reliefHeight(p - t1 * e, n)) / (2.0 * e);
  float g2 = (reliefHeight(p + t2 * e, n) - reliefHeight(p - t2 * e, n)) / (2.0 * e);
  return normalize(n - t1 * g1 - t2 * g2);
}
`;

const PAINT_GLSL = /* glsl */`
uniform vec3 msSunDirection;
uniform highp sampler2D groundColor;
uniform vec4 groundRect;
uniform vec3 shadeTint;
uniform vec3 shadeBand; // x: threshold, y: range, z: cavity
uniform highp sampler2DShadow msFoliageShadow;
uniform mat4 msFoliageShadowMatrix;
uniform vec4 msFoliageShadowParams;
float reliefCast(vec3 world, vec3 normal) {
  if (msFoliageShadowParams.x < 0.5) return 1.0;
  vec4 c = msFoliageShadowMatrix * vec4(world + normal * msFoliageShadowParams.z, 1.0);
  c.xyz /= c.w;
  if (c.x < 0.0 || c.x > 1.0 || c.y < 0.0 || c.y > 1.0 || c.z > 1.0) return 1.0;
  float depth = c.z + msFoliageShadowParams.y, lit = 0.0;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) lit += texture(msFoliageShadow, vec3(c.xy + vec2(float(x), float(y)) * msFoliageShadowParams.w * 1.5, depth));
  return lit / 9.0;
}
vec3 reliefPaint(vec3 world, vec3 n, float h) {
  vec2 uv = clamp((world.xy - groundRect.xy) / (groundRect.zw - groundRect.xy), 0.0, 1.0);
  vec3 base = texture(groundColor, uv).rgb * mix(1.0 - shadeBand.z, 1.0, clamp(h / max(reliefShape.x, 1e-4), 0.0, 1.0));
  float band = min(smoothstep(-shadeBand.y, shadeBand.y, dot(n, normalize(msSunDirection)) - shadeBand.x), reliefCast(world, n));
  return base * mix(shadeTint, vec3(1.0), band);
}
`;

/** The lawn's triangles in world space, each edge divided into `n` (n² triangles each), with normals. */
function divided(ground: THREE.Mesh, n: number): THREE.BufferGeometry {
  ground.updateWorldMatrix(true, false);
  const source = ground.geometry.index ? ground.geometry.toNonIndexed() : ground.geometry;
  const position = source.getAttribute("position"), normal = source.getAttribute("normal");
  const turn = new THREE.Matrix3().getNormalMatrix(ground.matrixWorld);
  const out: number[] = [], normals: number[] = [];
  const p = [new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3()], q = [new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3()];
  const at = (u: number, v: number) => {
    const w = 1 - u - v;
    const point = p[0].clone().multiplyScalar(w).addScaledVector(p[1], u).addScaledVector(p[2], v);
    const nn = q[0].clone().multiplyScalar(w).addScaledVector(q[1], u).addScaledVector(q[2], v).normalize();
    return [point, nn] as const;
  };
  const push = (...vs: (readonly [THREE.Vector3, THREE.Vector3])[]) => { for (const [a, b] of vs) { out.push(a.x, a.y, a.z); normals.push(b.x, b.y, b.z); } };
  for (let i = 0; i + 2 < position.count; i += 3) {
    for (let k = 0; k < 3; k++) {
      p[k].fromBufferAttribute(position, i + k).applyMatrix4(ground.matrixWorld);
      if (normal) q[k].fromBufferAttribute(normal, i + k).applyMatrix3(turn).normalize(); else q[k].set(0, 0, 1);
      if (q[k].z < 0) q[k].negate();
    }
    for (let a = 0; a < n; a++) for (let b = 0; b < n - a; b++) {
      push(at(a / n, b / n), at((a + 1) / n, b / n), at(a / n, (b + 1) / n));
      if (a + b < n - 1) push(at((a + 1) / n, b / n), at((a + 1) / n, (b + 1) / n), at(a / n, (b + 1) / n));
    }
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(out, 3));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute(normals, 3));
  return geometry;
}

const VERTEX = /* glsl */`
uniform float lift; // 0: none, 1: by the relief (displace), 2: by its full height (parallax shell)
varying vec3 vWorld;
varying vec3 vGround;
varying vec3 vBase;
${RELIEF_GLSL}
void main() {
  vec3 world = (modelMatrix * vec4(position, 1.0)).xyz;
  vBase = world;
  vGround = normalize(mat3(modelMatrix) * normal);
  if (lift > 1.5) world += vGround * reliefShape.x;
  else if (lift > 0.5) world += vGround * reliefHeight(world, vGround);
  vWorld = world;
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}`;

function fragment(mode: ReliefMode, steps: number): string {
  const body = mode === "parallax" ? `
  // From the top of the relief down along the view ray until it meets the clumps.
  vec3 v = normalize(vWorld - cameraPosition);
  float down = max(-dot(v, ground), 0.12);
  float reach = reliefShape.x / down;
  float stepLength = reach / float(${steps});
  float t = 0.0, before = 0.0;
  vec3 point = vWorld;
  bool met = false;
  for (int i = 0; i < ${steps}; i++) {
    t = float(i + 1) * stepLength;
    point = vWorld + v * t;
    float below = -dot(point - vWorld, ground);
    if (below >= reliefShape.x - reliefHeight(point + ground * below - ground * reliefShape.x, ground)) { met = true; break; }
    before = t;
  }
  if (met) for (int i = 0; i < 5; i++) {
    float mid = 0.5 * (before + t);
    vec3 m = vWorld + v * mid;
    float mb = -dot(m - vWorld, ground);
    if (mb >= reliefShape.x - reliefHeight(m + ground * mb - ground * reliefShape.x, ground)) t = mid; else before = mid;
  }
  point = vWorld + v * t;
  vec4 clip = projectionMatrix * viewMatrix * vec4(point, 1.0);
  gl_FragDepth = clamp(0.5 * clip.z / clip.w + 0.5, 0.0, 1.0);
  float pb = -dot(point - vWorld, ground);
  vec3 base = point + ground * pb - ground * reliefShape.x;
  float h = reliefHeight(base, ground);
  vec3 n = reliefNormal(base, ground);` : `
  vec3 point = vWorld;
  // The point on the undisplaced lawn this one rose from.
  vec3 base = vBase;
  float h = reliefHeight(base, ground);
  vec3 n = reliefNormal(base, ground);`;
  return /* glsl */`
#include <common>
uniform mat4 projectionMatrix;
varying vec3 vWorld;
varying vec3 vGround;
varying vec3 vBase;
${RELIEF_GLSL}
${PAINT_GLSL}
void main() {
  vec3 ground = normalize(vGround);${body}
  gl_FragColor = vec4(reliefPaint(point, n, h), 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}`;
}

/** The lawn drawn with grass relief by `mode`; the caller hides the lawn itself. */
export function grassRelief(ground: THREE.Mesh, mode: ReliefMode, options: GrassReliefOptions = {}): THREE.Mesh {
  const geometry = divided(ground, mode === "displace" ? options.divisions ?? 10 : 1);
  const field = sceneFieldUniforms("ground.color");
  const material = new THREE.ShaderMaterial({
    vertexShader: VERTEX, fragmentShader: fragment(mode, options.steps ?? 32),
    uniforms: {
      msSunDirection: programSun, groundColor: field.texture, groundRect: field.rect,
      lift: { value: mode === "displace" ? 1 : mode === "parallax" ? 2 : 0 },
      reliefShape: { value: new THREE.Vector4(options.height ?? 0.11, options.cell ?? 0.45, options.blades ?? 14, options.ridge ?? 0.08) },
      reliefMore: { value: new THREE.Vector4(options.warp ?? 0.16, options.moundScale ?? 0.9, options.moundWeight ?? 0.8, options.fine ?? 0.03) },
      shadeTint: { value: new THREE.Vector3(...(options.shadeTint ?? [0.3, 0.44, 0.66])) },
      shadeBand: { value: new THREE.Vector3(options.threshold ?? 0.76, options.range ?? 0.3, options.cavity ?? 0.25) },
      ...(options.shadow ?? {}),
    },
  });
  followPaintedFinishing(material);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = `grass/relief-${mode}`;
  mesh.receiveShadow = true;
  // The scene's `?clumpblur=` pass may blend its colours among themselves.
  mesh.userData.clumpBlur = true;
  mesh.userData.grassRelief = { mode, triangles: geometry.getAttribute("position").count / 3 };
  return mesh;
}
