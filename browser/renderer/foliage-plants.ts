import * as THREE from "three";
import type { FoliageCanopy, FoliageLook } from "../model/packet.ts";
import { attributeName } from "./batch.ts";
import { leafBrush } from "./leaf-brush.ts";
import { leafCardAtlas } from "./leaf-cards.ts";
import type { FoliageTextureJob, FoliageTextureResult } from "./foliage-texture-worker.ts";
import { followPaintedFinishing, programSun, programTime } from "./shade.ts";
import { FAMILY_FUNCTIONS, FAMILY_UNIFORMS, fieldUniforms, programChunk, receiverUniforms, type FoliageProfile, type FoliageProgram } from "./foliage.ts";

/**
 * The plant families of the foliage view (roadmap stage 7, TreeSpec phase 5): a plant's levels of detail are batches
 * of the families `bark`, `leaves`, `hull` and `shadow` (`superspec_types::batch::FoliageFamily`), drawn like grass by
 * fixed shader families with parameters, never by shader code.
 *
 * - **Wind**: the whole plant sways about its base (`msFoliageOrigin`) by a gust sampled there, each point by its
 *   `bendWeight` (bark vertices, a leaf cluster's instance, hull vertices), so a cluster moves with the branch it hangs
 *   from and branches do not tear where they join. Leaf cards flutter about their cluster by their own `bendWeight`.
 * - **Leaves** are paint dabs: each cluster turns toward the camera by `canopy.facing`, is scaled by `canopy.dab`, rolled
 *   by its random value and cut out (`canopy.cutout` against a brush-edged coverage). Its paint is the recipe evaluated
 *   once per cluster in the vertex stage at the cluster's centre with the canopy shade normal (from the metaball hull)
 *   as `normal`, the sun's cast shadow there folded into the band; a share (`canopy.highlights`) of the clusters on the
 *   lit side of the band are highlight leaves.
 * - **Bark** and **hull** evaluate the recipe per pixel with their shade normals. The hull's silhouette is broken into
 *   dabs (fragments where it turns away from the view are cut by a world-anchored noise).
 * - **Shadow**: the hull's proxy, drawn only by the sun's shadow camera (`SHADOW_PROXY_LAYER`); it is what the canopy casts.
 * - **Lit** profile: the light loop with the shade normal tipped toward the card, plus two-sided diffuse (`canopy.wrap`),
 *   light through the leaves from behind (`canopy.translucency`) and glossy clusters (`canopy.glint`).
 */

/** The layer only the sun's shadow camera draws: a plant's shadow proxy. */
export const SHADOW_PROXY_LAYER = 7;

export function isPlant(look: FoliageLook): boolean { return look.family !== "grass"; }

/**
 * The canopy jag (a prototype set from the page's query, not yet a canopy parameter): a noise anchored to the plant
 * that moves the threshold of the canopy's shade band, so its light and shade edge is bitten. `jag=fbm` is the
 * painterly-shader study's (two planar fBM samples of the plant-local point, `ilFbm`), `jag=leaf` the leaf brush
 * (browser/renderer/leaf-brush.ts) sampled the same way; `jagscale` is meters per noise lobe, `jagstrength` its
 * amplitude in the band's units. Leaves read it once per cluster, the hull per pixel.
 */
const JAG = (() => {
  const query = typeof location === "undefined" ? new URLSearchParams() : new URLSearchParams(location.search);
  const mode = ({ fbm: 1, leaf: 2 } as Record<string, number>)[query.get("jag") ?? ""] ?? 0;
  // Defaults from a sweep on the washroom shrub (about 1.3 m across): a lobe about a quarter of the canopy's width.
  return { mode, scale: Number(query.get("jagscale") ?? 0.3) || 0.3, strength: Number(query.get("jagstrength") ?? 0.97) };
})();
/** Noise lobes across one tile of the leaf brush (its `lobes` option), so a lobe spans `jagscale` meters as fBM's do. */
const BRUSH_LOBES = 3;
let brushTexture: THREE.DataTexture | undefined;
const PLANT = (() => {
  const query = typeof location === "undefined" ? new URLSearchParams() : new URLSearchParams(location.search);
  const number = (name: string, fallback: number) => { const v = Number(query.get(name)); return query.has(name) && Number.isFinite(v) ? v : fallback; };
  return {
    shade: number("plantshade", 0.25), jitter: number("leafjitter", 0.15),
    // How far a card's dome tips the dab's normal; how much each leaf's own noise moves its band; the paint's melt (texels).
    curve: number("cardcurve", 1.6), noise: number("leafnoise", 0.2), melt: Math.round(number("melt", 2)), debug: number("carddebug", 0), backing: number("backing", 1) > 0, bounce: number("bounce", 0.16), sparkle: number("sparkle", 1), lift: number("lift", 1.0), core: number("core", 0.7), round: number("cardround", 0.6),
    // The backlit rim glow is off by default (`?glow=` brings it back).
    selfShadow: number("selfshadow", 0.4), buried: number("buried", 0.4), glow: number("glow", 0),
  };
})();
/**
 * The generated textures are made off the page's thread (foliage-texture-worker.ts): a texture exists at once, empty,
 * and takes its texels when they arrive, which `foliageTexturesReady` waits for. A scene with plants used to stand
 * still for the second or two the leaf cards and the brush take; now that time passes beside the scene's other
 * preparation. Without workers (and on a worker's failure) the texels are made here, as before.
 */
interface Making { settled: boolean; promise: Promise<void>; finish(buffers?: ArrayBuffer[]): void }
const making = new Set<Making>();
let textureWorker: Worker | null | undefined;
let lastJob = 0;
const awaited = new Map<number, Making>();

/** Starts `job` and gives its buffers to `apply` when it is done; `here` makes them on this thread. */
function make(job: Omit<FoliageTextureJob, "id">, here: () => ArrayBuffer[], apply: (buffers: ArrayBuffer[]) => void): Making {
  let resolve = (): void => {};
  const entry: Making = {
    settled: false,
    promise: new Promise<void>(done => { resolve = done; }),
    finish(buffers) {
      if (entry.settled) return;
      entry.settled = true;
      apply(buffers ?? here());
      making.delete(entry);
      resolve();
    },
  };
  making.add(entry);
  if (textureWorker === undefined) {
    textureWorker = null;
    if (typeof Worker !== "undefined") {
      try {
        const worker = new Worker(new URL("./foliage-texture-worker.ts", import.meta.url), { type: "module" });
        worker.onmessage = (event: MessageEvent<FoliageTextureResult>) => {
          const waiting = awaited.get(event.data.id);
          awaited.delete(event.data.id);
          waiting?.finish("buffers" in event.data ? event.data.buffers : undefined);
        };
        // A worker that cannot run leaves every texture to this thread.
        worker.onerror = () => { textureWorker = null; for (const waiting of [...awaited.values()]) waiting.finish(); awaited.clear(); };
        textureWorker = worker;
      } catch { textureWorker = null; }
    }
  }
  if (!textureWorker) { entry.finish(); return entry; }
  const id = ++lastJob;
  awaited.set(id, entry);
  textureWorker.postMessage({ id, ...job } as FoliageTextureJob);
  return entry;
}

