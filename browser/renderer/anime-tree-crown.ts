import * as THREE from "three";
import type { FoliageMass } from "../model/packet.ts";

/**
 * Illustrated trees (prototype; generated/verification/anime-tree): what the looks in anime-tree.ts share. A plant's
 * mass (`FoliageMass`: its leafy twigs) is read as a crown of leaf clusters: stations along every twig, grouped into
 * rounded clusters about `clusterRadius` across, each station knowing the light it would get as part of its cluster
 * and of the crown: a "puffy" normal (out of the cluster's ellipsoid, leaning out of the crown's and upward) and an
 * occlusion (clusters above it, its depth in its cluster and in the crown). Every look draws distinct leaves at those
 * stations and shades each whole leaf with one step of a ramp from that light, so a cluster reads as a rounded mass
 * of leaves, lit on top and dark beneath, whatever the sun's exact angle.
 */

/** A station on a twig where leaves grow. */
export interface CrownStation {
  position: THREE.Vector3;
  /** The twig's direction there, and how far along its leafy stretch (0 its start, 1 its tip). */
  direction: THREE.Vector3; along: number;
  cluster: number;
  /** The light's normal there and its occlusion (0 open to the sky, 1 buried). */
  normal: THREE.Vector3; occlusion: number;
}
export interface CrownCluster {
  centre: THREE.Vector3; radii: THREE.Vector3;
  /** How much of the sky above it other clusters cover, 0 to 1. */
  covered: number;
  stations: number;
}
export interface Crown { stations: CrownStation[]; clusters: CrownCluster[]; centre: THREE.Vector3; radii: THREE.Vector3; scale: number }

