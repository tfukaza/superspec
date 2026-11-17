import * as THREE from "three";
import { BAND_GLSL } from "./leaf-normals.ts";
import { type DensityField, gradient, sampleDensity } from "./leaf-density.ts";

/**
 * Density cards (prototype): leaf cards through the foliage below the painted core (leaf-density.ts), so the plant
 * thins from its solid mass through looser leaves to single shoots instead of ending at the core's outline.
 *
 * Candidate points on a jittered grid through the field are kept by how dense the field is there: many near the core,
 * few at the faint rim, none in empty air or deep inside the core (where nothing would show). A card's look follows the
 * density too: the dense atlas cards near the core, sparse ones further out, each smaller as the field thins. Every
 * card faces the camera, turned by its own angle, and is shaded as the core is: the field's outward normal (its falling
 * gradient) tipped by the surface's own leaf map at the card's world position, so light carries on from the core into
 * the loose leaves around it.
 */
export interface DensityCardOptions {
  /** The core's level (a share of the field's `full`): cards fill the field below it. */
  level: number;
  /** Below this share of `full` the field is empty air. */
  floor?: number;
  /** Meters between candidate points. */
  spacing?: number;
  /** Share of candidates kept at the core's level; fewer as the field thins. */
  keep?: number;
  /** Card size at the core, meters; smaller toward the rim. */
  size?: number;
  /** Above `level` by this share, candidates are inside the core and dropped (a little over 1 lets cards straddle it). */
  inside?: number;
  seed?: number;
  sun: THREE.Vector3;
  leafMap: { texture: THREE.Texture; tile: number; origin: THREE.Vector3; strength?: number; offset?: number };
}