/** Resolves when every generated foliage texture asked for so far holds its texels. */
export function foliageTexturesReady(): Promise<void> { return Promise.all([...making].map(entry => entry.promise)).then(() => undefined); }

let cardTextures: { cards: THREE.DataTexture; normals: THREE.DataTexture; making: Making } | undefined;
/**
 * The leaf-card atlas the dabs draw (leaf-cards.ts) and its normals, made once. The textures are empty until their
 * texels arrive (`foliageTexturesReady`); `now` makes them before returning, for a caller that reads or draws them at once.
 */
export function leafCardTextures(now = false): { cards: THREE.DataTexture; normals: THREE.DataTexture } {
  if (!cardTextures) {
    const grid = 4, size = 256, width = grid * size, height = 2 * grid * size;
    const texture = () => {
      const t = new THREE.DataTexture(new Uint8Array(width * height * 4), width, height, THREE.RGBAFormat, THREE.UnsignedByteType);
      t.magFilter = THREE.LinearFilter;
      t.minFilter = THREE.LinearMipmapLinearFilter;
      t.generateMipmaps = true;
      t.needsUpdate = true;
      return t;
    };
    const cards = texture(), normals = texture();
    const fill = (target: THREE.DataTexture, buffer: ArrayBuffer) => { (target.image as { data: Uint8Array }).data = new Uint8Array(buffer); target.needsUpdate = true; };
    cardTextures = { cards, normals, making: make({ job: "cards", arguments: [grid, size, 1, PLANT.melt] },
      () => (atlas => [atlas.data.buffer, atlas.normals.buffer] as ArrayBuffer[])(leafCardAtlas(grid, size, 1, PLANT.melt)),
      ([data, shade]) => { fill(cards, data); fill(normals, shade); }) };
  }
  if (now) cardTextures.making.finish();
  return cardTextures;
}

let brushMaking: Making | undefined;
/** The leaf brush (leaf-brush.ts) as a tiling texture, made once and filled like the leaf cards (`leafCardTextures`). */
export function leafBrushTexture(now = false): THREE.DataTexture {
  if (!brushTexture) {
    const size = 512, options = { size, lobes: BRUSH_LOBES };
    const texture = brushTexture = new THREE.DataTexture(new Uint8Array(size * size), size, size, THREE.RedFormat, THREE.UnsignedByteType);
    texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
    texture.magFilter = THREE.LinearFilter;
    texture.minFilter = THREE.LinearMipmapLinearFilter;
    texture.generateMipmaps = true;
    texture.needsUpdate = true;
    brushMaking = make({ job: "brush", arguments: [options] }, () => [leafBrush(options).data.buffer as ArrayBuffer], ([buffer]) => {
      const data = new Float32Array(buffer), bytes = new Uint8Array(size * size);
      for (let i = 0; i < bytes.length; i++) bytes[i] = Math.round(data[i] * 255);
      (texture.image as { data: Uint8Array }).data = bytes;
      texture.needsUpdate = true;
    });
  }
  if (now) brushMaking?.finish();
  return brushTexture;
}

// The brush is read at its full level in the vertex stage (once per leaf cluster) and filtered across a pixel's
// footprint in the fragment stage (the hull), so its fine leaves do not alias into speckle at a distance.
const jagFunctions = (fragment: boolean) => /* glsl */`
#define MS_JAG_SAMPLE(uv) ${fragment ? "texture(msFoliageBrush, uv).r" : "textureLod(msFoliageBrush, uv, 0.0).r"}
float msJagHash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453123); }
float msJagValue(vec2 p) {
  vec2 cell = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(msJagHash(cell), msJagHash(cell + vec2(1.0, 0.0)), f.x), mix(msJagHash(cell + vec2(0.0, 1.0)), msJagHash(cell + vec2(1.0)), f.x), f.y);
}
float msJagFbm(vec2 p) { return msJagValue(p) * 0.5714 + msJagValue(p * 2.0) * 0.2857 + msJagValue(p * 4.0) * 0.1429; }
// The jag at a plant-local point (Z up): two planar samples, across and up, summed about zero.
float msFoliageJagAt(vec3 local) {
  if (msFoliageJagParams.x < 0.5) return 0.0;
  vec2 a = local.xz / msFoliageJagParams.y + msFoliageOrigin.xy, b = local.yz / msFoliageJagParams.y + msFoliageOrigin.yx;
  float n = msFoliageJagParams.x < 1.5 ? msJagFbm(a) + msJagFbm(b)
    : MS_JAG_SAMPLE(a / ${BRUSH_LOBES.toFixed(1)}) + MS_JAG_SAMPLE(b / ${BRUSH_LOBES.toFixed(1)} + 0.5);
  return (n - 1.0) * msFoliageJagParams.z;
}
`;

