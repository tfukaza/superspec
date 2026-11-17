import * as THREE from "three";
import { BAND_GLSL } from "./leaf-normals.ts";
import { type DensityField, gradient, sampleDensity } from "./leaf-density.ts";

/**
 * Twig strips (prototype): a leafy twig painted as one stroke, a strip of paper laid along it, instead of round cards
 * that float off the branches like seeds. Each leaf-bearing branch's leafy stretch becomes a ribbon that faces the
 * camera across its width and follows the branch along its length, so a long shoot is a long stroke and a short one
 * a short one. Its width swells over the first third and tapers to a point at the tip, scaled by the leaf density
 * there (thin where the foliage is thin). The edges are cut into leaf-sized lobes, staggered between the two sides as
 * alternate leaves are, with some lobes short, so the outline reads as leaves along a stem.
 *
 * Shading follows the core and the density cards: the field's outward normal at each point, tipped across the strip
 * (one edge toward the light more than the other, as a painter models a sprig), then by the leaf map, then banded.
 */
export interface TwigStripOptions {
  /** Half the strip's width at its widest, meters, where the foliage is as dense as the core. */
  width?: number;
  /** Meters between a side's lobes (about a leaf's length). */
  lobe?: number;
  /** The core's level (share of the field's `full`): a strip is as wide as `width` there, narrower below. */
  level: number;
  /** Samples along each strip. */
  samples?: number;
  /** Sag under the leaves' weight: the tip drops this share of the strip's length, the drop growing as the square. */
  droop?: number;
  seed?: number;
  sun: THREE.Vector3;
  leafMap: { texture: THREE.Texture; tile: number; origin: THREE.Vector3; strength?: number; offset?: number };
}

/** Catmull-Rom through `points` at `t` (0 to 1 over the whole polyline). */
function along(points: THREE.Vector3[], t: number): THREE.Vector3 {
  const span = (points.length - 1) * Math.min(Math.max(t, 0), 1);
  const i = Math.min(Math.floor(span), points.length - 2), f = span - i;
  const p0 = points[Math.max(i - 1, 0)], p1 = points[i], p2 = points[i + 1], p3 = points[Math.min(i + 2, points.length - 1)];
  const f2 = f * f, f3 = f2 * f;
  return new THREE.Vector3()
    .addScaledVector(p0, -0.5 * f3 + f2 - 0.5 * f)
    .addScaledVector(p1, 1.5 * f3 - 2.5 * f2 + 1)
    .addScaledVector(p2, -1.5 * f3 + 2 * f2 + 0.5 * f)
    .addScaledVector(p3, 0.5 * f3 - 0.5 * f2);
}

