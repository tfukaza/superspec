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
vec2 reliefHash2(vec2 c) { return fract(sin(vec2(dot(c, vec2(127.1, 311.7)), dot(c, vec2(269.5, 183.3)))) * 43758.5453); }
float reliefHash(vec2 c) { return fract(sin(dot(c, vec2(419.2, 371.9))) * 43758.5453); }
float reliefNoise(vec2 p) {
  vec2 i = floor(p), f = fract(p), s = f * f * (3.0 - 2.0 * f);
  return mix(mix(reliefHash(i), reliefHash(i + vec2(1, 0)), s.x), mix(reliefHash(i + vec2(0, 1)), reliefHash(i + vec2(1, 1)), s.x), s.y);
}
float reliefFractal(vec2 p) { return 0.5 * reliefNoise(p) + 0.3 * reliefNoise(p * 2.03 + 7.1) + 0.2 * reliefNoise(p * 4.07 + 3.3); }
// One grid of clumps: a ridged dome per cell, the tallest (smoothly) where they overlap.
float reliefClumps(vec2 p, float cell, float seed) {
  vec2 q = p / cell, base = floor(q);
  float best = 0.0;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) {
    vec2 c = base + vec2(float(x), float(y));
    vec2 r = reliefHash2(c + seed);
    vec2 centre = c + 0.15 + 0.7 * r;
    float size = 0.55 + 0.5 * reliefHash(c + seed + 3.7);
    vec2 d = (q - centre) / size;
    float d2 = dot(d, d);
    if (d2 >= 1.0) continue;
    float dome = (1.0 - d2) * (1.0 - d2);
    float angle = atan(d.y, d.x);
    float ridges = pow(0.5 + 0.5 * cos(angle * reliefShape.z + r.x * 6.2831853 + sqrt(d2) * 2.0), 1.5);
    float crown = dome * (1.0 - reliefShape.w * sqrt(d2) * (1.0 - ridges)) * (0.65 + 0.35 * size);
    // A smooth maximum, so where clumps meet the valley between them is rounded.
    float k = 0.08;
    float h = clamp(0.5 + 0.5 * (crown - best) / k, 0.0, 1.0);
    best = mix(best, crown, h) + k * h * (1.0 - h);
  }
  return best;
}
// The relief at a point of the plan, 0 to height.
float reliefHeight(vec2 p) {
  float meadow = 0.55 + 0.9 * smoothstep(0.25, 0.75, reliefFractal(p * 0.45 + 11.3));
  float a = reliefClumps(p, reliefShape.y, 0.0);
  float b = reliefClumps(p + vec2(0.37, 0.71) * reliefShape.y, reliefShape.y * 0.7, 17.0);
  float h = max(a, 0.8 * b) * meadow + 0.08 * (reliefFractal(p * 25.0) - 0.5);
  return reliefShape.x * clamp(h / 1.3, 0.0, 1.0);
}
// The ground normal tipped by the relief's slope at p.
vec3 reliefNormal(vec2 p, vec3 ground) {
  float e = 0.004;
  vec2 g = vec2(reliefHeight(p + vec2(e, 0.0)) - reliefHeight(p - vec2(e, 0.0)), reliefHeight(p + vec2(0.0, e)) - reliefHeight(p - vec2(0.0, e))) / (2.0 * e);
  vec3 slope = vec3(g, 0.0);
  return normalize(ground - (slope - ground * dot(ground, slope)));
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
${RELIEF_GLSL}
void main() {
  vec3 world = (modelMatrix * vec4(position, 1.0)).xyz;
  vGround = normalize(mat3(modelMatrix) * normal);
  if (lift > 1.5) world += vGround * reliefShape.x;
  else if (lift > 0.5) world += vGround * reliefHeight(world.xy);
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
    if (below >= reliefShape.x - reliefHeight(point.xy)) { met = true; break; }
    before = t;
  }
  if (met) for (int i = 0; i < 5; i++) {
    float mid = 0.5 * (before + t);
    vec3 m = vWorld + v * mid;
    if (-dot(m - vWorld, ground) >= reliefShape.x - reliefHeight(m.xy)) t = mid; else before = mid;
  }
  point = vWorld + v * t;
  vec4 clip = projectionMatrix * viewMatrix * vec4(point, 1.0);
  gl_FragDepth = clamp(0.5 * clip.z / clip.w + 0.5, 0.0, 1.0);
  float h = reliefHeight(point.xy);
  vec3 n = reliefNormal(point.xy, ground);` : `
  vec3 point = vWorld;
  float h = reliefHeight(point.xy);
  vec3 n = reliefNormal(point.xy, ground);`;
  return /* glsl */`
#include <common>
uniform mat4 projectionMatrix;
varying vec3 vWorld;
varying vec3 vGround;
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
      reliefShape: { value: new THREE.Vector4(options.height ?? 0.045, options.cell ?? 0.16, options.blades ?? 7, options.ridge ?? 0.3) },
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