/** The uniforms of a plant look's canopy parameters. */
export function canopyUniforms(canopy: FoliageCanopy): Record<string, THREE.IUniform> {
  return {
    // x: jag mode (0 off, 1 fBM, 2 leaf brush), y: meters per lobe, z: strength.
    msFoliageJagParams: { value: new THREE.Vector3(JAG.mode, JAG.scale, JAG.strength) },
    // The leaf brush is also every paint dab's edge, so a plant always has it.
    msFoliageBrush: { value: leafBrushTexture() },
    msFoliageCards: { value: leafCardTextures().cards },
    msFoliageCardNormals: { value: leafCardTextures().normals },
    // x: the cards' dome curvature, y: per-leaf band noise.
    msFoliageCardShade: { value: new THREE.Vector2(PLANT.curve, PLANT.noise) },
    msFoliageCardDebug: { value: PLANT.debug },
    // The fake bounce light's floor: the least linear luminance a small plant's leaves keep (`?bounce=`).
    msFoliageBounce: { value: PLANT.bounce },
    // In band thresholds: x, how far the leaves' shadow toward the sun darkens; y, how far being buried darkens; z, how
    // far the glow with the sun behind lightens (`?selfshadow=`, `?buried=`, `?glow=`).
    msFoliageSelf: { value: new THREE.Vector3(PLANT.selfShadow, PLANT.buried, PLANT.glow) },
    // How readily sparkles show (`?sparkle=`, 0 hides them).
    msFoliageSparkle: { value: PLANT.sparkle },
    // How much bounce light lifts a small plant's darker bands (`?lift=`; 0.35 raises the darkest by 35%).
    msFoliageLift: { value: PLANT.lift },
    // How dark the most buried clumps go (a multiplier; `?core=`).
    msFoliageCore: { value: PLANT.core },
    // How much each card's own lit side brightens and far side darkens within a band (`?cardround=`).
    msFoliageCardRound: { value: PLANT.round },
    // How much darker the plant's side away from the sun steps (band threshold at the far side), and how far a dab's
    // leaves may stray from its band.
    msFoliagePlantShade: { value: PLANT.shade },
    msFoliageLeafJitter: { value: PLANT.jitter },
    // x: facing, y: dab size, z: cutout, w: share of highlight leaves.
    msFoliageCanopy: { value: new THREE.Vector4(canopy.facing, canopy.dab, canopy.cutout, canopy.highlights) },
    msFoliageCanopyHighlight: { value: new THREE.Color().setRGB(...canopy.highlight, THREE.LinearSRGBColorSpace) },
    // x: translucency, y: glint, z: wrap.
    msFoliageLit: { value: new THREE.Vector3(canopy.translucency, canopy.glint, canopy.wrap) },
    // The plant's base, which the patch sets from its bark's placement.
    msFoliageOrigin: { value: new THREE.Vector3() },
    // The canopy surface's leafy outline (prototype, from the query): x: meters the surface swells at its outline,
    // y: meters across one tile of the leaf brush there, z: how much of the outline band the brush cuts.
    msFoliageFringe: { value: new THREE.Vector3(FRINGE.width, FRINGE.tile, FRINGE.cut) },
  };
}
/**
 * Whether the canopy masses are drawn as 2D strokes whose edges a leaf brush redraws (`?strokes=1`, stroke-edges.ts):
 * the `hull` family then writes a stroke id in alpha and leaves its outline whole, for the edge pass to paint.
 */
export const STROKES = typeof location !== "undefined" && new URLSearchParams(location.search).get("strokes") === "1";
/** Bands a stroke's id tells apart (the recipe's band count; `?strokebands=`), so band edges are stroke edges. */
export const STROKE_BANDS = Math.max(1, Math.min(8, Math.round(Number(typeof location === "undefined" ? 4 : new URLSearchParams(location.search).get("strokebands") ?? 4) || 4)));
const FRINGE = (() => {
  const query = typeof location === "undefined" ? new URLSearchParams() : new URLSearchParams(location.search);
  const number = (name: string, fallback: number) => { const v = Number(query.get(name)); return query.has(name) && Number.isFinite(v) ? v : fallback; };
  return STROKES ? { width: 0, tile: 1, cut: 0 } : { width: number("fringe", 0.035), tile: number("fringetile", 0.35), cut: number("fringecut", 0.9) };
})();

/**
 * Meters a plant's point moves along its shade normal before it reads the sun's cast shadow: the canopy's shadow proxy
 * stands just inside the backing shell and the leaves, and at a low sun the depth steps of the shadow map would stripe
 * them where the two surfaces nearly meet.
 */
const CAST_OFFSET = 0.12;

/** A program chunk whose cast shadow is read off the plant's surface (`CAST_OFFSET`). */
function plantChunk(chunk: { declarations: string; inputs: string }): { declarations: string; inputs: string } {
  return { ...chunk, inputs: chunk.inputs.replace("msFoliageCast(root, ground)", `msFoliageCast(root + ground * ${CAST_OFFSET.toFixed(2)}, ground)`) };
}

const VERTEX = { bendWeight: "0.0", heightRatio: "clamp(uv.y, 0.0, 1.0)" } as const;
const INSTANCE = { shadeNormal: "vec3(0.0, 0.0, 1.0)", threshold: "0.0", variation: "0.5", random: "0.0", bendWeight: "0.0" } as const;

const PLANT_UNIFORMS = /* glsl */`
uniform vec4 msFoliageLevel;
uniform float msFoliageFar;
uniform vec4 msFoliageWind;
uniform vec2 msFoliageWindAzimuth;
uniform vec4 msFoliageCanopy;
uniform vec3 msFoliageCanopyHighlight;
uniform vec3 msFoliageLit;
uniform vec3 msFoliageOrigin;
uniform vec3 msFoliageSunLight;
uniform vec3 msFoliageJagParams;
uniform vec3 msFoliageFringe;
uniform sampler2D msFoliageBrush;
varying vec3 vFoliageColor;
varying float vFoliageBand;
varying float vFoliageHeight;
varying float vFoliageVariation;
varying float vFoliageRandom;
varying float vFoliageHighlight;
varying float vFoliageCard;
varying float vFoliageDab;
varying float vFoliageElement;
varying float vFoliageSparse;
varying float vFoliageGlow;
varying float vFoliageOpen;
varying vec3 vFoliageDark;
varying vec3 vFoliageLight;
varying vec3 vFoliageDarker;
varying vec3 vFoliageLighter;
uniform float msFoliagePlantShade;
uniform vec4 msFoliagePlantMass;
uniform float msFoliageLeafJitter;
uniform sampler2D msFoliageCards;
uniform sampler2D msFoliageCardNormals;
uniform vec2 msFoliageCardShade;
uniform float msFoliageCardDebug;
uniform float msFoliageBackingShade;
uniform float msFoliageBounce;
uniform vec3 msFoliageSelf;
uniform float msFoliageSparkle;
uniform float msFoliageLift;
uniform float msFoliageCore;
uniform float msFoliageCardRound;
// An ambient cube's value toward the unit d: +axis values in high, -axis in low, weighted by d's squared components.
float msFoliageOpen(vec3 d, vec3 high, vec3 low) {
  vec3 w = d * d;
  return w.x * (d.x > 0.0 ? high.x : low.x) + w.y * (d.y > 0.0 ? high.y : low.y) + w.z * (d.z > 0.0 ? high.z : low.z);
}
varying vec3 vFoliageCardX;
varying vec3 vFoliageCardY;
varying vec3 vFoliageWorld;
varying vec3 vFoliageShade;
varying vec3 vFoliageAnchor;
varying vec2 vFoliageUv;
`;

