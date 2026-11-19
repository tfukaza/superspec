import * as THREE from "three";
import { DEFAULT_CANOPY, type FoliageLook } from "../model/packet.ts";
import { canopyUniforms, treeMaterial, isPlant, SHADOW_PROXY_LAYER } from "./foliage-plants.ts";
import { attributeName } from "./batch.ts";
import { followPaintedFinishing, programSun, programTime, type MaterialShading } from "./shade.ts";
import { sceneFieldUniforms } from "./scene-fields.ts";
import { paintedMass } from "./painted-mass.ts";
import { animeTree, animeTreeLook } from "./anime-tree.ts";

/**
 * Foliage in the viewer (roadmap stage 6, TreeSpec phase 4): instanced batches that carry a foliage look
 * (`SceneBatch.foliage`) are drawn by a small fixed set of shader families with parameters, never by shader code:
 *
 * - **bend**: a static lean of each blade about its root toward a stable random heading;
 * - **wind**: a rigid lean of each blade about its root by a gust field that travels over the world's XY plane with the
 *   viewer's clock (`programTime`, advanced by the time-driven render mode), plus a small flutter per blade;
 * - **rejection**: in the painted profile the vertex stage drops every blade whose root is far from the boundary of the
 *   ground's shade (a distance field remade when the sun or the ground changes, `FoliageBoundary`), unless the view grazes
 *   the ground there; in both profiles it thins instances by their stable `threshold` with distance and drops everything
 *   past the look's `far`. A dropped blade collapses to one point outside the clip volume, so it costs its vertices and
 *   nothing else. The candidate set never changes: moving the sun scatters nothing again.
 *
 * Color is a MaterialSpec painted recipe (`look.material`, a project material) evaluated in the vertex stage, once per
 * vertex at the blade's root, so a blade takes the paint of the ground under its root and only reads where it crosses a
 * shade boundary. The program's `world` and `placedWorld` are the blade's root, its `normal` the ground normal there,
 * `instanceRoot` the clump's origin; screen derivatives are zero, so a shade band's softness in pixels does not apply.
 * Only the tip highlight is per pixel. The lit profile draws the same geometry and instances through the light loop,
 * with the recipe's base color as albedo.
 *
 * Shrubs and trees (roadmap stage 7) are batches of the plant families `bark`, `leaves`, `hull` and `shadow`, drawn by
 * foliage-plants.ts within the same patches, levels and shared uniforms; a plant is one patch of one tile.
 *
 * Tiles: a level's instances are grouped by their `tile` attribute (consecutive per tile). Each frame the viewer culls
 * tiles against the view and picks a level per tile by its distance; the instances of the tiles each level draws are
 * packed to the front of its buffers (only when that choice changes), so a level is one draw call per pass.
 */

/** A painted recipe as the viewer compiled it for a scene: its shading and the uniforms its meshes share. */
export interface FoliageProgram { shading: MaterialShading; uniforms: Record<string, THREE.IUniform> }

/** The look actually drawn: the batch's own, or the page's override (`?foliage=lit|painted`, `setProfile`). */
export type FoliageProfile = FoliageLook["profile"];

/** Vertex and instance attributes the families read, with what they read without one. */
const VERTEX = { pivot: "vec3(0.0)", heightRatio: "clamp(uv.y, 0.0, 1.0)", random: "fract(sin(dot(msFoliagePivot, vec3(12.9898, 78.233, 37.719))) * 43758.5453)" } as const;
const INSTANCE = { groundNormal: "vec3(0.0, 0.0, 1.0)", threshold: "0.0", variation: "0.5", random: "0.0" } as const;

/** Degrees to radians. */
export const RADIANS = Math.PI / 180;

/**
 * The shared GLSL of every family: gust noise, the turn about a root and the sun's cast shadow at a point, read in the
 * vertex stage from the light's shadow map (`sampler2DShadow`, as three.js binds a PCF map).
 */
export const FAMILY_FUNCTIONS = /* glsl */`
vec3 msFoliageHash(vec2 lattice) {
  vec3 projections = vec3(dot(lattice, vec2(127.1, 311.7)), dot(lattice, vec2(269.5, 183.3)), dot(lattice, vec2(419.2, 371.9)));
  return fract(sin(projections) * 43758.5453);
}
vec3 msFoliageGust(vec2 point) {
  vec2 cell = floor(point), local = fract(point);
  vec2 s = local * local * local * (local * (local * 6.0 - 15.0) + 10.0);
  vec3 lower = mix(msFoliageHash(cell), msFoliageHash(cell + vec2(1.0, 0.0)), s.x);
  vec3 upper = mix(msFoliageHash(cell + vec2(0.0, 1.0)), msFoliageHash(cell + vec2(1.0)), s.x);
  return mix(lower, upper, s.y);
}
// Turns v about the horizontal axis that takes +Z toward the unit direction 'toward' (in XY) by 'angle'.
vec3 msFoliageTurn(vec3 v, vec2 toward, float angle) {
  vec3 axis = vec3(-toward.y, toward.x, 0.0);
  float c = cos(angle), s = sin(angle);
  return v * c + cross(axis, v) * s + axis * dot(axis, v) * (1.0 - c);
}
// The sun's cast shadow at a world point: 1 lit, 0 shadowed (nine comparison taps).
float msFoliageCast(vec3 world, vec3 normal) {
  if (msFoliageShadowParams.x < 0.5) return 1.0;
  vec4 c = msFoliageShadowMatrix * vec4(world + normal * msFoliageShadowParams.z, 1.0);
  c.xyz /= c.w;
  if (c.x < 0.0 || c.x > 1.0 || c.y < 0.0 || c.y > 1.0 || c.z > 1.0) return 1.0;
  float depth = c.z + msFoliageShadowParams.y, lit = 0.0;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) lit += texture(msFoliageShadow, vec3(c.xy + vec2(float(x), float(y)) * msFoliageShadowParams.w, depth));
  return lit / 9.0;
}
`;

/** Uniform declarations every family reads, in the vertex stage and in the boundary's band pass. */
export const FAMILY_UNIFORMS = /* glsl */`
uniform float msFoliageTime;
uniform vec3 msFoliageSun;
uniform highp sampler2DShadow msFoliageShadow;
uniform mat4 msFoliageShadowMatrix;
// x: 1 when the sun casts shadows, y: depth bias, z: normal bias (m), w: one texel of the map.
uniform vec4 msFoliageShadowParams;
float msFoliageBand = 1.0;
// Added to every shade band's threshold: the plant families' canopy jag (foliage-plants.ts), 0 elsewhere.
float msFoliageJag = 0.0;
`;

/** Neutralizes screen derivatives where there are none (the vertex stage, a band pass of one value per texel). */
const NO_DERIVATIVES = "#define fwidth(x) ((x) * 0.0)\n#define dFdx(x) ((x) * 0.0)\n#define dFdy(x) ((x) * 0.0)\n";

/** The engine's `shade_band` helper, which the families wrap to read the band a recipe computed (`msFoliageBand`). */
const SHADE_BAND = "float ms_shade_band(vec3 n, vec3 s, float threshold, float softness, float range) {";

/**
 * A painted recipe's GLSL for evaluation at a blade's root (or a texel's point): its source with screen derivatives at
 * zero and `shade_band` wrapped so the band it reaches is kept in `msFoliageBand` (the least of all its bands, cast
 * shadow folded in), and the render inputs it reads declared from what the stage has: `attributes` names the geometry's
 * attributes (none in a band pass), so a named input without one reads its fallback.
 */
