import * as THREE from "three";
import type { FoliageCanopy, FoliageLook } from "../model/packet.ts";
import { attributeName } from "./batch.ts";
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

/** The uniforms of a plant look's canopy parameters. */
export function canopyUniforms(canopy: FoliageCanopy): Record<string, THREE.IUniform> {
  return {
    // x: facing, y: dab size, z: cutout, w: share of highlight leaves.
    msFoliageCanopy: { value: new THREE.Vector4(canopy.facing, canopy.dab, canopy.cutout, canopy.highlights) },
    msFoliageCanopyHighlight: { value: new THREE.Color().setRGB(...canopy.highlight, THREE.LinearSRGBColorSpace) },
    // x: translucency, y: glint, z: wrap.
    msFoliageLit: { value: new THREE.Vector3(canopy.translucency, canopy.glint, canopy.wrap) },
    // The plant's base, which the patch sets from its bark's placement.
    msFoliageOrigin: { value: new THREE.Vector3() },
  };
}

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
varying vec3 vFoliageColor;
varying float vFoliageBand;
varying float vFoliageHeight;
varying float vFoliageVariation;
varying float vFoliageRandom;
varying float vFoliageHighlight;
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
  float size = length(turn[1]);
  vec3 centre = (place * vec4(0.0, 0.5, 0.0, 1.0)).xyz;
  vec3 view = normalize(cameraPosition - centre);
  vec3 side = cross(vec3(0.0, 0.0, 1.0), view);
  side = dot(side, side) > 1e-8 ? normalize(side) : vec3(1.0, 0.0, 0.0);
  vec3 up = cross(view, side);
  float roll = 6.2831853 * fract(rank * 3.17 + 0.21);
  vec2 local = mat2(cos(roll), sin(roll), -sin(roll), cos(roll)) * vec2(position.x, position.y - 0.5);
  vec3 dab = centre + (side * local.x + up * local.y) * size * msFoliageCanopy.y;
  point = mix(point, dab, msFoliageCanopy.x);
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
  vec3 delta = point - original;
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
  vFoliageHighlight = 0.0;
${leaves ? (chunk ? `  {
    vec3 root = centre, clumpRoot = anchor, ground = shade;
    float height = 0.5, instanceRandom = rank;
    vec3 tint = vec3(1.0);
#ifdef USE_INSTANCING_COLOR
    tint = instanceColor;
#endif
    ${chunk.inputs}
    msFoliageBand = 1.0;
    ms_evaluate();
    vFoliageColor = ${color};
    vFoliageBand = msFoliageBand;
  }` : `  vFoliageColor = ${color};
  vFoliageBand = min(smoothstep(-0.1, 0.1, dot(shade, normalize(msFoliageSun))), msFoliageCast(centre, shade));`) + `
  // Highlight leaves: a stable share of the clusters, where the band is lit.
  vFoliageHighlight = fract(rank * 5.71 + 0.37) < msFoliageCanopy.w ? smoothstep(0.5, 0.95, vFoliageBand) : 0.0;` : ""}
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
${chunk?.declarations ?? ""}
float msFoliageNoise(vec3 p) { return 0.5 * (msFoliageGust(p.xy + p.z * vec2(0.37, 1.13)).x + msFoliageGust(p.yz * 1.31 + 7.1).y); }`;
  const color = leaves ? `
  {
    // The dab's coverage: a brush-edged blob over the card, cut out at the threshold.
    float n = msFoliageGust(vFoliageUv * vec2(3.0, 4.0) + vFoliageRandom * 17.0).x;
    float coverage = 1.0 - smoothstep(0.55, 1.05, length((vFoliageUv - 0.5) * 2.0) + 0.45 * (n - 0.5));
    if (coverage < msFoliageCanopy.z) discard;
    vec3 paint = vFoliageColor * (0.9 + 0.2 * vFoliageVariation);
    diffuseColor.rgb = ${painted ? "mix(paint, msFoliageCanopyHighlight * (0.9 + 0.2 * vFoliageVariation), vFoliageHighlight)" : "paint"};
  }` : `
  {
    vec3 root = vFoliageWorld, clumpRoot = vFoliageAnchor, ground = normalize(vFoliageShade);
    float height = vFoliageHeight, instanceRandom = 0.0;
    vec3 tint = vec3(1.0);
${hull ? `    // The silhouette broken into dabs: where the hull turns away from the view, a world-anchored noise cuts it.
    float n = msFoliageNoise(root * 3.5);
    float rim = 1.0 - abs(dot(ground, normalize(cameraPosition - root)));
    if (rim + 0.5 * (n - 0.5) > 0.78) discard;
` : ""}${chunk ? `    ${chunk.inputs}
    msFoliageBand = 1.0;
    ms_evaluate();
` : ""}    diffuseColor.rgb = ${evaluated}${hull ? " * (0.93 + 0.14 * n)" : ""};
  }`;
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
  const vertex = vertexStage(look, profile, program, attributes);
  const fragment = fragmentStage(look, profile, program);
  const uniforms: Record<string, THREE.IUniform> = {
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
