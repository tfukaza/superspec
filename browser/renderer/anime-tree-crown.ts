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
  /** Its dome when tiered: how high it stands (1 the usual) and where its top is, in its radii from its middle. */
  dome?: { height: number; x: number; y: number };
}
export interface Crown { stations: CrownStation[]; clusters: CrownCluster[]; centre: THREE.Vector3; radii: THREE.Vector3; scale: number;
  /** How far the stations were gathered onto their clusters' domes, 0 (not at all) to 1. */
  tier?: number }

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
export function crownShade(crown: Pick<Crown, "centre" | "radii" | "tier">, cluster: CrownCluster, p: THREE.Vector3): { normal: THREE.Vector3; occlusion: number } {
  const d = p.clone().sub(cluster.centre).divide(cluster.radii);
  const own = d.clone().divide(cluster.radii);
  if (own.lengthSq() < 1e-12) own.set(0, 0, 1);
  own.normalize();
  // Under a tiered cluster's dome: in the dome's shade, and facing down.
  let below = 0;
  if (crown.tier) {
    below = Math.min(1, Math.max(0, (domeHeight(cluster, d.x, d.y) - d.z) * 2.4 - 0.15));
    own.z -= 1.1 * below;
    own.normalize();
  }
  const c = p.clone().sub(crown.centre).divide(crown.radii);
  const broad = c.clone().divide(crown.radii);
  if (broad.lengthSq() < 1e-12) broad.set(0, 0, 1);
  broad.normalize();
  const normal = own.multiplyScalar(0.72).addScaledVector(broad, 0.28).add(new THREE.Vector3(0, 0, 0.22)).normalize();
  const inner = 1 - Math.min(1, d.length()), under = Math.min(1, Math.max(0, -d.z)), deep = 1 - Math.min(1, c.length());
  // Clusters above shade its top less the higher a point stands in it; its underside is in its own shade.
  const occlusion = Math.min(1, 0.75 * cluster.covered * (1 - 0.35 * Math.max(0, Math.min(1, d.z))) + 0.32 * inner + 0.35 * under + 0.28 * deep + 0.42 * below);
  return { normal, occlusion };
}

/** A tiered cluster's dome: its height over the cluster's middle, in the cluster's vertical radii, at (x, y) radii from it. */
function domeHeight(cluster: CrownCluster, x: number, y: number): number {
  const dome = cluster.dome ?? { height: 1, x: 0, y: 0 };
  const f = Math.min(1, Math.hypot(x - dome.x, y - dome.y));
  return dome.height * (0.95 * Math.sqrt(1 - 0.85 * f * f) - 0.3);
}

/**
 * The crown of `mass` placed by `placement`: stations every `spacing` meters along its twigs (and at every tip),
 * grouped into clusters about `clusterRadius` meters in radius.
 */