export function programChunk(shader: MaterialShading["shader"], attributes: ReadonlySet<string>, declared: Set<string>, derivatives = false): { declarations: string; inputs: string } {
  const lines: string[] = [];
  const inputs: string[] = [];
  for (const input of shader.renderInputs ?? []) {
    const { symbol, glsl } = input;
    if (input.kind === "instance" || input.kind === "vertex") {
      if (symbol === "msInstanceValue") { if (!declared.has(symbol)) { lines.push("vec4 msInstanceValue = vec4(0.0);"); declared.add(symbol); } continue; }
      if (declared.has(symbol)) continue;
      declared.add(symbol);
      lines.push(attributes.has(symbol) ? `attribute ${glsl} ${symbol};` : `#define ${symbol} (${input.fallback ?? `${glsl}(0.0)`})`);
    } else if (input.kind === "derived" && !declared.has(symbol)) {
      declared.add(symbol);
      lines.push(`${glsl} ${symbol};`);
    }
  }
  let source = shader.source;
  if (source.includes(SHADE_BAND)) {
    source = source.replace(SHADE_BAND, `float ms_shade_band_source(vec3 n, vec3 s, float threshold, float softness, float range);
float ms_shade_band(vec3 n, vec3 s, float threshold, float softness, float range) {
  float band = ms_shade_band_source(n, s, threshold + msFoliageJag, softness, range);
  msFoliageBand = min(msFoliageBand, band);
  return band;
}
float ms_shade_band_source(vec3 n, vec3 s, float threshold, float softness, float range) {`);
  }
  // The inputs of one evaluation at a root: set by the caller's locals `root`, `clumpRoot`, `ground`, `rank`.
  const derived = new Set((shader.renderInputs ?? []).filter(input => input.kind === "derived").map(input => input.symbol));
  if (derived.has("msInstanceRoot")) inputs.push("msInstanceRoot = clumpRoot;");
  if (derived.has("msPlacedWorld")) inputs.push("msPlacedWorld = root;");
  if (derived.has("msInstanceTint")) inputs.push("msInstanceTint = tint;");
  if (derived.has("msInstanceRandom")) inputs.push("msInstanceRandom = instanceRandom;");
  inputs.push("ms_world = root; ms_object = root; ms_normal = ground; ms_uv = vec2(0.0, height); ms_surface = root.xy; ms_footprint = 0.0; ms_uv_footprint = 0.0;");
  if (/\bms_cast_shadow\b/.test(source)) inputs.push("ms_cast_shadow = msFoliageCast(root, ground);");
  if (derivatives) return { declarations: `${lines.join("\n")}\n${source}\n`, inputs: inputs.join("\n    ") };
  return { declarations: `${lines.join("\n")}\n${NO_DERIVATIVES}${source}\n#undef fwidth\n#undef dFdx\n#undef dFdy\n`, inputs: inputs.join("\n    ") };
}

/** The shared uniforms of a program evaluated outside `programMaterial`: what describes a receiver, set to nothing. */
export function receiverUniforms(): Record<string, THREE.IUniform> {
  const features = new THREE.DataTexture(new Float32Array(12), 3, 1, THREE.RGBAFormat, THREE.FloatType);
  features.needsUpdate = true;
  return {
    ms_features: { value: features }, ms_feature_count: { value: 0 }, ms_gravity: { value: new THREE.Vector3(0, 0, -1) },
    ms_bounds_min: { value: new THREE.Vector3() }, ms_bounds_max: { value: new THREE.Vector3(1, 1, 1) }, ms_loop: { value: 0 },
  };
}

/** Uniforms of a program's scene fields (`sampleField`), shared with every other material that samples them. */
export function fieldUniforms(shader: MaterialShading["shader"]): Record<string, THREE.IUniform> {
  const uniforms: Record<string, THREE.IUniform> = {};
  for (const input of shader.renderInputs ?? []) {
    if (input.kind !== "field" || !input.name) continue;
    const field = sceneFieldUniforms(input.name);
    uniforms[input.symbol] = input.glsl === "sampler2D" ? field.texture : field.rect;
  }
  return uniforms;
}

/** The sun's shadow map as the families read it; shared by every foliage material and the boundary pass. */
class FoliageShadow {
  /** A one-texel comparison depth texture standing in while the sun casts no shadow, so the sampler always has one. */
  private readonly empty: THREE.WebGLRenderTarget;
  readonly uniforms = {
    msFoliageShadow: { value: null as THREE.Texture | null },
    msFoliageShadowMatrix: { value: new THREE.Matrix4() },
    msFoliageShadowParams: { value: new THREE.Vector4() },
  };

  constructor() {
    const depth = new THREE.DepthTexture(1, 1, THREE.FloatType);
    depth.compareFunction = THREE.LessEqualCompare;
    depth.minFilter = depth.magFilter = THREE.LinearFilter;
    this.empty = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: true, depthTexture: depth });
    this.uniforms.msFoliageShadow.value = depth;
  }

  /** Follows the sun's current shadow map (three.js makes it on the first frame that casts). */
  update(sun: THREE.DirectionalLight | undefined, casting: boolean): void {
    const map = sun?.shadow.map?.depthTexture;
    const usable = casting && sun && map && map.compareFunction !== null;
    this.uniforms.msFoliageShadow.value = usable ? map : this.empty.depthTexture;
    if (usable) {
      this.uniforms.msFoliageShadowMatrix.value.copy(sun.shadow.matrix);
      this.uniforms.msFoliageShadowParams.value.set(1, sun.shadow.bias, sun.shadow.normalBias, 1 / Math.max(1, sun.shadow.mapSize.x));
    } else this.uniforms.msFoliageShadowParams.value.set(0, 0, 0, 0);
  }

  /** A changing value of the map (none, or the texture and matrix the sun has), to remake what depends on it. */
  signature(): string {
    const params = this.uniforms.msFoliageShadowParams.value;
    return params.x ? `${this.uniforms.msFoliageShadow.value?.uuid}:${this.uniforms.msFoliageShadowMatrix.value.elements.map(v => v.toFixed(5)).join(",")}` : "none";
  }

  dispose(): void { this.empty.dispose(); }
}

/** The family uniforms of one look, by level (each level of a patch shares the look's parameters but not its range). */
function lookUniforms(look: FoliageLook, next: { distance: number; keep: number }): Record<string, THREE.IUniform> {
  return {
    // x: this level's distance, y: its keep, z: where the next level starts (or `far`), w: the next level's keep (0 at far).
    msFoliageLevel: { value: new THREE.Vector4(look.distance, look.keep, next.distance, next.keep) },
    msFoliageFar: { value: look.far },
    msFoliageBend: { value: new THREE.Vector2(look.bend.angle * RADIANS, look.bend.randomness) },
    // x: lean, y: 1 / gust size, z: speed, w: bias.
    msFoliageWind: { value: new THREE.Vector4(look.wind.angle * RADIANS, 1 / look.wind.scale, look.wind.speed, look.wind.bias) },
    msFoliageWindAzimuth: { value: new THREE.Vector2(look.wind.azimuth * RADIANS, look.wind.flutter * RADIANS) },
    // x: boundary (m), y: fill, z: sine of the grazing angle, w: grazing keep.
    msFoliageReject: { value: new THREE.Vector4(look.rejection.boundary, look.rejection.fill, Math.sin(look.rejection.grazing * RADIANS), look.rejection.grazingKeep) },
    msFoliageTip: { value: new THREE.Vector3(look.tip.lift, look.tip.start, look.tip.amount) },
    msFoliageHighlight: { value: new THREE.Color().setRGB(...look.tip.highlight, THREE.LinearSRGBColorSpace) },
    // Highlight grass: four tip colors, one per blade (x of the first is negative without a palette).
    msFoliagePalette: { value: (look.tip.palette ?? [[-1, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0]]).map(c => new THREE.Vector3(...c)) },
    ...(look.family === "grass" ? {} : canopyUniforms(look.canopy ?? DEFAULT_CANOPY)),
  };
}