/** A deterministic generator of numbers in [0, 1). */
export function generator(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** The light's normal and occlusion at `p`, as a part of `cluster` and of the crown. */
export function crownShade(crown: Pick<Crown, "centre" | "radii">, cluster: CrownCluster, p: THREE.Vector3): { normal: THREE.Vector3; occlusion: number } {
  const d = p.clone().sub(cluster.centre).divide(cluster.radii);
  const own = d.clone().divide(cluster.radii);
  if (own.lengthSq() < 1e-12) own.set(0, 0, 1);
  own.normalize();
  const c = p.clone().sub(crown.centre).divide(crown.radii);
  const broad = c.clone().divide(crown.radii);
  if (broad.lengthSq() < 1e-12) broad.set(0, 0, 1);
  broad.normalize();
  const normal = own.multiplyScalar(0.72).addScaledVector(broad, 0.28).add(new THREE.Vector3(0, 0, 0.22)).normalize();
  const inner = 1 - Math.min(1, d.length()), under = Math.min(1, Math.max(0, -d.z)), deep = 1 - Math.min(1, c.length());
  // Clusters above shade its top less the higher a point stands in it; its underside is in its own shade.
  const occlusion = Math.min(1, 0.38 * cluster.covered * (1 - 0.35 * Math.max(0, Math.min(1, d.z))) + 0.25 * inner + 0.35 * under + 0.28 * deep);
  return { normal, occlusion };
}

/**
 * The crown of `mass` placed by `placement`: stations every `spacing` meters along its twigs (and at every tip),
 * grouped into clusters about `clusterRadius` meters in radius.
 */
export function analyseCrown(mass: FoliageMass, placement: THREE.Matrix4, options: { spacing?: number; clusterRadius?: number; seed?: number; fill?: number; tier?: number } = {}): Crown {
  const scale = placement.getMaxScaleOnAxis();
  const spacing = (options.spacing ?? 0.3) * scale, clusterRadius = (options.clusterRadius ?? 0.85) * scale;
  const random = generator(options.seed ?? 17);
  const stations: CrownStation[] = [];
  const a = new THREE.Vector3(), b = new THREE.Vector3();
  for (const twig of mass.twigs) {
    if (twig.length < 2) continue;
    const points = twig.map(([x, y, z]) => new THREE.Vector3(x, y, z).applyMatrix4(placement));
    let total = 0;
    for (let k = 1; k < points.length; k++) total += points[k].distanceTo(points[k - 1]);
    if (total <= 1e-6) continue;
    // Stations from the tip back, so every twig ends in leaves; a random phase keeps neighbours from lining up.
    const count = Math.max(1, Math.round(total / spacing));
    for (let s = 0; s < count; s++) {
      const at = total * (1 - (s + 0.35 * random()) / count);
      let walked = 0;
      for (let k = 1; k < points.length; k++) {
        const length = points[k].distanceTo(points[k - 1]);
        if (walked + length >= at || k === points.length - 1) {
          a.copy(points[k - 1]); b.copy(points[k]);
          const t = length > 0 ? Math.min(1, Math.max(0, (at - walked) / length)) : 0;
          stations.push({ position: a.clone().lerp(b, t), direction: b.clone().sub(a).normalize(), along: at / total, cluster: 0,
            normal: new THREE.Vector3(0, 0, 1), occlusion: 0 });
          break;
        }
        walked += length;
      }
    }
  }
  const box = new THREE.Box3();
  for (const station of stations) box.expandByPoint(station.position);
  const centre = box.getCenter(new THREE.Vector3());
  const radii = box.getSize(new THREE.Vector3()).multiplyScalar(0.5).addScalar(0.3 * scale);
  // Cluster centres: stations taken in a shuffled order, each a new centre when none is near; then a few rounds of
  // moving every centre to the middle of its stations.
  const order = stations.map((_, index) => index);
  for (let i = order.length - 1; i > 0; i--) { const j = Math.floor(random() * (i + 1)); [order[i], order[j]] = [order[j], order[i]]; }
  let centres: THREE.Vector3[] = [];
  // Clusters are flatter than wide (tiers): vertical distances count for more.
  const stretch = 1.5;
  const distance = (p: THREE.Vector3, q: THREE.Vector3) => Math.hypot(p.x - q.x, p.y - q.y, (p.z - q.z) * stretch);
  for (const index of order) {
    const p = stations[index].position;
    if (!centres.some(c => distance(c, p) < clusterRadius * 1.35)) centres.push(p.clone());
  }
  for (let round = 0; round < 4; round++) {
    const sums = centres.map(() => new THREE.Vector3()), counts = centres.map(() => 0);
    for (const station of stations) {
      let best = 0, nearest = Infinity;
      centres.forEach((c, i) => { const d = distance(c, station.position); if (d < nearest) { nearest = d; best = i; } });
      station.cluster = best; sums[best].add(station.position); counts[best]++;
    }
    const kept: THREE.Vector3[] = [];
    centres.forEach((c, i) => { if (counts[i] > 0) kept.push(sums[i].multiplyScalar(1 / counts[i])); });
    if (round < 3) centres = kept;
  }
  const clusters: CrownCluster[] = centres.map(c => ({ centre: c.clone(), radii: new THREE.Vector3(), covered: 0, stations: 0 }));
  for (const station of stations) {
    const cluster = clusters[station.cluster], d = station.position.clone().sub(cluster.centre);
    cluster.radii.add(d.multiply(d)); cluster.stations++;
  }
  for (const cluster of clusters) {
    const n = Math.max(1, cluster.stations), floor = 0.35 * clusterRadius;
    const rx = Math.max(floor, 1.9 * Math.sqrt(cluster.radii.x / n)), ry = Math.max(floor, 1.9 * Math.sqrt(cluster.radii.y / n));
    const wide = Math.max(rx, ry);
    cluster.radii.set(Math.max(rx, 0.7 * wide), Math.max(ry, 0.7 * wide), Math.min(Math.max(floor, 1.9 * Math.sqrt(cluster.radii.z / n)), 0.72 * wide));
  }
  for (const cluster of clusters) {
    let above = 0;
    for (const other of clusters) {
      const rise = other.centre.z - cluster.centre.z;
      if (other === cluster || rise < 0.25 * clusterRadius) continue;
      const flat = Math.hypot(other.centre.x - cluster.centre.x, other.centre.y - cluster.centre.y);
      // A cluster above covers it by how far their outlines overlap from above, less so the farther it is up.
      const reach = 0.5 * (cluster.radii.x + cluster.radii.y + other.radii.x + other.radii.y) + 0.35 * rise;
      above += Math.max(0, 1 - flat / reach) * Math.min(1, other.stations / 12);
    }
    cluster.covered = 1 - Math.exp(-0.75 * above);
  }
  // Fuller clusters: `fill` more stations about each one, pointing out of its cluster.
  const fill = options.fill ?? 0, spread = 0.38 * scale;
  for (const station of stations.slice()) {
    for (let k = 0; k < fill; k++) {
      const cluster = clusters[station.cluster];
      const position = station.position.clone().add(new THREE.Vector3(random() - 0.5, random() - 0.5, random() - 0.5).multiplyScalar(2 * spread));
      const out = position.clone().sub(cluster.centre).divide(cluster.radii).divide(cluster.radii);
      const direction = station.direction.clone().multiplyScalar(0.5).add(out.lengthSq() > 1e-9 ? out.normalize() : out.set(0, 0, 1)).normalize();
      stations.push({ position, direction, along: station.along, cluster: station.cluster, normal: new THREE.Vector3(0, 0, 1), occlusion: 0 });
    }
  }
  // Tiers: each cluster's stations gather toward a dome over its middle (a canopy of leaves over a hollow), and draw
  // in a little from its neighbours, so clusters part and the limbs show between them.
  const tier = options.tier ?? 0;
  if (tier > 0) for (const station of stations) {
    const cluster = clusters[station.cluster], d = station.position.clone().sub(cluster.centre);
    const flat = Math.min(1, Math.hypot(d.x / cluster.radii.x, d.y / cluster.radii.y));
    const dome = cluster.radii.z * (0.75 * Math.sqrt(1 - 0.85 * flat * flat) - 0.2);
    station.position.set(cluster.centre.x + d.x * (1 - 0.14 * tier), cluster.centre.y + d.y * (1 - 0.14 * tier), cluster.centre.z + d.z + (dome - d.z) * tier);
  }
  const crown = { stations, clusters, centre, radii, scale };
  for (const station of stations) {
    const shade = crownShade(crown, clusters[station.cluster], station.position);
    station.normal = shade.normal; station.occlusion = shade.occlusion;
  }
  return crown;
}

/**
 * The ramp the leaves are painted with, dark to light, as display colours (sampled from the anime tree concept: a deep
 * cool green in the shade through muted olives to a pale yellow-olive in the light), and a trunk's two browns.
 */
export const ANIME_RAMP = ["#232a25", "#2f372e", "#3d4435", "#50553c", "#6c6d49", "#8d8a5b", "#aeaa76"] as const;

/** How the leaves are lit: see `ANIME_SHADE_GLSL`. */
export const ANIME_LIGHT = {
  /** Share of the light that comes from straight above whatever the sun (top-lit clusters). */
  top: 0.45,
  /** How far full occlusion darkens, and how far being seen from beneath does. */
  occlusion: 0.55, underside: 0.45,
  /** A whole leaf's random shift of light: what makes single leaves read. */
  jitter: 0.2,
  /** Steps of the ramp. */
  bands: 12,
  /** The light stretched about its middle, and shifted. */
  contrast: 1.3, offset: 0.06,
  /** Warm light through leaves on the crown's edge seen against the sun. */
  back: 0.16,
} as const;

/** The uniforms of `ANIME_SHADE_GLSL`; `sun` and `time` are the scene's shared ones. */
export function animeUniforms(sun: THREE.IUniform<THREE.Vector3>, time: THREE.IUniform<number>): Record<string, THREE.IUniform> {
  return {
    sun, atTime: time,
    atRamp: { value: ANIME_RAMP.map(hex => new THREE.Color(hex)) },
    atLight: { value: new THREE.Vector4(ANIME_LIGHT.top, ANIME_LIGHT.occlusion, ANIME_LIGHT.jitter, ANIME_LIGHT.bands) },
    atCurve: { value: new THREE.Vector4(ANIME_LIGHT.contrast, ANIME_LIGHT.offset, ANIME_LIGHT.back, ANIME_LIGHT.underside) },
    // Wind: direction (x, y), sway in meters at a crown's edge, gusts a second.
    atWind: { value: new THREE.Vector4(0.8, 0.6, 0.035, 0.6) },
  };
}

/**
 * Shared GLSL. `atLightValue(normal, occlusion, jitter)` is the light of a leaf in [0, 1]: half the sun on its puffy
 * normal and half straight from above, darkened by its occlusion, shifted by the leaf's own jitter and stretched.
 * `atRampColour(light)` is its colour: the ramp at one of `atLight.w` steps. `atBackLight` adds the light that comes
 * through an edge leaf seen against the sun. `atSway` is a leaf's wind offset.
 */
export const ANIME_SHADE_GLSL = /* glsl */`
uniform vec3 sun;
uniform float atTime;
uniform vec3 atRamp[7];
uniform vec4 atLight;
uniform vec4 atCurve;
uniform vec4 atWind;
// A leaf on the far side of its cluster (its puffy normal turned from the camera) is seen from beneath: darker.
float atSeen(float light, vec3 n, vec3 toCamera) {
  return light * (1.0 - atCurve.w * smoothstep(0.2, -0.5, dot(n, toCamera)));
}
float atLightValue(vec3 n, float occlusion, float jitter) {
  float lit = dot(sun, sun) > 0.0 ? dot(n, sun) * 0.5 + 0.5 : 0.5;
  float v = mix(lit, n.z * 0.5 + 0.5, atLight.x);
  v *= 1.0 - occlusion * atLight.y;
  v = (v - 0.5) * atCurve.x + 0.5 + atCurve.y + (jitter - 0.5) * atLight.z;
  return clamp(v, 0.0, 1.0);
}
vec3 atRampColour(float v) {
  float q = min((floor(v * atLight.w) + 0.5) / atLight.w, 0.9999) * 6.0;
  float i = floor(q);
  vec3 a = atRamp[0], b = atRamp[1];
  if (i > 0.5) { a = atRamp[1]; b = atRamp[2]; }
  if (i > 1.5) { a = atRamp[2]; b = atRamp[3]; }
  if (i > 2.5) { a = atRamp[3]; b = atRamp[4]; }
  if (i > 3.5) { a = atRamp[4]; b = atRamp[5]; }
  if (i > 4.5) { a = atRamp[5]; b = atRamp[6]; }
  return mix(a, b, q - i);
}
vec3 atBackLight(vec3 colour, vec3 n, vec3 toCamera, float occlusion) {
  float against = max(0.0, dot(-toCamera, sun));
  float edge = 1.0 - abs(dot(n, toCamera));
  return colour + atRamp[5] * atCurve.z * against * against * edge * (1.0 - occlusion);
}
vec3 atSway(vec3 world, float phase, float weight) {
  float t = atTime * atWind.w * 6.2832;
  float gust = sin(t + dot(world.xy, atWind.xy) * 0.35) + 0.5 * sin(t * 2.3 + phase * 6.2832);
  return vec3(atWind.xy * gust, 0.25 * sin(t * 3.1 + phase * 12.0)) * atWind.z * weight;
}
`;

/** A pointed leaf's outline in a 2D canvas, from the origin along +y, `length` long (the context is transformed to it). */
function leafPath(context: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D, length: number, width: number, half = 0): void {
  context.beginPath();
  context.moveTo(0, 0);
  context.bezierCurveTo(width * 0.95, length * 0.22, width * 0.62, length * 0.68, 0, length);
  if (half) context.lineTo(0, 0);
  else context.bezierCurveTo(-width * 0.62, length * 0.68, -width * 0.95, length * 0.22, 0, 0);
  context.closePath();
}

function canvas2d(width: number, height: number): { canvas: OffscreenCanvas | HTMLCanvasElement; context: OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D } {
  const canvas = typeof OffscreenCanvas !== "undefined" ? new OffscreenCanvas(width, height) : Object.assign(document.createElement("canvas"), { width, height });
  const context = canvas.getContext("2d", { willReadFrequently: true }) as OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D;
  return { canvas, context };
}

/** One leaf stamped at (x, y) along `angle` (radians from +y toward +x, y up): red its tone, green its own id. */
function stampLeaf(context: OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D, x: number, y: number, angle: number, length: number, width: number, tone: number, id: number): void {
  const colour = (t: number) => `rgb(${Math.round(255 * Math.min(1, t))},${Math.round(255 * id)},0)`;
  context.save();
  context.translate(x, y);
  context.rotate(-angle);
  leafPath(context, length, width);
  context.fillStyle = colour(tone);
  context.fill();
  // One half a little darker (a folded leaf) and a fine midrib.
  leafPath(context, length, width, 1);
  context.fillStyle = colour(tone * 0.9);
  context.fill();
  context.strokeStyle = colour(tone * 0.78);
  context.lineWidth = length * 0.03;
  context.beginPath(); context.moveTo(0, length * 0.04); context.lineTo(0, length * 0.9); context.stroke();
  context.restore();
}

export const SPRAY_GRID = 4;
let sprays: THREE.DataTexture | undefined;
/**
 * An atlas of 4 × 4 painted sprays of distinct pointed leaves, for cards: ten fans (leaves spreading from a stem at the
 * card's foot, a smaller tier above) and six whorls (leaves all round the card's middle). Texels: r a leaf's tone (the
 * leaves behind darker, a darker half and midrib), g the leaf's own id (the same over the whole leaf, so a light step
 * taken from it falls on whole leaves), a coverage with antialiased edges. A card's uv has v = 0 at its tip.
 */
export function sprayAtlas(cell = 256): THREE.DataTexture {
  if (sprays) return sprays;
  const size = SPRAY_GRID * cell, { context } = canvas2d(size, size);
  const random = generator(91);
  const between = (a: number, b: number) => a + (b - a) * random();
  for (let index = 0; index < SPRAY_GRID * SPRAY_GRID; index++) {
    context.save();
    // The cell's frame: x across from its middle, y up from its foot, one unit its side.
    context.translate((index % SPRAY_GRID + 0.5) * cell, (Math.floor(index / SPRAY_GRID) + 1) * cell);
    context.scale(cell, -cell);
    context.beginPath(); context.rect(-0.5, 0, 1, 1); context.clip();
    const leaves: { x: number; y: number; angle: number; length: number; order: number }[] = [];
    if (index < 10) {
      // A fan: a tier of leaves from the stem's foot and a smaller one from higher up it.
      const lean = between(-0.12, 0.12);
      context.strokeStyle = "rgb(110,128,0)"; context.lineWidth = 0.012;
      context.beginPath(); context.moveTo(0, 0); context.lineTo(lean * 0.4, 0.42); context.stroke();
      for (const [foot, count, reach, spread] of [[0.1, 5 + Math.floor(random() * 3), 0.46, 1.35], [0.36, 4 + Math.floor(random() * 3), 0.4, 1.2]] as const) {
        for (let k = 0; k < count; k++) {
          const angle = lean + (count > 1 ? (k / (count - 1) * 2 - 1) : 0) * spread + between(-0.14, 0.14);
          // Side leaves are shorter and lie behind the middle ones.
          const length = reach * (1 - 0.3 * Math.abs(angle - lean) / spread) * between(0.85, 1.1);
          leaves.push({ x: lean * foot + Math.sin(angle) * 0.03, y: foot + Math.cos(angle) * 0.03, angle, length, order: -Math.abs(angle - lean) + foot + between(-0.2, 0.2) });
        }
      }
    } else {
      // A whorl seen from above: leaves all round, the last ones over the first.
      const count = 7 + Math.floor(random() * 3), turn = between(0, Math.PI * 2);
      for (let k = 0; k < count; k++) {
        const angle = turn + k / count * Math.PI * 2 + between(-0.2, 0.2);
        leaves.push({ x: Math.sin(angle) * 0.03, y: 0.5 + Math.cos(angle) * 0.03, angle, length: between(0.34, 0.44), order: random() });
      }
      for (let k = 0; k < 3; k++) {
        const angle = between(0, Math.PI * 2);
        leaves.push({ x: 0, y: 0.5, angle, length: between(0.2, 0.28), order: 2 + random() });
      }
    }
    leaves.sort((p, q) => p.order - q.order);
    leaves.forEach((leaf, k) => {
      // Later leaves lie on top and are lighter; every leaf has its own id.
      const depth = leaves.length > 1 ? k / (leaves.length - 1) : 1;
      stampLeaf(context, leaf.x, leaf.y, leaf.angle, leaf.length, leaf.length * between(0.3, 0.38), 0.62 + 0.38 * depth, between(0.04, 1));
    });
    context.restore();
  }
  const image = context.getImageData(0, 0, size, size), data = image.data;
  // Every texel a leaf does not fully cover takes the tone and id of the nearest one it does (two sweeps of a
  // city-block distance), so a leaf's filtered edge, at any mip level, is still that leaf's own step of the ramp.
  const far = new Uint16Array(size * size).fill(65535);
  for (let i = 0; i < size * size; i++) if (data[i * 4 + 3] >= 250) far[i] = 0;
  const take = (i: number, j: number) => { if (far[j] + 1 < far[i]) { far[i] = far[j] + 1; data[i * 4] = data[j * 4]; data[i * 4 + 1] = data[j * 4 + 1]; } };
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) { const i = y * size + x; if (x > 0) take(i, i - 1); if (y > 0) take(i, i - size); }
  for (let y = size - 1; y >= 0; y--) for (let x = size - 1; x >= 0; x--) { const i = y * size + x; if (x < size - 1) take(i, i + 1); if (y < size - 1) take(i, i + size); }
  const texture = new THREE.DataTexture(new Uint8Array(data.buffer), size, size, THREE.RGBAFormat, THREE.UnsignedByteType);
  texture.generateMipmaps = true;
  texture.minFilter = THREE.LinearMipmapLinearFilter; texture.magFilter = THREE.LinearFilter;
  texture.anisotropy = 4;
  texture.needsUpdate = true;
  sprays = texture;
  return texture;
}

