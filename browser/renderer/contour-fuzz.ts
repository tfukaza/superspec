import * as THREE from "three";
import { BAND_GLSL } from "./leaf-normals.ts";

/**
 * Contour fuzz (prototype): leafy billboards around the outline of any mesh, so a plain shape (a pyramid, a blob) reads
 * as a bush whose silhouette is made of leaves from every view.
 *
 * Anchors are scattered once along every edge of the mesh, each with the normals of the two faces that meet there. A
 * frame shows an anchor's billboard only while its edge is on the silhouette, one face toward the camera and the other
 * away, faded by how clearly it is (so billboards ease in and out as the view turns rather than pop), and pushes it
 * outward across the edge so half of it stands past the outline. The anchors never move, so the same leaves stay on the
 * same edge as the camera orbits: the fuzz is temporally consistent. A billboard is a leaf card from an atlas, turned
 * by its own random angle, in the mesh's colour.
 */
export interface ContourFuzzOptions {
  /** Billboard size, meters. */
  size?: number;
  /** Anchors per meter of edge. */
  density?: number;
  /** How far past the edge a billboard's middle stands, as a share of its size. */
  reach?: number;
  color?: THREE.ColorRepresentation;
  seed?: number;
  /** With a sun and the cards' normals, the cards are banded like a leaf-normal-mapped surface (leaf-normals.ts). */
  sun?: THREE.Vector3;
  normals?: THREE.Texture;
  /** With a leaf map (leaf-normals.ts), the cards take their normals and tones from it, as the surface does. */
  leafMap?: { texture: THREE.Texture; tile: number; origin: THREE.Vector3; strength?: number; offset?: number };
  /**
   * Meters: anchor the cards on a coarsened copy of the mesh, its vertices merged per cell of this size within each
   * connected part ({@link coarsen}). The fuzz needs edges about a card long, not the surface's fine ones, and its
   * anchor count follows the edges' total length. 0 keeps the mesh's own edges.
   */
  coarse?: number;
}

/**
 * The mesh simplified by vertex clustering: positions welded, split into connected parts (so separate clumps never
 * merge), each part's vertices merged per cubic cell of `cell` meters into their mean, and the triangles that collapse
 * dropped. Only positions are kept (the fuzz takes face normals from them).
 */