/** The vertex stage of every family: which blades are drawn, where their vertices move and the color they carry. */
function vertexStage(look: FoliageLook, profile: FoliageProfile, program: FoliageProgram | undefined, attributes: ReadonlySet<string>): { declarations: string; body: string } {
  const declared = new Set<string>();
  const lines: string[] = [];
  const named = (domain: "vertex" | "instance", name: string, glsl: string, fallback: string): string => {
    const symbol = attributeName(domain, name);
    declared.add(symbol);
    lines.push(attributes.has(symbol) ? `attribute ${glsl} ${symbol};` : `#define ${symbol} (${fallback})`);
    return symbol;
  };
  const pivot = named("vertex", "pivot", "vec3", VERTEX.pivot);
  const height = named("vertex", "heightRatio", "float", VERTEX.heightRatio);
  lines.push(`#define msFoliagePivot ${pivot}`);
  const bladeRandom = named("vertex", "random", "float", VERTEX.random);
  const groundNormal = named("instance", "groundNormal", "vec3", INSTANCE.groundNormal);
  const threshold = named("instance", "threshold", "float", INSTANCE.threshold);
  const variation = named("instance", "variation", "float", INSTANCE.variation);
  const instanceRandom = named("instance", "random", "float", INSTANCE.random);
  const chunk = program ? programChunk(program.shading.shader, attributes, declared) : undefined;
  const painted = profile === "painted";
  const color = !chunk ? "vec3(0.22, 0.42, 0.1) * mix(0.55, 1.0, height)"
    : painted ? `(${program!.shading.shader.painted ?? "ms_base_color"})` : "ms_base_color";
  const declarations = `
#define MS_FOLIAGE
uniform vec4 msFoliageLevel;
uniform float msFoliageFar;
uniform vec2 msFoliageBend;
uniform vec4 msFoliageWind;
uniform vec2 msFoliageWindAzimuth;
uniform vec4 msFoliageReject;
uniform highp sampler2D msFoliageBoundary;
uniform vec4 msFoliageBoundaryRect;
uniform float msFoliageBoundaryReady;
${FAMILY_UNIFORMS}
${lines.join("\n")}
varying vec3 vFoliageColor;
varying float vFoliageHeight;
varying float vFoliageBand;
varying float vFoliageVariation;
// Per blade (its own random with its clump's): picks a highlight blade's palette color.
varying float vFoliageBlade;
// The blade's own coordinates: x across it (0 to 1), y up it (0 at the root, 1 at the tip).
varying vec2 vFoliageUv;
bool msFoliageRejected = false;
vec3 msFoliageOffset = vec3(0.0);
${FAMILY_FUNCTIONS}
// Meters from a point of the plan to the nearest shade boundary (FoliageBoundary), or far without the field.
float msFoliageBoundaryAt(vec2 p) {
  if (msFoliageBoundaryReady < 0.5) return 1e6;
  ivec2 size = textureSize(msFoliageBoundary, 0);
  vec2 at = (p - msFoliageBoundaryRect.xy) / (msFoliageBoundaryRect.zw - msFoliageBoundaryRect.xy);
  if (any(lessThan(at, vec2(0.0))) || any(greaterThan(at, vec2(1.0)))) return 1e6;
  return texelFetch(msFoliageBoundary, min(ivec2(at * vec2(size)), size - 1), 0).r;
}
${chunk?.declarations ?? ""}
void msFoliageVertex(inout vec3 objectNormal) {
  mat4 place = modelMatrix;
#ifdef USE_INSTANCING
  place = modelMatrix * instanceMatrix;
#endif
  vec3 root = (place * vec4(${pivot}, 1.0)).xyz;
  vec3 clumpRoot = (place * vec4(0.0, 0.0, 0.0, 1.0)).xyz;
  vec3 point = (place * vec4(position, 1.0)).xyz;
  vec3 ground = normalize(${groundNormal});
  float rank = ${bladeRandom};
  float height = ${height};
  float away = length(cameraPosition - root);
  // Thinning toward the next level's share, by the instance's stable threshold; nothing past far.
  float onward = clamp((away - msFoliageLevel.x) / max(msFoliageLevel.z - msFoliageLevel.x, 1e-3), 0.0, 1.0);
  bool keep = away < msFoliageFar && ${threshold} < mix(msFoliageLevel.y, msFoliageLevel.w, onward);
${painted ? `  // Rejection: blades near the shade boundary, where the view grazes the ground, and a share everywhere.
  if (keep && msFoliageReject.x > 0.0) {
    float share = max(msFoliageReject.y, 1.0 - smoothstep(0.0, msFoliageReject.x, msFoliageBoundaryAt(root.xy)));
    float facing = abs(dot(normalize(cameraPosition - root), ground));
    share = max(share, (1.0 - smoothstep(0.5 * msFoliageReject.z, msFoliageReject.z, facing)) * msFoliageReject.w);
    keep = rank < share;
  }
` : ""}  msFoliageRejected = !keep;
  if (!keep) return;
  mat3 turn = mat3(place);
  vec3 normalWorld = normalize(turn * objectNormal);
  vec3 moved = point - root;
  // Bend: a static lean about the root toward a stable heading.
  if (msFoliageBend.x > 0.0) {
    float heading = 6.2831853 * fract(rank * 7.31 + 0.137);
    vec2 facingAway = vec2(cos(heading), sin(heading));
    float lean = msFoliageBend.x * (1.0 + msFoliageBend.y * (2.0 * fract(rank * 13.71 + 0.5) - 1.0));
    moved = msFoliageTurn(moved, facingAway, lean);
    normalWorld = msFoliageTurn(normalWorld, facingAway, lean);
  }
  // Wind: a rigid lean about the root by a travelling gust field, plus each blade's flutter.
  if (msFoliageWind.x > 0.0 || msFoliageWindAzimuth.y > 0.0) {
    vec2 travel = vec2(cos(msFoliageWindAzimuth.x), sin(msFoliageWindAzimuth.x));
    vec3 gust = msFoliageGust((root.xy - travel * msFoliageTime * msFoliageWind.z) * msFoliageWind.y);
    vec2 own = gust.xy * 2.0 - 1.0;
    vec2 mixed = mix(length(own) > 1e-4 ? normalize(own) : travel, travel, msFoliageWind.w);
    vec2 direction = length(mixed) > 1e-4 ? normalize(mixed) : travel;
    float angle = msFoliageWind.x * gust.z * gust.z + msFoliageWindAzimuth.y * sin(msFoliageTime * (2.3 + 2.0 * rank) + rank * 6.2831853);
    moved = msFoliageTurn(moved, direction, angle);
    normalWorld = msFoliageTurn(normalWorld, direction, angle);
  }
  // Back to the instance's own space: its axes are perpendicular, so each is divided by its squared length.
  vec3 delta = root + moved - point;
  vec3 a = turn[0], b = turn[1], c = turn[2];
  msFoliageOffset = vec3(dot(a, delta) / dot(a, a), dot(b, delta) / dot(b, b), dot(c, delta) / dot(c, c));
  objectNormal = normalize(vec3(dot(a, normalWorld) / dot(a, a), dot(b, normalWorld) / dot(b, b), dot(c, normalWorld) / dot(c, c)));
  vFoliageHeight = height;
  vFoliageVariation = ${variation};
  vFoliageBlade = fract(rank * 5.37 + ${variation} * 1.913);
  vFoliageUv = uv;
${chunk ? `  {
    vec3 tint = vec3(1.0);
#ifdef USE_INSTANCING_COLOR
    tint = instanceColor;
#endif
    float instanceRandom = ${instanceRandom};
    ${chunk.inputs}
    msFoliageBand = 1.0;
    ms_evaluate();
    vFoliageColor = ${color};
    vFoliageBand = msFoliageBand;
  }` : `  vFoliageColor = ${color};
  vFoliageBand = msFoliageCast(root, ground);`}
}
`;
  return { declarations, body: "" };
}

/**
 * Painted: the vertex color with the tip's lift and, on the lit side of the band, its highlight. Lit: the albedo.
 * `brush` (highlight grass, a look with a tip palette): each blade is a brushstroke of its own, drawn translucent over the
 * grass: feathered across its width, streaked along its length by bristles, its tip breaking up as a dry brush lifts
 * and its root fading in, so the stroke sits on the lawn without a hard base. Each blade keeps its own edges: strokes
 * overlap as paint does and never run together as a blur of the picture would.
 */
function fragmentStage(profile: FoliageProfile, brush = false): { declarations: string; color: string } {
  const declarations = `
uniform vec3 msFoliageTip;
uniform vec3 msFoliageHighlight;
uniform vec3 msFoliagePalette[4];
varying vec3 vFoliageColor;
varying float vFoliageHeight;
varying float vFoliageBand;
varying float vFoliageVariation;
varying float vFoliageBlade;
varying vec2 vFoliageUv;
float msBrushNoise(float x) {
  float i = floor(x), f = fract(x);
  float a = fract(sin(i * 91.3458) * 43758.5453), b = fract(sin((i + 1.0) * 91.3458) * 43758.5453);
  return mix(a, b, f * f * (3.0 - 2.0 * f));
}`;
  const stroke = brush ? `
    {
      float across = abs(vFoliageUv.x * 2.0 - 1.0), along = clamp(vFoliageUv.y, 0.0, 1.0);
      // Bristles: a few streaks along the stroke, each its own strength, wandering a little up it.
      float bristle = msBrushNoise(vFoliageUv.x * 6.0 + vFoliageBlade * 37.0 + along * 0.9);
      float edge = 1.0 - smoothstep(0.35, 1.0, across);
      // The dry tip: past 70 % of its height a streak ends where its own value says.
      float dry = 1.0 - smoothstep(0.62 + 0.3 * bristle, 0.75 + 0.3 * bristle, along);
      float root = smoothstep(0.02, 0.3, along);
      diffuseColor.a = 0.9 * edge * mix(0.45, 1.0, bristle) * dry * root;
      diffuseColor.rgb *= 0.93 + 0.14 * bristle;
    }` : "";
  const color = profile === "painted" ? `
  {
    float h = clamp(vFoliageHeight, 0.0, 1.0);
    vec3 paint = vFoliageColor * mix(1.0, msFoliageTip.x, h);
    float fade = smoothstep(msFoliageTip.y, 1.0, h) * msFoliageTip.z * clamp(vFoliageBand, 0.0, 1.0);
    // The tip lightens toward a lighter, warmer version of the blade's own colour, with only a fifth of the shared
    // highlight in it: blades rooted on different greens keep different tips instead of all ending in one colour.
    vec3 own = paint * vec3(1.24, 1.17, 0.86) * (0.92 + 0.16 * vFoliageVariation);
    vec3 target = mix(own, msFoliageHighlight * (0.85 + 0.3 * vFoliageVariation), 0.2);
    if (msFoliagePalette[0].x >= 0.0) {
      // Highlight grass: the blade is the ground's paint up to start, then its own palette color fades in over the next
      // 0.35 of its height, on the lit side only, so its root melts into the ground and its tip is a stroke of color.
      int k = int(min(floor(vFoliageBlade * 4.0), 3.0));
      vec3 tipColor = k == 0 ? msFoliagePalette[0] : k == 1 ? msFoliagePalette[1] : k == 2 ? msFoliagePalette[2] : msFoliagePalette[3];
      fade = smoothstep(msFoliageTip.y, min(msFoliageTip.y + 0.35, 1.0), h) * msFoliageTip.z * clamp(vFoliageBand, 0.0, 1.0);
      target = tipColor;
    }
    diffuseColor.rgb = mix(paint, target, fade);${stroke}
  }` : `
  diffuseColor.rgb = vFoliageColor * mix(0.5, 1.0, clamp(vFoliageHeight, 0.0, 1.0)) * (0.88 + 0.24 * vFoliageVariation);`;
  return { declarations, color };
}

