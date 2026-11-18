import * as THREE from "three";
import { followPaintedFinishing, programSun } from "./shade.ts";
import { sceneFieldUniforms } from "./scene-fields.ts";

/**
 * Grass as imaginary clumps (an experiment, as the painted trees began from rough masses): soft mounds scattered over a
 * ground mesh, each a squat ellipsoid sunk partly into the ground, drawn on a camera-facing card and traced per pixel
 * (the ray's hit gives the mound's normal and depth). A mound takes the ground's paint under its root (`ground.color`)
 * and the lawn's shade band on its own normal, so its top melts into the lawn and only its flanks turn lighter toward
 * the sun or into the blue-green shade. Its outline is a wide soft fringe, so neighbours run together as a blurred
 * carpet of lumps rather than discs. Cards are sorted far to near before each draw and blended.
 */
export interface GrassClumpOptions {
  /** Mean meters between clump roots. */
  spacing?: number;
  /** Clump radius range (m). */
  radius?: [number, number];
  /** Height over radius range. */
  height?: [number, number];
  /** How far below its root a mound's centre sinks, as a share of its height. */
  sink?: number;
  /** Width of the soft fringe, as a share of the radius. */
  soft?: number;
  /** How much a mound's normal flattens toward the ground's as its fringe fades (0 none, 1 fully). */
  flatten?: number;
  /** The lawn's shade: tint, threshold and gradient range (painted-lawn.material.ts). */
  shadeTint?: [number, number, number];
  threshold?: number;
  range?: number;
  /** Random lighter or darker per clump (share). */
  tone?: number;
  seed?: number;
  /** The foliage's shadow-map uniforms (`msFoliageShadow`, `msFoliageShadowMatrix`, `msFoliageShadowParams`), so the sun's cast shadow falls on the clumps. */
  shadow?: Record<string, THREE.IUniform>;
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

const VERTEX = /* glsl */`
attribute vec3 clumpRoot;
attribute vec3 clumpUp;
// x: radius, y: height, z: random.
attribute vec3 clumpSize;
uniform float sink;
uniform float soft;
varying vec3 vWorld;
varying vec3 vCentre;
varying vec3 vUp;
varying vec3 vSize;
varying vec3 vRoot;
void main() {
  vec3 up = normalize(clumpUp);
  float r = clumpSize.x, h = clumpSize.y;
  vec3 centre = clumpRoot - up * h * sink;
  vec3 toCamera = normalize(cameraPosition - centre);
  vec3 side = cross(vec3(0.0, 0.0, 1.0), toCamera);
  side = length(side) < 1e-4 ? vec3(1.0, 0.0, 0.0) : normalize(side);
  vec3 lift = cross(toCamera, side);
  float reach = max(r, h) * (1.0 + soft) * 1.15;
  // The card stands in front of the mound, so the ground behind it never cuts it; depth is the traced hit's.
  vec3 p = centre + toCamera * reach + (side * position.x + lift * position.y) * reach * 1.6;
  vWorld = p; vCentre = centre; vUp = up; vSize = clumpSize; vRoot = clumpRoot;
  gl_Position = projectionMatrix * viewMatrix * vec4(p, 1.0);
}`;

const FRAGMENT = /* glsl */`
#include <common>
uniform mat4 projectionMatrix;
uniform vec3 msSunDirection;
uniform highp sampler2D groundColor;
uniform vec4 groundRect;
uniform float soft;
uniform float flatten;
uniform vec3 shadeTint;
uniform float threshold;
uniform float range;
uniform float tone;
uniform highp sampler2DShadow msFoliageShadow;
uniform mat4 msFoliageShadowMatrix;
uniform vec4 msFoliageShadowParams;
float castAt(vec3 world, vec3 normal) {
  if (msFoliageShadowParams.x < 0.5) return 1.0;
  vec4 c = msFoliageShadowMatrix * vec4(world + normal * msFoliageShadowParams.z, 1.0);
  c.xyz /= c.w;
  if (c.x < 0.0 || c.x > 1.0 || c.y < 0.0 || c.y > 1.0 || c.z > 1.0) return 1.0;
  float depth = c.z + msFoliageShadowParams.y, lit = 0.0;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) lit += texture(msFoliageShadow, vec3(c.xy + vec2(float(x), float(y)) * msFoliageShadowParams.w * 1.5, depth));
  return lit / 9.0;
}
varying vec3 vWorld;
varying vec3 vCentre;
varying vec3 vUp;
varying vec3 vSize;
varying vec3 vRoot;
void main() {
  vec3 up = normalize(vUp);
  vec3 t1 = normalize(abs(up.x) < 0.9 ? cross(up, vec3(1.0, 0.0, 0.0)) : cross(up, vec3(0.0, 1.0, 0.0)));
  vec3 t2 = cross(up, t1);
  float r = vSize.x, h = vSize.y;
  vec3 rd = normalize(vWorld - cameraPosition), o = cameraPosition - vCentre;
  // In the mound's own space it is the unit sphere.
  vec3 oe = vec3(dot(o, t1) / r, dot(o, t2) / r, dot(o, up) / h);
  vec3 de = vec3(dot(rd, t1) / r, dot(rd, t2) / r, dot(rd, up) / h);
  float a = dot(de, de), b = dot(oe, de), c = dot(oe, oe) - 1.0;
  float closest = -b / a;
  // How far the ray passes from the centre: 0 through it, 1 at the outline.
  float miss = sqrt(max(dot(oe, oe) - b * b / a, 0.0));
  float alpha = 1.0 - smoothstep(1.0 - soft, 1.0 + soft, miss);
  if (alpha < 0.004) discard;
  float t = miss < 1.0 ? (-b - sqrt(max(b * b - a * c, 0.0))) / a : closest;
  vec3 pe = miss < 1.0 ? oe + de * t : normalize(oe + de * closest);
  vec3 n = normalize(t1 * pe.x / r + t2 * pe.y / r + up * pe.z / h);
  n = normalize(mix(up, n, mix(1.0, alpha, flatten)));
  vec3 hit = cameraPosition + rd * t;
  vec4 clip = projectionMatrix * viewMatrix * vec4(hit, 1.0);
  gl_FragDepth = clamp(0.5 * clip.z / clip.w + 0.5, 0.0, 1.0);
  vec2 uv = clamp((vRoot.xy - groundRect.xy) / (groundRect.zw - groundRect.xy), 0.0, 1.0);
  vec3 base = texture(groundColor, uv).rgb * (1.0 + (vSize.z - 0.5) * 2.0 * tone);
  float d = dot(n, normalize(msSunDirection)) - threshold;
  float band = min(smoothstep(-range, range, d), castAt(hit, n));
  vec4 diffuseColor = vec4(base * mix(shadeTint, vec3(1.0), band), alpha);
  gl_FragColor = diffuseColor;
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}`;

/** Clumps scattered over `ground` (its world placement as it is now). */
export function grassClumps(ground: THREE.Mesh, options: GrassClumpOptions = {}): THREE.Mesh {
  const spacing = options.spacing ?? 0.12;
  const [r0, r1] = options.radius ?? [0.07, 0.16];
  const [h0, h1] = options.height ?? [0.45, 0.75];
  const next = random(options.seed ?? 7);
  ground.updateWorldMatrix(true, false);
  const source = ground.geometry.index ? ground.geometry.toNonIndexed() : ground.geometry;
  const position = source.getAttribute("position"), normal = source.getAttribute("normal");
  const turn = new THREE.Matrix3().getNormalMatrix(ground.matrixWorld);
  const triangles: { a: THREE.Vector3; b: THREE.Vector3; c: THREE.Vector3; na: THREE.Vector3; nb: THREE.Vector3; nc: THREE.Vector3 }[] = [];
  const cumulative: number[] = [];
  let total = 0;
  for (let i = 0; i + 2 < position.count; i += 3) {
    const [a, b, c] = [i, i + 1, i + 2].map(k => new THREE.Vector3().fromBufferAttribute(position, k).applyMatrix4(ground.matrixWorld));
    const face = new THREE.Vector3().subVectors(b, a).cross(new THREE.Vector3().subVectors(c, a));
    const area = face.length() / 2;
    if (area <= 0) continue;
    face.normalize();
    if (face.z < 0) face.negate();
    const [na, nb, nc] = [i, i + 1, i + 2].map(k => normal ? new THREE.Vector3().fromBufferAttribute(normal, k).applyMatrix3(turn).normalize() : face.clone());
    for (const n of [na, nb, nc]) if (n.z < 0) n.negate();
    // Only ground that faces up enough to hold grass.
    if (face.z < 0.2) continue;
    total += area;
    triangles.push({ a, b, c, na, nb, nc });
    cumulative.push(total);
  }
  const count = Math.max(1, Math.round(total / (spacing * spacing)));
  const roots = new Float32Array(count * 3), ups = new Float32Array(count * 3), sizes = new Float32Array(count * 3);
  for (let k = 0; k < count; k++) {
    const pick = next() * total;
    let lo = 0, hi = cumulative.length - 1;
    while (lo < hi) { const mid = (lo + hi) >> 1; if (cumulative[mid] < pick) lo = mid + 1; else hi = mid; }
    const tri = triangles[lo];
    let u = next(), v = next();
    if (u + v > 1) { u = 1 - u; v = 1 - v; }
    const w = 1 - u - v;
    const p = tri.a.clone().multiplyScalar(w).addScaledVector(tri.b, u).addScaledVector(tri.c, v);
    const n = tri.na.clone().multiplyScalar(w).addScaledVector(tri.nb, u).addScaledVector(tri.nc, v).normalize();
    const radius = r0 + (r1 - r0) * next() ** 1.5;
    roots.set([p.x, p.y, p.z], k * 3);
    ups.set([n.x, n.y, n.z], k * 3);
    sizes.set([radius, radius * (h0 + (h1 - h0) * next()), next()], k * 3);
  }
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([-1, -1, 0, 1, -1, 0, 1, 1, 0, -1, 1, 0], 3));
  geometry.setIndex([0, 1, 2, 0, 2, 3]);
  const rootAttribute = new THREE.InstancedBufferAttribute(roots.slice(), 3);
  const upAttribute = new THREE.InstancedBufferAttribute(ups.slice(), 3);
  const sizeAttribute = new THREE.InstancedBufferAttribute(sizes.slice(), 3);
  geometry.setAttribute("clumpRoot", rootAttribute);
  geometry.setAttribute("clumpUp", upAttribute);
  geometry.setAttribute("clumpSize", sizeAttribute);
  geometry.instanceCount = count;
  geometry.boundingSphere = new THREE.Sphere();
  new THREE.Box3().setFromBufferAttribute(new THREE.Float32BufferAttribute(roots, 3)).expandByScalar(r1 * 3).getBoundingSphere(geometry.boundingSphere);
  const field = sceneFieldUniforms("ground.color");
  const material = new THREE.ShaderMaterial({
    vertexShader: VERTEX, fragmentShader: FRAGMENT, transparent: true, depthWrite: false, depthTest: true,
    uniforms: {
      msSunDirection: programSun, groundColor: field.texture, groundRect: field.rect,
      sink: { value: options.sink ?? 0.35 }, soft: { value: options.soft ?? 0.45 }, flatten: { value: options.flatten ?? 0.8 },
      shadeTint: { value: new THREE.Vector3(...(options.shadeTint ?? [0.3, 0.44, 0.66])) },
      threshold: { value: options.threshold ?? 0.76 }, range: { value: options.range ?? 0.3 }, tone: { value: options.tone ?? 0.06 },
      ...(options.shadow ?? { msFoliageShadow: { value: null }, msFoliageShadowMatrix: { value: new THREE.Matrix4() }, msFoliageShadowParams: { value: new THREE.Vector4() } }),
    },
  });
  followPaintedFinishing(material);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = "grass/clumps";
  mesh.frustumCulled = false;
  mesh.renderOrder = 2;
  // Far to near before each draw, so the soft fringes blend over what is behind them.
  const order = new Uint32Array(count), distance = new Float32Array(count);
  const eye = new THREE.Vector3();
  let last = "";
  mesh.onBeforeRender = (_renderer, _scene, camera) => {
    eye.setFromMatrixPosition(camera.matrixWorld);
    const key = eye.toArray().map(v => v.toFixed(3)).join(",");
    if (key === last) return;
    last = key;
    for (let k = 0; k < count; k++) {
      order[k] = k;
      const dx = roots[k * 3] - eye.x, dy = roots[k * 3 + 1] - eye.y, dz = roots[k * 3 + 2] - eye.z;
      distance[k] = dx * dx + dy * dy + dz * dz;
    }
    order.sort((x, y) => distance[y] - distance[x]);
    for (let k = 0; k < count; k++) {
      const from = order[k] * 3;
      for (let a = 0; a < 3; a++) {
        rootAttribute.array[k * 3 + a] = roots[from + a];
        upAttribute.array[k * 3 + a] = ups[from + a];
        sizeAttribute.array[k * 3 + a] = sizes[from + a];
      }
    }
    rootAttribute.needsUpdate = upAttribute.needsUpdate = sizeAttribute.needsUpdate = true;
  };
  mesh.userData.grassClumps = { count, area: total };
  // The scene's `?clumpblur=` pass blends their colours among themselves.
  mesh.userData.clumpBlur = true;
  return mesh;
}