export function twigStrips(field: DensityField, twigs: number[][], options: TwigStripOptions): THREE.Mesh {
  const { width = 0.2, lobe = 0.12, level, samples = 16, droop = 0.3, seed = 5, sun, leafMap } = options;
  let state = seed >>> 0;
  const random = () => { state = (Math.imul(state ^ (state >>> 15), 0x2c1b3c6d) + 0x6d2b79f5) >>> 0; return state / 4294967296; };
  const centres: number[] = [], tangents: number[] = [], normals: number[] = [], sides: number[] = [];
  const us: number[] = [], widths: number[] = [], strips: number[] = [], indices: number[] = [];
  const middle = field.origin.clone().addScaledVector(new THREE.Vector3(...field.dims), field.cell / 2);
  for (const flat of twigs) {
    const points: THREE.Vector3[] = [];
    for (let i = 0; i < flat.length; i += 3) points.push(new THREE.Vector3(flat[i], flat[i + 1], flat[i + 2]));
    if (points.length < 2) continue;
    const id = random();
    let length = 0;
    for (let i = 1; i < points.length; i++) length += points[i].distanceTo(points[i - 1]);
    if (length < 0.05) continue;
    // The leaves' weight bends the shoot down toward its tip.
    points.forEach((p, i) => { p.z -= droop * length * (i / (points.length - 1)) ** 2; });
    const first = centres.length / 3;
    for (let k = 0; k <= samples; k++) {
      const u = k / samples;
      const p = along(points, u);
      const tangent = along(points, Math.min(1, u + 0.02)).sub(along(points, Math.max(0, u - 0.02))).normalize();
      // Thin where the foliage is thin: half as wide in empty air as at the core's level.
      const share = Math.min(1, sampleDensity(field, p) / field.full / level);
      // Full-bodied: wide early, holding its width, then rounding off to the tip.
      const profile = u < 0.2 ? 0.7 + 0.3 * u / 0.2 : Math.sqrt(Math.max(0, 1 - Math.pow((u - 0.2) / 0.8, 3)));
      const w = width * profile * (0.5 + 0.5 * share);
      const n = gradient(field, p).negate();
      if (n.lengthSq() < 1e-12) n.subVectors(p, middle);
      n.normalize();
      for (const side of [-1, 1]) {
        centres.push(p.x, p.y, p.z);
        tangents.push(tangent.x, tangent.y, tangent.z);
        normals.push(n.x, n.y, n.z);
        sides.push(side);
        // u in meters along the strip, so lobes keep their size on any strip.
        us.push(u * length);
        widths.push(w);
        strips.push(id);
      }
      if (k < samples) {
        const a = first + 2 * k;
        indices.push(a, a + 1, a + 3, a, a + 3, a + 2);
      }
    }
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(centres, 3));
  geometry.setAttribute("aTangent", new THREE.Float32BufferAttribute(tangents, 3));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute(normals, 3));
  geometry.setAttribute("aSide", new THREE.Float32BufferAttribute(sides, 1));
  geometry.setAttribute("aU", new THREE.Float32BufferAttribute(us, 1));
  geometry.setAttribute("aWidth", new THREE.Float32BufferAttribute(widths, 1));
  geometry.setAttribute("aStrip", new THREE.Float32BufferAttribute(strips, 1));
  geometry.setIndex(indices);
  const material = new THREE.ShaderMaterial({
    side: THREE.DoubleSide,
    uniforms: {
      sun: { value: sun.clone().normalize() }, lobe: { value: lobe },
      leafMap: { value: leafMap.texture }, mapTile: { value: leafMap.tile }, mapOrigin: { value: leafMap.origin.clone() },
      mapStrength: { value: leafMap.strength ?? 1 }, offset: { value: leafMap.offset ?? 0.1 },
    },
    vertexShader: /* glsl */`
attribute vec3 aTangent;
attribute float aSide;
attribute float aU;
attribute float aWidth;
attribute float aStrip;
varying float vSide;
varying float vU;
varying float vStrip;
varying vec3 vNormal;
varying vec3 vAcross;
varying vec3 vWorld;
void main() {
  vec3 centre = (modelMatrix * vec4(position, 1.0)).xyz;
  vec3 tangent = normalize(mat3(modelMatrix) * aTangent);
  vec3 view = normalize(cameraPosition - centre);
  // Across the strip in the picture: the branch seen side-on gives the full width, end-on a point.
  vec3 across = cross(tangent, view);
  across = length(across) > 1e-4 ? normalize(across) : vec3(1.0, 0.0, 0.0);
  // The lobes stand past the strip's middle width by up to its half again; the fragment cuts them back.
  vec3 world = centre + across * aSide * aWidth * 1.5;
  vSide = aSide * 1.5;
  vU = aU;
  vStrip = aStrip;
  vNormal = normalize(mat3(modelMatrix) * normal);
  vAcross = across;
  vWorld = world;
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}`,
    fragmentShader: /* glsl */`
uniform vec3 sun;
uniform float lobe;
uniform sampler2D leafMap;
uniform float mapTile;
uniform vec3 mapOrigin;
uniform float mapStrength;
uniform float offset;
varying float vSide;
varying float vU;
varying float vStrip;
varying vec3 vNormal;
varying vec3 vAcross;
varying vec3 vWorld;
${BAND_GLSL}
float hash(float x) { return fract(sin(x * 91.17 + 3.1) * 43758.5453); }
void main() {
  // Lobes: a leaf every lobe meters on each side, the two sides half a leaf apart (alternate leaves), each its own
  // length, a pointed oval from its stem outward.
  // Not a comb: the spacing drifts along the strip, the two sides' leaves differ in size, some leaves are missing,
  // and the edge is ragged at a finer scale, as a loose stroke's would be.
  float side = vSide < 0.0 ? 0.0 : 1.0;
  float spacing = lobe * (side > 0.5 ? 1.0 : 1.3);
  float x = vU / spacing + side * 0.5 + vStrip * 7.0 + 0.45 * sin(vU / spacing * 1.7 + vStrip * 40.0 + side * 2.0);
  float index = floor(x), f = fract(x);
  float h = hash(index + side * 31.0 + vStrip * 17.0);
  float reach = h < 0.15 ? 0.15 : 0.3 + 0.55 * hash(index * 1.3 + side * 7.0 + vStrip * 5.0);
  // The leaf's outline across its length: 0 at its base and tip, round and full (a plump leaf, not a frond's
  // pinna), leaning a little toward the tip.
  float leaf = pow(sin(3.14159265 * pow(f, 0.8)), 0.45);
  float ragged = 0.14 * (hash(floor(vU / 0.025) + side * 13.0 + vStrip * 91.0) - 0.5);
  // A solid body most of the way out, the lobes only its rim.
  float edge = 0.65 + reach * leaf + ragged;
  float d = abs(vSide);
  if (d > edge) discard;
  vec3 t, b;
  vec3 m = msLeafMap(leafMap, vWorld, mapOrigin, mapTile, t, b);
  float z = sqrt(max(0.0, 1.0 - dot(m.xy, m.xy)));
  // The strip's own form: its normal turns toward each edge, so the edge nearer the light is lit.
  vec3 n = normalize(normalize(vNormal) + vAcross * clamp(vSide, -1.0, 1.0) * 0.6);
  n = normalize(n * z + (t * m.x + b * m.y) * mapStrength);
  float light = msLeafLight(n, sun, offset);
  gl_FragColor = vec4(msLeafBandAA(light, fwidth(light)) * (0.78 + 0.3 * m.z), 1.0);
  #include <colorspace_fragment>
}`,
  });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.frustumCulled = false;
  mesh.userData.twigStrips = { strips: twigs.length, triangles: indices.length / 3 };
  return mesh;
}