/**
 * A foliage material: `MeshBasicMaterial` for the painted profile (its paint reaches the screen as authored, following
 * the scene's painted finishing), `MeshStandardMaterial` for the lit one. Blades are delivered with both faces, so only
 * front faces are drawn. `shared` holds the uniforms every material of the scene shares (sun, time, shadow, boundary).
 */
function makeMaterial(look: FoliageLook, profile: FoliageProfile, program: FoliageProgram | undefined, attributes: ReadonlySet<string>,
  shared: Record<string, THREE.IUniform>, level: Record<string, THREE.IUniform>): THREE.Material {
  // A plant's parts have families of their own (foliage-plants.ts).
  if (isPlant(look)) return treeMaterial(look, profile, program, attributes, shared, level);
  const material: THREE.Material = profile === "painted" ? new THREE.MeshBasicMaterial({ color: 0xffffff }) : new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.85, metalness: 0 });
  if (profile === "painted") followPaintedFinishing(material);
  material.side = THREE.FrontSide;
  // Highlight grass in the painted profile is drawn as translucent brushstrokes after the opaque grass.
  const brush = profile === "painted" && look.family === "grass" && !!look.tip.palette;
  if (brush) { material.transparent = true; material.depthWrite = false; }
  const vertex = vertexStage(look, profile, program, attributes);
  const fragment = fragmentStage(profile, brush);
  const uniforms: Record<string, THREE.IUniform> = {
    ...(program ? { ...receiverUniforms(), ...program.uniforms, ...fieldUniforms(program.shading.shader), msTime: programTime, msSunDirection: programSun } : {}),
    ...shared, ...level,
  };
  material.onBeforeCompile = shader => {
    Object.assign(shader.uniforms, uniforms);
    const call = profile === "painted"
      ? ["#include <begin_vertex>", "{ vec3 msFoliageNormal = normal; msFoliageVertex(msFoliageNormal); }\n#include <begin_vertex>"]
      : ["#include <beginnormal_vertex>", "#include <beginnormal_vertex>\n  msFoliageVertex(objectNormal);"];
    shader.vertexShader = shader.vertexShader
      .replace("#include <common>", `#include <common>\n${vertex.declarations}`)
      .replace(call[0], call[1])
      .replace("#include <begin_vertex>", "#include <begin_vertex>\n  transformed += msFoliageOffset;")
      .replace("#include <fog_vertex>", "#include <fog_vertex>\n  if (msFoliageRejected) gl_Position = vec4(0.0, 0.0, 2.0, 1.0);");
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", `#include <common>\n${fragment.declarations}`)
      .replace("#include <color_fragment>", `#include <color_fragment>${fragment.color}`);
  };
  const key = material.customProgramCacheKey;
  material.customProgramCacheKey = () => `foliage:${look.family}:${profile}:${brush ? "brush:" : ""}${program?.shading.shader.id ?? program?.shading.shader.source.length ?? "none"}:${[...attributes].sort().join(",")}|${key.call(material)}`;
  material.userData.foliageUniforms = uniforms;
  // The scene fields the families need: the ground's (the boundary's ground) and those the recipe samples.
  material.userData.sceneFields = [...new Set(["ground.normal", ...(program?.shading.shader.renderInputs ?? [])
    .filter(input => input.kind === "field" && input.glsl === "sampler2D").map(input => input.name!)])];
  return material;
}

/** Where one tile's instances are in a level's batch. */
interface TileRange { start: number; count: number }

/** One level's instanced mesh, the instances it was delivered with and where each tile's are. */
interface LevelMesh {
  mesh: THREE.InstancedMesh;
  look: FoliageLook;
  /** Copies of the delivered instance data, which the drawn buffers are packed from. */
  source: { attribute: THREE.BufferAttribute; values: Float32Array; size: number }[];
  tiles: Map<number, TileRange>;
  /** Instances drawn now. */
  drawn: number;
  /** The level's family uniforms (its range of distances). */
  level: Record<string, THREE.IUniform>;
  /** Vertices of the mesh and blades in it (distinct roots). */
  vertices: number;
  blades: number;
  /** A plant's leaves or hull whose patch is painted as a mass (`FoliageLook.mass`): never drawn. */
  replaced?: boolean;
  /** The painted mass this level carries (painted-mass.ts), drawn whenever the level is. */
  painted?: THREE.Mesh[];
}

/** One field of foliage: its levels, its tiles' bounds and the level each tile draws. */
interface Patch { id: string; levels: LevelMesh[]; tiles: Map<number, THREE.Box3>; far: number; chosen: Map<number, number>; signature: string; origin?: THREE.Vector3 }

/** The stated look with what the page overrides. */
function drawnProfile(look: FoliageLook, override: FoliageProfile | null): FoliageProfile { return override ?? look.profile; }

/**
 * The foliage of a displayed scene: its patches, their materials and the boundary field. `RustScene` adds each foliage
 * batch's instanced mesh as it builds a scene (`prepare`), gives them their recipe's program when the scene's shading
 * arrives (`withProgram`), and calls `update` before every frame and `afterFrame` after it.
 */
export class FoliageView {
  /** Shared by every foliage material: the sun, the clock, the sun's shadow map and the boundary field. */
  private readonly shadow = new FoliageShadow();
  readonly boundary: FoliageBoundary;
  private readonly shared: Record<string, THREE.IUniform>;
  private patches: Patch[] = [];
  private group?: THREE.Object3D;
  private override: FoliageProfile | null;
  private boundarySignature = "";
  /** What of the plants casts shadows now (see `update`). */
  private casting = "";
  private readonly frustum = new THREE.Frustum();
  private readonly matrix = new THREE.Matrix4();
  /** Time to make the boundary field last, and how often it was made. */
  readonly stats = { boundaryMs: 0, boundaryGpuMs: null as number | null, boundaryMade: 0, packs: 0, packMs: 0 };

  constructor(private readonly renderer: THREE.WebGLRenderer, override: string | null) {
    this.override = override === "lit" || override === "painted" ? override : null;
    this.boundary = new FoliageBoundary(renderer, this.shadow);
    this.shared = { msFoliageTime: programTime, msFoliageSun: programSun, msFoliageSunLight: this.sunLight, ...this.shadow.uniforms, ...this.boundary.uniforms };
  }

  /** The sun's irradiance (color times intensity), for the plant families' additions to the light loop. */
  private readonly sunLight = { value: new THREE.Color(1, 1, 1) };

  /** The profile the page draws every patch with, or null for each batch's own. */
  get profileOverride(): FoliageProfile | null { return this.override; }