function vertexStage(look: FoliageLook, profile: FoliageProfile, program: FoliageProgram | undefined, attributes: ReadonlySet<string>): string {
  const declared = new Set<string>();
  const lines: string[] = [];
  const named = (domain: "vertex" | "instance", name: string, glsl: string, fallback: string): string => {
    const symbol = attributeName(domain, name);
    declared.add(symbol);
    lines.push(attributes.has(symbol) ? `attribute ${glsl} ${symbol};` : `#define ${symbol} (${fallback})`);
    return symbol;
  };
  const leaves = look.family === "leaves";
  const vertexBend = named("vertex", "bendWeight", "float", VERTEX.bendWeight);
  const height = named("vertex", "heightRatio", "float", VERTEX.heightRatio);
  const threshold = named("instance", "threshold", "float", INSTANCE.threshold);
  const variation = named("instance", "variation", "float", INSTANCE.variation);
  const random = named("instance", "random", "float", INSTANCE.random);
  const shadeNormal = leaves ? named("instance", "shadeNormal", "vec3", INSTANCE.shadeNormal) : "";
  const clusterBend = leaves ? named("instance", "bendWeight", "float", INSTANCE.bendWeight) : "";
  // A leaf card's index in its cluster, so each card of a cluster draws its own leaf.
  // A hull's part (TreeSpec's clumps and sprigs) is its stroke for the edge pass.
  const element = leaves || look.family === "hull" ? named("vertex", "element", "float", "0.0") : "0.0";
  // How open the instance is through the plant's own leaves toward ±x, ±y, ±z (TreeSpec's mass model), negated for a
  // paint dab on a clump; fully open (and no dab) without one. `dab` is read from its sign: no attribute slot is spare.
  const openRaw = leaves ? named("instance", "openness", "vec3", "vec3(255.999)") : "vec3(255.999)";
  const openness = `abs(${openRaw})`;
  const dab = `(${openRaw}.x < 0.0 ? 1.0 : 0.0)`;
  // A sparkle (a highlight leaf just outside a clump) is negative in y.
  const sparkle = `(${openRaw}.y < 0.0)`;
  // The plant's middle and size, for shading the whole plant as one mass (0 size: off).
  const plantCentre = "msFoliagePlantMass.xyz", plantExtent = "msFoliagePlantMass.w";
  // Leaves take their paint per cluster here; bark and hull per pixel in the fragment stage.
  const chunk = leaves && program ? plantChunk(programChunk(program.shading.shader, attributes, declared)) : undefined;
  const painted = profile === "painted";
  const color = !chunk ? "vec3(0.2, 0.4, 0.12) * mix(0.55, 1.0, 0.5 + 0.5 * shade.z)"
    : painted ? `(${program!.shading.shader.painted ?? "ms_base_color"})` : "ms_base_color";
  return `
#define MS_FOLIAGE
${FAMILY_UNIFORMS}
${PLANT_UNIFORMS}
${lines.join("\n")}
bool msFoliageRejected = false;
vec3 msFoliageOffset = vec3(0.0);
${FAMILY_FUNCTIONS}
${jagFunctions(false)}
${chunk?.declarations ?? ""}
void msFoliageVertex(inout vec3 objectNormal) {
  mat4 place = modelMatrix;
#ifdef USE_INSTANCING
  place = modelMatrix * instanceMatrix;
#endif
  vec3 anchor = (place * vec4(0.0, 0.0, 0.0, 1.0)).xyz;
  vec3 original = (place * vec4(position, 1.0)).xyz;
  vec3 point = original;
  float away = length(cameraPosition - ${leaves ? "anchor" : "msFoliageOrigin"});
  // Thinning toward the next level's share by the stable threshold; nothing past far.
  float onward = clamp((away - msFoliageLevel.x) / max(msFoliageLevel.z - msFoliageLevel.x, 1e-3), 0.0, 1.0);
  bool keep = away < msFoliageFar && ${threshold} < mix(msFoliageLevel.y, msFoliageLevel.w, onward);
  msFoliageRejected = !keep;
  if (!keep) return;
  mat3 turn = mat3(place);
  vec3 normalWorld = normalize(turn * objectNormal);
  vec3 shade = normalWorld;
  float rank = ${random};
  float weight = ${leaves ? clusterBend : vertexBend};
  vec2 travel = vec2(cos(msFoliageWindAzimuth.x), sin(msFoliageWindAzimuth.x));
${leaves ? `  shade = normalize(${shadeNormal});
  // A paint dab: the cluster turned toward the camera about its centre, scaled and rolled by its random value.
  // A sparkle is a fleck: about half a leaf.
  float size = length(turn[1]) * (${sparkle} ? 0.3 : 1.0);
  vec3 centre = (place * vec4(0.0, 0.5, 0.0, 1.0)).xyz;
  vec3 view = normalize(cameraPosition - centre);
  vec3 side = cross(vec3(0.0, 0.0, 1.0), view);
  side = dot(side, side) > 1e-8 ? normalize(side) : vec3(1.0, 0.0, 0.0);
  vec3 up = cross(view, side);
  float roll = 6.2831853 * fract(rank * 3.17 + 0.21);
  // A paint dab (TreeSpec's mass model) is a square card wholly facing the camera; a leaf keeps its card's shape.
  bool isDab = ${dab} > 0.5;
  // A dab at the plant's rim (edge-on to the view, or far out from the plant's middle) draws a sparse card, more often
  // the farther out it lies.
  {
    float rim = 1.0 - abs(dot(shade, view));
    float out_ = ${plantExtent} > 0.0 ? length(centre - ${plantCentre}) / ${plantExtent} : 0.0;
    float edge = max(smoothstep(0.55, 0.9, rim), smoothstep(0.7, 1.0, out_));
    vFoliageSparse = fract(rank * 13.37 + 0.11) < edge ? 1.0 : 0.0;
    vFoliageGlow = 0.0;
    vFoliageOpen = 1.0;
  }
  vec2 local = mat2(cos(roll), sin(roll), -sin(roll), cos(roll)) * (isDab ? uv - 0.5 : vec2(position.x, position.y - 0.5));
  vec3 dab = centre + (side * local.x + up * local.y) * size * (isDab ? 1.0 : msFoliageCanopy.y);
  // The card's own axes in the world (its +x and +y after the roll), for its baked dome normals.
  vFoliageCardX = side * cos(roll) + up * sin(roll);
  vFoliageCardY = -side * sin(roll) + up * cos(roll);
  // Every billboard faces the camera wholly, as the painterly study's do. Each is nudged toward the camera by its own
  // stable amount, so two overlapping cards at nearly one depth resolve to one in front instead of fighting per pixel.
  point = dab + view * (fract(rank * 7.31 + 0.13) * 0.03 * size);
  vec3 card = dot(normalWorld, view) < 0.0 ? -normalWorld : normalWorld;
  normalWorld = normalize(mix(shade, mix(card, view, msFoliageCanopy.x), 0.3));
  // Flutter: each card about its cluster by the card's own weight.
  if (msFoliageWindAzimuth.y > 0.0) {
    float flap = msFoliageWindAzimuth.y * ${vertexBend} * sin(msFoliageTime * (3.1 + 2.3 * rank) + 6.2831853 * rank);
    point = anchor + msFoliageTurn(point - anchor, vec2(-travel.y, travel.x), flap);
  }
` : ""}  // Sway: the plant leans about its base by a gust at the base, each point by its bend weight.
  if (msFoliageWind.x > 0.0) {
    vec3 gust = msFoliageGust((msFoliageOrigin.xy - travel * msFoliageTime * msFoliageWind.z) * msFoliageWind.y);
    vec2 own = gust.xy * 2.0 - 1.0;
    vec2 mixed = mix(length(own) > 1e-4 ? normalize(own) : travel, travel, msFoliageWind.w);
    vec2 direction = length(mixed) > 1e-4 ? normalize(mixed) : travel;
    float sway = msFoliageWind.x * weight * (0.35 + 0.65 * gust.z) * (0.8 + 0.2 * sin(msFoliageTime * 1.3 + 6.2831853 * gust.x));
    point = msFoliageOrigin + msFoliageTurn(point - msFoliageOrigin, direction, sway);
    normalWorld = msFoliageTurn(normalWorld, direction, sway);
    shade = msFoliageTurn(shade, direction, sway);
  }
${look.family === "hull" ? `  // The outline swells: where the surface turns edge-on to the view it moves out along its normal, so the leaf
  // brush in the fragment stage can leave leaf clumps standing past the true outline as well as bite into it.
  {
    float edgeOn = 1.0 - abs(dot(normalWorld, normalize(cameraPosition - point)));
    point += normalWorld * msFoliageFringe.x * edgeOn * edgeOn;
  }
` : ""}  vec3 delta = point - original;
  vec3 a = turn[0], b = turn[1], c = turn[2];
  msFoliageOffset = vec3(dot(a, delta) / dot(a, a), dot(b, delta) / dot(b, b), dot(c, delta) / dot(c, c));
  objectNormal = normalize(vec3(dot(a, normalWorld) / dot(a, a), dot(b, normalWorld) / dot(b, b), dot(c, normalWorld) / dot(c, c)));
  vFoliageWorld = point;
  vFoliageShade = shade;
  vFoliageAnchor = anchor;
  vFoliageUv = uv;
  vFoliageHeight = ${height};
  vFoliageVariation = ${variation};
  vFoliageRandom = rank;
  vFoliageCard = fract(sin(rank * 91.7 + ${element} * 13.1 + 0.37) * 43758.5453);
  vFoliageDab = ${dab};
  vFoliageElement = ${element};
  vFoliageHighlight = 0.0;
${leaves ? (chunk ? `  {
    vec3 root = centre, clumpRoot = anchor, ground = shade;
    float height = 0.5, instanceRandom = rank;
    vec3 tint = vec3(1.0);
#ifdef USE_INSTANCING_COLOR
    tint = instanceColor;
#endif
    msFoliageJag = msFoliageJagAt(centre - msFoliageOrigin);
    // The whole plant shaded as one mass (the painterly study's cluster shade): the farther a point lies from the
    // plant's middle on the side away from the sun, the higher its band threshold, so its clumps step darker.
    if (${plantExtent} > 0.0) {
      float depth = clamp(0.5 - dot(centre - ${plantCentre}, normalize(msFoliageSun)) / ${plantExtent} * 0.5, 0.0, 1.0);
      msFoliageJag += depth * msFoliagePlantShade;
    }
    // Light through the plant's own leaves (an ambient cube of the instance's openness): toward the sun it is the
    // leaves' shadow on each other, every way it is how buried the instance is, and with the sun behind it and both the
    // sun and the camera seeing through, the light that comes through the leaves (a simplified subsurface glow).
    vec3 openHigh = floor(${openness}) / 255.0, openLow = fract(${openness});
    vec3 toSun = normalize(msFoliageSun), toEye = normalize(cameraPosition - centre);
    float sunOpen = msFoliageOpen(toSun, openHigh, openLow), eyeOpen = msFoliageOpen(toEye, openHigh, openLow);
    float ambientOpen = (openHigh.x + openHigh.y + openHigh.z + openLow.x + openLow.y + openLow.z) / 6.0;
    // Backlight: how far the camera looks toward the sun, times how much sun reaches the leaf (light through the leaves
    // reaches the eye from any leaf the sun lights from behind). Painted as a warm tint in the fragment stage.
    float behind = smoothstep(-0.1, 0.8, -dot(toSun, toEye));
    // (A plant without a mass model has no openness: no glow either.)
    // Only leaves the sun reaches well glow: the plant's rim and thin parts, not its whole mass.
    // Light through leaves reaches the eye only where both the sun and the eye see through: the plant's rim and the
    // sprigs that hang out of it, not its body. The fragment stage keeps it to a card's own rim.
    float glow = ${plantExtent} > 0.0 ? behind * smoothstep(0.6, 0.95, sunOpen) * smoothstep(0.55, 0.9, eyeOpen) : 0.0;
    vFoliageGlow = glow;
    vFoliageOpen = ${plantExtent} > 0.0 ? ambientOpen : 1.0;
    msFoliageJag += msFoliageSelf.x * (1.0 - sunOpen) + msFoliageSelf.y * (1.0 - ambientOpen) - 0.3 * msFoliageSelf.z * glow;
    ${chunk.inputs}
    msFoliageBand = 1.0;
    ms_evaluate();
    vFoliageColor = ${color};
    vFoliageBand = msFoliageBand;
    vFoliageDark = vFoliageColor;
    vFoliageLight = vFoliageColor;
    vFoliageDarker = vFoliageColor;
    vFoliageLighter = vFoliageColor;
    // ?carddebug=3: openness toward the sun (red), every way (green), the glow (blue).
    if (msFoliageCardDebug > 2.5) vFoliageColor = vFoliageDark = vFoliageLight = vFoliageDarker = vFoliageLighter = vec3(sunOpen, ambientOpen, glow);
    if (${dab} > 0.5) {
      // A dab's leaves may fall a band either side of the dab's own: the paint a band darker and lighter, which the
      // fragment stage picks per leaf (leaf noise in the normals), so band edges run along leaves.
      float own = msFoliageJag;
      msFoliageJag = own + msFoliageLeafJitter;
      ms_evaluate();
      vFoliageDark = ${color};
      msFoliageJag = own - msFoliageLeafJitter;
      ms_evaluate();
      vFoliageLight = ${color};
      // Two bands either way, so a leaf can bridge a jump of two between neighbouring dabs.
      msFoliageJag = own + 2.0 * msFoliageLeafJitter;
      ms_evaluate();
      vFoliageDarker = ${color};
      msFoliageJag = own - 2.0 * msFoliageLeafJitter;
      ms_evaluate();
      vFoliageLighter = ${color};
      msFoliageJag = own;
    }
  }` : `  vFoliageColor = ${color};
  vFoliageBand = min(smoothstep(-0.1, 0.1, dot(shade, normalize(msFoliageSun))), msFoliageCast(centre, shade));`) + `
  // Highlight leaves: a stable share of the clusters, where the band is lit.
  vFoliageHighlight = vFoliageDab < 0.5 && fract(rank * 5.71 + 0.37) < msFoliageCanopy.w ? smoothstep(0.5, 0.95, vFoliageBand) : 0.0;
  if (${sparkle}) {
    // A sparkle shows only in the clump's brightest light, where it faces the sun or turns edge-on at the silhouette
    // (the painterly study's sun cap and contour arc), each at its own threshold so they thin in and out.
    float lit = smoothstep(0.35, 0.7, vFoliageBand);
    float rim = smoothstep(0.35, 0.75, 1.0 - abs(dot(shade, view)));
    float facing = smoothstep(0.3, 0.75, dot(shade, normalize(msFoliageSun)));
    // Most at a sun-facing clump's rim; some anywhere it faces the sun.
    float show = lit * max(rim * max(facing, 0.4), facing * 0.8) * (0.7 + 0.6 * fract(rank * 13.73)) * msFoliageSparkle;
    if (show < 0.25) msFoliageRejected = true;
    // 2: a sparkle, painted lighter than a highlight leaf.
    vFoliageHighlight = 2.0;
  }` : ""}
}
`;
}