let pattern: THREE.DataTexture | undefined;
/**
 * A repeating tile wholly covered with overlapping pointed leaves, for a surface painted with leaves: r a leaf's tone,
 * g the leaf's own id (see `sprayAtlas`).
 */
export function leafPattern(size = 512): THREE.DataTexture {
  if (pattern) return pattern;
  const { context } = canvas2d(size, size);
  const random = generator(23);
  context.fillStyle = "rgb(150,128,0)"; context.fillRect(0, 0, size, size);
  context.scale(size, size);
  for (let k = 0; k < 520; k++) {
    const x = random(), y = random(), angle = random() * Math.PI * 2, length = 0.11 + 0.06 * random(), tone = 0.62 + 0.38 * k / 520, id = 0.04 + 0.96 * random();
    // Stamped again across each border it is near, so the tile repeats.
    for (const dx of [-1, 0, 1]) for (const dy of [-1, 0, 1]) {
      if (Math.abs(x + dx - 0.5) > 0.7 || Math.abs(y + dy - 0.5) > 0.7) continue;
      stampLeaf(context, x + dx, y + dy, angle, length, length * 0.36, tone, id);
    }
  }
  const data = context.getImageData(0, 0, size, size).data;
  const texture = new THREE.DataTexture(new Uint8Array(data.buffer), size, size, THREE.RGBAFormat, THREE.UnsignedByteType);
  texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  texture.generateMipmaps = true;
  texture.minFilter = THREE.LinearMipmapLinearFilter; texture.magFilter = THREE.LinearFilter;
  texture.anisotropy = 4;
  texture.needsUpdate = true;
  pattern = texture;
  return texture;
}