  /**
   * Makes a foliage batch's mesh ready to draw with its families: its first material (without a program, until the
   * scene's shading names one), no cast shadow (blades take the ground's shade at their roots, and casting would shade
   * the roots they sample), and the copies its tiles are packed from.
   */
  prepare(mesh: THREE.InstancedMesh, look: FoliageLook): void {
    const geometry = mesh.geometry;
    // A plant's middle and size are one value for the whole plant: read once into a uniform (foliage-plants.ts) and
    // dropped from the geometry, which has no attribute slots to spare.
    // A mass-model plant (its leaves carry `dab`): its middle and size from where its leaf instances stand.
    if (look.family === "leaves" && geometry.getAttribute(attributeName("instance", "openness")) && mesh.count > 0) {
      const box = new THREE.Box3(), m = new THREE.Matrix4(), p = new THREE.Vector3();
      for (let i = 0; i < mesh.count; i++) { m.fromArray(mesh.instanceMatrix.array, i * 16); box.expandByPoint(p.setFromMatrixPosition(m)); }
      const centre = box.getCenter(new THREE.Vector3()), size = box.getSize(new THREE.Vector3());
      mesh.userData.plantMass = new THREE.Vector4(centre.x, centre.y, centre.z, 0.5 * Math.max(size.x, size.y, size.z));
    }
    const attributes = new Set(Object.keys(geometry.attributes));
    mesh.userData.foliage = look;
    // The level's own uniforms, made once: every material the mesh gets shares them, and the patch sets their range.
    mesh.userData.foliageLevel = lookUniforms(look, { distance: look.far, keep: 0 });
    // xyz: the plant's middle, w: its size (0: not shaded as one mass).
    mesh.userData.foliageLevel.msFoliagePlantMass = { value: mesh.userData.plantMass ?? new THREE.Vector4() };
    // A plant casts its shadow from its bark and its canopy's proxy, which only the sun's shadow camera draws; leaves and
    // the hull cast none (the proxy stands for them) and fold the cast shadow into their band.
    // A painted mass casts from its own parts (painted-mass.ts), not from the coarse proxy that carries it.
    mesh.userData.foliageCasts = look.family === "bark" || (look.family === "shadow" && !look.mass);
    mesh.castShadow = mesh.userData.foliageCasts;
    mesh.receiveShadow = look.family !== "shadow" && drawnProfile(look, this.override) === "lit";
    if (look.family === "shadow") mesh.layers.set(SHADOW_PROXY_LAYER);
    (mesh.material as THREE.Material).dispose();
    mesh.material = makeMaterial(look, drawnProfile(look, this.override), undefined, attributes, this.shared, mesh.userData.foliageLevel);
  }

  /** The material of a foliage mesh with its recipe's program (null when the batch names none the scene has). */
  withProgram(mesh: THREE.InstancedMesh, program: FoliageProgram | undefined): THREE.Material {
    mesh.userData.foliageProgram = program;
    const look = mesh.userData.foliage as FoliageLook;
    return makeMaterial(look, drawnProfile(look, this.override), program, new Set(Object.keys(mesh.geometry.attributes)), this.shared, mesh.userData.foliageLevel);
  }

  /** Every foliage mesh of the displayed scene. */
  meshes(): THREE.InstancedMesh[] { return this.patches.flatMap(patch => patch.levels.map(level => level.mesh)); }

  /** Draws every patch with `profile` (null: each batch's own) and returns the materials to compile. */
  setProfile(profile: FoliageProfile | null): THREE.Material[] {
    this.override = profile;
    const made: THREE.Material[] = [];
    for (const patch of this.patches) for (const level of patch.levels) {
      const mesh = level.mesh, look = mesh.userData.foliage as FoliageLook;
      if (look.family === "shadow") continue;
      (mesh.material as THREE.Material).dispose();
      mesh.material = makeMaterial(look, drawnProfile(look, profile), mesh.userData.foliageProgram as FoliageProgram | undefined,
        new Set(Object.keys(mesh.geometry.attributes)), this.shared, level.level);
      mesh.receiveShadow = drawnProfile(look, profile) === "lit";
      made.push(mesh.material as THREE.Material);
    }
    return made;
  }

  /** Asks for the boundary field to be made again before the next frame (the ground, the shading or the shadows changed). */
  invalidate(): void { this.boundarySignature = ""; }

  /**
   * Before a frame: finds the patches of a newly displayed scene, follows the sun's shadow map, culls tiles against the
   * view and picks each one's level, packing the instances each level draws when that choice changed. Returns whether
   * the scene has foliage.
   */
  update(models: THREE.Object3D, camera: THREE.Camera, sun: THREE.DirectionalLight | undefined, casting: boolean): boolean {
    if (models !== this.group) this.collect(models);
    if (!this.patches.length) return false;
    this.shadow.update(sun, casting);
    if (sun) {
      this.sunLight.value.copy(sun.color).multiplyScalar(sun.intensity);
      // Plants' shadow proxies are drawn by the sun's shadow camera alone.
      sun.shadow.camera.layers.enable(SHADOW_PROXY_LAYER);
    }
    this.matrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.frustum.setFromProjectionMatrix(this.matrix);
    const eye = camera.getWorldPosition(new THREE.Vector3());
    for (const patch of this.patches) {
      const levels = [...new Set(patch.levels.map(level => level.look.level))].sort((a, b) => a - b);
      const start = new Map(patch.levels.map(level => [level.look.level, level.look.distance]));
      let signature = "";
      for (const [tile, box] of patch.tiles) {
        let chosen = -1;
        if (this.frustum.intersectsBox(box)) {
          const distance = box.distanceToPoint(eye);
          if (distance < patch.far) for (const level of levels) if (start.get(level)! <= distance) chosen = level;
        }
        patch.chosen.set(tile, chosen);
        signature += `${chosen},`;
      }
      if (signature === patch.signature) continue;
      patch.signature = signature;
      const began = performance.now();
      for (const level of patch.levels) this.pack(level, patch.chosen);
      this.stats.packs++;
      this.stats.packMs += performance.now() - began;
    }
    // The sun's shadow map is made again only when asked: when a plant's casting parts are shown or hidden (by a level
    // change or by the page), its shadow must follow.
    let shown = "";
    for (const patch of this.patches) for (const level of patch.levels) {
      if (level.mesh.castShadow) shown += level.mesh.visible ? "1" : "0";
      for (const part of level.painted ?? []) shown += part.visible && level.mesh.visible ? "1" : "0";
    }
    if (shown !== this.casting) { this.casting = shown; this.renderer.shadowMap.needsUpdate = true; }
    return true;
  }

  /**
   * After a frame: makes the boundary field again when the sun, its shadow map or the ground changed since it was made
   * (the shadow map is made during a frame, so this follows one). Returns whether it did, so the caller draws again.
   */
  afterFrame(ground: { positions: THREE.Texture; normals: THREE.Texture; rect: THREE.Vector4; width: number; height: number } | null): boolean {
    // The boundary field is grass's: plants have none.
    const grass = this.patches.filter(patch => patch.levels.some(level => level.look.family === "grass"));
    if (!grass.length) return false;
    const programs = grass.flatMap(patch => patch.levels.map(level => level.mesh.userData.foliageProgram as FoliageProgram | undefined)).filter(Boolean) as FoliageProgram[];
    const program = programs[0];
    const look = grass[0].levels[0].look;
    const sun = programSun.value;
    const values = program ? Object.values(program.uniforms).map(uniform => {
      const value = uniform.value as { toArray?: () => number[] } | number | boolean;
      return typeof value === "object" && value && value.toArray ? value.toArray().join(",") : String(value);
    }).join(";") : "";
    const signature = `${ground ? `${ground.positions.uuid}:${ground.rect.toArray().join(",")}` : "none"}|${sun.toArray().map(v => v.toFixed(6)).join(",")}|${this.shadow.signature()}|${program?.shading.shader.id ?? "none"}|${values}`;
    if (signature === this.boundarySignature) return false;
    this.boundarySignature = signature;
    // A look that keeps every blade (no boundary distance) needs no boundary field: none is made, also when the sun moves.
    if (!ground || look.rejection.boundary <= 0) { this.boundary.clear(); return false; }
    const began = performance.now();
    this.boundary.make(ground, program, look.rejection.boundary);
    this.stats.boundaryMs = performance.now() - began;
    this.stats.boundaryGpuMs = this.boundary.lastGpuMs;
    this.stats.boundaryMade++;
    return true;
  }

  /** What is drawn: per patch, tiles in view and per level the instances drawn, for `data-foliage` and tests. */
  describe(): { profile: FoliageProfile | "batch"; patches: unknown[]; boundary: unknown } {
    return {
      profile: this.override ?? "batch",
      patches: this.patches.map(patch => {
        type Entry = { level: number; meshes: number; instances: number; drawn: number; blades: number; bladesDrawn: number; vertices: number; verticesDrawn: number; families?: Record<string, { instances: number; drawn: number; triangles: number }> };
        const byLevel = new Map<number, Entry>();
        const plant = patch.levels.some(level => isPlant(level.look));
        for (const level of patch.levels) {
          const entry: Entry = byLevel.get(level.look.level) ?? { level: level.look.level, meshes: 0, instances: 0, drawn: 0, blades: 0, bladesDrawn: 0, vertices: 0, verticesDrawn: 0 };
          const total = level.source[0] ? level.source[0].values.length / level.source[0].size : 0;
          entry.meshes += level.drawn ? 1 : 0;
          entry.instances += total; entry.drawn += level.drawn;
          entry.blades += total * level.blades; entry.bladesDrawn += level.drawn * level.blades;
          entry.vertices += total * level.vertices; entry.verticesDrawn += level.drawn * level.vertices;
          // A plant's levels say what each family holds and draws.
          if (plant) {
            const families = entry.families ??= {};
            const triangles = (level.mesh.geometry.index?.count ?? 0) / 3;
            families[level.look.family] = { instances: total, drawn: level.drawn, triangles: level.drawn * triangles };
          }
          byLevel.set(level.look.level, entry);
        }
        return { id: patch.id, kind: plant ? "plant" : "grass", tiles: patch.tiles.size, tilesDrawn: [...patch.chosen.values()].filter(level => level >= 0).length, far: patch.far, levels: [...byLevel.values()].sort((a, b) => a.level - b.level) };
      }),
      boundary: { ...this.stats, ...this.boundary.describe() },
    };
  }