/** The fragment stage: the dab cutout, per-pixel paint of bark and hull, and the lit profile's additions. */
function fragmentStage(look: FoliageLook, profile: FoliageProfile, program: FoliageProgram | undefined): { declarations: string; color: string; roughness: string; lights: string } {
  const leaves = look.family === "leaves", hull = look.family === "hull";
  const painted = profile === "painted";
  const chunk = !leaves && program ? plantChunk(programChunk(program.shading.shader, new Set(), new Set(), true)) : undefined;
  const evaluated = !chunk ? "vec3(0.24, 0.17, 0.11) * mix(0.55, 1.0, max(dot(ground, normalize(msFoliageSun)), 0.0))".replace("0.24, 0.17, 0.11", hull ? "0.2, 0.4, 0.12" : "0.24, 0.17, 0.11")
    : painted ? `(${program!.shading.shader.painted ?? "ms_base_color"})` : "ms_base_color";
  const declarations = `
${FAMILY_UNIFORMS}
${PLANT_UNIFORMS}
${FAMILY_FUNCTIONS}
${jagFunctions(true)}
${chunk?.declarations ?? ""}
float msFoliageNoise(vec3 p) { return 0.5 * (msFoliageGust(p.xy + p.z * vec2(0.37, 1.13)).x + msFoliageGust(p.yz * 1.31 + 7.1).y); }`;
  const color = leaves ? `
  {
    // A leaf drawn over its card (+V from the stalk to the tip): ovate and pointed, each card's own width, widest
    // point, tip and bent midrib, its outline loosened by a little noise as a brush would leave it.
    float r1 = vFoliageCard, r2 = fract(r1 * 7.13 + 0.31), r3 = fract(r1 * 13.7 + 0.71), r4 = fract(r1 * 29.3 + 0.17);
    vec3 paint;
    // Band debug (?carddebug=1: each billboard's own band; 2: the band each pixel ends in): the band's index, 0 to 3.
    float debugBand = floor(clamp(vFoliageBand * 6.0, 0.0, 5.999)), debugShift = 0.0;
    // Where on its billboard the backlight shows: a single leaf wholly, a clump card only toward its rim.
    float glowMask = 1.0;
    if (vFoliageDab > 0.5) {
      // A leaf card from the atlas (leaf-cards.ts): one of 16 (the roll turns it), dense cards (rows 0 to 3) in the
      // plant's body and sparse ones (rows 4 to 7) at its rim.
      vec2 uv = clamp(vFoliageUv, 0.0, 1.0);
      float cell = floor(r1 * 16.0) + (vFoliageSparse > 0.5 ? 16.0 : 0.0);
      vec2 at = (vec2(mod(cell, 4.0), floor(cell / 4.0)) + 0.02 + uv * 0.96) / vec2(4.0, 8.0);
      vec4 card = texture(msFoliageCards, at);
      if (card.a < 0.5) discard;
      // The card is a clump: its baked dome normal tips the dab's normal, so bands run along the clump's curve, and
      // each leaf's own noise (its hue, a stable random per leaf) moves the band edge leaf by leaf.
      vec2 dome = texture(msFoliageCardNormals, at).rg * 2.0 - 1.0;
      glowMask = smoothstep(0.55, 0.85, length(dome));
      vec3 own = normalize(vFoliageShade), sun = normalize(msFoliageSun);
      vec3 curved = normalize(own + (vFoliageCardX * dome.x + vFoliageCardY * dome.y) * msFoliageCardShade.x);
      float leaf = card.g;
      // Where a pixel covers several leaves of the card (seen small), their per-leaf values blend into noise: the
      // per-leaf shift fades out there, so a far card keeps clean bands rather than a dither.
      float texels = max(fwidth(at.x) * 1024.0, fwidth(at.y) * 2048.0);
      float perLeaf = 1.0 - smoothstep(0.8, 2.5, texels);
      float offset = dot(curved, sun) - dot(own, sun) + (leaf - 0.5) * msFoliageCardShade.y * perLeaf;
      // Up to two bands either way of the dab's own, by how far the leaf's curved normal turns from it.
      // Up to two bands either way across a card, so each reads as a lit and a shaded side of a clump.
      float steps = clamp(floor(abs(offset) / msFoliageLeafJitter), 0.0, 2.0) * sign(offset);
      vec3 band = steps <= -2.0 ? vFoliageDarker : steps <= -1.0 ? vFoliageDark : steps >= 2.0 ? vFoliageLighter : steps >= 1.0 ? vFoliageLight : vFoliageColor;
      debugShift = steps;
      // Within the band: the leaf's tone (its rim darker, its tip lighter) and a lean warm or cool.
      float lean = (fract(leaf * 7.31) - 0.5) * 2.0;
      // The warm lean is mild and the lightest tone capped, so no leaf goes pale lime.
      vec3 temper = lean > 0.0 ? mix(vec3(1.0), vec3(1.04, 1.03, 0.9), lean) : mix(vec3(1.0), vec3(0.9, 1.0, 1.1), -lean);
      // Within its bands too, a card is lighter on its sun side and darker away from it, so it reads as a rounded clump
      // rather than a flat disc.
      float round_ = clamp(1.0 + msFoliageCardRound * (dot(curved, sun) - dot(own, sun)), 0.6, 1.4);
      paint = band * (0.82 + 0.22 * card.r) * temper * round_;
    } else {
      float v = (clamp(vFoliageUv.y, 0.0, 1.0) - 0.05) / 0.95;
      float u = (vFoliageUv.x - 0.5) * 2.0 - (r4 - 0.5) * 0.45 * v * v;
      float halfWidth = mix(0.62, 0.95, r1) * pow(max(sin(3.14159265 * pow(max(v, 0.0), mix(0.6, 1.0, r2))), 0.0), mix(0.5, 0.95, r3));
      halfWidth *= 1.0 + 0.14 * (msFoliageGust(vec2(v * 7.0, u < 0.0 ? 1.7 : 9.3) + r1 * 23.0).x - 0.5);
      if (v <= 0.0 || abs(u) > halfWidth) discard;
      paint = vFoliageColor * (0.9 + 0.2 * vFoliageVariation) * (0.92 + 0.16 * r2) * mix(0.9, 1.04, v);
    }
    // Fake bounce light: a small plant's shade never gets darker than a floor (light from the ground and sky reaches
    // into a shrub), lifted keeping its hue. The floor fades as the plant grows (a tree's deep inside is dark).
    if (msFoliagePlantMass.w > 0.0) {
      // Bounce light reaches what is open to the surroundings; a clump buried in the plant's middle gets none and
      // steps darker instead.
      float exposed = smoothstep(0.15, 0.55, vFoliageOpen);
      float small = (1.0 - smoothstep(1.0, 8.0, 2.0 * msFoliagePlantMass.w)) * exposed;
      float lift = msFoliageBounce * small;
      paint *= mix(msFoliageCore, 1.0, smoothstep(0.08, 0.35, vFoliageOpen));
      float lum = dot(paint, vec3(0.2126, 0.7152, 0.0722));
      // Bounce light lifts every band of a small plant, its dark and middle bands most and its lit top hardly.
      float raise = 1.0 + msFoliageLift * small * (1.0 - smoothstep(0.05, 0.35, lum));
      paint *= raise;
      lum *= raise;
      // Below the floor: lifted toward it, and a little cool sky light mixed in, so the shade reads soft blue-green.
      if (lum < lift) {
        float under = 1.0 - lum / max(lift, 1e-4);
        paint *= mix(1.0, lift / max(lum, 1e-4), 0.8 * under);
        paint += vec3(0.006, 0.012, 0.014) * under * small;
      }
      // Backlight: light through the leaves, a warm translucent yellow-green over the leaf's own paint.
      paint = mix(paint, paint * 0.5 + vec3(0.38, 0.5, 0.06), clamp(vFoliageGlow * glowMask * msFoliageSelf.z * 0.5, 0.0, 0.9));
    }
    diffuseColor.rgb = ${painted ? `vFoliageHighlight > 1.5
      ? vec3(0.62, 1.0, 0.02) * (0.9 + 0.1 * vFoliageVariation)
      : mix(paint, msFoliageCanopyHighlight * (0.9 + 0.2 * vFoliageVariation), vFoliageHighlight)` : "paint"};
    if (msFoliageCardDebug > 2.5) diffuseColor.rgb = vFoliageColor;
    else if (msFoliageCardDebug > 0.5) {
      // Bands 0 to 3 as blue, cyan, yellow, red; single leaves (not billboards) a shade paler.
      // Bands 0 to 5 as blue, cyan, green, yellow, orange, red.
      float index = msFoliageCardDebug > 1.5 ? clamp(debugBand + debugShift, 0.0, 5.0) : debugBand;
      vec3 hue = index < 0.5 ? vec3(0.1, 0.2, 0.9) : index < 1.5 ? vec3(0.1, 0.8, 0.9) : index < 2.5 ? vec3(0.2, 0.8, 0.2)
        : index < 3.5 ? vec3(0.95, 0.85, 0.1) : index < 4.5 ? vec3(0.95, 0.5, 0.1) : vec3(0.9, 0.15, 0.1);
      diffuseColor.rgb = vFoliageDab > 0.5 ? hue : mix(hue, vec3(1.0), 0.45);
    }
  }` : `
  {
    vec3 root = vFoliageWorld, clumpRoot = vFoliageAnchor, ground = normalize(vFoliageShade);
    float height = vFoliageHeight, instanceRandom = 0.0;
    vec3 tint = vec3(1.0);
${hull ? `    // The outline painted with the leaf brush, as if the surface were flattened into the picture: the brush is read
    // in the plane facing the camera through the plant's base (so it stays on the plant, not the screen), and across
    // the band where the surface turns edge-on only its leaf clumps survive, standing out past the outline (the
    // vertex stage swelled it there) and biting into it.
    float n = msFoliageNoise(root * 3.5);
    {
      vec3 toCamera = normalize(cameraPosition - msFoliageOrigin);
      vec3 across = normalize(cross(vec3(0.0, 0.0, 1.0), toCamera) + vec3(1e-5, 0.0, 0.0));
      vec3 upward = cross(toCamera, across);
      vec2 plane = vec2(dot(root - msFoliageOrigin, across), dot(root - msFoliageOrigin, upward)) / msFoliageFringe.y;
      float leaves = texture(msFoliageBrush, plane).r;
      float edge = smoothstep(0.45, 1.0, 1.0 - abs(dot(ground, normalize(cameraPosition - root))));
      if (leaves < edge * msFoliageFringe.z) discard;
    }
` : ""}${hull ? "    msFoliageJag = msFoliageJagAt(root - msFoliageOrigin) + (msFoliagePlantMass.w > 0.0 ? msFoliageBackingShade : 0.0);\n" : ""}${chunk ? `    ${chunk.inputs}
    msFoliageBand = 1.0;
    ms_evaluate();
` : ""}    diffuseColor.rgb = ${evaluated}${hull ? " * (0.93 + 0.14 * n)" : ""};
${hull && STROKES ? `    // The stroke's id for the edge pass (1 to 248; 0 is nothing, 255 an occluder): its part (offset per plant) times
    // ${STROKE_BANDS} bands plus its band, so a change of band inside a stroke is an edge the leaf brush paints too, as the
    // painterly study's cards each carried one band.
    float strokeBand = clamp(floor(msFoliageBand * ${STROKE_BANDS.toFixed(1)}), 0.0, ${(STROKE_BANDS - 1).toFixed(1)});
    float strokePart = mod(floor(vFoliageElement + 0.5) + floor(fract(dot(msFoliageOrigin.xy, vec2(7.31, 3.17))) * 97.0), ${Math.floor(248 / STROKE_BANDS).toFixed(1)});
    diffuseColor.a = (strokePart * ${STROKE_BANDS.toFixed(1)} + strokeBand + 1.0) / 255.0;
` : ""}  }`;
  const roughness = leaves ? `
  roughnessFactor = mix(roughnessFactor, 0.3, step(fract(vFoliageRandom * 7.13 + 0.5), msFoliageLit.y));` : "";
  // Two-sided diffuse and light through the leaves, in the light loop's units (the sun's irradiance, Lambert).
  const lights = `
  {
    vec3 L = normalize(msFoliageSun), N = normalize(vFoliageShade), V = normalize(cameraPosition - vFoliageWorld);
    float d = dot(N, L), sunlit = msFoliageCast(vFoliageWorld, N);
    float wrapped = max(0.0, (d + msFoliageLit.z) / (1.0 + msFoliageLit.z)) - max(0.0, d);
    float through = ${leaves ? "msFoliageLit.x * (0.35 * max(0.0, -d) + pow(max(0.0, dot(-V, L)), 4.0)) * mix(0.35, 1.0, sunlit)" : "0.0"};
    reflectedLight.directDiffuse += msFoliageSunLight * BRDF_Lambert(diffuseColor.rgb) * (msFoliageLit.z * wrapped * sunlit + through);
  }`;
  return { declarations, color, roughness, lights };
}