export function analyseCrown(mass: FoliageMass, placement: THREE.Matrix4, options: { spacing?: number; clusterRadius?: number; seed?: number; fill?: number; tier?: number; under?: number; vary?: number } = {}): Crown {
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
  // Clusters are flatter than wide (tiers): vertical distances count for more. No two are alike: each has its own
  // size, and many are long one way across (a share `vary` of the full variety; 0 makes them all round and equal).
  type Centre = THREE.Vector3 & { size: number; long: number; cos: number; sin: number };
  let centres: Centre[] = [];
  const stretch = 1.5, vary = options.vary ?? 1;
  const distance = (c: Centre, p: THREE.Vector3) => {
    const x = p.x - c.x, y = p.y - c.y;
    return Math.hypot((x * c.cos + y * c.sin) / c.long, -x * c.sin + y * c.cos, (p.z - c.z) * stretch) / c.size;
  };
  for (const index of order) {
    const p = stations[index].position;
    if (centres.some(c => distance(c, p) < clusterRadius * 1.35)) continue;
    const turn = random() * Math.PI, pick = random(), size = random();
    centres.push(Object.assign(p.clone(), { size: 1 + vary * (size * size * 0.9 - 0.3), long: 1 + vary * (pick < 0.55 ? 0.9 * random() : 0), cos: Math.cos(turn), sin: Math.sin(turn) }));
  }
  for (let round = 0; round < 4; round++) {
    const sums = centres.map(() => new THREE.Vector3()), counts = centres.map(() => 0);
    for (const station of stations) {
      let best = 0, nearest = Infinity;
      centres.forEach((c, i) => { const d = distance(c, station.position); if (d < nearest) { nearest = d; best = i; } });
      station.cluster = best; sums[best].add(station.position); counts[best]++;
    }
    const kept: Centre[] = [];
    centres.forEach((c, i) => { if (counts[i] > 0) kept.push(Object.assign(sums[i].multiplyScalar(1 / counts[i]), { size: c.size, long: c.long, cos: c.cos, sin: c.sin })); });
    if (round < 3) centres = kept;
  }
  // Some domes stand high, some lie nearly flat, and their tops are off their middles.
  const clusters: CrownCluster[] = centres.map(c => ({ centre: new THREE.Vector3(c.x, c.y, c.z), radii: new THREE.Vector3(), covered: 0, stations: 0,
    dome: { height: 1 + vary * (random() * 0.8 - 0.45), x: vary * (random() - 0.5) * 0.6, y: vary * (random() - 0.5) * 0.6 } }));
  for (const station of stations) {
    const cluster = clusters[station.cluster], d = station.position.clone().sub(cluster.centre);
    cluster.radii.add(d.multiply(d)); cluster.stations++;
  }
  for (const cluster of clusters) {
    const n = Math.max(1, cluster.stations), floor = 0.35 * clusterRadius;
    const rx = Math.max(floor, 1.9 * Math.sqrt(cluster.radii.x / n)), ry = Math.max(floor, 1.9 * Math.sqrt(cluster.radii.y / n));
    const wide = Math.max(rx, ry);
    cluster.radii.set(Math.max(rx, 0.5 * wide), Math.max(ry, 0.5 * wide), Math.min(Math.max(floor, 1.9 * Math.sqrt(cluster.radii.z / n)), 0.72 * wide));
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
  const fill = options.fill ?? 0, spread = 0.45 * scale;
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
  // A share of them (`under`) hang below the dome instead: the cluster's dark underside.
  const tier = options.tier ?? 0, under = options.under ?? 0;
  if (tier > 0) for (const station of stations) {
    const cluster = clusters[station.cluster], d = station.position.clone().sub(cluster.centre);
    let dome = cluster.radii.z * domeHeight(cluster, d.x / cluster.radii.x, d.y / cluster.radii.y), pull = 1 - 0.24 * tier;
    if (random() < under) { dome -= cluster.radii.z * (0.3 + 0.4 * random()); pull *= 0.85; }
    station.position.set(cluster.centre.x + d.x * pull, cluster.centre.y + d.y * pull, cluster.centre.z + d.z + (dome - d.z) * tier);
  }
  // The clusters are as wide as their gathered leaves: lit and shaded by their own new extent, not darker inside for
  // having drawn in.
  if (tier > 0) for (const cluster of clusters) { cluster.radii.x *= 1 - 0.24 * tier; cluster.radii.y *= 1 - 0.24 * tier; }
  const crown = { stations, clusters, centre, radii, scale, tier };
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
// The darkest four steps a fifth to a tenth darker and a little cooler than first sampled, so the shade reads deeper
// against the unchanged mids and lights.
export const ANIME_RAMP = ["#222824", "#2a302a", "#333a2f", "#404534", "#5c5d42", "#78764f", "#a19c6c"] as const;

/** How the leaves are lit: see `ANIME_SHADE_GLSL`. */
export const ANIME_LIGHT = {
  /** Share of the light that comes from straight above whatever the sun (top-lit clusters). */
  top: 0.3,
  /** How far full occlusion darkens, and how far being seen from beneath does. */
  occlusion: 0.5, underside: 0.25,
  /** A whole leaf's random shift of light: what makes single leaves read. */
  jitter: 0.26,
  /** Steps of the ramp. */
  bands: 12,
  /** The light stretched about its middle, and shifted. */
  contrast: 1.35, offset: 0.0,
  /** Warm light through leaves on the crown's edge seen against the sun. */
  back: 0.16,
  /** The fewest pixels a leaf and a spray card are long, and the pixels a limb is thickened by on each side. */
  leafPixels: 7, cardPixels: 13, barkPixels: 0.9,
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
    atMinimum: { value: new THREE.Vector3(ANIME_LIGHT.leafPixels, ANIME_LIGHT.cardPixels, ANIME_LIGHT.barkPixels) },
    atViewport,
  };
}

/** The height in pixels of what is being drawn into, kept by `trackViewport`. */
const atViewport = { value: 1000 };
/** Keeps `atViewport` as `mesh` is drawn (the sun's shadow pass draws with the last view's). */
export function trackViewport(mesh: THREE.Mesh): void {
  const before = mesh.onBeforeRender, size = new THREE.Vector4();
  mesh.onBeforeRender = function (renderer, ...rest) { atViewport.value = renderer.getCurrentViewport(size).w; before.call(this, renderer, ...rest); };
}

/**
 * Vertex shader GLSL: `atPixel(world)` is the meters one pixel spans at a point of the scene. Nothing is drawn thinner
 * than an illustrator's brush: `atMinimum` holds the fewest pixels a leaf is long, a card is long, and the pixels a
 * limb is thickened by on each side.
 */
export const ANIME_VERTEX_GLSL = /* glsl */`
uniform vec3 atMinimum;
uniform float atViewport;
float atPixel(vec3 world) {
  float depth = isOrthographic ? 1.0 : (projectionMatrix * viewMatrix * vec4(world, 1.0)).w;
  return 2.0 * depth / (projectionMatrix[1][1] * atViewport);
}
`;

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
// A highlight leaf: lighter by boost where it is already in the light, an ordinary leaf in the shade.
float atGlint(float light, float boost) {
  return min(1.0, light + boost * smoothstep(0.5, 0.75, light));
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
  vec3 colour = mix(a, b, q - i);
  // The few leaves in full light: paler still.
  return mix(colour, colour * 1.16 + vec3(0.012, 0.01, 0.0), step(0.93, v));
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
function leafPath(context: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D, length: number, width: number, half = 0, bend = 0): void {
  // `bend` curls the leaf sideways toward its tip, as a share of its length.
  const tip = bend * length, a = tip * 0.15, b = tip * 0.6;
  context.beginPath();
  context.moveTo(0, 0);
  context.bezierCurveTo(a + width * 0.95, length * 0.22, b + width * 0.62, length * 0.68, tip, length);
  if (half) context.quadraticCurveTo(b, length * 0.6, 0, 0);
  else context.bezierCurveTo(b - width * 0.62, length * 0.68, a - width * 0.95, length * 0.22, 0, 0);
  context.closePath();
}

function canvas2d(width: number, height: number): { canvas: OffscreenCanvas | HTMLCanvasElement; context: OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D } {
  const canvas = typeof OffscreenCanvas !== "undefined" ? new OffscreenCanvas(width, height) : Object.assign(document.createElement("canvas"), { width, height });
  const context = canvas.getContext("2d", { willReadFrequently: true }) as OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D;
  return { canvas, context };
}

/**
 * For a material whose fragments' alpha is their coverage (alpha to coverage): what it writes to the target's alpha.
 * The colour replaces what was there, but its alpha is added to it, so a leaf's soft edge over another leaf leaves
 * the target opaque there. Otherwise an edge's partial alpha would show the page through three's canvas (which has an
 * alpha channel) and the backdrop through the scene's target (whose alpha its output pass lays over the backdrop)
 * along every leaf's edge. Where the edge is drawn before what lies behind it, the fringe remains.
 */
export function coverageAlpha(material: THREE.Material): void {
  material.blending = THREE.CustomBlending;
  material.blendSrc = THREE.OneFactor; material.blendDst = THREE.ZeroFactor;
  material.blendSrcAlpha = THREE.OneFactor; material.blendDstAlpha = THREE.OneFactor;
}
/** One leaf stamped at (x, y) along `angle` (radians from +y toward +x, y up): red its tone, green its own id. */
function stampLeaf(context: OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D, x: number, y: number, angle: number, length: number, width: number, tone: number, id: number, bend = 0): void {
  const colour = (t: number) => `rgb(${Math.round(255 * Math.min(1, t))},${Math.round(255 * id)},0)`;
  context.save();
  context.translate(x, y);
  context.rotate(-angle);
  leafPath(context, length, width, 0, bend);
  context.fillStyle = colour(tone);
  context.fill();
  // One half a little darker (a folded leaf) and a fine midrib.
  leafPath(context, length, width, 1, bend);
  context.fillStyle = colour(tone * 0.9);
  context.fill();
  context.strokeStyle = colour(tone * 0.78);
  context.lineWidth = length * 0.045;
  context.beginPath(); context.moveTo(0, length * 0.04); context.quadraticCurveTo(bend * length * 0.6, length * 0.6, bend * length * 0.92, length * 0.9); context.stroke();
  context.restore();
}

export const SPRAY_GRID = 4;
/** The atlas's cells in order: fans, then whorls, then highlight sprigs. */
export const SPRAY_FANS = 8, SPRAY_WHORLS = 4, SPRAY_SPRIGS = 4;
let sprays: THREE.DataTexture | undefined;
/**
 * An atlas of 4 × 4 painted sprays of distinct pointed leaves, for cards: eight fans (leaves spreading from a stem at
 * the card's foot, often a smaller tier above), four whorls (leaves all round the card's middle) and four sprigs (one to
 * three leaves filling the card, for small highlight cards). Texels: r a leaf's tone (the
 * leaves behind darker, a darker half and midrib), g the leaf's own id (the same over the whole leaf, so a light step
 * taken from it falls on whole leaves), a coverage with antialiased edges. A card's uv has v = 0 at its tip.
 */
export function sprayAtlas(cell = 256): THREE.DataTexture {
  if (sprays) return sprays;
  const size = SPRAY_GRID * cell, { context } = canvas2d(cell, cell);
  const data = new Uint8ClampedArray(size * size * 4);
  const random = generator(91);
  const between = (a: number, b: number) => a + (b - a) * random();
  for (let index = 0; index < SPRAY_GRID * SPRAY_GRID; index++) {
    const ox = (index % SPRAY_GRID) * cell, oy = Math.floor(index / SPRAY_GRID) * cell;
    // Each leaf is drawn alone and laid over the cell's texels: where it covers a texel by half or more the texel is
    // wholly its own (tone and id), so ids never blend where leaves overlap; coverage keeps the antialiased edge.
    const lay = (id: number, draw: () => void) => {
      context.setTransform(1, 0, 0, 1, 0, 0);
      context.clearRect(0, 0, cell, cell);
      // The cell's frame: x across from its middle, y up from its foot, one unit its side.
      context.setTransform(cell, 0, 0, -cell, cell / 2, cell);
      draw();
      const drawn = context.getImageData(0, 0, cell, cell).data;
      for (let y = 0; y < cell; y++) for (let x = 0; x < cell; x++) {
        const j = (y * cell + x) * 4, alpha = drawn[j + 3];
        if (!alpha) continue;
        const i = ((oy + y) * size + ox + x) * 4;
        if (alpha >= 128 || data[i + 3] < alpha) { data[i] = drawn[j]; data[i + 1] = Math.round(255 * id); }
        data[i + 3] = Math.max(data[i + 3], alpha);
      }
    };
    // No two sprays alike: each has its own number of leaves, spread, lean, and leaves slender or broad, straight or
    // curled.
    const leaves: { x: number; y: number; angle: number; length: number; order: number }[] = [];
    const slender = between(0.2, 0.4), curl = between(-0.22, 0.22);
    if (index < SPRAY_FANS) {
      // A fan: a tier of leaves from the stem's foot and often a smaller one from higher up it, upright or leaning
      // to one side.
      const lean = between(-0.4, 0.4), tiers = random() < 0.7 ? 2 : 1, wide = between(0.75, 1.45);
      lay(0.5, () => { context.strokeStyle = "rgb(110,128,0)"; context.lineWidth = 0.03; context.lineCap = "round"; context.beginPath(); context.moveTo(0, 0.02); context.lineTo(Math.sin(lean) * 0.3, 0.3); context.stroke(); });
      for (const [foot, count, reach, spread] of ([[0.1, 3 + Math.floor(random() * 5), 0.46, wide], [0.36, 2 + Math.floor(random() * 5), 0.4, wide * 0.9]] as const).slice(0, tiers)) {
        for (let k = 0; k < count; k++) {
          const angle = lean + (count > 1 ? (k / (count - 1) * 2 - 1) : 0) * spread + between(-0.2, 0.2);
          // Side leaves are shorter and lie behind the middle ones.
          const length = reach * (1 - 0.3 * Math.abs(angle - lean) / spread) * between(0.7, 1.15);
          leaves.push({ x: Math.sin(lean) * foot + Math.sin(angle) * 0.03, y: foot + Math.cos(angle) * 0.03, angle, length, order: -Math.abs(angle - lean) + foot + between(-0.2, 0.2) });
        }
      }
    } else if (index < SPRAY_FANS + SPRAY_WHORLS) {
      // A whorl seen from above: leaves all round, the last ones over the first.
      const count = 5 + Math.floor(random() * 5), turn = between(0, Math.PI * 2);
      for (let k = 0; k < count; k++) {
        const angle = turn + k / count * Math.PI * 2 + between(-0.3, 0.3);
        leaves.push({ x: Math.sin(angle) * 0.03, y: 0.5 + Math.cos(angle) * 0.03, angle, length: between(0.28, 0.46), order: random() });
      }
      for (let k = 0; k < 3; k++) {
        const angle = between(0, Math.PI * 2);
        leaves.push({ x: 0, y: 0.5, angle, length: between(0.2, 0.28), order: 2 + random() });
      }
    } else {
      // A sprig for highlights: one to three leaves filling the card, from its foot.
      const count = 1 + Math.floor(random() * 3), lean = between(-0.25, 0.25);
      for (let k = 0; k < count; k++) {
        const angle = lean + (count > 1 ? (k / (count - 1) * 2 - 1) * between(0.35, 0.6) : 0);
        leaves.push({ x: 0, y: 0.04, angle, length: between(0.62, 0.9) * (1 - 0.25 * Math.abs(angle - lean)), order: -Math.abs(angle - lean) });
      }
    }
    leaves.sort((p, q) => p.order - q.order);
    leaves.forEach((leaf, k) => {
      // Later leaves lie on top and are lighter; every leaf has its own id.
      const depth = leaves.length > 1 ? k / (leaves.length - 1) : 1, id = between(0.04, 1), width = leaf.length * slender * between(0.8, 1.2), bend = curl + between(-0.12, 0.12);
      lay(id, () => stampLeaf(context, leaf.x, leaf.y, leaf.angle, leaf.length, width, 0.62 + 0.38 * depth, id, bend));
    });
  }
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
  // Each leaf is drawn alone in a small square about its foot and laid over the tile (wrapping at its borders), a
  // texel it covers by half or more becoming wholly its own, as in `sprayAtlas`.
  const box = Math.ceil(size * 0.38), { context } = canvas2d(box, box);
  const random = generator(23);
  const data = new Uint8ClampedArray(size * size * 4);
  for (let i = 0; i < size * size; i++) { data[i * 4] = 150; data[i * 4 + 1] = 128; data[i * 4 + 3] = 255; }
  for (let k = 0; k < 520; k++) {
    const x = Math.floor(random() * size), y = Math.floor(random() * size), angle = random() * Math.PI * 2, length = 0.11 + 0.06 * random(), tone = 0.62 + 0.38 * k / 520, id = 0.04 + 0.96 * random();
    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, box, box);
    context.setTransform(size, 0, 0, size, box / 2, box / 2);
    stampLeaf(context, 0, 0, angle, length, length * 0.36, tone, id);
    const drawn = context.getImageData(0, 0, box, box).data;
    for (let v = 0; v < box; v++) for (let u = 0; u < box; u++) {
      const j = (v * box + u) * 4;
      if (drawn[j + 3] < 128) continue;
      const i = (((y + v) % size) * size + (x + u) % size) * 4;
      data[i] = drawn[j]; data[i + 1] = Math.round(255 * id);
    }
  }
  const texture = new THREE.DataTexture(new Uint8Array(data.buffer), size, size, THREE.RGBAFormat, THREE.UnsignedByteType);
  texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  texture.generateMipmaps = true;
  texture.minFilter = THREE.LinearMipmapLinearFilter; texture.magFilter = THREE.LinearFilter;
  texture.anisotropy = 4;
  texture.needsUpdate = true;
  pattern = texture;
  return texture;
}