  /**
   * Counts the blades the vertex stage draws now, on the CPU, from the drawn instances, the boundary field read back and
   * the same tests the shader makes (tests and measurements; the GPU says nothing about what it rejected).
   */
  countDrawnBlades(camera: THREE.Camera): { candidates: number; drawn: number; byReason: { boundary: number; grazing: number; fill: number } } {
    const eye = camera.getWorldPosition(new THREE.Vector3());
    const field = this.boundary.read();
    let candidates = 0, drawn = 0;
    const byReason = { boundary: 0, grazing: 0, fill: 0 };
    const matrix = new THREE.Matrix4(), root = new THREE.Vector3(), ground = new THREE.Vector3(), view = new THREE.Vector3();
    for (const patch of this.patches) for (const level of patch.levels) {
      const mesh = level.mesh, look = level.look, profile = drawnProfile(look, this.override);
      if (isPlant(look)) continue;
      const geometry = mesh.geometry;
      const pivots = geometry.getAttribute(attributeName("vertex", "pivot")), ranks = geometry.getAttribute(attributeName("vertex", "random"));
      const element = geometry.getAttribute(attributeName("vertex", "element"));
      const normals = geometry.getAttribute(attributeName("instance", "groundNormal")), thresholds = geometry.getAttribute(attributeName("instance", "threshold"));
      if (!pivots || !ranks || !element) continue;
      // One root and rank per blade of the clump.
      const blades = new Map<number, { pivot: THREE.Vector3; rank: number }>();
      for (let vertex = 0; vertex < element.count; vertex++) {
        const id = element.getX(vertex);
        if (!blades.has(id)) blades.set(id, { pivot: new THREE.Vector3(pivots.getX(vertex), pivots.getY(vertex), pivots.getZ(vertex)), rank: ranks.getX(vertex) });
      }
      const uniforms = level.level;
      const levelRange = uniforms.msFoliageLevel.value as THREE.Vector4;
      for (let instance = 0; instance < level.drawn; instance++) {
        matrix.fromArray(mesh.instanceMatrix.array, instance * 16);
        ground.set(normals?.getX(instance) ?? 0, normals?.getY(instance) ?? 0, normals?.getZ(instance) ?? 1).normalize();
        const threshold = thresholds?.getX(instance) ?? 0;
        for (const blade of blades.values()) {
          candidates++;
          root.copy(blade.pivot).applyMatrix4(matrix);
          const distance = root.distanceTo(eye);
          const toward = Math.min(1, Math.max(0, (distance - levelRange.x) / Math.max(levelRange.z - levelRange.x, 1e-3)));
          if (!(distance < look.far && threshold < levelRange.y + (levelRange.w - levelRange.y) * toward)) continue;
          if (profile === "painted" && look.rejection.boundary > 0) {
            const boundary = field ? field.at(root.x, root.y) : 1e6;
            const near = 1 - smooth(0, look.rejection.boundary, boundary);
            const facing = Math.abs(view.copy(eye).sub(root).normalize().dot(ground));
            const sine = Math.sin(look.rejection.grazing * RADIANS);
            const grazing = (1 - smooth(0.5 * sine, sine, facing)) * look.rejection.grazingKeep;
            if (!(blade.rank < Math.max(look.rejection.fill, near, grazing))) continue;
            if (blade.rank < near) byReason.boundary++; else if (blade.rank < grazing) byReason.grazing++; else byReason.fill++;
          }
          drawn++;
        }
      }
    }
    return { candidates, drawn, byReason };
  }

  /** Finds the foliage meshes of a newly displayed scene and their tiles. */
  private collect(models: THREE.Object3D): void {
    this.group = models;
    const byPatch = new Map<string, Patch>();
    for (const object of models.children) {
      const mesh = object as THREE.InstancedMesh;
      const look = mesh.userData.foliage as FoliageLook | undefined;
      if (!look || !mesh.isInstancedMesh) continue;
      let patch = byPatch.get(look.patch);
      if (!patch) byPatch.set(look.patch, patch = { id: look.patch, levels: [], tiles: new Map(), far: look.far, chosen: new Map(), signature: "" });
      patch.far = Math.max(patch.far, look.far);
      const geometry = mesh.geometry;
      const tile = geometry.getAttribute(attributeName("instance", "tile"));
      const count = mesh.count;
      // The delivered instance data, which drawn buffers are packed from.
      const source: LevelMesh["source"] = [{ attribute: mesh.instanceMatrix, values: (mesh.instanceMatrix.array as Float32Array).slice(0, count * 16), size: 16 }];
      if (mesh.instanceColor) source.push({ attribute: mesh.instanceColor, values: (mesh.instanceColor.array as Float32Array).slice(0, count * 3), size: 3 });
      for (const attribute of Object.values(geometry.attributes)) {
        if (!(attribute as THREE.InstancedBufferAttribute).isInstancedBufferAttribute) continue;
        source.push({ attribute: attribute as THREE.BufferAttribute, values: (attribute.array as Float32Array).slice(0, count * attribute.itemSize), size: attribute.itemSize });
      }
      // Tiles: consecutive runs of one tile index (one run for a batch without tiles), with their bounds.
      const tiles = new Map<number, TileRange>();
      // A clump stands on its origin: twice its bounding radius around the origin holds it however it is turned.
      const radius = 2 * (geometry.boundingSphere?.radius ?? 0.5);
      const matrix = new THREE.Matrix4(), origin = new THREE.Vector3();
      // A plant's part is bounded by its mesh where each instance puts it (a dab grows by its size; sway is small).
      if (isPlant(look) && !geometry.boundingBox) geometry.computeBoundingBox();
      const part = new THREE.Box3();
      for (let instance = 0; instance < count; instance++) {
        const index = tile ? Math.round(tile.getX(instance)) : 0;
        const range = tiles.get(index);
        if (range) range.count++;
        else tiles.set(index, { start: instance, count: 1 });
        matrix.fromArray(mesh.instanceMatrix.array, instance * 16);
        const box = patch.tiles.get(index) ?? new THREE.Box3();
        if (isPlant(look)) box.union(part.copy(geometry.boundingBox!).applyMatrix4(matrix).expandByScalar(0.5 * matrix.getMaxScaleOnAxis()));
        else {
          origin.setFromMatrixPosition(matrix);
          const scale = matrix.getMaxScaleOnAxis();
          box.expandByPoint(origin.clone().addScalar(-radius * scale)).expandByPoint(origin.clone().addScalar(radius * scale));
        }
        patch.tiles.set(index, box);
      }
      // The plant sways about its base: where its bark stands.
      if (look.family === "bark" && count) {
        matrix.fromArray(mesh.instanceMatrix.array, 0);
        patch.origin = new THREE.Vector3().setFromMatrixPosition(matrix);
      }
      const element = geometry.getAttribute(attributeName("vertex", "element"));
      let blades = 1;
      if (element) { const ids = new Set<number>(); for (let vertex = 0; vertex < element.count; vertex++) ids.add(element.getX(vertex)); blades = ids.size; }
      const level = (mesh.userData.foliageLevel as Record<string, THREE.IUniform> | undefined) ?? lookUniforms(look, { distance: look.far, keep: 0 });
      patch.levels.push({ mesh, look, source, tiles, drawn: count, level, vertices: geometry.getAttribute("position").count, blades });
    }
    this.patches = [...byPatch.values()];
    for (const patch of this.patches) {
      // A plant painted as a mass: the mass is drawn with the batch that carries it (once per placement, built once
      // per mesh), in place of the patch's leaves and hull.
      if (!patch.levels.some(level => level.look.mass)) continue;
      for (const level of patch.levels) {
        if (level.look.family === "leaves" || level.look.family === "hull") level.replaced = true;
        const mass = level.look.mass;
        if (!mass) continue;
        const mesh = level.mesh;
        let parts = mesh.userData.paintedMass as THREE.Mesh[] | undefined;
        if (!parts) {
          const placement = new THREE.Matrix4();
          parts = [];
          for (let instance = 0; instance < level.drawn; instance++) {
            placement.fromArray(level.source[0].values, instance * 16);
            // An illustrated look (`FoliageMass.look`, or the page's `?treelook`) draws distinct leaves on the mass's
            // twigs in the painted mass's stead (anime-tree.ts); like its parts, theirs are painted output.
            const look = animeTreeLook(mass);
            if (!look) { parts.push(...paintedMass(mass, placement)); continue; }
            const drawn = animeTree(mass, placement, look, programSun, programTime);
            for (const part of drawn) {
              followPaintedFinishing(part.material as THREE.Material);
              // A look that draws from an atlas of its own draws it now, before the sun's shadow pass needs it.
              (part.userData.animeTree?.bake as ((renderer: THREE.WebGLRenderer) => void) | undefined)?.(this.renderer);
            }
            parts.push(...drawn);
          }
          // Foliage to the passes that look for it (ambient occlusion leaves it out), drawn as leaves.
          for (const part of parts) { part.userData.foliage = { ...level.look, family: "leaves" }; mesh.add(part); }
          mesh.userData.paintedMass = parts;
          // Gone with the batch's mesh (the shared textures stay).
          const made = parts;
          mesh.geometry.addEventListener("dispose", () => { for (const part of made) { part.geometry.dispose(); (part.material as THREE.Material).dispose(); } });
        }
        level.painted = parts;
      }
    }
    for (const patch of this.patches) {
      // Each level thins toward the next level's share, and the last toward nothing at far. The values change in place:
      // compiled programs hold these uniform objects.
      // A plant's families thin toward the next level of the same family; one the next level does not draw (leaves at
      // an impostor) is not thinned toward it, the level switches at once.
      const starts = [...new Map(patch.levels.map(level => [level.look.level, { distance: level.look.distance, keep: level.look.keep }])).entries()].sort((a, b) => a[0] - b[0]);
      // A plant's mass (its middle and size, from its leaves) reaches every part, so its hull knows it backs dabs.
      const mass = patch.levels.map(level => level.level.msFoliagePlantMass?.value as THREE.Vector4 | undefined).find(v => v && v.w > 0);
      for (const level of patch.levels) {
        if (mass && level.level.msFoliagePlantMass) (level.level.msFoliagePlantMass.value as THREE.Vector4).copy(mass);
        const at = starts.findIndex(([index]) => index === level.look.level);
        let next = starts[at + 1]?.[1] ?? { distance: patch.far, keep: 0 };
        if (isPlant(level.look) && starts[at + 1]) {
          const following = patch.levels.find(other => other.look.level === starts[at + 1][0] && other.look.family === level.look.family);
          next = { distance: next.distance, keep: following ? following.look.keep : level.look.keep };
        }
        (level.level.msFoliageLevel.value as THREE.Vector4).set(level.look.distance, level.look.keep, next.distance, next.keep);
        level.level.msFoliageFar.value = patch.far;
        if (patch.origin && level.level.msFoliageOrigin) (level.level.msFoliageOrigin.value as THREE.Vector3).copy(patch.origin);
      }
    }
    this.invalidate();
  }