/**
 * A plant family's material: unlit (`MeshBasicMaterial`, the scene's painted finishing) for the painted profile, the
 * light loop (`MeshStandardMaterial`) for the lit one; the shadow proxy draws nothing on screen. `shared` holds the
 * scene's foliage uniforms, `level` the batch's own.
 */
export function treeMaterial(look: FoliageLook, profile: FoliageProfile, program: FoliageProgram | undefined, attributes: ReadonlySet<string>,
  shared: Record<string, THREE.IUniform>, level: Record<string, THREE.IUniform>): THREE.Material {
  if (look.family === "shadow") return new THREE.MeshBasicMaterial({ colorWrite: false, depthWrite: false });
  const painted = profile === "painted";
  const material: THREE.Material = painted ? new THREE.MeshBasicMaterial({ color: 0xffffff }) : new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.75, metalness: 0 });
  if (painted) followPaintedFinishing(material);
  material.side = THREE.FrontSide;
  // A stroke writes its id in alpha: without blending three.js leaves alpha as the shader set it.
  if (STROKES && look.family === "hull") material.blending = THREE.NoBlending;
  const vertex = vertexStage(look, profile, program, attributes);
  const fragment = fragmentStage(look, profile, program);
  const uniforms: Record<string, THREE.IUniform> = {
    // A hull behind dabs (the near levels; prototype: levels 0 and 1) is painted in deep shade: it only fills gaps.
    msFoliageBackingShade: { value: look.family === "hull" && look.level <= 1 && PLANT.backing ? 2 : 0 },
    ...(program ? { ...receiverUniforms(), ...program.uniforms, ...fieldUniforms(program.shading.shader), msTime: programTime, msSunDirection: programSun } : {}),
    ...shared, ...level,
  };
  material.onBeforeCompile = shader => {
    Object.assign(shader.uniforms, uniforms);
    const call = painted
      ? ["#include <begin_vertex>", "{ vec3 msFoliageNormal = normal; msFoliageVertex(msFoliageNormal); }\n#include <begin_vertex>"]
      : ["#include <beginnormal_vertex>", "#include <beginnormal_vertex>\n  msFoliageVertex(objectNormal);"];
    shader.vertexShader = shader.vertexShader
      .replace("#include <common>", `#include <common>\n${vertex}`)
      .replace(call[0], call[1])
      .replace("#include <begin_vertex>", "#include <begin_vertex>\n  transformed += msFoliageOffset;")
      .replace("#include <fog_vertex>", "#include <fog_vertex>\n  if (msFoliageRejected) gl_Position = vec4(0.0, 0.0, 2.0, 1.0);");
    let fragmentShader = shader.fragmentShader
      .replace("#include <common>", `#include <common>\n${fragment.declarations}`)
      .replace("#include <color_fragment>", `#include <color_fragment>${fragment.color}`);
    if (!painted) {
      fragmentShader = fragmentShader
        .replace("#include <roughnessmap_fragment>", `#include <roughnessmap_fragment>${fragment.roughness}`)
        .replace("#include <lights_fragment_end>", `#include <lights_fragment_end>${look.family === "bark" ? "" : fragment.lights}`);
    }
    shader.fragmentShader = fragmentShader;
  };
  const key = material.customProgramCacheKey;
  material.customProgramCacheKey = () => `foliage:${look.family}:${profile}:${program?.shading.shader.id ?? program?.shading.shader.source.length ?? "none"}:${[...attributes].sort().join(",")}|${key.call(material)}`;
  material.userData.foliageUniforms = uniforms;
  material.userData.sceneFields = [...new Set((program?.shading.shader.renderInputs ?? [])
    .filter(input => input.kind === "field" && input.glsl === "sampler2D").map(input => input.name!))];
  return material;
}