function generator(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function densityCards(field: DensityField, cards: THREE.Texture, options: DensityCardOptions): THREE.Mesh {
  const { level, floor = 0.1, spacing = 0.18, keep = 0.9, size = 0.55, inside = 1.12, seed = 3, sun, leafMap } = options;
  const random = generator(seed);
  const centres: number[] = [], normals: number[] = [], randoms: number[] = [], looks: number[] = [];
  const lo = field.origin, span = field.dims.map(d => (d - 1) * field.cell);
  const steps = span.map(s => Math.floor(s / spacing));
  const p = new THREE.Vector3();
  const middle = new THREE.Vector3(lo.x + span[0] / 2, lo.y + span[1] / 2, lo.z + span[2] / 2);
  for (let z = 0; z < steps[2]; z++) for (let y = 0; y < steps[1]; y++) for (let x = 0; x < steps[0]; x++) {
    p.set(lo.x + (x + random()) * spacing, lo.y + (y + random()) * spacing, lo.z + (z + random()) * spacing);
    const share = sampleDensity(field, p) / field.full;
    if (share < floor || share > level * inside) continue;
    // How far from air to the core this point is, 0 to 1.
    const depth = Math.min(1, (share - floor) / (level - floor));
    // Kept on an eased ramp, so the cards thin out gradually toward the rim, where the twig strips take over, with no
    // step where they stop.
    const ramp = depth * depth * (3 - 2 * depth);
    if (random() > keep * Math.pow(ramp, 1.3)) continue;
    const n = gradient(field, p).negate();
    if (n.lengthSq() < 1e-12) n.subVectors(p, middle);
    n.normalize();
    centres.push(p.x, p.y, p.z);
    normals.push(n.x, n.y, n.z);
    randoms.push(random());
    // Dense cards near the core, sparse toward the rim (blended at random between), and smaller as the field thins.
    const sparse = random() > Math.min(1, Math.max(0, (depth - 0.25) / 0.5)) ? 1 : 0;
    looks.push(sparse + 0.5 * (0.65 + 0.35 * depth));
  }
  const quad = new THREE.PlaneGeometry(1, 1);
  const instanced = new THREE.InstancedBufferGeometry();
  instanced.index = quad.index;
  instanced.setAttribute("position", quad.getAttribute("position"));
  instanced.setAttribute("uv", quad.getAttribute("uv"));
  instanced.setAttribute("aCentre", new THREE.InstancedBufferAttribute(new Float32Array(centres), 3));
  instanced.setAttribute("aNormal", new THREE.InstancedBufferAttribute(new Float32Array(normals), 3));
  instanced.setAttribute("aRandom", new THREE.InstancedBufferAttribute(new Float32Array(randoms), 1));
  // Whole part: 1 for a sparse card; fraction: half its scale.
  instanced.setAttribute("aLook", new THREE.InstancedBufferAttribute(new Float32Array(looks), 1));
  instanced.instanceCount = randoms.length;
  const material = new THREE.ShaderMaterial({
    uniforms: {
      cards: { value: cards }, size: { value: size }, sun: { value: sun.clone().normalize() },
      leafMap: { value: leafMap.texture }, mapTile: { value: leafMap.tile }, mapOrigin: { value: leafMap.origin.clone() },
      mapStrength: { value: leafMap.strength ?? 1 }, offset: { value: leafMap.offset ?? 0.1 }, highlight: { value: 1 },
    },
    vertexShader: /* glsl */`
attribute vec3 aCentre;
attribute vec3 aNormal;
attribute float aRandom;
attribute float aLook;
uniform float size;
varying vec2 vUv;
varying float vRandom;
varying float vSparse;
varying vec3 vNormal;
varying vec3 vWorld;
void main() {
  vec3 centre = (modelMatrix * vec4(aCentre, 1.0)).xyz;
  vec3 view = normalize(cameraPosition - centre);
  vec3 side = normalize(cross(vec3(0.0, 0.0, 1.0), view) + vec3(1e-5, 0.0, 0.0)), up = cross(view, side);
  float roll = 6.2831853 * fract(aRandom * 7.0);
  vec2 corner = mat2(cos(roll), sin(roll), -sin(roll), cos(roll)) * position.xy;
  vSparse = floor(aLook);
  float s = size * 2.0 * fract(aLook) * (0.85 + 0.3 * fract(aRandom * 13.7));
  vec3 world = centre + (side * corner.x + up * corner.y) * s;
  vUv = uv;
  vRandom = aRandom;
  vNormal = normalize(mat3(modelMatrix) * aNormal);
  vWorld = world;
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}`,
    fragmentShader: /* glsl */`
uniform sampler2D cards;
uniform sampler2D leafMap;
uniform vec3 sun;
uniform float mapTile;
uniform vec3 mapOrigin;
uniform float mapStrength;
uniform float offset;
uniform float highlight;
varying vec2 vUv;
varying float vRandom;
varying float vSparse;
varying vec3 vNormal;
varying vec3 vWorld;
${BAND_GLSL}
void main() {
  // One of the atlas's 16 dense cards (rows 0 to 3) or 16 sparse ones (rows 4 to 7).
  float cell = floor(fract(vRandom * 31.7) * 16.0) + 16.0 * vSparse;
  vec2 at = (vec2(mod(cell, 4.0), floor(cell / 4.0)) + 0.02 + clamp(vUv, 0.0, 1.0) * 0.96) / vec2(4.0, 8.0);
  vec4 card = texture2D(cards, at);
  if (card.a < 0.5) discard;
  vec3 t, b;
  vec3 leaf = msLeafMap(leafMap, vWorld, mapOrigin, mapTile, t, b);
  float z = sqrt(max(0.0, 1.0 - dot(leaf.xy, leaf.xy)));
  vec3 n = normalize(normalize(vNormal) * z + (t * leaf.x + b * leaf.y) * mapStrength);
  float light = msLeafLight(n, sun, offset);
  vec3 paint = msLeafBandAA(light, fwidth(light)) * (0.78 + 0.3 * leaf.z);
  gl_FragColor = vec4(msLeafPaint(paint, light, leaf.z, highlight), 1.0);
  #include <colorspace_fragment>
}`,
  });
  const mesh = new THREE.Mesh(instanced, material);
  mesh.frustumCulled = false;
  mesh.userData.densityCards = { cards: randoms.length };
  return mesh;
}