export function coarsen(geometry: THREE.BufferGeometry, cell: number): THREE.BufferGeometry {
  const position = geometry.getAttribute("position");
  const index = geometry.getIndex();
  const count = index ? index.count : position.count;
  const corner = (i: number) => (index ? index.getX(i) : i);
  // Weld by position, then join the parts through the triangles.
  const weld = new Map<string, number>(), welded: number[] = [], points: THREE.Vector3[] = [];
  for (let v = 0; v < position.count; v++) {
    const p = new THREE.Vector3().fromBufferAttribute(position, v);
    const key = `${p.x.toFixed(5)},${p.y.toFixed(5)},${p.z.toFixed(5)}`;
    let w = weld.get(key);
    if (w === undefined) { w = points.length; weld.set(key, w); points.push(p); }
    welded.push(w);
  }
  const parent = points.map((_, i) => i);
  const root = (i: number): number => { while (parent[i] !== i) i = parent[i] = parent[parent[i]]; return i; };
  for (let t = 0; t < count; t += 3) {
    const a = root(welded[corner(t)]);
    for (const k of [1, 2]) { const b = root(welded[corner(t + k)]); if (a !== b) parent[Math.max(a, b)] = Math.min(a, b); }
  }
  // One merged vertex per part and cell, at the mean of its members.
  const merged = new Map<string, number>(), sums: THREE.Vector3[] = [], counts: number[] = [];
  const target = points.map((p, i) => {
    const key = `${root(i)}:${Math.floor(p.x / cell)},${Math.floor(p.y / cell)},${Math.floor(p.z / cell)}`;
    let m = merged.get(key);
    if (m === undefined) { m = sums.length; merged.set(key, m); sums.push(new THREE.Vector3()); counts.push(0); }
    sums[m].add(p); counts[m]++;
    return m;
  });
  const positions = new Float32Array(sums.length * 3);
  sums.forEach((s, m) => s.divideScalar(counts[m]).toArray(positions, m * 3));
  const triangles: number[] = [], seen = new Set<string>();
  for (let t = 0; t < count; t += 3) {
    const [a, b, c] = [0, 1, 2].map(k => target[welded[corner(t + k)]]);
    if (a === b || b === c || a === c) continue;
    const key = [a, b, c].sort((x, y) => x - y).join(",");
    if (seen.has(key)) continue;
    seen.add(key);
    triangles.push(a, b, c);
  }
  const out = new THREE.BufferGeometry();
  out.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  out.setIndex(triangles);
  return out;
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

/** Every edge of a triangle mesh with the normals of the faces that share it (one for a border edge). */
function edges(geometry: THREE.BufferGeometry): { a: THREE.Vector3; b: THREE.Vector3; normals: THREE.Vector3[] }[] {
  const position = geometry.getAttribute("position");
  const index = geometry.getIndex();
  const count = index ? index.count : position.count;
  const vertex = (i: number) => new THREE.Vector3().fromBufferAttribute(position, index ? index.getX(i) : i);
  const key = (p: THREE.Vector3) => `${p.x.toFixed(5)},${p.y.toFixed(5)},${p.z.toFixed(5)}`;
  const found = new Map<string, { a: THREE.Vector3; b: THREE.Vector3; normals: THREE.Vector3[] }>();
  for (let t = 0; t < count; t += 3) {
    const p = [vertex(t), vertex(t + 1), vertex(t + 2)];
    const normal = new THREE.Vector3().subVectors(p[1], p[0]).cross(new THREE.Vector3().subVectors(p[2], p[0])).normalize();
    for (let e = 0; e < 3; e++) {
      const a = p[e], b = p[(e + 1) % 3];
      const ka = key(a), kb = key(b), k = ka < kb ? `${ka}|${kb}` : `${kb}|${ka}`;
      const edge = found.get(k) ?? { a, b, normals: [] };
      edge.normals.push(normal);
      found.set(k, edge);
    }
  }
  return [...found.values()];
}

export function contourFuzz(geometry: THREE.BufferGeometry, cards: THREE.Texture, options: ContourFuzzOptions = {}): THREE.Mesh {
  const { size = 0.3, density = 14, reach = 0.08, color = 0x3a3d3f, seed = 1, sun, normals, leafMap, coarse = 0 } = options;
  const random = generator(seed);
  const anchors: number[] = [], first: number[] = [], second: number[] = [], randoms: number[] = [];
  for (const edge of edges(coarse > 0 ? coarsen(geometry, coarse) : geometry)) {
    // An edge with one face is a hole the coarsening tore, inside the surface rather than on its outline: no card.
    // (Taking the missing face as the first turned over made the card always shown, and its light normal, their sum,
    // zero: a flat card in the darkest band.)
    if (edge.normals.length < 2) continue;
    const n1 = edge.normals[0], n2 = edge.normals[1];
    const length = edge.a.distanceTo(edge.b);
    // `density` per meter on average, the fraction rounded at random: short edges get a card only sometimes, so the
    // count follows the edges' total length rather than how finely the mesh is divided.
    const share = length * density;
    const count = Math.floor(share) + (random() < share - Math.floor(share) ? 1 : 0);
    if (count === 0) continue;
    for (let k = 0; k < count; k++) {
      // Spread along the edge, a little jittered; the ends overlap the corners.
      const t = Math.min(Math.max((k + 0.5 + (random() - 0.5) * 0.8) / count, 0), 1);
      const p = new THREE.Vector3().lerpVectors(edge.a, edge.b, t);
      anchors.push(p.x, p.y, p.z);
      first.push(n1.x, n1.y, n1.z);
      second.push(n2.x, n2.y, n2.z);
      randoms.push(random());
    }
  }
  const quad = new THREE.PlaneGeometry(1, 1);
  const instanced = new THREE.InstancedBufferGeometry();
  instanced.index = quad.index;
  instanced.setAttribute("position", quad.getAttribute("position"));
  instanced.setAttribute("uv", quad.getAttribute("uv"));
  instanced.setAttribute("aAnchor", new THREE.InstancedBufferAttribute(new Float32Array(anchors), 3));
  instanced.setAttribute("aFirst", new THREE.InstancedBufferAttribute(new Float32Array(first), 3));
  instanced.setAttribute("aSecond", new THREE.InstancedBufferAttribute(new Float32Array(second), 3));
  instanced.setAttribute("aRandom", new THREE.InstancedBufferAttribute(new Float32Array(randoms), 1));
  instanced.instanceCount = randoms.length;
  const material = new THREE.ShaderMaterial({
    uniforms: {
      cards: { value: cards }, size: { value: size }, reach: { value: reach }, color: { value: new THREE.Color(color) },
      cardNormals: { value: normals ?? cards }, sun: { value: (sun ?? new THREE.Vector3(0, 0, 1)).clone().normalize() },
      shaded: { value: sun && (normals || leafMap) ? 1 : 0 }, offset: { value: leafMap?.offset ?? 0.1 },
      useMap: { value: leafMap ? 1 : 0 }, leafMap: { value: leafMap?.texture ?? cards }, mapTile: { value: leafMap?.tile ?? 1 },
      mapOrigin: { value: leafMap?.origin.clone() ?? new THREE.Vector3() }, mapStrength: { value: leafMap?.strength ?? 1 },
    },
    vertexShader: /* glsl */`
attribute vec3 aAnchor;
attribute vec3 aFirst;
attribute vec3 aSecond;
attribute float aRandom;
uniform float size;
uniform float reach;
varying vec2 vUv;
varying float vRandom;
varying vec3 vEdgeNormal;
varying vec3 vSide;
varying vec3 vUp;
varying vec3 vWorld;
void main() {
  vec3 anchor = (modelMatrix * vec4(aAnchor, 1.0)).xyz;
  vec3 n1 = normalize(mat3(modelMatrix) * aFirst), n2 = normalize(mat3(modelMatrix) * aSecond);
  vec3 view = normalize(cameraPosition - anchor);
  float d1 = dot(n1, view), d2 = dot(n2, view);
  // On the silhouette when one face looks toward the camera and the other away; how clearly, by the smaller of the two.
  float silhouette = d1 * d2 < 0.0 ? smoothstep(0.0, 0.12, min(abs(d1), abs(d2))) : 0.0;
  // A billboard grows in as its edge turns onto the outline, each at its own threshold.
  float grow = smoothstep(aRandom * 0.35, aRandom * 0.35 + 0.35, silhouette);
  if (grow < 0.02) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
  // Outward across the edge in the picture: the two faces' mean normal, flattened onto the view plane.
  vec3 out_ = n1 + n2;
  out_ -= view * dot(out_, view);
  out_ = length(out_) > 1e-4 ? normalize(out_) : vec3(0.0, 0.0, 1.0);
  vec3 side = normalize(cross(vec3(0.0, 0.0, 1.0), view) + vec3(1e-5, 0.0, 0.0)), up = cross(view, side);
  float roll = 6.2831853 * aRandom * 7.0;
  vec2 corner = mat2(cos(roll), sin(roll), -sin(roll), cos(roll)) * position.xy;
  float s = size * grow * (0.8 + 0.4 * fract(aRandom * 13.7));
  // The card sits across the edge (its middle just past it), and a little toward the camera, so its inner half lies
  // over the face and the leaves join the shape rather than floating beside it.
  vec3 world = anchor + out_ * s * reach + (side * corner.x + up * corner.y) * s + view * s * 0.5;
  vUv = uv;
  vRandom = aRandom;
  // The card's light comes from its edge (the two faces' mean normal), tipped by the card's own dome in its axes.
  // Two faces folded flat against each other have no mean: their outward direction across the edge stands in.
  vEdgeNormal = length(n1 + n2) > 1e-3 ? normalize(n1 + n2) : out_;
  vSide = side * cos(roll) + up * sin(roll);
  vUp = -side * sin(roll) + up * cos(roll);
  vWorld = world;
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}`,
    fragmentShader: /* glsl */`
uniform sampler2D cards;
uniform sampler2D cardNormals;
uniform vec3 color;
uniform vec3 sun;
uniform float shaded;
uniform float offset;
varying vec2 vUv;
varying float vRandom;
varying vec3 vEdgeNormal;
varying vec3 vSide;
varying vec3 vUp;
varying vec3 vWorld;
uniform float useMap;
uniform sampler2D leafMap;
uniform float mapTile;
uniform vec3 mapOrigin;
uniform float mapStrength;
${BAND_GLSL}
void main() {
  // One of the atlas's 16 dense cards (rows 0 to 3): solid in the middle, leafy at the rim.
  float cell = floor(fract(vRandom * 31.7) * 16.0);
  vec2 at = (vec2(mod(cell, 4.0), floor(cell / 4.0)) + 0.02 + clamp(vUv, 0.0, 1.0) * 0.96) / vec2(4.0, 8.0);
  vec4 card = texture2D(cards, at);
  if (card.a < 0.5) discard;
  if (shaded > 0.5 && useMap > 0.5) {
    // The surface's own leaf map at this point: the leaves run on from the face into the fuzz.
    vec3 t, b;
    vec3 leaf = msLeafMap(leafMap, vWorld, mapOrigin, mapTile, t, b);
    float z = sqrt(max(0.0, 1.0 - dot(leaf.xy, leaf.xy)));
    vec3 n = normalize(normalize(vEdgeNormal) * z + (t * leaf.x + b * leaf.y) * mapStrength);
    float light = msLeafLight(n, sun, offset);
    gl_FragColor = vec4(msLeafBandAA(light, fwidth(light)) * (0.78 + 0.3 * leaf.z), 1.0);
  } else if (shaded > 0.5) {
    vec2 dome = texture2D(cardNormals, at).rg * 2.0 - 1.0;
    // A leaf's own tilt from its hue (a stable random per leaf), as the normal map's leaves have.
    float h = card.g * 6.2831853;
    vec3 n = normalize(vEdgeNormal + (vSide * (dome.x + 0.3 * cos(h)) + vUp * (dome.y + 0.3 * sin(h))) * 0.9);
    gl_FragColor = vec4(msLeafBand(dot(n, sun) * 0.5 + 0.5 + offset) * (0.9 + 0.2 * card.r), 1.0);
  } else gl_FragColor = vec4(color * (0.9 + 0.2 * card.r), 1.0);
  #include <colorspace_fragment>
}`,
  });
  const mesh = new THREE.Mesh(instanced, material);
  mesh.frustumCulled = false;
  mesh.userData.contourFuzz = { anchors: randoms.length };
  return mesh;
}