  /** Packs the instances of the tiles that draw `level` to the front of its buffers. */
  private pack(level: LevelMesh, chosen: Map<number, number>): void {
    let drawn = 0;
    for (const [tile, range] of level.tiles) {
      // A shadow proxy draws at every level of its plant.
      if (level.look.family === "shadow" ? (chosen.get(tile) ?? -1) < 0 : chosen.get(tile) !== level.look.level) continue;
      for (const { attribute, values, size } of level.source) (attribute.array as Float32Array).set(values.subarray(range.start * size, (range.start + range.count) * size), drawn * size);
      drawn += range.count;
    }
    for (const { attribute, size } of level.source) {
      attribute.clearUpdateRanges();
      attribute.addUpdateRange(0, Math.max(1, drawn) * size);
      attribute.needsUpdate = true;
    }
    level.drawn = drawn;
    level.mesh.count = drawn;
    // A level no tile draws issues no draw call.
    level.mesh.visible = drawn > 0 && (level.mesh.userData.source?.visible ?? true) && !level.replaced;
    // Its painted mass shows with it (passes that gather foliage by its own flag, not its parent's, see it too).
    for (const part of level.painted ?? []) part.visible = level.mesh.visible;
  }

  dispose(): void {
    this.shadow.dispose();
    this.boundary.dispose();
  }
}

/** Smoothstep, as GLSL has it. */
function smooth(edge0: number, edge1: number, x: number): number {
  const t = Math.min(1, Math.max(0, (x - edge0) / (edge1 - edge0)));
  return t * t * (3 - 2 * t);
}

const QUAD_VERTEX = /* glsl */`void main() { gl_Position = vec4(position.xy, 0.0, 1.0); }`;

/**
 * The boundary-distance field of the painted profile: for each texel of the ground's plan (the ground fields' rectangle
 * and resolution), meters to the nearest boundary of the ground's shade. The band is the foliage recipe's own, evaluated
 * at the ground point under each texel as a blade rooted there would evaluate it (the sun's cast shadow folded in), so
 * the field marks exactly where blades read; its 0.5 contour is the boundary. A jump flood turns the boundary texels
 * into distances. Made again only when the sun, its shadow map, the ground or the recipe's values change.
 */
export class FoliageBoundary {
  readonly uniforms = {
    msFoliageBoundary: { value: null as THREE.Texture | null },
    msFoliageBoundaryRect: { value: new THREE.Vector4(0, 0, 1, 1) },
    msFoliageBoundaryReady: { value: 0 },
  };
  private targets: THREE.WebGLRenderTarget[] = [];
  private readonly quad = new THREE.Mesh(new THREE.PlaneGeometry(2, 2));
  private readonly camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private bandMaterials = new Map<string, THREE.ShaderMaterial>();
  private readonly seed: THREE.ShaderMaterial;
  private readonly jump: THREE.ShaderMaterial;
  private readonly measure: THREE.ShaderMaterial;
  private size = { width: 0, height: 0 };
  private metersPerTexel = 0;
  lastGpuMs: number | null = null;
  private boundaryTexels = 0;

  constructor(private readonly renderer: THREE.WebGLRenderer, private readonly shadow: FoliageShadow) {
    this.uniforms.msFoliageBoundary.value = this.placeholder();
    this.quad.frustumCulled = false;
    this.seed = new THREE.ShaderMaterial({
      uniforms: { band: { value: null } }, vertexShader: QUAD_VERTEX,
      fragmentShader: /* glsl */`
uniform highp sampler2D band;
void main() {
  ivec2 at = ivec2(gl_FragCoord.xy), size = textureSize(band, 0);
  vec4 here = texelFetch(band, at, 0);
  bool edge = false;
  if (here.a > 0.5) {
    for (int i = 0; i < 4; i++) {
      ivec2 q = at + (i == 0 ? ivec2(1, 0) : i == 1 ? ivec2(-1, 0) : i == 2 ? ivec2(0, 1) : ivec2(0, -1));
      if (any(lessThan(q, ivec2(0))) || any(greaterThanEqual(q, size))) continue;
      vec4 there = texelFetch(band, q, 0);
      if (there.a > 0.5 && (here.r < 0.5) != (there.r < 0.5)) edge = true;
    }
  }
  gl_FragColor = edge ? vec4(gl_FragCoord.xy, 0.0, 1.0) : vec4(-1.0);
}`,
    });
    this.jump = new THREE.ShaderMaterial({
      uniforms: { seeds: { value: null }, step: { value: 1 } }, vertexShader: QUAD_VERTEX,
      fragmentShader: /* glsl */`
uniform highp sampler2D seeds;
uniform int step;
void main() {
  ivec2 at = ivec2(gl_FragCoord.xy), size = textureSize(seeds, 0);
  vec4 best = texelFetch(seeds, at, 0);
  float nearest = best.a > 0.5 ? distance(best.xy, gl_FragCoord.xy) : 1e20;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) {
    ivec2 q = at + ivec2(x, y) * step;
    if (any(lessThan(q, ivec2(0))) || any(greaterThanEqual(q, size))) continue;
    vec4 seed = texelFetch(seeds, q, 0);
    if (seed.a < 0.5) continue;
    float d = distance(seed.xy, gl_FragCoord.xy);
    if (d < nearest) { nearest = d; best = seed; }
  }
  gl_FragColor = best;
}`,
    });
    this.measure = new THREE.ShaderMaterial({
      uniforms: { seeds: { value: null }, meters: { value: 0 } }, vertexShader: QUAD_VERTEX,
      fragmentShader: /* glsl */`
uniform highp sampler2D seeds;
uniform float meters;
void main() {
  vec4 seed = texelFetch(seeds, ivec2(gl_FragCoord.xy), 0);
  gl_FragColor = vec4(seed.a > 0.5 ? distance(seed.xy, gl_FragCoord.xy) * meters : 1e4, 0.0, 0.0, 1.0);
}`,
    });
  }

