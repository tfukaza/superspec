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
  /** A leaf's length, meters: every leaf on a strip is this size, smaller only toward the shoot's tip. */
  width?: number;
  /** Meters between a side's leaves. */
  lobe?: number;
  /** The core's level (share of the field's `full`): below it, the thinner the foliage the more leaves are missing. */
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
  const { width = 0.11, lobe = 0.08, level, samples = 16, droop = 0.3, seed = 5, sun, leafMap } = options;
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
      const n = gradient(field, p).negate();
      if (n.lengthSq() < 1e-12) n.subVectors(p, middle);
      n.normalize();
      for (const side of [-1, 1]) {
        centres.push(p.x, p.y, p.z);
        tangents.push(tangent.x, tangent.y, tangent.z);
        normals.push(n.x, n.y, n.z);
        sides.push(side);
        // u in meters along the strip, so leaves keep their size on any strip.
        us.push(u * length);
        // x: the strip's length (for where its tip is); y: how dense the foliage is there, 0 to 1 of the core's.
        widths.push(length, share);
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
  geometry.setAttribute("aShoot", new THREE.Float32BufferAttribute(widths, 2));
  geometry.setAttribute("aStrip", new THREE.Float32BufferAttribute(strips, 1));
  geometry.setIndex(indices);
  const material = new THREE.ShaderMaterial({
    side: THREE.DoubleSide,
    uniforms: {
      sun: { value: sun.clone().normalize() }, lobe: { value: lobe }, leafLength: { value: width },
      leafMap: { value: leafMap.texture }, mapTile: { value: leafMap.tile }, mapOrigin: { value: leafMap.origin.clone() },
      mapStrength: { value: leafMap.strength ?? 1 }, offset: { value: leafMap.offset ?? 0.1 }, highlight: { value: 1 },
    },
    vertexShader: /* glsl */`
attribute vec3 aTangent;
attribute float aSide;
attribute float aU;
attribute vec2 aShoot;
attribute float aStrip;
uniform float leafLength;
varying float vSide;
varying float vNarrow;
varying vec2 vShoot;
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
  // How side-on the shoot is seen (1 broadside, 0 end-on). Seen end-on its length shortens on screen while a full
  // width would stay, and the leaves would pile into a stack of round tiers; there too the strip's sideways direction
  // turns over between neighbouring samples and twists the quads between them, leaving pinholes. So the strip
  // narrows as it turns end-on, to nothing straight along the view, as a sprig seen from its tip does.
  float sideOn = length(across);
  float narrow = smoothstep(0.2, 0.65, sideOn);
  across = sideOn > 1e-4 ? across / sideOn : vec3(1.0, 0.0, 0.0);
  // A band just wide enough for a leaf standing off the stem at its angle; the fragment cuts the leaves out of it.
  float reach = leafLength * 0.95 * narrow;
  vec3 world = centre + across * aSide * reach;
  // Meters from the stem, signed by side.
  vSide = aSide * reach;
  vNarrow = narrow;
  vShoot = aShoot;
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
uniform float highlight;
uniform float leafLength;
varying float vSide;
varying float vNarrow;
varying vec2 vShoot;
varying float vU;
varying float vStrip;
varying vec3 vNormal;
varying vec3 vAcross;
varying vec3 vWorld;
${BAND_GLSL}
float hash(float x) { return fract(sin(x * 91.17 + 3.1) * 43758.5453); }
void main() {
  // Leaves: one every lobe meters on each side, the two sides half a step apart (alternate leaves). Each is a pointed
  // oval of one size (leafLength long, two fifths as wide) standing off the stem about 50 degrees toward the tip, its
  // place and angle a little jittered; smaller only toward the shoot's tip, as a real shoot's are. Some are missing,
  // more where the foliage is thin, so a shoot at the rim is sparse. A leaf reaches past its own step along the
  // shoot, so each pixel tests the leaves of its step and the two before.
  float side = vSide < 0.0 ? 0.0 : 1.0;
  // The strip's random, rounded: interpolation does not carry a value the same at every vertex through bit for bit,
  // and the hash below magnifies the difference, so unrounded it picked leaves per pixel (white pinholes).
  float strip = floor(vStrip * 4096.0 + 0.5) / 4096.0;
  // Across in leaf space: end-on, the band narrows on screen, and the leaves with it.
  float s = abs(vSide) / max(vNarrow, 1e-3);
  float shift = side * 0.5 + strip * 7.0;
  float first = floor(vU / lobe - shift);
  bool inside = false;
  // The next step's leaf too: its jittered base can fall just behind this pixel.
  for (int j = 0; j < 4; j++) {
    float index = first + 1.0 - float(j);
    float h = hash(index + side * 31.0 + strip * 17.0);
    if (h < 0.1 + 0.3 * (1.0 - clamp(vShoot.y, 0.0, 1.0))) continue;
    float u0 = (index + shift + 0.3 * (hash(index * 2.7 + side * 3.0 + strip * 11.0) - 0.5)) * lobe;
    float at = u0 / max(vShoot.x, 1e-3);
    if (at < 0.0 || at > 1.0) continue;
    float size = leafLength * clamp(1.0 - pow(max(at - 0.55, 0.0) / 0.45, 2.0), 0.35, 1.0);
    float angle = 0.87 + 0.35 * (hash(index * 5.3 + side * 9.0 + strip * 3.0) - 0.5);
    float dx = vU - u0;
    float along = dx * cos(angle) + s * sin(angle), off = -dx * sin(angle) + s * cos(angle);
    if (along <= 0.0 || along >= size) continue;
    if (abs(off) < 0.21 * size * pow(sin(3.14159265 * along / size), 0.75)) { inside = true; break; }
  }
  if (!inside) discard;
  vec3 t, b;
  vec3 m = msLeafMap(leafMap, vWorld, mapOrigin, mapTile, t, b);
  float z = sqrt(max(0.0, 1.0 - dot(m.xy, m.xy)));
  // Banded by the leaf map's normals over the foliage's outward normal there, as the core is: no shading of its own.
  vec3 n = normalize(normalize(vNormal) * z + (t * m.x + b * m.y) * mapStrength);
  float light = msLeafLight(n, sun, offset);
  vec3 paint = msLeafBandAA(light, fwidth(light)) * (0.78 + 0.3 * m.z);
  gl_FragColor = vec4(msLeafPaint(paint, light, m.z, highlight), 1.0);
  #include <colorspace_fragment>
}`,
  });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.frustumCulled = false;
  mesh.userData.twigStrips = { strips: twigs.length, triangles: indices.length / 3 };
  return mesh;
}
