import * as THREE from "three";
import { BAND_GLSL } from "./leaf-normals.ts";
import { type DensityField, gradient, sampleDensity } from "./leaf-density.ts";
import { branchCardAtlas } from "./branch-cards.ts";

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
  /**
   * How much a card turns to face the camera: 1 always facing it (a billboard), 0 fixed in the world at its own
   * orientation (a game tree's card: it keeps its place as the view turns and goes thin edge-on), between a blend.
   */
  facing?: number;
  /**
   * Hybrid: with a branch card atlas (branch-cards.ts), the sparser cards are branchlets instead of round sparse
   * clumps, rooted at their point and reaching out of the foliage (drooping a little), `branchSize` meters long, a
   * `branchKeep` share of them kept (being bigger, fewer are needed), and shaded by their own leaves' normals.
   */
  branches?: THREE.Texture;
  branchSize?: number;
  branchKeep?: number;
  seed?: number;
  sun: THREE.Vector3;
  leafMap: { texture: THREE.Texture; tile: number; origin: THREE.Vector3; strength?: number; offset?: number };
}

let branchTexture: THREE.DataTexture | undefined;
/** The branch card atlas (branch-cards.ts) as a texture, made once: 16 dense sprays, mipmapped. */
export function branchCardTexture(): THREE.DataTexture {
  if (branchTexture) return branchTexture;
  const atlas = branchCardAtlas(4, 256, 1, 2);
  branchTexture = new THREE.DataTexture(atlas.data, atlas.width, atlas.height, THREE.RGBAFormat, THREE.UnsignedByteType);
  branchTexture.magFilter = THREE.LinearFilter;
  branchTexture.minFilter = THREE.LinearMipmapLinearFilter;
  branchTexture.generateMipmaps = true;
  branchTexture.needsUpdate = true;
  return branchTexture;
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
  const { level, floor = 0.1, spacing = 0.18, keep = 0.9, size = 0.55, inside = 1.12, seed = 3, facing = 1, sun, leafMap,
    branches, branchSize = 1.2, branchKeep = 0.45 } = options;
  const random = generator(seed);
  const centres: number[] = [], normals: number[] = [], randoms: number[] = [], looks: number[] = [];
  const planes: number[] = [], axes: number[] = [];
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
    // Dense cards near the core, sparse toward the rim (blended at random between), and smaller as the field thins.
    const sparse = random() > Math.min(1, Math.max(0, (depth - 0.25) / 0.5)) ? 1 : 0;
    // A branch card is bigger, so fewer of them.
    if (sparse && branches && random() > branchKeep) continue;
    centres.push(p.x, p.y, p.z);
    normals.push(n.x, n.y, n.z);
    let plane: THREE.Vector3, up: THREE.Vector3;
    if (sparse && branches) {
      // A branchlet lies along the way out of the foliage, sagging a little: its up is that way, and its plane holds
      // it, turned about it at random but mostly lying flat (its normal toward the sky), as a spray carries its leaves.
      // Out from the trunk as well as out of the foliage, and pulled down, so a spray at the crown (where the foliage's
      // outward is straight up) arches over and out instead of standing up.
      const radial = new THREE.Vector3(p.x - middle.x, p.y - middle.y, 0);
      if (radial.lengthSq() > 1e-8) radial.normalize();
      up = n.clone().addScaledVector(radial, 0.6).addScaledVector(new THREE.Vector3(0, 0, 1), -0.6).normalize();
      plane = new THREE.Vector3(random() - 0.5, random() - 0.5, 0.9 + random() - 0.5).projectOnPlane(up);
      if (plane.lengthSq() < 1e-6) plane.set(1, 0, 0).projectOnPlane(up);
      plane.normalize();
    } else {
      // The card's own orientation in the world, as a game tree's cards have: its plane faces out of the foliage with
      // a random tilt, its up roughly against gravity, turned a little at random about its normal.
      plane = n.clone().add(new THREE.Vector3(random() - 0.5, random() - 0.5, random() - 0.5).multiplyScalar(1.4)).normalize();
      up = new THREE.Vector3(0, 0, 1).projectOnPlane(plane);
      if (up.lengthSq() < 1e-6) up.set(1, 0, 0).projectOnPlane(plane);
      up.normalize().applyAxisAngle(plane, (random() - 0.5) * 1.4);
    }
    planes.push(plane.x, plane.y, plane.z);
    axes.push(up.x, up.y, up.z);
    randoms.push(random());
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
  instanced.setAttribute("aPlane", new THREE.InstancedBufferAttribute(new Float32Array(planes), 3));
  instanced.setAttribute("aAxis", new THREE.InstancedBufferAttribute(new Float32Array(axes), 3));
  instanced.instanceCount = randoms.length;
  const material = new THREE.ShaderMaterial({
    uniforms: {
      cards: { value: cards }, size: { value: size }, sun: { value: sun.clone().normalize() }, facing: { value: facing },
      branches: { value: branches ?? cards }, hybrid: { value: branches ? 1 : 0 }, branchSize: { value: branchSize },
      leafMap: { value: leafMap.texture }, mapTile: { value: leafMap.tile }, mapOrigin: { value: leafMap.origin.clone() },
      mapStrength: { value: leafMap.strength ?? 1 }, offset: { value: leafMap.offset ?? 0.1 }, highlight: { value: 1 },
    },
    vertexShader: /* glsl */`
attribute vec3 aCentre;
attribute vec3 aNormal;
attribute float aRandom;
attribute float aLook;
attribute vec3 aPlane;
attribute vec3 aAxis;
uniform float size;
uniform float facing;
uniform float hybrid;
uniform float branchSize;
varying vec2 vUv;
varying float vRandom;
varying float vSparse;
varying vec3 vNormal;
varying vec3 vWorld;
varying vec3 vCard;
varying vec3 vSideAxis;
varying vec3 vUpAxis;
void main() {
  vec3 centre = (modelMatrix * vec4(aCentre, 1.0)).xyz;
  vec3 view = normalize(cameraPosition - centre);
  vSparse = floor(aLook);
  // A branch card (the hybrid's sparse cards) keeps its up the way out of the foliage, also as it faces the camera.
  bool branch = hybrid > 0.5 && vSparse > 0.5;
  vec3 plane = normalize(mat3(modelMatrix) * aPlane), upW = normalize(mat3(modelMatrix) * aAxis);
  // The camera's axes and the card's own, blended by facing. The card's are turned (a half turn in its plane, or a
  // mirror) to agree with the camera's as nearly as they can, so the blend never passes through zero. A branch card's
  // camera up is its own up laid into the picture, and it is never turned over: its stem stays at its root.
  vec3 sideC = normalize(cross(vec3(0.0, 0.0, 1.0), view) + vec3(1e-5, 0.0, 0.0)), upC = cross(view, sideC);
  if (branch) {
    vec3 laid = upW - view * dot(upW, view);
    if (length(laid) > 1e-3) { upC = normalize(laid); sideC = normalize(cross(upC, view)); }
  }
  vec3 sideW = cross(upW, plane);
  if (dot(sideW, sideC) < 0.0) sideW = -sideW;
  if (!branch && dot(upW, upC) < 0.0) upW = -upW;
  vec3 side = normalize(mix(sideW, sideC, facing)), up = normalize(mix(upW, upC, facing));
  // The card's own normal, turned toward the viewer, for its shading.
  vCard = dot(plane, view) < 0.0 ? -plane : plane;
  // Rolled at random only as far as it faces the camera: a world card's turn is in its own up. A branch card only
  // sways a little about its root.
  float roll = branch ? 0.5 * (fract(aRandom * 7.0) - 0.5) : 6.2831853 * fract(aRandom * 7.0) * facing;
  vec2 corner = mat2(cos(roll), sin(roll), -sin(roll), cos(roll)) * position.xy;
  float s = (branch ? branchSize * (0.6 + 0.4 * fract(aLook) * 2.0) : size * 2.0 * fract(aLook)) * (0.85 + 0.3 * fract(aRandom * 13.7));
  // A branch card stands on its root (the bottom of the card, a little inside), the others on their middle.
  vec3 world = centre + (side * corner.x + up * (corner.y + (branch ? 0.42 : 0.0))) * s;
  vSideAxis = side;
  vUpAxis = up;
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
uniform float facing;
uniform sampler2D branches;
uniform float hybrid;
varying vec2 vUv;
varying float vRandom;
varying float vSparse;
varying vec3 vNormal;
varying vec3 vWorld;
varying vec3 vCard;
varying vec3 vSideAxis;
varying vec3 vUpAxis;
${BAND_GLSL}
void main() {
  // The foliage's outward normal, tipped toward the card's own as far as it keeps its own orientation, so cards at
  // different angles take different light, as a game tree's do.
  vec3 base = normalize(normalize(vNormal) + normalize(vCard) * 0.7 * (1.0 - facing));
  if (hybrid > 0.5 && vSparse > 0.5) {
    // A branch card: one of 16 branchlets (4 by 4), its stem at the card's bottom. Shaded by its own leaves' normals
    // in the card's axes, and their tones, so the bands fall on its leaves wherever it turns.
    float cell = floor(fract(vRandom * 31.7) * 16.0);
    vec2 at = (vec2(mod(cell, 4.0), floor(cell / 4.0)) + 0.01 + clamp(vUv, 0.0, 1.0) * 0.98) / 4.0;
    vec4 branch = texture2D(branches, at);
    if (branch.a < 0.5) discard;
    vec2 own = branch.rg * 2.0 - 1.0;
    float z = sqrt(max(0.0, 1.0 - dot(own, own)));
    vec3 n = normalize(base * z + (normalize(vSideAxis) * own.x + normalize(vUpAxis) * own.y) * 1.1);
    float light = msLeafLight(n, sun, offset);
    vec3 paint = msLeafBandAA(light, fwidth(light)) * (0.78 + 0.3 * branch.b);
    gl_FragColor = vec4(msLeafPaint(paint, light, branch.b, highlight), 1.0);
    #include <colorspace_fragment>
    return;
  }
  // One of the atlas's 16 dense cards (rows 0 to 3) or 16 sparse ones (rows 4 to 7).
  float cell = floor(fract(vRandom * 31.7) * 16.0) + 16.0 * vSparse;
  vec2 at = (vec2(mod(cell, 4.0), floor(cell / 4.0)) + 0.02 + clamp(vUv, 0.0, 1.0) * 0.96) / vec2(4.0, 8.0);
  vec4 card = texture2D(cards, at);
  if (card.a < 0.5) discard;
  vec3 t, b;
  vec3 leaf = msLeafMap(leafMap, vWorld, mapOrigin, mapTile, t, b);
  float z = sqrt(max(0.0, 1.0 - dot(leaf.xy, leaf.xy)));
  vec3 n = normalize(base * z + (t * leaf.x + b * leaf.y) * mapStrength);
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