  private placeholder(): THREE.DataTexture {
    const texture = new THREE.DataTexture(new Float32Array([1e4, 0, 0, 1]), 1, 1, THREE.RGBAFormat, THREE.FloatType);
    texture.needsUpdate = true;
    return texture;
  }

  /** The band pass of a recipe: the recipe evaluated at each texel's ground point, its band kept in red. */
  private bandMaterial(program: FoliageProgram | undefined): THREE.ShaderMaterial {
    const key = program ? program.shading.shader.id ?? program.shading.shader.source : "none";
    let material = this.bandMaterials.get(key);
    if (material) return material;
    const chunk = program ? programChunk(program.shading.shader, new Set(), new Set()) : undefined;
    material = new THREE.ShaderMaterial({
      uniforms: { ...(program ? { ...receiverUniforms(), ...program.uniforms, ...fieldUniforms(program.shading.shader), msTime: programTime, msSunDirection: programSun } : {}),
        msFoliageTime: programTime, msFoliageSun: programSun, ...this.shadow.uniforms, positions: { value: null }, normals: { value: null } },
      vertexShader: QUAD_VERTEX,
      fragmentShader: /* glsl */`
${FAMILY_UNIFORMS}
${FAMILY_FUNCTIONS}
uniform highp sampler2D positions;
uniform highp sampler2D normals;
${chunk?.declarations ?? ""}
void main() {
  ivec2 at = ivec2(gl_FragCoord.xy);
  vec4 point = texelFetch(positions, at, 0);
  if (point.a < 0.5) { gl_FragColor = vec4(-1.0, 0.0, 0.0, 0.0); return; }
  vec3 root = point.xyz, clumpRoot = point.xyz, ground = normalize(texelFetch(normals, at, 0).xyz);
  float height = 0.0, instanceRandom = 0.0;
  vec3 tint = vec3(1.0);
  msFoliageBand = 1.0;
${chunk ? `  ${chunk.inputs}\n  ms_evaluate();` : "  msFoliageBand = msFoliageCast(root, ground);"}
  gl_FragColor = vec4(msFoliageBand, 0.0, 0.0, 1.0);
}`,
    });
    this.bandMaterials.set(key, material);
    return material;
  }

  private draw(material: THREE.Material, target: THREE.WebGLRenderTarget): void {
    this.quad.material = material;
    this.renderer.setRenderTarget(target);
    this.renderer.render(this.quad, this.camera);
  }

  /** Makes the field from the ground's points and normals for `program`'s band; `boundary` (m) is what blades read. */
  make(ground: { positions: THREE.Texture; normals: THREE.Texture; rect: THREE.Vector4; width: number; height: number }, program: FoliageProgram | undefined, boundary: number): void {
    const { width, height } = ground;
    if (this.size.width !== width || this.size.height !== height) {
      for (const target of this.targets) target.dispose();
      const type = this.renderer.extensions.has("EXT_color_buffer_float") ? THREE.FloatType : THREE.HalfFloatType;
      this.targets = [0, 1, 2].map(() => {
        const target = new THREE.WebGLRenderTarget(width, height, { type, format: THREE.RGBAFormat, depthBuffer: false });
        target.texture.minFilter = target.texture.magFilter = THREE.NearestFilter;
        target.texture.generateMipmaps = false;
        return target;
      });
      this.size = { width, height };
    }
    this.metersPerTexel = Math.max((ground.rect.z - ground.rect.x) / width, (ground.rect.w - ground.rect.y) / height);
    const previous = this.renderer.getRenderTarget();
    const autoClear = this.renderer.autoClear;
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    const timer = gl.getExtension("EXT_disjoint_timer_query_webgl2");
    const query = timer ? gl.createQuery() : null;
    if (timer && query) gl.beginQuery(timer.TIME_ELAPSED_EXT, query);
    try {
      this.renderer.autoClear = false;
      const band = this.bandMaterial(program);
      band.uniforms.positions.value = ground.positions;
      band.uniforms.normals.value = ground.normals;
      const [bandTarget, a, b] = this.targets;
      this.draw(band, bandTarget);
      this.seed.uniforms.band.value = bandTarget.texture;
      this.draw(this.seed, a);
      // Jump flooding: steps from half the field down to one texel.
      let from = a, to = b;
      for (let step = 2 ** Math.ceil(Math.log2(Math.max(width, height))) / 2; step >= 1; step /= 2) {
        this.jump.uniforms.seeds.value = from.texture;
        this.jump.uniforms.step.value = step;
        this.draw(this.jump, to);
        [from, to] = [to, from];
      }
      this.measure.uniforms.seeds.value = from.texture;
      this.measure.uniforms.meters.value = this.metersPerTexel;
      // The band target is free now; the distances go there.
      this.draw(this.measure, bandTarget);
    } finally {
      if (timer && query) gl.endQuery(timer.TIME_ELAPSED_EXT);
      this.renderer.autoClear = autoClear;
      this.renderer.setRenderTarget(previous);
    }
    this.uniforms.msFoliageBoundary.value = this.targets[0].texture;
    this.uniforms.msFoliageBoundaryRect.value.copy(ground.rect);
    this.uniforms.msFoliageBoundaryReady.value = boundary > 0 ? 1 : 0;
    this.pendingQuery = query && timer ? { query, timer } : null;
    this.lastGpuMs = null;
  }

  private pendingQuery: { query: WebGLQuery; timer: { TIME_ELAPSED_EXT: number; GPU_DISJOINT_EXT: number } } | null = null;

  /** No ground: no blade is near a boundary. */
  clear(): void {
    this.uniforms.msFoliageBoundaryReady.value = 0;
  }

  /** The GPU time of the last make, once its timer query has answered (null before or without timer queries). */
  private gpuTime(): number | null {
    const pending = this.pendingQuery;
    if (!pending) return this.lastGpuMs;
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    if (!gl.getQueryParameter(pending.query, gl.QUERY_RESULT_AVAILABLE)) return null;
    if (!gl.getParameter(pending.timer.GPU_DISJOINT_EXT)) this.lastGpuMs = (gl.getQueryParameter(pending.query, gl.QUERY_RESULT) as number) / 1e6;
    gl.deleteQuery(pending.query);
    this.pendingQuery = null;
    return this.lastGpuMs;
  }

  describe(): { size: [number, number]; metersPerTexel: number; ready: boolean; gpuMs: number | null } {
    return { size: [this.size.width, this.size.height], metersPerTexel: +this.metersPerTexel.toFixed(4), ready: this.uniforms.msFoliageBoundaryReady.value === 1, gpuMs: this.gpuTime() };
  }

  /** The distances read back, with a lookup by world XY (tests). */
  read(): { at(x: number, y: number): number; boundaryTexels: number } | null {
    if (!this.targets.length || this.uniforms.msFoliageBoundaryReady.value !== 1) return null;
    const { width, height } = this.size, target = this.targets[0];
    const values = new Float32Array(width * height * 4);
    if (target.texture.type === THREE.FloatType) this.renderer.readRenderTargetPixels(target, 0, 0, width, height, values);
    else {
      const half = new Uint16Array(values.length);
      this.renderer.readRenderTargetPixels(target, 0, 0, width, height, half);
      half.forEach((value, index) => { values[index] = THREE.DataUtils.fromHalfFloat(value); });
    }
    const rect = this.uniforms.msFoliageBoundaryRect.value;
    let boundaryTexels = 0;
    for (let i = 0; i < width * height; i++) if (values[i * 4] < 0.5 * this.metersPerTexel) boundaryTexels++;
    this.boundaryTexels = boundaryTexels;
    return {
      boundaryTexels,
      at: (x, y) => {
        const u = (x - rect.x) / (rect.z - rect.x), v = (y - rect.y) / (rect.w - rect.y);
        if (u < 0 || u > 1 || v < 0 || v > 1) return 1e6;
        const column = Math.min(width - 1, Math.floor(u * width)), row = Math.min(height - 1, Math.floor(v * height));
        return values[(row * width + column) * 4];
      },
    };
  }

  dispose(): void {
    for (const target of this.targets) target.dispose();
    for (const material of this.bandMaterials.values()) material.dispose();
    this.seed.dispose(); this.jump.dispose(); this.measure.dispose();
    this.quad.geometry.dispose();
  }
}
