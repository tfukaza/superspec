import * as THREE from "three";
import { shaderIdentity, type MaterialShading, type ProgramShader } from "./shade.ts";
import { MAP_ANISOTROPY } from "./textures.ts";
import { NORMAL_ENCODE, RELIEF_ADJUST } from "./relief-glsl.ts";
import { ATLAS_MAPPING_FRAGMENT, ATLAS_NORMAL_FRAGMENT, ATLAS_VERTEX, LATTICE_MAPPING_FRAGMENT, MAX_TILES, PLANE_VALUES, STACK_COMPOSE_FRAGMENT, TILE_CELLS, TILE_START,
  atlasFragment, attachmentReads, intrinsicTileFragment, latticeTiles, outputCpuFields, outputIntrinsic, outputReads, planeFragment, programLayout,
  type AtlasAttachment, type AtlasLayout, type AtlasProgramLayout } from "./atlas.ts";
import { BakeGraph, type BandWork, type DrawStep, type GraphInstance, type NodeOp, type NodeSpec, type TargetSpec } from "./bake-graph.ts";

/**
 * GPU preview bake: a MaterialSpec program's shader (`Program::shader`, see shade.ts) drawn over a
 * texel grid into render targets, so an edit shows textures in milliseconds instead of waiting for
 * the CPU baker. It is a PREVIEW. The CPU baker (`authoring/material.rs`) stays the authority for
 * ready revisions, saved projects and export; nothing here is ever accepted, saved or exported, and
 * its output is labeled with its own profile and cache key so it cannot be mistaken for a bake.
 *
 * Conventions follow the program README: texel centers, atlas rows counted from the upper edge,
 * base color in sRGB, linear properties as R=1, G=roughness, B=metalness, tangent-space normals
 * with +Y up derived from height differences along the texel grid. Deviations, all preview-only:
 * emissive is kept as linear half floats (HDR needs no `emissiveStrength` factor), opacity is not
 * stored, filtering follows screen derivatives of the shader (which equal the texel footprint in a
 * full-screen draw), and values are 32-bit floats.
 */
export const GPU_PREVIEW_PROFILE = "gpu-preview";
/**
 * GPU memory the baked maps of one scene may take, mips included. It is the allowance of the scene view: for the maps
 * on screen (scene-bake.ts refuses a mesh beyond it, which then stays on its per-pixel look), for the maps the pass
 * graph keeps after their last user (`GraphLimits.idleBytes`), so a scene that fits on screen is also kept whole while
 * a control is dragged, and for both together (`GraphLimits.totalBytes`): besides the maps on screen the graph keeps
 * what still fits (the textures only other passes read, the scene shown before), the oldest dropped first.
 * Measured on the item washroom (115 receivers, 736 textures, 128 texels per meter, real GPU): the maps on screen take
 * 39.4 MiB at 256 px and 71.2 MiB at 512 px; after a wall drag and release the graph holds 853 textures, 62.5 and
 * 129.4 MiB (117 idle, 23.2 and 59.7 MiB). The old count of 96 idle nodes kept under a seventh of the scene's textures.
 */
export const SCENE_MAP_BYTES = sceneMapMebibytes() * 1024 * 1024;
/** 512 MiB, or what the page asks for with `?bakememory=<MiB>` (64 to 8192): a large scene on a real GPU wants more. */
function sceneMapMebibytes(): number {
  const asked = typeof location === "undefined" ? NaN : Number(new URLSearchParams(location.search).get("bakememory"));
  return Number.isFinite(asked) && asked >= 64 && asked <= 8192 ? asked : 512;
}
/**
 * The least an idle node is charged: a render target is a texture and a framebuffer object to the driver whatever
 * its size (a 16 by 16 map with mips is 1.4 KiB of texels).
 */
export const NODE_FLOOR_BYTES = 16 * 1024;
/** Attachments of an atlas bake that only its normals pass reads. */
const INTERMEDIATE_ATTACHMENTS: ReadonlySet<string> = new Set(["height", "offset"]);
/** Bumped when the bake's output for the same inputs changes; part of every cache key. */
const GPU_PREVIEW_VERSION = 1;

/** Where a bake's texels sit in the world: a rectangle of `width` x `height` texels with a clamped gutter. */
export interface BakeDomain {
  /** Atlas size in texels. */
  width: number;
  height: number;
  /** Texels at every edge that clamp to the nearest inner texel (the CPU atlas's chart gutter). */
  gutter: number;
  /** World position of the inner rectangle's upper-left corner (row 0, column 0 edge). */
  origin: [number, number, number];
  /** World vector across the inner rectangle along atlas rows (image right), and down its columns. */
  u: [number, number, number];
  v: [number, number, number];
  /** Surface UV at `origin` and across `u` and `v`. */
  uvOrigin: [number, number];
  uvU: [number, number];
  uvV: [number, number];
  /** Receiver normal across the rectangle (a planar domain). */
  normal: [number, number, number];
  mapping: "uv" | "object" | "world";
  /** Physical size for `uv` mapping, meters. */
  extent: [number, number];
  /** `ms_object` is the world position minus this point. */
  objectOrigin: [number, number, number];
  boundsMin: [number, number, number];
  boundsMax: [number, number, number];
  gravity: [number, number, number];
  /** The receiver's features as rows of twelve numbers (see `shade_project`); programs that query them need this. */
  features?: number[];
}

/** Why a program cannot be previewed on the GPU, or null when it can. `features` says the domain supplies its receiver's features. */
export function gpuBakeUnsupported(shading: MaterialShading & { requiredInputs?: string[] }, features = false): string | null {
  const shader = shading.shader;
  if (!shader.source || !shader.outputs.includes("base_color")) return "the program has no base color output";
  if (shader.features && !features) return "receiver features need the receiver's edges";
  // A plan bakes runoff (the only CPU-only operation) on the CPU; without one it needs finite source edges.
  if (shading.requiredInputs?.includes("edges") && !shader.cpuFields?.length) return "runoff needs finite source edges";
  // Live renderer operations are not baked here: the viewer keeps running them with the last CPU build's arguments.
  return null;
}

export interface GpuBakeTimings {
  /** Shader compile (only when the shader source is new), milliseconds. */
  compile: number;
  /** Uniform binding. */
  uniforms: number;
  /** Pass 1 (color, properties, emissive, height), all bands. */
  pass1: number;
  /** Mip generation for the color, properties, emissive and normal textures. */
  mips: number;
  /** Pass 2 (normals); 0 when the program has no height and the flat normal is already there. */
  pass2: number;
  total: number;
  /** Bands pass 1 was split into (1 unless the target exceeds the band budget). */
  bands: number;
  /** Live renderer arguments (0 when the program has none). */
  live: number;
}

/** Textures of one GPU preview bake. They stay owned by the baker and are overwritten by the next bake. */
export interface GpuBake {
  profile: typeof GPU_PREVIEW_PROFILE;
  key: string;
  width: number;
  height: number;
  baseColor: THREE.Texture;
  normal: THREE.Texture;
  /** Packed properties: R=1, G=roughness, B=metalness. */
  properties: THREE.Texture;
  emissive: THREE.Texture;
  timings: GpuBakeTimings;
  /**
   * The program's live renderer arguments evaluated on this grid, as half-float textures (four arguments per texel, 
   * channels in order), described as the operations a viewer applies. Present when the program has live operations.
   */
  live?: { key: string; operations: LiveArgumentOperation[]; textures: Record<string, THREE.Texture> };
  /** Texture units used by the baking program and by the viewer material showing the result, and the device limit. */
  units: { bake: number; viewer: number; limit: number };
}

/** A live operation whose every argument reads a baked texture channel, unscaled. */
export interface LiveArgumentOperation { id: string; arguments: Record<string, { texture: string; channel: number; scale: number; offset: number }> }

/**
 * One receiver's atlas bake: the nodes of its pass graph (bake-graph.ts) by name. The evaluate pass's textures are one
 * node per attachment of the program's layout (`base`, `properties`, `height`, and by program `emissive`, `offset`,
 * `live0`...; see `AtlasProgramLayout`), and `normal` is the normals pass's, which reads `height` (and `offset`). Every
 * node is shared with any other bake that wants the same texels. `key` names the whole bake (`GpuBaker.atlasKey`).
 * The maps hold one use of each node a viewer shows until `releaseAtlasMaps`; the nodes only other passes read
 * (`height`, `offset`, a layered slot's planes) are the graph's to keep (`GpuBaker.keepIntermediates`).
 */
export interface AtlasMaps {
  profile: typeof GPU_PREVIEW_PROFILE;
  key: string;
  width: number;
  height: number;
  layout: AtlasProgramLayout;
  nodes: GraphInstance;
  baseColor: THREE.Texture;
  /** Packed properties: R=1, G=roughness, B=metalness. */
  properties: THREE.Texture;
  emissive?: THREE.Texture;
  normal: THREE.Texture;
  /** Live renderer argument textures by the identity `layout.liveOperations` names them with. */
  live: Record<string, THREE.Texture>;
}

export interface AtlasBakeTimings {
  /** Shader compile (only when the program is new), milliseconds. */
  compile: number;
  evaluate: number;
  normals: number;
  mips: number;
  total: number;
  bands: number;
  /** Nodes drawn, by name, and the number answered from textures that already held them. */
  drawn: string[];
  reused: number;
  units: { bake: number; limit: number };
}

/** What an atlas bake adds to `BakeOptions`: how it treats the maps it replaces (see `RealizeOptions`). */
export interface AtlasBakeOptions extends BakeOptions {
  inPlace?: boolean;
  allocate?: (bytes: number) => void;
  beforeOverwrite?: () => void;
}

/** One receiver's atlas as a bake is given it. */
export interface AtlasBakeInput {
  layout: AtlasLayout;
  /** The atlas's charts as drawn (`bakeGeometry`). */
  geometry: THREE.BufferGeometry;
  /** Physical size of the UV square for `uv` mapping, meters. */
  extent: [number, number];
  /** Names the receiver inputs that are textures (its features), for the bake key. */
  receiver: string;
  /** The program's layout, when the caller already has it (`GpuBaker.atlasProgram`). */
  program?: AtlasProgramLayout;
}

/**
 * One weathering layer of a layered slot as a stack bake is given it (the engine's `metadata.layers`, see
 * `Session::shade_project`): the layer's plan, in which every `below.<channel>` is a CPU field this bake binds to the
 * plane of that channel beneath, its uniforms over the receiver (its parameters, seeds and images, the receiver's own
 * with the features as the layer queries them), what each `base.<channel>` input reads, and what names its features.
 */
export interface StackLayer {
  /** `stack.1`, `stack.2`, ...: the lowest first. */
  id: string;
  shading: MaterialShading;
  uniforms: Uniforms;
  /** External input (`base.<channel>`) to the stack material whose plane it reads (`stack.base`, `stack.<k>`). */
  reads: Record<string, string>;
  receiver: string;
}
/** A layered slot: its layers and the stack material each channel of the result comes from (`metadata.channels`). */
export interface StackInput { layers: StackLayer[]; channels: Record<string, string> }

/** Where a shared intrinsic bake lies in a receiver's surface coordinates (meters), for `GpuBaker.mappingOp`. */
export interface IntrinsicDomain {
  /** The surface coordinate of the intrinsic texture's corner at UV (0, 0), and the size it covers. */
  origin: [number, number];
  size: [number, number];
  /** Intrinsic texels per meter, for choosing the mip a receiver's texel reads. */
  density: number;
}

const atlasPassKey = (shader: ProgramShader, layout: AtlasProgramLayout): string => `atlas:${layout.key}:${shaderIdentity(shader)}`;

const VERTEX = `void main() { gl_Position = vec4(position.xy, 0.0, 1.0); }`;

const PASS1_HEAD = `
uniform vec2 msAtlas;
uniform float msGutter;
uniform vec2 msInner;
uniform vec3 msOrigin;
uniform vec3 msU;
uniform vec3 msV;
uniform vec2 msUvOrigin;
uniform vec2 msUvU;
uniform vec2 msUvV;
uniform vec3 msReceiverNormal;
uniform vec3 msObjectOrigin;
uniform vec2 msExtent;
uniform int msMapping;
uniform float msFootprint;
uniform float msUvFootprint;
// Which texture this draw writes (see \`pass1Source\`): 0 base color, 1 properties, 2 emissive, 3 height, 4 normal
// offset, then the live argument textures.
uniform int msOutput;
layout(location = 0) out vec4 msOut;
`;

const PASS1_MAIN = `
void main() {
  // Rows count from the upper edge; framebuffer row 0 is the lowest, so the atlas row is flipped.
  vec2 texel = vec2(floor(gl_FragCoord.x), msAtlas.y - 1.0 - floor(gl_FragCoord.y));
  // A gutter texel takes the nearest inner texel's center, so it repeats that texel's values.
  vec2 inside = texel + 0.5 - msGutter;
  inside = mix(inside, fract(inside), vec2(lessThan(inside, vec2(0.0))));
  inside = mix(inside, msInner - fract(msInner - inside), vec2(greaterThan(inside, msInner)));
  vec2 st = clamp(inside / msInner, 0.0, 1.0);
  ms_world = msOrigin + msU * st.x + msV * st.y;
  ms_object = ms_world - msObjectOrigin;
  ms_normal = msReceiverNormal;
  ms_uv = msUvOrigin + msUvU * st.x + msUvV * st.y;
  {
    vec3 at = msMapping == 2 ? ms_world : ms_object;
    vec3 facing = abs(ms_normal);
    ms_surface = msMapping == 0 ? ms_uv * msExtent
      : (facing.z >= max(facing.x, facing.y) ? at.xy : (facing.y >= facing.x ? at.xz : at.yz));
  }
  ms_footprint = msFootprint;
  ms_uv_footprint = msUvFootprint;
  ms_texel = ivec2(int(texel.x), int(texel.y));
  ms_evaluate();
  // The base color target is an sRGB attachment: the hardware encodes these linear values on write.
  if (msOutput == 0) msOut = vec4(clamp(ms_base_color, 0.0, 1.0), 1.0);
  else if (msOutput == 1) msOut = vec4(1.0, clamp(ms_roughness, 0.0, 1.0), clamp(ms_metalness, 0.0, 1.0), 1.0);
  else if (msOutput == 2) msOut = vec4(max(ms_emissive, vec3(0.0)), 1.0);
  // Height in meters, with the world-space receiver_normal output beside it; the normal pass turns the relief by it.
  else if (msOutput == 3) msOut = vec4(ms_height, ms_receiver_normal);
  else if (msOutput == 4) msOut = vec4(ms_normal_offset, 0.0);
`;

/** Textures pass 1 writes besides live arguments: base color, properties, emissive, height, normal offset. */
const PASS1_TEXTURES = 5;
/**
 * Texture units a viewer material uses besides live arguments when the viewer does not say (`BakeOptions.viewerUnits`):
 * base color, normal, roughness, metalness (the packed properties texture is bound once per sampler), emissive, the
 * environment and the sun's shadow map.
 */
const VIEWER_UNITS = 7;
/** Compiled bake programs kept, least recently used dropped first. */
const KEPT_PASSES = 8;
/**
 * Compiled programs of weathering stacks kept (each material of a stack has its plane program, a slot has at most
 * eight layers, and a scene several layered slots), least recently used dropped first.
 */
const KEPT_PLANE_PASSES = 32;
/** Channels of a stack's result (`STACK_CHANNELS` in the engine). */
const STACK_CHANNELS = ["base_color", "roughness", "metalness", "height", "opacity", "emissive", "normal_offset"];
/** Sources whose compile failed, remembered so an edit that keeps the source does not compile it again. */
const KEPT_FAILURES = 16;

/** Cells per side of the grid that reports failed normal texels. */
const FAIL_GRID = 16;

const PASS2_FRAGMENT = `
uniform sampler2D msHeightMap;
uniform sampler2D msOffsetMap;
uniform vec2 msStep;
// The receiver's tangent frame (z on the receiver normal, x along image right, y up the image), as the CPU atlas gives it.
uniform vec3 msFrameX;
uniform vec3 msFrameY;
uniform vec3 msFrameZ;
uniform bool msOrient;
uniform bool msBend;
layout(location = 0) out vec4 msNormalOut;
// A tangent-space normal whose world direction faces away from the receiver cannot be encoded; alpha 0 marks it.
${RELIEF_ADJUST}
vec3 relief(ivec2 p, ivec2 size) {
  vec4 center = texelFetch(msHeightMap, p, 0);
  float h = center.r;
  // Image right is +x; the next atlas row is the framebuffer row below. Clamped at the edge.
  float right = texelFetch(msHeightMap, ivec2(min(p.x + 1, size.x - 1), p.y), 0).r;
  float down = texelFetch(msHeightMap, ivec2(p.x, max(p.y - 1, 0)), 0).r;
  vec3 relief = normalize(vec3(-(right - h) / msStep.x, (down - h) / msStep.y, 1.0));
  vec3 offset = msBend ? texelFetch(msOffsetMap, p, 0).rgb : vec3(0.0);
  return msAdjustRelief(relief, center.gba, offset, msFrameX, msFrameY, msFrameZ, msOrient, msBend);
}
${NORMAL_ENCODE}
void main() {
  ivec2 size = textureSize(msHeightMap, 0);
  ivec2 p = ivec2(gl_FragCoord.xy);
  vec3 n = relief(p, size);
  vec3 before = p.x > 0 ? relief(p - ivec2(1, 0), size) : n;
  msNormalOut = vec4(msEncodeNormal(n, before, ivec2(p.x, size.y - 1 - p.y)), msFailed ? 0.0 : 1.0);
}
`;

// Whether any texel of the normal pass failed: a small grid whose cells each scan a block of texels.
const PASS3_FRAGMENT = `
uniform sampler2D msNormalMap;
layout(location = 0) out vec4 msFailedOut;
void main() {
  ivec2 size = textureSize(msNormalMap, 0);
  ivec2 cell = ivec2(gl_FragCoord.xy);
  ivec2 low = cell * size / ${FAIL_GRID}, high = (cell + 1) * size / ${FAIL_GRID};
  float failed = 0.0;
  for (int y = low.y; y < high.y; y++) for (int x = low.x; x < high.x; x++) {
    if (texelFetch(msNormalMap, ivec2(x, y), 0).a < 0.5) failed = 1.0;
  }
  msFailedOut = vec4(failed, 0.0, 0.0, 1.0);
}
`;

/** Taps to each side a blur node may take. */
const BLUR_MAX_RADIUS = 64;

// One direction of a separable Gaussian blur (see `GpuBaker.blurNodes`): texels are read, not sampled, and the edge repeats.
const BLUR_FRAGMENT = `
uniform sampler2D msSource;
uniform vec2 msDirection;
uniform int msRadius;
uniform float msSigma;
layout(location = 0) out vec4 msOut;
void main() {
  ivec2 size = textureSize(msSource, 0), p = ivec2(gl_FragCoord.xy), direction = ivec2(msDirection);
  vec4 sum = vec4(0.0);
  float weights = 0.0;
  for (int tap = -msRadius; tap <= msRadius; tap++) {
    float weight = exp(-0.5 * float(tap * tap) / (msSigma * msSigma));
    sum += weight * texelFetch(msSource, clamp(p + tap * direction, ivec2(0), size - 1), 0);
    weights += weight;
  }
  msOut = sum / weights;
}
`;

type Uniforms = Record<string, THREE.IUniform>;

/** One shader's compiled bake material, shared by every bake of that source. */
interface Pass { material: THREE.ShaderMaterial; scene: THREE.Scene; mesh: THREE.Mesh }
/**
 * A swatch bake's nodes of one size: pass 1's five textures (`PASS1_TEXTURES`), the normals and the live argument
 * textures, in the baker's pass graph. The next bake of the size takes their place (drawing over them where it can).
 */
interface Swatch { instance?: GraphInstance }

/**
 * Texels drawn per band. A draw longer than the driver's watchdog (typically a second or two) loses the
 * context, so large targets are drawn as horizontal bands with the browser given a turn between them.
 */
export const BAND_TEXELS = 1 << 17;
/** At most this many swatch sizes keep their nodes: the low-resolution and full-size bake of a swatch. */
const KEPT_TARGETS = 3;

/** A program's live renderer arguments in order: four to a texel, textures in sequence. */
function liveArguments(shading: MaterialShading): { operationIndex: number; name: string; expression: string }[] {
  return shading.shader.live.flatMap((operation, operationIndex) =>
    Object.entries(operation.arguments).map(([name, expression]) => ({ operationIndex, name, expression })));
}

/** One CPU-baked field: RGBA float texels of a bake grid, rows from its upper edge. `id` names its content. */
export interface FieldTexels { id: string; width: number; height: number; rgba: Float32Array }

/** The field at a smaller grid: each texel the mean of the full-size texels it covers (box filter). */
export function downsampleField(field: FieldTexels, width: number, height: number): FieldTexels {
  if (field.width === width && field.height === height) return field;
  const rgba = new Float32Array(width * height * 4);
  for (let y = 0; y < height; y++) {
    const y0 = Math.floor(y * field.height / height), y1 = Math.max(y0 + 1, Math.ceil((y + 1) * field.height / height));
    for (let x = 0; x < width; x++) {
      const x0 = Math.floor(x * field.width / width), x1 = Math.max(x0 + 1, Math.ceil((x + 1) * field.width / width));
      const sum = [0, 0, 0, 0];
      for (let row = y0; row < y1; row++) for (let column = x0; column < x1; column++) {
        for (let channel = 0; channel < 4; channel++) sum[channel] += field.rgba[(row * field.width + column) * 4 + channel];
      }
      const count = (y1 - y0) * (x1 - x0);
      for (let channel = 0; channel < 4; channel++) rgba[(y * width + x) * 4 + channel] = sum[channel] / count;
    }
  }
  return { id: `${field.id}@${width}x${height}`, width, height, rgba };
}

export interface BakeOptions {
  /** CPU-baked fields by the shader's sampler uniform, for a hybrid program; each is brought to the bake's grid size. */
  fields?: Record<string, FieldTexels>;
  /** Reads the GPU after each pass so timings are real (a stall; for tests and diagnostics). */
  measure?: boolean;
  /** Checked between bands; a false answer abandons the bake (a newer edit cancels refinement). */
  current?: () => boolean;
  bandTexels?: number;
  /**
   * Texture units the viewer material that will show the result uses besides live argument textures, counted from that
   * material (its maps, the environment, shadow maps). The bake refuses, before drawing, a result the viewer cannot bind.
   */
  viewerUnits?: number;
}

const yieldToBrowser = (): Promise<void> => new Promise(done => setTimeout(done, 0));

/**
 * Runs `callback` in a task of its own as soon as the browser has had a turn (its input, its frames). Not a timer:
 * timers set from timers wait at least 4 ms each once they are a few deep, which a loop of awaited timers always is.
 */
const nextTurn = (() => {
  if (typeof MessageChannel === "undefined") return (callback: () => void): void => { setTimeout(callback, 0); };
  const channel = new MessageChannel(), waiting: (() => void)[] = [];
  channel.port1.onmessage = () => { waiting.shift()?.(); };
  return (callback: () => void): void => { waiting.push(callback); channel.port2.postMessage(0); };
})();
/** A band's draws are waited for at most this long before the next band is issued anyway. */
const BAND_WAIT_MS = 250;
/** The GPU is asked every turn of the browser for this long after a band, before it is asked by timer. */
const BAND_SPIN_MS = 3;
/** The time a band should take the GPU: bands are sized to it from the speed the GPU has shown for the program. */
const BAND_TARGET_MS = 16;
/** The most texels in a band, however fast the GPU: a 2048 px square. */
const BAND_TEXELS_MAX = 1 << 22;

/** A bake whose WebGL context went away under it: it ends without a result, like one a newer edit cancelled. */
class Abandoned extends Error {}

/** The domain at 1/`factor` size with the gutter scaled to match, for a quick first look at the same layout. */
export function scaledDomain(domain: BakeDomain, factor: number): BakeDomain {
  return { ...domain, width: Math.max(16, Math.round(domain.width / factor)), height: Math.max(16, Math.round(domain.height / factor)),
    gutter: domain.gutter / factor };
}

export class GpuBaker {
  private readonly camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly geometry = new THREE.PlaneGeometry(2, 2);
  private passes = new Map<string, Pass>();
  private normalPass?: Pass;
  /** Swatch bakes by size (`WxH`), least recently used first. */
  private swatches = new Map<string, Swatch>();
  private featureData?: { key: string; texture: THREE.DataTexture };
  /** What each returned bake drew into, so a viewer can say which bake it displays. */
  private drewInto = new WeakMap<GpuBake, Swatch>();
  /** The swatch a viewer displays (`display`): never given up, whatever else is baked or warmed. */
  private displayed = new Set<object>();
  /** Swatches and passes bakes in flight are using, counted: never evicted or disposed under them. */
  private held = new Map<object, number>();
  /** Advances when the context is lost or restored: a bake that began before gives up instead of using dead objects. */
  private generation = 0;
  private lost = false;
  /** Test seam: GLSL appended to the next programs compiled, to make them fail (see the GPU preview browser test). */
  inject = "";
  /** Called when the context is lost, so a viewer can stop showing textures that no longer exist. */
  onLost?: () => void;

  constructor(private readonly renderer: THREE.WebGLRenderer) {
    renderer.domElement.addEventListener("webglcontextlost", this.contextLost);
    renderer.domElement.addEventListener("webglcontextrestored", this.contextRestored);
  }

  /** Why this renderer cannot bake, or null: WebGL2 render targets with float and half-float color are required. */
  unavailable(): string | null {
    if (this.lost) return "the WebGL context is lost";
    if (!this.renderer.capabilities.isWebGL2) return "WebGL2 is not available";
    if (!this.renderer.extensions.has("EXT_color_buffer_float")) return "float render targets are not available";
    return null;
  }

  private readonly contextLost = (event: Event): void => {
    event.preventDefault();
    this.lost = true;
    this.forget();
    this.onLost?.();
  };

  private readonly contextRestored = (): void => {
    this.lost = false;
    this.forget();
  };

  /** Drops GPU objects that belonged to a lost context; the next bake creates new ones. */
  private forget(): void {
    this.generation++;
    this.passes.clear();
    this.normalPass = undefined;
    this.failureCheck = undefined;
    this.atlasPasses.clear();
    this.atlasNormalPass = undefined;
    this.atlasCompileTarget = undefined;
    this.opPasses.clear();
    this.planePasses.clear();
    this.blankTexture = undefined;
    this.graph.forget();
    this.displayed.clear();
    this.held.clear();
    this.swatches.clear();
    this.fieldTextures.clear();
    this.featureData = undefined;
    // Nothing compiled or drawn survives, and a failure may have been the dying context's.
    this.drawn.clear();
    this.compiling.clear();
    this.failed.clear();
  }

  /**
   * Tells the baker which bake a viewer displays (null: none). Its targets are kept until another bake is displayed,
   * however many bakes and warm-ups run meanwhile.
   */
  display(bake: GpuBake | null): void {
    this.displayed.clear();
    const drew = bake && this.drewInto.get(bake);
    if (drew) this.displayed.add(drew);
  }

  private hold(object: object, holds: object[]): void {
    this.held.set(object, (this.held.get(object) ?? 0) + 1);
    holds.push(object);
  }

  private release(holds: object[]): void {
    for (const object of holds) {
      const count = (this.held.get(object) ?? 1) - 1;
      if (count > 0) this.held.set(object, count); else this.held.delete(object);
    }
    holds.length = 0;
  }

  private evictable(object: object): boolean { return !this.displayed.has(object) && !this.held.has(object); }

  /** Drops the least recently used programs beyond what is kept; one a bake is using waits for the next trim. */
  private trim(): void {
    for (const [passes, kept] of [[this.passes, KEPT_PASSES], [this.atlasPasses, KEPT_PASSES], [this.planePasses, KEPT_PLANE_PASSES]] as const) {
      for (const [key, pass] of [...passes]) {
        if (passes.size <= kept) break;
        if (!this.evictable(pass)) continue;
        // Disposing the material releases its program in three.js.
        pass.material.dispose();
        passes.delete(key);
        this.drawn.delete(key);
      }
    }
  }

  /**
   * The fragment shader of pass 1 for a program's shader source (also used by diagnostics). It has ONE color output:
   * `msOutput` chooses which texture a draw writes, and a bake draws the grid once per texture. `live` are the GLSL
   * expressions of the program's live renderer arguments, four to a texture after the first five. Several draws of one
   * program cost its pixels again (a millisecond or so), where one draw into several buffers costs the compile again:
   * ANGLE's Direct3D backend compiles a pixel shader for a single color output when a program links, and recompiles it,
   * on the page's thread, at the first draw into several draw buffers (measured: as long as the link itself).
   */
  static pass1Source(source: string, live: string[] = []): string {
    const writes = Array.from({ length: Math.ceil(live.length / 4) }, (_, texture) =>
      `  else if (msOutput == ${PASS1_TEXTURES + texture}) msOut = vec4(${[0, 1, 2, 3].map(channel => live[texture * 4 + channel] ?? "0.0").join(", ")});`);
    return `${PASS1_HEAD}\n${source}\n${PASS1_MAIN}${writes.join("\n")}\n}\n`;
  }

  /** The cache key of a bake: everything that changes a pixel, with the profile and version. */
  static key(shading: MaterialShading, uniforms: Uniforms, domain: BakeDomain, fieldIds: string[] = []): string {
    const values = Object.entries(uniforms).filter(([, uniform]) => !(uniform.value as { isTexture?: boolean } | null)?.isTexture)
      .map(([name, uniform]) => [name, JSON.stringify(uniform.value)]).sort();
    const images = shading.shader.images.map(image => `${image.name}:${image.linear}`);
    return `${GPU_PREVIEW_PROFILE}:${GPU_PREVIEW_VERSION}:${factsOf(shading.shader).hash}:${hash(JSON.stringify([values, images, domain, fieldIds]))}`;
  }

  /** The swatch bake's nodes of one size, kept (a few sizes at a time) so the next bake of the size draws over them. */
  private swatch(width: number, height: number): Swatch {
    const name = `${width}x${height}`;
    const kept = this.swatches.get(name);
    if (kept) { this.swatches.delete(name); this.swatches.set(name, kept); return kept; }
    // A size on screen or in use by a bake in flight is never the one given up (the set may exceed its size instead).
    for (const [other, candidate] of [...this.swatches]) {
      if (this.swatches.size < KEPT_TARGETS) break;
      if (!this.evictable(candidate)) continue;
      this.graph.release(candidate.instance); this.swatches.delete(other);
    }
    const created: Swatch = {};
    this.swatches.set(name, created);
    return created;
  }

  /** The texture one output of a swatch bake is drawn into: pass 1's five, the normals and the live argument textures. */
  private static swatchTarget(name: string, width: number, height: number): TargetSpec {
    const spec: TargetSpec = { width, height, format: THREE.RGBAFormat, type: THREE.UnsignedByteType, colorSpace: THREE.NoColorSpace,
      filter: "mips", wrap: THREE.RepeatWrapping, anisotropy: MAP_ANISOTROPY, name: `gpu-preview ${name}` };
    // Base color (sRGB) and packed properties are bytes; emissive is HDR; heights (meters) and offsets are read with texelFetch only.
    if (name === "base color") spec.colorSpace = THREE.SRGBColorSpace;
    else if (name === "emissive") spec.type = THREE.HalfFloatType;
    else if (name === "height" || name === "normal offset") {
      spec.type = name === "height" ? THREE.FloatType : THREE.HalfFloatType;
      spec.filter = "nearest"; spec.wrap = THREE.ClampToEdgeWrapping; spec.anisotropy = undefined;
    } else if (name.startsWith("live")) { spec.type = THREE.HalfFloatType; spec.filter = "linear"; spec.wrap = THREE.ClampToEdgeWrapping; spec.anisotropy = undefined; }
    return spec;
  }

  private disposeTargets(): void {
    for (const swatch of this.swatches.values()) this.graph.release(swatch.instance);
    this.swatches.clear();
    this.featureData?.texture.dispose();
    this.featureData = undefined;
    for (const texture of this.fieldTextures.values()) texture.dispose();
    this.fieldTextures.clear();
  }

  /** Builds the mip chain of rendered textures once, after every band is drawn. */
  private mips(textures: THREE.Texture[]): void {
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    for (const texture of textures) {
      const handle = (this.renderer.properties.get(texture) as { __webglTexture?: WebGLTexture }).__webglTexture;
      if (!handle) continue;
      this.renderer.state.bindTexture(gl.TEXTURE_2D, handle);
      gl.generateMipmap(gl.TEXTURE_2D);
    }
    this.renderer.state.unbindTexture();
  }

  private fieldTextures = new Map<string, THREE.DataTexture>();

  /** A CPU field as a nearest-sampled float texture, kept (a few at a time) while its content is in use. */
  private fieldTexture(field: FieldTexels): THREE.DataTexture {
    const kept = this.fieldTextures.get(field.id);
    if (kept) { this.fieldTextures.delete(field.id); this.fieldTextures.set(field.id, kept); return kept; }
    const texture = new THREE.DataTexture(field.rgba as Float32Array<ArrayBuffer>, field.width, field.height, THREE.RGBAFormat, THREE.FloatType);
    texture.minFilter = texture.magFilter = THREE.NearestFilter;
    texture.generateMipmaps = false;
    texture.needsUpdate = true;
    this.fieldTextures.set(field.id, texture);
    while (this.fieldTextures.size > 6) {
      const [oldest, evicted] = this.fieldTextures.entries().next().value!;
      evicted.dispose(); this.fieldTextures.delete(oldest);
    }
    return texture;
  }

  /** Receiver features as a float texture of three texels per feature, one feature per row. */
  private featureTexture(features: number[]): THREE.DataTexture {
    const key = features.join(",");
    if (this.featureData?.key === key) return this.featureData.texture;
    this.featureData?.texture.dispose();
    const rows = Math.max(1, features.length / 12);
    const data = new Float32Array(rows * 12);
    data.set(features);
    const texture = new THREE.DataTexture(data, 3, rows, THREE.RGBAFormat, THREE.FloatType);
    texture.minFilter = texture.magFilter = THREE.NearestFilter;
    texture.generateMipmaps = false;
    texture.needsUpdate = true;
    this.featureData = { key, texture };
    return texture;
  }

  /** The shader's bake material, compiled on first use of its source (asynchronously where the driver allows). */
  private async pass(shading: MaterialShading): Promise<Pass> {
    // Programs are kept by the identity of their source (the engine's, where it states one).
    const source = shaderIdentity(shading.shader);
    const cached = this.passes.get(source);
    if (cached) { this.passes.delete(source); this.passes.set(source, cached); return cached; }
    // One compile per source, however many bakes (a late first preview and the next edit) wait for it.
    const waiting = this.compiling.get(source);
    if (waiting) return waiting;
    const material = new THREE.ShaderMaterial({
      glslVersion: THREE.GLSL3,
      vertexShader: VERTEX,
      // The program's live renderer arguments are further outputs of the same program (a shader's source fixes them).
      fragmentShader: GpuBaker.pass1Source(shading.shader.source, liveArguments(shading).map(argument => argument.expression)),
      uniforms: {},
      depthTest: false, depthWrite: false, blending: THREE.NoBlending,
    });
    const started: Promise<Pass> = this.compiled(material, this.compileTarget(), this.passes, source)
      .finally(() => { if (this.compiling.get(source) === started) this.compiling.delete(source); });
    this.compiling.set(source, started);
    return started;
  }

  private compiling = new Map<string, Promise<Pass>>();
  /** Why a source's program did not compile, by the key it would be cached under: it is not compiled again. */
  private failed = new Map<string, string>();
  /**
   * Sources whose program has been drawn once. A program is complete when its parallel compile says so (each has a
   * single color output, and is compiled for the kind of geometry it draws: see `pass1Source` and `compiled`); the
   * first draw still creates its uniform and texture state, so a program is counted warm only after it.
   */
  private drawn = new Set<string>();

  /** Whether the program has been compiled and drawn once, so the next bake pays only for its pixels. */
  hasDrawn(shading: MaterialShading): boolean { return this.drawn.has(shaderIdentity(shading.shader)); }

  /**
   * Compiles and draws a program once at a tiny size, off the critical path: the first draw is where drivers finish the
   * (sometimes seconds long) shader compile, and a bake that arrives later than the CPU result would otherwise skip it.
   * Draws nothing a viewer shows. Errors are swallowed; the next real bake reports them.
   */
  async warm(shading: MaterialShading, uniforms: Uniforms, domain: BakeDomain, fields?: Record<string, FieldTexels>): Promise<void> {
    if (this.busy > 0) return;
    try { await this.bake(shading, uniforms, scaledDomain(domain, 16), { fields, current: () => true }); } catch { /* reported by a real bake */ }
  }

  /**
   * Compiles against a render target (any gives the same program: three keys programs by whether they draw to the canvas,
   * not by which target) and checks that the program linked. three.js checks that only when a program is first used to draw, and then only logs it, so the link
   * status is read here: a program the driver rejects fails the bake instead of leaving the targets as they were. A
   * failure is remembered under `key`.
   */
  private async compiled(material: THREE.ShaderMaterial, target: THREE.WebGLRenderTarget, into: Map<string, Pass> | undefined, key: string,
    geometry: THREE.BufferGeometry = this.geometry): Promise<Pass> {
    const known = this.failed.get(key);
    if (known) { material.dispose(); throw new Error(known); }
    if (this.inject) material.fragmentShader += this.inject;
    const generation = this.generation;
    const scene = new THREE.Scene();
    // three.js keys a program by what its mesh's geometry has (normals, colors, tangents) as well as by its material: a
    // program compiled for one kind of geometry is compiled again, on the page's thread, at the first draw of another.
    const quad = new THREE.Mesh(geometry, material);
    quad.frustumCulled = false;
    scene.add(quad);
    // compileAsync creates the programs at once and then waits for the driver (KHR_parallel_shader_compile where present,
    // so the page stays responsive). The target is only bound while the programs are created: frames drawn during the wait
    // go where the viewer sends them.
    const before = this.renderer.getRenderTarget();
    let ready: Promise<unknown>;
    this.renderer.setRenderTarget(target);
    try { ready = this.renderer.compileAsync(scene, this.camera); } finally { this.renderer.setRenderTarget(before); }
    await ready;
    if (generation !== this.generation) { material.dispose(); throw new Abandoned(); }
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    const program = (this.renderer.properties.get(material) as { currentProgram?: { program?: WebGLProgram } }).currentProgram?.program;
    let failure = "";
    if (!program) failure = "no program was created";
    else if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      if (gl.isContextLost()) { material.dispose(); throw new Abandoned(); }
      const logs = [gl.getProgramInfoLog(program), ...(gl.getAttachedShaders(program) ?? []).map(shader => gl.getShaderInfoLog(shader))];
      failure = logs.map(log => (log ?? "").trim()).find(Boolean) ?? "the program did not link";
    }
    if (failure) {
      material.dispose();
      const message = `GPU preview shader did not compile: ${failure.slice(0, 400)}`;
      this.failed.set(key, message);
      while (this.failed.size > KEPT_FAILURES) this.failed.delete(this.failed.keys().next().value!);
      throw new Error(message);
    }
    const pass = { material, scene, mesh: quad };
    into?.set(key, pass);
    return pass;
  }

  private async normals(): Promise<Pass> {
    if (this.normalPass) return this.normalPass;
    const material = new THREE.ShaderMaterial({
      glslVersion: THREE.GLSL3,
      vertexShader: VERTEX,
      fragmentShader: PASS2_FRAGMENT,
      uniforms: { msHeightMap: { value: null }, msOffsetMap: { value: null }, msStep: { value: new THREE.Vector2(1, 1) },
        msFrameX: { value: new THREE.Vector3() }, msFrameY: { value: new THREE.Vector3() }, msFrameZ: { value: new THREE.Vector3() },
        msOrient: { value: false }, msBend: { value: false } },
      depthTest: false, depthWrite: false, blending: THREE.NoBlending,
    });
    const pass = await this.compiled(material, this.compileTarget(), undefined, "normals");
    // Two bakes may have compiled it at once; one is kept.
    if (this.normalPass) { pass.material.dispose(); return this.normalPass; }
    return this.normalPass = pass;
  }

  private failureCheck?: { pass: Pass; target: THREE.WebGLRenderTarget };

  /** Compiles the program `normalsFailed` draws, ahead of any draw into a target that may be on screen. */
  private async failureChecker(): Promise<void> {
    if (this.failureCheck) return;
    const target = new THREE.WebGLRenderTarget(FAIL_GRID, FAIL_GRID, { depthBuffer: false, stencilBuffer: false,
      format: THREE.RGBAFormat, type: THREE.UnsignedByteType, colorSpace: THREE.NoColorSpace,
      minFilter: THREE.NearestFilter, magFilter: THREE.NearestFilter, generateMipmaps: false });
    const material = new THREE.ShaderMaterial({ glslVersion: THREE.GLSL3, vertexShader: VERTEX, fragmentShader: PASS3_FRAGMENT,
      uniforms: { msNormalMap: { value: null } }, depthTest: false, depthWrite: false, blending: THREE.NoBlending });
    try {
      const pass = await this.compiled(material, target, undefined, "failures");
      // Two bakes may have compiled it at once; one is kept.
      if (this.failureCheck) { pass.material.dispose(); target.dispose(); } else this.failureCheck = { pass, target };
    } catch (error) { target.dispose(); throw error; }
  }

  /** True when any texel of a normal target is marked failed (alpha 0): one small draw and a read of its cells. */
  private async normalsFailed(normal: THREE.WebGLRenderTarget): Promise<boolean> {
    await this.failureChecker();
    const { pass, target } = this.failureCheck!;
    pass.material.uniforms.msNormalMap.value = normal.texture;
    this.renderInto(target, pass.scene, null);
    const cells = new Uint8Array(FAIL_GRID * FAIL_GRID * 4);
    this.renderer.readRenderTargetPixels(target, 0, 0, FAIL_GRID, FAIL_GRID, cells);
    for (let index = 0; index < cells.length; index += 4) if (cells[index] > 0) return true;
    return false;
  }

  /** Whether the shader source is already compiled, so a parameter or seed edit needs no recompile. */
  compiledFor(shading: MaterialShading): boolean {
    return this.passes.has(shaderIdentity(shading.shader));
  }

  /**
   * Bakes `shading` over `domain`. `uniforms` holds the program's parameter, seed and image uniforms
   * (shade.ts binding). Throws when the shader does not compile or the context cannot bake; callers
   * fall back to the CPU draft. The returned textures are overwritten by the next bake of this size.
   */
  async bake(shading: MaterialShading, uniforms: Uniforms, domain: BakeDomain, options: BakeOptions = {}): Promise<GpuBake | null> {
    this.busy++;
    const holds: object[] = [];
    try { return await this.bakeNow(shading, uniforms, domain, options, holds); }
    catch (error) { if (error instanceof Abandoned) return null; throw error; }
    finally { this.release(holds); this.busy--; this.trim(); }
  }

  /** Bakes in flight: a warm-up draw waits for them, since the pass uniforms are shared. */
  private busy = 0;

  /**
   * Pass 1 of a swatch bake as a graph operation: the program's bake shader, whose outputs (chosen by `msOutput`, see
   * `pass1Source`) are the bake's textures and, from `PASS1_TEXTURES` on, its live argument textures. `bind` gives the
   * bake's uniforms and is asked only when a node is drawn.
   */
  private swatchEvaluateOp(label: string, shading: MaterialShading, bind: () => Uniforms, holds: object[]): NodeOp {
    let pass: Pass | undefined;
    return {
      label,
      prepare: async () => { pass = await this.pass(shading); this.hold(pass, holds); },
      draw: ({ node, output, rows }) => {
        // Three keeps a reference to the material's uniform table, so binding assigns into it. Assigned at every draw:
        // another bake may have used the program since the last band.
        Object.assign(pass!.material.uniforms, bind());
        pass!.material.uniforms.msOutput = { value: output };
        pass!.material.uniformsNeedUpdate = true;
        this.renderInto(node.target, pass!.scene, rows);
      },
    };
  }

  /** Pass 2 of a swatch bake as a graph operation: the `height` node (and, bending, the `offset` node) to tangent-space normals. */
  private swatchNormalsOp(domain: BakeDomain, orient: boolean, bend: boolean): NodeOp {
    let pass: Pass | undefined;
    const length = (vector: number[]) => Math.hypot(vector[0], vector[1], vector[2]);
    const vec3 = (value: number[]) => new THREE.Vector3(...(value as [number, number, number]));
    return {
      label: "normals",
      prepare: async () => { pass = await this.normals(); if (orient) await this.failureChecker(); },
      draw: ({ node, inputs, rows }) => {
        const inner = [domain.width - 2 * domain.gutter, domain.height - 2 * domain.gutter];
        const uniform = pass!.material.uniforms;
        uniform.msHeightMap.value = inputs.height.texture;
        uniform.msOffsetMap.value = (inputs.offset ?? inputs.height).texture;
        (uniform.msStep.value as THREE.Vector2).set(length(domain.u) / inner[0], length(domain.v) / inner[1]);
        // The tangent frame the CPU atlas uses: z the receiver normal, x along image right, y up the image.
        const z = vec3(domain.normal).normalize();
        const x = vec3(domain.u).addScaledVector(z, -z.dot(vec3(domain.u))).normalize();
        const y = new THREE.Vector3().crossVectors(z, x);
        if (y.dot(vec3(domain.v)) > 0) y.negate();
        (uniform.msFrameX.value as THREE.Vector3).copy(x); (uniform.msFrameY.value as THREE.Vector3).copy(y);
        (uniform.msFrameZ.value as THREE.Vector3).copy(z);
        uniform.msOrient.value = orient; uniform.msBend.value = bend;
        pass!.material.uniformsNeedUpdate = true;
        this.renderInto(node.target, pass!.scene, rows);
      },
      // A receiver normal facing away from the receiver is a bake error on the CPU; the preview then gives way to it.
      finish: orient ? async node => {
        if (await this.normalsFailed(node.target)) throw new Error("receiver_normal output must face the same side as the receiver");
      } : undefined,
    };
  }

  /**
   * A swatch bake as a pass graph, like an atlas bake (`atlasNodes`): pass 1's five textures and the program's live
   * argument textures are nodes of one program, keyed by the bake (`key`); the normal is keyed by the domain's frame
   * and otherwise by its inputs, and a program without relief has the flat normal, drawn once per size.
   */
  private async bakeNow(shading: MaterialShading, uniforms: Uniforms, domain: BakeDomain, options: BakeOptions, holds: object[]): Promise<GpuBake | null> {
    // (A swatch keeps the fixed bands: one bake at a time is on screen, and its first look is a small one.)
    const { measure = false, bandTexels = BAND_TEXELS, fields = {} } = options;
    const reason = this.unavailable();
    if (reason) throw new Error(`GPU preview unavailable: ${reason}.`);
    const unsupported = gpuBakeUnsupported(shading, domain.features !== undefined);
    if (unsupported) throw new Error(`GPU preview does not support this program: ${unsupported}.`);
    const { width, height } = domain;
    if (width < 1 || height < 1 || width > 4096 || height > 4096) throw new Error("GPU preview size is out of range.");
    // Texture units: the baking program, and the viewer material that will show the maps (its maps, the environment,
    // shadow maps and the live argument textures), must each fit the device or the preview gives way to the CPU result.
    // Both are known from the program, so they are checked before anything is compiled or drawn.
    const unitLimit = this.renderer.capabilities.maxTextures;
    const bakeUnits = factsOf(shading.shader).samplers;
    if (bakeUnits > unitLimit) throw new Error(`GPU preview needs ${bakeUnits} texture units to bake; this device has ${unitLimit}.`);
    const args = liveArguments(shading);
    const liveTextures = Math.ceil(args.length / 4);
    const viewerUnits = (options.viewerUnits ?? VIEWER_UNITS) + liveTextures;
    if (viewerUnits > unitLimit) throw new Error(`GPU preview needs ${viewerUnits} texture units to show its maps and lighting arguments; this device has ${unitLimit}.`);
    const hadPass = this.compiledFor(shading);
    const outputs = shading.shader.outputs;
    const orient = outputs.includes("receiver_normal"), bend = outputs.includes("normal_offset");

    // CPU fields: float textures on the bake grid, read by texelFetch, so alignment is exact and nothing is filtered.
    const fieldIds: string[] = [];
    const bakedFields: Record<string, FieldTexels> = {};
    for (const wanted of shading.shader.cpuFields ?? []) {
      const given = fields[wanted.uniform];
      if (!given) throw new Error(`GPU preview needs the CPU field ${wanted.name}.`);
      const field = downsampleField(given, width, height);
      if (field.width !== width || field.height !== height) throw new Error("CPU field and bake grid sizes differ.");
      bakedFields[wanted.uniform] = field;
      fieldIds.push(`${wanted.name}=${field.id}`);
    }
    const inner = [width - 2 * domain.gutter, height - 2 * domain.gutter];
    const length = (vector: number[]) => Math.hypot(vector[0], vector[1], vector[2]);
    let bound: Uniforms | undefined, bindMs = 0;
    const bind = (): Uniforms => {
      if (bound) return bound;
      const started = performance.now();
      const footprint = Math.max(length(domain.u) / inner[0], length(domain.v) / inner[1]);
      const uvFootprint = Math.max(Math.hypot(...domain.uvU) / inner[0], Math.hypot(...domain.uvV) / inner[1]);
      const vec3 = (value: number[]) => new THREE.Vector3(...(value as [number, number, number]));
      const vec2 = (value: number[]) => new THREE.Vector2(...(value as [number, number]));
      bound = {
        ...uniforms,
        msAtlas: { value: new THREE.Vector2(width, height) },
        msGutter: { value: domain.gutter },
        msInner: { value: new THREE.Vector2(inner[0], inner[1]) },
        msOrigin: { value: vec3(domain.origin) }, msU: { value: vec3(domain.u) }, msV: { value: vec3(domain.v) },
        msUvOrigin: { value: vec2(domain.uvOrigin) }, msUvU: { value: vec2(domain.uvU) }, msUvV: { value: vec2(domain.uvV) },
        msReceiverNormal: { value: vec3(domain.normal).normalize() },
        msObjectOrigin: { value: vec3(domain.objectOrigin) },
        msExtent: { value: vec2(domain.extent) },
        msMapping: { value: ["uv", "object", "world"].indexOf(domain.mapping) },
        msFootprint: { value: footprint }, msUvFootprint: { value: uvFootprint },
        ms_gravity: { value: vec3(domain.gravity) },
        ms_bounds_min: { value: vec3(domain.boundsMin) }, ms_bounds_max: { value: vec3(domain.boundsMax) },
        ms_feature_count: { value: (domain.features?.length ?? 0) / 12 },
        ...(domain.features ? { ms_features: { value: this.featureTexture(domain.features) } } : {}),
      };
      for (const [uniform, field] of Object.entries(bakedFields)) bound[uniform] = { value: this.fieldTexture(field) };
      bindMs = performance.now() - started;
      return bound;
    };

    const key = GpuBaker.key(shading, uniforms, domain, fieldIds);
    // The bake's key names its images by name and role (it is shown and compared across sessions); a node's texels
    // also depend on which decoded images those are, so the nodes are keyed by them as well.
    const content = `${key}:${shading.shader.images.map(image => (uniforms[image.uniform]?.value as THREE.Texture | undefined)?.uuid).join(",")}`;
    const evaluate = this.swatchEvaluateOp("evaluate", shading, bind, holds);
    const specs: NodeSpec[] = ["base color", "properties", "emissive", "height", "normal offset"].map((name, output) =>
      ({ id: name, key: `${content}:${name}`, target: GpuBaker.swatchTarget(name, width, height), op: evaluate, output }));
    // A program without a height output (or receiver-normal outputs) has a flat normal, drawn once per size.
    const flat = !outputs.includes("height") && !orient && !bend;
    const frame = JSON.stringify([domain.u, domain.v, domain.normal, domain.gutter]);
    specs.push(flat ? { id: "normal", key: "flat-normal:1", target: GpuBaker.swatchTarget("normal", width, height), op: this.fillOp("normals", 0.5, 0.5, 1) }
      : { id: "normal", key: `swatch-normals:1:${hash(frame)}:${frame.length}:${orient}:${bend}`, target: GpuBaker.swatchTarget("normal", width, height),
        op: this.swatchNormalsOp(domain, orient, bend), inputs: bend ? { height: "height", offset: "normal offset" } : { height: "height" } });
    // The live renderer arguments are further outputs of pass 1's own program (`pass1Source`): no other program is
    // compiled for them. (An operation of their own only so their time is told apart.)
    const live = liveTextures ? this.swatchEvaluateOp("live", shading, bind, holds) : undefined;
    for (let index = 0; index < liveTextures; index++) {
      specs.push({ id: `live${index}`, key: `${content}:live${index}`, target: GpuBaker.swatchTarget(`live ${index}`, width, height), op: live!, output: PASS1_TEXTURES + index });
    }

    const swatch = this.swatch(width, height);
    this.hold(swatch, holds);
    const result = await this.graph.realize(specs, swatch.instance, { current: options.current, measure, bandTexels,
      stable: () => GpuBaker.key(shading, uniforms, domain, fieldIds) === key });
    if (!result) return null;
    swatch.instance = result.instance;
    const nodes = result.instance;
    if (this.passes.has(shaderIdentity(shading.shader))) this.drawn.add(shaderIdentity(shading.shader));
    let liveResult: GpuBake["live"];
    if (liveTextures) {
      const textures: Record<string, THREE.Texture> = {};
      const operations: LiveArgumentOperation[] = shading.shader.live.map(operation => ({ id: operation.id, arguments: {} }));
      for (let index = 0; index < liveTextures; index++) textures[`gpu-live-${index}`] = nodes.texture(`live${index}`)!;
      args.forEach((argument, index) => {
        operations[argument.operationIndex].arguments[argument.name] = { texture: `gpu-live-${Math.floor(index / 4)}`, channel: index % 4, scale: 1, offset: 0 };
      });
      liveResult = { key: JSON.stringify(operations.map(operation => [operation.id, Object.keys(operation.arguments)])), operations, textures };
    }
    const { timings } = result;
    const bake: GpuBake = {
      profile: GPU_PREVIEW_PROFILE, key, width, height,
      baseColor: nodes.texture("base color")!, properties: nodes.texture("properties")!, emissive: nodes.texture("emissive")!, normal: nodes.texture("normal")!,
      timings: { compile: hadPass ? 0 : timings.prepare, uniforms: bindMs, pass1: timings.draw.evaluate ?? 0, mips: timings.mips, pass2: timings.draw.normals ?? 0,
        total: timings.total, bands: result.bands, live: timings.draw.live ?? 0 },
      live: liveResult,
      units: { bake: bakeUnits, viewer: viewerUnits, limit: unitLimit },
    };
    this.drewInto.set(bake, swatch);
    return bake;
  }

  // ---- Receiver atlases (scene projects) ----

  private atlasPasses = new Map<string, Pass>();
  private atlasNormalPass?: Pass;
  /** What an atlas pass's mesh holds between draws: positions only, like the bake geometry of a receiver (`bakeGeometry`). */
  private readonly atlasIdle = new THREE.BufferGeometry().setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));

  /** How the evaluate pass of `shading` is laid out on this device (see `programLayout`). */
  atlasProgram(shading: MaterialShading): AtlasProgramLayout {
    return programLayout(shading);
  }

  /**
   * The pass graph every atlas bake is realized in: it owns the baked textures (one per node), shares a node between
   * all the bakes that want the same texels, and keeps complete nodes for a while after their last user.
   */
  readonly graph = new BakeGraph({
    generation: () => this.generation,
    mips: textures => this.mips(textures),
    settle: target => this.settle(target),
    yield: issued => this.afterBand(issued),
    batch: draws => {
      // The renderer's state is saved once for the band and restored before the browser's next turn: the viewer's own
      // frames render with it in between.
      const before = this.renderer.getRenderTarget(), clear = this.renderer.autoClear;
      this.batching = true;
      try { this.renderer.autoClear = false; draws(); }
      finally { this.batching = false; this.renderer.setRenderTarget(before); this.renderer.autoClear = clear; }
    },
    // A scene of items has a receiver per part and face group (the item washroom: 115 receivers, 736 nodes, most of
    // them small). While an item control is dragged the scene is shown per pixel and every node is idle; they must
    // all still be there on release. What bounds them is memory alone, and the same memory that bounds the maps on
    // screen (`SCENE_MAP_BYTES`): a scene that may be displayed may also be kept whole while it is not. The same
    // number bounds the two together: the maps on screen come first, and what is kept besides them (a scene's
    // intermediates, the scene before it) is what still fits, so the allowance is what the bakes take of the GPU.
  }, { idleBytes: SCENE_MAP_BYTES, nodeFloorBytes: NODE_FLOOR_BYTES, totalBytes: SCENE_MAP_BYTES });
  /** Whether `renderInto` is called from the graph's batch, which saves and restores the renderer's state around it. */
  private batching = false;

  /**
   * How a bake draws a large node: in bands of rows, with the browser given a turn between them, so no single draw
   * keeps the GPU longer than its watchdog allows or the viewer's frames waiting.
   *
   * `gpu` (the default) paces the bands by the GPU: the next band is issued when the GPU has executed the one before
   * (a fence, polled each turn), so the GPU is never more than a band behind. A scene's bakes (atlases and layered
   * slots) also size their bands by it: how long a band took says how many texels of that program (`NodeOp.pace`) the
   * GPU draws in `BAND_TARGET_MS`. A program's bands start at `BAND_TEXELS` and take that many texels from then on
   * (up to `BAND_TEXELS_MAX`), so a GPU that is fast at it draws a whole atlas at once and a slow one keeps the small
   * bands. The time measured includes the wait for the fence, so the speed is never overstated.
   *
   * `timer` is the pace before this: a zero timer between bands of `BAND_TEXELS`, which the browser stretches to 4 ms
   * in a loop whatever the GPU is doing (most of a large scene's bake time on a fast GPU). `?bakepace=timer` in the
   * viewer, for comparisons, and where the context has no fences.
   */
  bandPacing: "gpu" | "timer" = "gpu";
  /** A fixed number of texels per band for every program, when set (`?bakeband=<texels>` in the viewer, for measurements). */
  bandTexels?: number;
  /** Texels a millisecond the GPU has shown for each program it drew in bands, by `NodeOp.pace`. */
  private readonly bandRates = new Map<string, number>();

  /** Texels per band for a node, by what the GPU has shown for its operation's program. */
  private bandTexelsFor(spec: NodeSpec): number {
    if (this.bandTexels !== undefined) return this.bandTexels;
    const rate = this.bandPacing === "gpu" ? this.bandRates.get(spec.op.pace ?? spec.op.label) : undefined;
    return rate === undefined ? BAND_TEXELS : Math.min(BAND_TEXELS_MAX, Math.max(BAND_TEXELS, Math.floor(rate * BAND_TARGET_MS)));
  }

  private afterBand(issued: BandWork[]): Promise<void> {
    if (this.bandPacing === "timer") return yieldToBrowser();
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    const sync = typeof gl.fenceSync === "function" && !gl.isContextLost() ? gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0) : null;
    if (!sync) return yieldToBrowser();
    gl.flush();
    return new Promise(done => {
      const started = performance.now();
      const poll = (): void => {
        const waited = performance.now() - started;
        if (!this.lost && !gl.isContextLost() && waited < BAND_WAIT_MS && gl.clientWaitSync(sync, 0, 0) === gl.TIMEOUT_EXPIRED) {
          // Asked again every turn at first (a fast GPU answers within a few), then at a timer's pace, not in a spin.
          if (waited < BAND_SPIN_MS) nextTurn(poll); else setTimeout(poll, 1);
          return;
        }
        gl.deleteSync(sync);
        // What the band's operations drew in that time (each as if it had the GPU to itself: never too fast).
        for (const { op, texels } of issued) this.bandRates.set(op.pace ?? op.label, texels / Math.max(waited, 0.05));
        done();
      };
      nextTurn(poll);
    });
  }

  /** Forces the GPU to finish what was drawn into a target (draw calls return before they run): one texel is read from it. */
  private settle(target: THREE.WebGLRenderTarget): void {
    const type = target.textures[0].type;
    const texel = type === THREE.FloatType ? new Float32Array(4) : type === THREE.HalfFloatType ? new Uint16Array(4) : new Uint8Array(4);
    this.renderer.readRenderTargetPixels(target, 0, 0, 1, 1, texel, undefined, 0);
  }

  /** The texture an attachment of an atlas bake is drawn into (see `atlasFragment` for why each is its own target). */
  static atlasTarget(name: AtlasAttachment | "normal", width: number, height: number, layout: AtlasProgramLayout): TargetSpec {
    const spec: TargetSpec = { width, height, format: THREE.RGBAFormat, type: THREE.UnsignedByteType, colorSpace: THREE.NoColorSpace,
      filter: "mips", wrap: THREE.ClampToEdgeWrapping, anisotropy: MAP_ANISOTROPY, name: `gpu-atlas ${name}` };
    if (name === "base") spec.colorSpace = THREE.SRGBColorSpace;
    else if (name === "height") {
      // Read with texelFetch only. One channel unless the program's receiver_normal output rides beside the height.
      spec.type = THREE.FloatType; spec.filter = "nearest"; spec.anisotropy = undefined;
      if (!layout.orient) spec.format = THREE.RedFormat;
    } else if (name === "offset") { spec.type = THREE.HalfFloatType; spec.filter = "nearest"; spec.anisotropy = undefined; }
    else if (name !== "properties" && name !== "normal") spec.type = THREE.HalfFloatType;
    return spec;
  }

  /** Gives up the textures of an atlas bake: complete ones wait in the graph for another user. Releasing twice does nothing. */
  releaseAtlasMaps(maps: AtlasMaps | undefined): void { this.graph.release(maps?.nodes); }

  /** Whether `maps` exist on the GPU and hold their bake completely (a lost context, or a bake abandoned over them, leaves them not). */
  atlasMapsComplete(maps: AtlasMaps | undefined): maps is AtlasMaps {
    return !!maps && !this.lost && maps.nodes.complete && [...maps.nodes.nodes.values()].every(node => node.generation === this.generation);
  }

  /** GPU memory of the distinct textures of some atlas bakes, mips included (bakes share textures). */
  atlasBytes(maps: Iterable<AtlasMaps | undefined>): number {
    return this.graph.bytes([...maps].map(held => held?.nodes));
  }

  /** Whether the program that bakes `shading` into `layout` is compiled and has drawn once (so a bake costs only its pixels). */
  atlasWarm(shading: MaterialShading, layout: AtlasProgramLayout): boolean { return this.drawn.has(atlasPassKey(shading.shader, layout)); }

  /** A target the atlas programs are compiled against before any receiver's maps exist (any render target gives the same program). */
  private atlasCompileTarget?: THREE.WebGLRenderTarget;

  /**
   * Starts compiling the programs a bake of `shading` over an atlas draws with, without waiting for them: the driver
   * compiles beside whatever else is going on, and the first bake finds them ready (or waits for the rest).
   * Errors are left for that bake to report.
   */
  precompileAtlas(shading: MaterialShading): void {
    if (this.unavailable() || gpuBakeUnsupported(shading, true)) return;
    const target = this.atlasCompileTarget ??= new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false, stencilBuffer: false });
    const layout = this.atlasProgram(shading);
    void this.atlasPass(shading.shader, layout, target).catch(() => {});
    const flat = !shading.shader.outputs.includes("height") && !layout.orient && !layout.attachments.includes("offset");
    if (!flat) void this.atlasNormals(target).catch(() => {});
  }

  private async atlasPass(shader: ProgramShader, layout: AtlasProgramLayout, target: THREE.WebGLRenderTarget): Promise<Pass> {
    const key = atlasPassKey(shader, layout);
    const cached = this.atlasPasses.get(key);
    if (cached) { this.atlasPasses.delete(key); this.atlasPasses.set(key, cached); return cached; }
    const waiting = this.compiling.get(key);
    if (waiting) return waiting;
    const material = new THREE.ShaderMaterial({ glslVersion: THREE.GLSL3, vertexShader: ATLAS_VERTEX, fragmentShader: atlasFragment(shader.source, layout),
      uniforms: {}, depthTest: false, depthWrite: false, blending: THREE.NoBlending, side: THREE.DoubleSide });
    const started: Promise<Pass> = this.compiled(material, target, this.atlasPasses, key, this.atlasIdle)
      .finally(() => { if (this.compiling.get(key) === started) this.compiling.delete(key); });
    this.compiling.set(key, started);
    return started;
  }

  private async atlasNormals(target: THREE.WebGLRenderTarget): Promise<Pass> {
    if (this.atlasNormalPass) return this.atlasNormalPass;
    const waiting = this.compiling.get("atlas-normals");
    if (waiting) return waiting;
    const material = new THREE.ShaderMaterial({ glslVersion: THREE.GLSL3, vertexShader: ATLAS_VERTEX, fragmentShader: ATLAS_NORMAL_FRAGMENT,
      uniforms: { msHeightMap: { value: null }, msOffsetMap: { value: null }, msAtlas: { value: new THREE.Vector2(1, 1) }, msGutter: { value: 0 },
        msOrient: { value: false }, msBend: { value: false } },
      depthTest: false, depthWrite: false, blending: THREE.NoBlending, side: THREE.DoubleSide });
    const started: Promise<Pass> = this.compiled(material, target, undefined, "atlas-normals", this.atlasIdle)
      .then(pass => this.atlasNormalPass = pass)
      .finally(() => { if (this.compiling.get("atlas-normals") === started) this.compiling.delete("atlas-normals"); });
    this.compiling.set("atlas-normals", started);
    return started;
  }

  /** Fills a target with one value (a flat normal), leaving the renderer's own clear settings as they were. */
  private fill(target: THREE.WebGLRenderTarget, red: number, green: number, blue: number): void {
    const before = this.renderer.getRenderTarget(), color = this.renderer.getClearColor(new THREE.Color()), alpha = this.renderer.getClearAlpha();
    try {
      this.renderer.setRenderTarget(target);
      this.renderer.setClearColor(new THREE.Color().setRGB(red, green, blue, THREE.LinearSRGBColorSpace), 1);
      this.renderer.clear(true, false, false);
    } finally {
      this.renderer.setClearColor(color, alpha);
      this.renderer.setRenderTarget(before);
    }
  }

  /** The cache key of an atlas bake: everything that changes a texel. `receiver` names the receiver inputs that are textures (its features). */
  static atlasKey(shading: MaterialShading, uniforms: Uniforms, atlas: AtlasLayout, layout: AtlasProgramLayout, fieldIds: string[], receiver: string): string {
    return `${GpuBaker.atlasProgramKey(shading, atlas, layout)}:${GpuBaker.atlasInputsKey(shading, uniforms, fieldIds, receiver)}`;
  }

  /** The part of an atlas bake's key that its input values do not change: the profile, the program, its layout and the atlas. */
  private static atlasProgramKey(shading: MaterialShading, atlas: AtlasLayout, layout: AtlasProgramLayout): string {
    return `${GPU_PREVIEW_PROFILE}:${GPU_PREVIEW_VERSION}:atlas:${factsOf(shading.shader).hash}:${layout.key}:${atlas.key}`;
  }

  /**
   * The part of an atlas bake's key that names its input values: every uniform's value, the images, the CPU fields
   * (`fieldIds`, one per field of `shader.cpuFields`) and the receiver. With `reads`, the key of one texture that reads
   * only those of the program's own uniforms (`attachmentReads`): the other parameters, seeds, images and CPU fields
   * are left out, so a change to them leaves the key, and the texture, as they are. Everything that is not the
   * program's own (the receiver's inputs) is always in it.
   */
  private static atlasInputsKey(shading: MaterialShading, uniforms: Uniforms, fieldIds: string[], receiver: string, reads?: ReadonlySet<string>): string {
    const shader = shading.shader;
    const unread = new Set(reads ? [...shader.parameters, ...shader.seeds].map(entry => entry.uniform).filter(uniform => !reads.has(uniform)) : []);
    const values = Object.entries(uniforms).filter(([name, uniform]) => !unread.has(name) && !(uniform.value as { isTexture?: boolean } | null)?.isTexture)
      .map(([name, uniform]) => [name, JSON.stringify(uniform.value)]).sort();
    const images = shader.images.filter(image => !reads || reads.has(image.uniform))
      .map(image => `${image.name}:${image.linear}:${(uniforms[image.uniform]?.value as THREE.Texture | undefined)?.uuid}`);
    const fields = reads ? fieldIds.filter((_, index) => reads.has(shader.cpuFields![index].uniform)) : fieldIds;
    return hash(JSON.stringify([values, images, fields, receiver]));
  }

  /** What each texture of a program's evaluate pass reads (`attachmentReads`), found once per shader record and layout. */
  private readonly reads = new WeakMap<ProgramShader, Map<string, ReturnType<typeof attachmentReads>>>();
  private atlasReads(shader: ProgramShader, layout: AtlasProgramLayout): ReturnType<typeof attachmentReads> {
    let byLayout = this.reads.get(shader);
    if (!byLayout) this.reads.set(shader, byLayout = new Map());
    let found = byLayout.get(layout.key);
    if (!found) byLayout.set(layout.key, found = attachmentReads(shader, layout));
    return found;
  }

  /**
   * Whether each texture of an atlas bake's evaluate pass is keyed by the inputs it reads alone (`attachmentReads`), so
   * a changed value draws only the textures it reaches. Off, every texture is keyed by every value (`?bakekeys=whole`
   * in the viewer, for comparing the two).
   */
  nodeKeys = true;

  /**
   * Whether a scene bake's maps hold every texture of its pass graph. Off (the default), they hold what a viewer shows
   * (base color, properties, normal, emissive, live arguments); the textures only other passes read (the height and
   * offset the normal is made from, a layered slot's planes and tiles) are the graph's to keep while its memory allows
   * (`NodeSpec.transient`), which is most of a layered scene's memory. On (`?bakekeep=all` in the viewer), every
   * texture stays in use for as long as its mesh is on screen, as before; for comparisons.
   */
  keepIntermediates = false;

  /**
   * The key of an INTRINSIC bake of a program: the program and its own inputs alone (source, parameter and seed values,
   * images, CPU fields) over `domain`, a rectangle of surface coordinates named by the caller. No receiver and no atlas
   * are in it, so every receiver whose mapping nodes read the intrinsic node (`mappingOp`) shares one texture, and a
   * receiver that changes shape draws only its mapping again.
   */
  static intrinsicKey(shading: MaterialShading, uniforms: Uniforms, domain: string, fieldIds: string[] = []): string {
    const own = [...shading.shader.parameters, ...shading.shader.seeds].map(entry => [entry.uniform, JSON.stringify(uniforms[entry.uniform]?.value)]);
    const images = shading.shader.images.map(image => `${image.name}:${image.linear}:${(uniforms[image.uniform]?.value as THREE.Texture | undefined)?.uuid}`);
    return `${GPU_PREVIEW_PROFILE}:${GPU_PREVIEW_VERSION}:intrinsic:${factsOf(shading.shader).hash}:${domain}:${hash(JSON.stringify([own, images, fieldIds]))}`;
  }

  /** The key of a mapping node: the receiver's atlas, how it maps points to surface coordinates, and where the intrinsic bake lies. */
  static mappingKey(atlas: { layout: AtlasLayout; extent: [number, number] }, uniforms: Uniforms, domain: IntrinsicDomain): string {
    return `mapping:1:${atlas.layout.key}:${hash(JSON.stringify([atlas.extent, uniforms.msMapping?.value, uniforms.msObjectFromWorld?.value, uniforms.msAnchorFromWorld?.value, domain]))}`;
  }

  /** Draws `scene` into a target (a band of its rows, or all of it), leaving the renderer as the viewer's frames expect it. */
  private renderInto(target: THREE.WebGLRenderTarget, scene: THREE.Scene, rows: { start: number; count: number } | null): void {
    if (this.batching) {
      target.scissorTest = rows !== null;
      if (rows) target.scissor.set(0, rows.start, target.width, rows.count);
      this.renderer.setRenderTarget(target);
      try { this.renderer.render(scene, this.camera); } finally { target.scissorTest = false; }
      return;
    }
    const before = this.renderer.getRenderTarget(), clear = this.renderer.autoClear;
    try {
      this.renderer.autoClear = false;
      target.scissorTest = rows !== null;
      if (rows) target.scissor.set(0, rows.start, target.width, rows.count);
      this.renderer.setRenderTarget(target);
      this.renderer.render(scene, this.camera);
    } finally {
      target.scissorTest = false;
      this.renderer.setRenderTarget(before);
      this.renderer.autoClear = clear;
    }
  }

  /** Programs of the graph's built-in operations (blur, mapping, a caller's own shader), by what they are compiled from. */
  private opPasses = new Map<string, Pass>();

  private opPass(key: string, vertex: string, fragment: string, geometry: THREE.BufferGeometry): Promise<Pass> {
    const cached = this.opPasses.get(key);
    if (cached) return Promise.resolve(cached);
    const waiting = this.compiling.get(key);
    if (waiting) return waiting;
    const material = new THREE.ShaderMaterial({ glslVersion: THREE.GLSL3, vertexShader: vertex, fragmentShader: fragment, uniforms: {},
      depthTest: false, depthWrite: false, blending: THREE.NoBlending, side: THREE.DoubleSide });
    const started: Promise<Pass> = this.compiled(material, this.compileTarget(), this.opPasses, key, geometry)
      .finally(() => { if (this.compiling.get(key) === started) this.compiling.delete(key); });
    this.compiling.set(key, started);
    return started;
  }

  /**
   * A graph operation that draws a whole target with one fragment shader (single color output `msOut` is the shader's to
   * declare). `uniforms` gives the shader's uniform values for a draw, from the node's inputs. Operations made from the
   * same shader text share one compiled program.
   */
  shaderOp(label: string, fragment: string, uniforms: (step: DrawStep) => Record<string, unknown>): NodeOp {
    let pass: Pass | undefined;
    return {
      label,
      prepare: async () => { pass = await this.opPass(`op:${hash(fragment)}:${fragment.length}`, VERTEX, fragment, this.geometry); },
      draw: step => {
        for (const [name, value] of Object.entries(uniforms(step))) pass!.material.uniforms[name] = { value };
        pass!.material.uniformsNeedUpdate = true;
        this.renderInto(step.node.target, pass!.scene, step.rows);
      },
    };
  }

  /** An operation that fills its node with one value. */
  fillOp(label: string, red: number, green: number, blue: number): NodeOp {
    return { label, draw: ({ node, first }) => { if (first) this.fill(node.target, red, green, blue); } };
  }

  /**
   * A separable Gaussian blur of any node, as two nodes: `<id>:rows` (the source blurred along its rows, kept at the
   * target's precision and read by texel) and `<id>` (that, blurred along its columns). `radius` is in texels, taps
   * beyond the edge repeat the edge texel. The blur knows nothing of charts: across an atlas it mixes neighbouring
   * charts' texels, so an atlas-space mask wants gutters at least `radius` wide or a chart-aware variant.
   */
  blurNodes(id: string, source: string, target: TargetSpec, radius: number, sigma = Math.max(radius, 1) / 2): NodeSpec[] {
    if (!(radius >= 0 && radius <= BLUR_MAX_RADIUS)) throw new Error(`Blur radius must be between 0 and ${BLUR_MAX_RADIUS} texels.`);
    const op = (x: number, y: number) => this.shaderOp("blur", BLUR_FRAGMENT, step =>
      ({ msSource: step.inputs.source.texture, msDirection: new THREE.Vector2(x, y), msRadius: Math.round(radius), msSigma: sigma }));
    return [
      { id: `${id}:rows`, key: `blur:1:rows:${radius}:${sigma}`, target: { ...target, filter: "nearest", name: `${target.name} (rows)` }, op: op(1, 0), inputs: { source } },
      { id, key: `blur:1:columns:${radius}:${sigma}`, target, op: op(0, 1), inputs: { source: `${id}:rows` } },
    ];
  }

  /**
   * The operation of a mapping node: the receiver's atlas drawn with every texel reading the node's `source` input, an
   * intrinsic bake, at the texel's surface coordinate (`ATLAS_MAPPING_FRAGMENT`). `uniforms` holds the receiver's
   * `msObjectFromWorld` and `msMapping`. Key the node with `mappingKey` and its source with `intrinsicKey`.
   */
  mappingOp(atlas: { layout: AtlasLayout; geometry: THREE.BufferGeometry; extent: [number, number] }, uniforms: Uniforms, domain: IntrinsicDomain): NodeOp {
    let pass: Pass | undefined;
    return {
      label: "mapping",
      prepare: async () => { pass = await this.opPass("atlas-mapping", ATLAS_VERTEX, ATLAS_MAPPING_FRAGMENT, this.atlasIdle); },
      draw: ({ node, inputs, rows }) => {
        Object.assign(pass!.material.uniforms, {
          msAtlas: { value: new THREE.Vector2(atlas.layout.width, atlas.layout.height) }, msGutter: { value: atlas.layout.gutter },
          msExtent: { value: new THREE.Vector2(atlas.extent[0], atlas.extent[1]) },
          msObjectFromWorld: uniforms.msObjectFromWorld ?? { value: new THREE.Matrix4() }, msMapping: uniforms.msMapping ?? { value: 2 },
          msAnchorFromWorld: uniforms.msAnchorFromWorld ?? { value: new THREE.Matrix4() },
          msIntrinsic: { value: inputs.source.texture }, msIntrinsicOrigin: { value: new THREE.Vector2(...domain.origin) },
          msIntrinsicSize: { value: new THREE.Vector2(...domain.size) }, msIntrinsicDensity: { value: domain.density },
        });
        pass!.material.uniformsNeedUpdate = true;
        pass!.mesh.geometry = atlas.geometry;
        try { this.renderInto(node.target, pass!.scene, rows); } finally { pass!.mesh.geometry = this.atlasIdle; }
      },
    };
  }

  private compileTarget(): THREE.WebGLRenderTarget {
    return this.atlasCompileTarget ??= new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false, stencilBuffer: false });
  }

  /**
   * The evaluate pass of an atlas bake as a graph operation: the program's bake shader, whose outputs (chosen by
   * `msOutput`, see `atlasFragment`) are the nodes of the program's attachments. `bind` gives the uniforms of the bake,
   * and is asked only when a node is drawn.
   */
  private atlasEvaluateOp(shading: MaterialShading, layout: AtlasProgramLayout, geometry: THREE.BufferGeometry, bind: () => Uniforms, holds: object[]): NodeOp {
    let pass: Pass | undefined, bound: Uniforms | undefined;
    return {
      label: "evaluate", pace: atlasPassKey(shading.shader, layout),
      prepare: async () => { pass = await this.atlasPass(shading.shader, layout, this.compileTarget()); this.hold(pass, holds); },
      draw: ({ node, output, rows }) => {
        // Assigned at every draw: another bake may have used the program since the last band.
        Object.assign(pass!.material.uniforms, bound ??= bind());
        pass!.material.uniforms.msOutput = { value: output };
        pass!.material.uniformsNeedUpdate = true;
        pass!.mesh.geometry = geometry;
        try { this.renderInto(node.target, pass!.scene, rows); } finally { pass!.mesh.geometry = this.atlasIdle; }
      },
    };
  }

  /** The normals pass of an atlas bake as a graph operation: it reads the `height` node and, bending, the `offset` node. */
  private atlasNormalsOp(atlas: { layout: AtlasLayout; geometry: THREE.BufferGeometry }, orient: boolean, bend: boolean): NodeOp {
    let pass: Pass | undefined;
    return {
      label: "normals",
      prepare: async () => { pass = await this.atlasNormals(this.compileTarget()); },
      draw: ({ node, inputs, rows, first }) => {
        const uniform = pass!.material.uniforms;
        uniform.msHeightMap.value = inputs.height.texture;
        uniform.msOffsetMap.value = (inputs.offset ?? inputs.height).texture;
        (uniform.msAtlas.value as THREE.Vector2).set(atlas.layout.width, atlas.layout.height);
        uniform.msGutter.value = atlas.layout.gutter;
        uniform.msOrient.value = orient; uniform.msBend.value = bend;
        pass!.material.uniformsNeedUpdate = true;
        // Texels no chart covers read as a flat normal when a mip or a filter tap reaches them.
        if (first && node.fresh) this.fill(node.target, 0.5, 0.5, 1);
        pass!.mesh.geometry = atlas.geometry;
        try { this.renderInto(node.target, pass!.scene, rows); } finally { pass!.mesh.geometry = this.atlasIdle; }
      },
    };
  }

  /**
   * The nodes of an atlas bake of `shading`: one per attachment of the program's layout, drawn by the evaluate
   * operation, and `normal`. Each evaluate node has a key of its own (`keyOf`): the program, the atlas and the values
   * of the inputs that texture reads, so a parameter that reaches only the roughness draws only the properties
   * texture. `normal` is keyed by the atlas and how the relief is adjusted, and otherwise by its inputs, so it is drawn
   * again only when the height or the offset is. A program without relief has a flat normal, which is one texture for
   * every such receiver of the same atlas size.
   */
  private atlasNodes(shading: MaterialShading, layout: AtlasProgramLayout, atlas: { layout: AtlasLayout; geometry: THREE.BufferGeometry },
    keyOf: (attachment: AtlasAttachment) => string, evaluate: NodeOp): NodeSpec[] {
    const { width, height } = atlas.layout;
    // The height and the offset are read by the normals pass alone: nothing shows them.
    const specs: NodeSpec[] = layout.attachments.map((name, output) =>
      ({ id: name, key: keyOf(name), target: GpuBaker.atlasTarget(name, width, height, layout), op: evaluate, output,
        ...(this.keepIntermediates || !INTERMEDIATE_ATTACHMENTS.has(name) ? {} : { transient: true }) }));
    const bend = layout.attachments.includes("offset");
    const flat = !shading.shader.outputs.includes("height") && !layout.orient && !bend;
    const target = GpuBaker.atlasTarget("normal", width, height, layout);
    specs.push(flat ? { id: "normal", key: "flat-normal:1", target, op: this.fillOp("normals", 0.5, 0.5, 1) }
      : { id: "normal", key: `atlas-normals:1:${atlas.layout.key}:${layout.orient}:${bend}`, target, op: this.atlasNormalsOp(atlas, layout.orient, bend),
        inputs: bend ? { height: "height", offset: "offset" } : { height: "height" } });
    return specs;
  }

  /**
   * Bakes `shading` over one receiver's atlas, in UV space, as a pass graph (`atlasNodes`): the evaluate pass draws
   * every chart with the program once per texture it writes, the normals pass turns the height into tangent-space
   * normals at the same resolution, and mips are built for everything a viewer samples. Only nodes no texture holds
   * are drawn. `uniforms` holds the program's parameter, seed and image uniforms and the receiver's own (`ms_features`,
   * bounds, gravity, `msObjectFromWorld`, `msMapping`). The result takes the place of `previous`, which is released
   * (its textures are drawn over where only it holds them, unless `inPlace` is false). A PREVIEW, like every bake here.
   * Returns null when `current` went false or the context was lost, and throws when the program cannot be baked;
   * `previous` is then still the caller's, and holds nothing a viewer should show if it was being drawn over
   * (`atlasMapsComplete`).
   */
  async bakeAtlas(shading: MaterialShading, uniforms: Uniforms, atlas: AtlasBakeInput,
    previous?: AtlasMaps, options: AtlasBakeOptions = {}): Promise<{ maps: AtlasMaps; timings: AtlasBakeTimings } | null> {
    this.busy++;
    const holds: object[] = [];
    try { return await this.bakeAtlasNow(shading, uniforms, atlas, previous, options, holds); }
    catch (error) { if (error instanceof Abandoned) return null; throw error; }
    finally { this.release(holds); this.busy--; this.trim(); }
  }

  private async bakeAtlasNow(shading: MaterialShading, uniforms: Uniforms, atlas: AtlasBakeInput,
    previous: AtlasMaps | undefined, options: AtlasBakeOptions, holds: object[]): Promise<{ maps: AtlasMaps; timings: AtlasBakeTimings } | null> {
    const { measure = false, bandTexels = (spec: NodeSpec) => this.bandTexelsFor(spec), fields = {} } = options;
    const reason = this.unavailable();
    if (reason) throw new Error(`GPU preview unavailable: ${reason}.`);
    const layout = atlas.program ?? this.atlasProgram(shading);
    const unsupported = gpuBakeUnsupported(shading, true) ?? layout.unsupported;
    if (unsupported) throw new Error(`GPU preview does not support this program: ${unsupported}.`);
    const { width, height } = atlas.layout;
    if (width < 1 || height < 1 || width > 4096 || height > 4096) throw new Error("GPU preview size is out of range.");
    const unitLimit = this.renderer.capabilities.maxTextures;
    const bakeUnits = factsOf(shading.shader).samplers;
    if (bakeUnits > unitLimit) throw new Error(`GPU preview needs ${bakeUnits} texture units to bake; this device has ${unitLimit}.`);
    const passKey = atlasPassKey(shading.shader, layout);
    const hadPass = this.atlasPasses.has(passKey);
    const fieldIds: string[] = [];
    for (const wanted of shading.shader.cpuFields ?? []) {
      const field = fields[wanted.uniform];
      if (!field) throw new Error(`GPU preview needs the CPU field ${wanted.name}.`);
      if (field.width !== width || field.height !== height) throw new Error("CPU field and atlas sizes differ.");
      fieldIds.push(`${wanted.name}=${field.id}`);
    }
    const bind = (): Uniforms => {
      const bound: Uniforms = { ...uniforms,
        msAtlas: { value: new THREE.Vector2(width, height) }, msGutter: { value: atlas.layout.gutter },
        msExtent: { value: new THREE.Vector2(atlas.extent[0], atlas.extent[1]) } };
      for (const wanted of shading.shader.cpuFields ?? []) bound[wanted.uniform] = { value: this.fieldTexture(fields[wanted.uniform]) };
      return bound;
    };
    const key = GpuBaker.atlasKey(shading, uniforms, atlas.layout, layout, fieldIds, atlas.receiver);
    // Each texture's key: the bake's, with the input values narrowed to what the texture reads where that is known.
    const reads = this.nodeKeys ? this.atlasReads(shading.shader, layout) : {};
    const program = GpuBaker.atlasProgramKey(shading, atlas.layout, layout);
    const keyOf = (name: AtlasAttachment): string =>
      `${program}:${GpuBaker.atlasInputsKey(shading, uniforms, fieldIds, atlas.receiver, reads[name])}:${name}`;
    const specs = this.atlasNodes(shading, layout, atlas, keyOf, this.atlasEvaluateOp(shading, layout, atlas.geometry, bind, holds));
    // Atlas bakes run one at a time (the scene's baker awaits each), so a program's geometry and uniforms stay one
    // receiver's between the bands of a level.
    const result = await this.graph.realize(specs, previous?.nodes, {
      current: options.current, measure, bandTexels, inPlace: options.inPlace, allocate: options.allocate, beforeOverwrite: options.beforeOverwrite,
      // A value that changed while the bands were drawn is in some texels and not others.
      stable: () => GpuBaker.atlasKey(shading, uniforms, atlas.layout, layout, fieldIds, atlas.receiver) === key,
    });
    if (!result) return null;
    const nodes = result.instance;
    if (this.atlasPasses.has(passKey) && result.drawn.some(id => id !== "normal")) this.drawn.add(passKey);
    const live: Record<string, THREE.Texture> = {};
    for (const name of layout.attachments) if (name.startsWith("live")) live[`gpu-live-${name.slice(4)}`] = nodes.texture(name)!;
    const maps: AtlasMaps = { profile: GPU_PREVIEW_PROFILE, key, width, height, layout, nodes, baseColor: nodes.texture("base")!,
      properties: nodes.texture("properties")!, emissive: nodes.texture("emissive"), normal: nodes.texture("normal")!, live };
    const { timings } = result;
    return { maps, timings: { compile: hadPass ? 0 : timings.prepare, evaluate: timings.draw.evaluate ?? 0, normals: timings.draw.normals ?? 0, mips: timings.mips,
      total: timings.total, bands: result.bands, drawn: result.drawn, reused: result.reused, units: { bake: bakeUnits, limit: unitLimit } } };
  }

  // ---- Weathering stacks (layered slots) ----

  /** Programs of stack bakes (planes, intrinsic tiles, lattice mappings, composition), by what they are compiled from. */
  private planePasses = new Map<string, Pass>();
  /** Bound where a stack program's input is absent for the node being drawn (that output never reads it). */
  private blankTexture?: THREE.DataTexture;
  /**
   * Whether a stack's base reads its intrinsic outputs from shared lattice tiles where a receiver's charts all lie on
   * its pattern anchor's lattice (`?intrinsic=off` in the viewer evaluates them per receiver, for comparisons).
   */
  intrinsicTiles = true;

  private blank(): THREE.Texture {
    if (this.blankTexture) return this.blankTexture;
    const texture = new THREE.DataTexture(new Float32Array(4), 1, 1, THREE.RGBAFormat, THREE.FloatType);
    texture.minFilter = texture.magFilter = THREE.NearestFilter;
    texture.generateMipmaps = false;
    texture.needsUpdate = true;
    return this.blankTexture = texture;
  }

  private async planePass(key: string, vertex: string, fragment: string, geometry: THREE.BufferGeometry): Promise<Pass> {
    const cached = this.planePasses.get(key);
    if (cached) { this.planePasses.delete(key); this.planePasses.set(key, cached); return cached; }
    const waiting = this.compiling.get(key);
    if (waiting) return waiting;
    const material = new THREE.ShaderMaterial({ glslVersion: THREE.GLSL3, vertexShader: vertex, fragmentShader: fragment, uniforms: {},
      depthTest: false, depthWrite: false, blending: THREE.NoBlending, side: THREE.DoubleSide });
    const started: Promise<Pass> = this.compiled(material, this.compileTarget(), this.planePasses, key, geometry)
      .finally(() => { if (this.compiling.get(key) === started) this.compiling.delete(key); });
    this.compiling.set(key, started);
    return started;
  }

  /** The texture of one channel's plane: the value unencoded, read by texel (`PLANE_VALUES`). */
  private static planeTarget(channel: string, width: number, height: number, name = `gpu-plane ${channel}`): TargetSpec {
    const vector = channel === "base_color" || channel === "emissive" || channel === "normal_offset";
    // Heights in single precision, like the atlas bake's (the relief differences them); the rest in half floats.
    return { width, height, format: vector ? THREE.RGBAFormat : THREE.RedFormat, type: channel === "height" ? THREE.FloatType : THREE.HalfFloatType,
      colorSpace: THREE.NoColorSpace, filter: "nearest", wrap: THREE.ClampToEdgeWrapping, name };
  }

  /**
   * How the maps of a layered slot's bake are laid out (`bakeStack`): the result's textures, by the channels the stack
   * has (`StackInput.channels`), and the base's live renderer arguments, which layers never change. Like
   * `atlasProgram`, it is what the material showing the maps is made for.
   */
  stackProgram(base: MaterialShading, stack: StackInput): AtlasProgramLayout {
    const own = this.atlasProgram(base);
    const attachments: AtlasAttachment[] = ["base", "properties", "height"];
    if (stack.channels.emissive) attachments.push("emissive");
    if (stack.channels.normal_offset) attachments.push("offset");
    attachments.push(...own.attachments.filter(name => name.startsWith("live")));
    return { ...own, key: `stack:${attachments.join("+")}:${own.key}`, attachments, orient: false, unsupported: own.unsupported ?? stackUnsupported(base, stack) ?? undefined };
  }

  /**
   * Starts compiling the programs a stack bake draws with (see `precompileAtlas`): each material's plane program as
   * `bakeStack` makes it (`stackPlanes`), the base's intrinsic tile program, the composition and the normals.
   */
  precompileStack(base: MaterialShading, stack: StackInput): void {
    if (this.unavailable() || gpuBakeUnsupported(base, true) || stackUnsupported(base, stack)) return;
    const planes = stackPlanes(base, stack);
    const compile = (key: string, vertex: string, fragment: string, geometry: THREE.BufferGeometry) =>
      void this.planePass(`${key}:${shaderIdentity(base.shader)}:${hash(fragment)}:${fragment.length}`, vertex, fragment, geometry).catch(() => {});
    const own = this.atlasProgram(base);
    const baseChannels = planes.get("stack.base") ?? [];
    try {
      // A receiver off its anchor's lattice evaluates every plane of the base; one on it, the rest beside the tiles.
      compile("plane", ATLAS_VERTEX, planeFragment(base.shader.source, [...baseChannels.map(channel => PLANE_VALUES[channel]), ...liveValues(own)], []), this.atlasIdle);
      const tiled = this.intrinsicTiles ? baseChannels.filter(channel => outputIntrinsic(base.shader, channel)) : [];
      if (tiled.length) compile("tile", VERTEX, intrinsicTileFragment(base.shader.source, tiled.map(channel => PLANE_VALUES[channel])), this.geometry);
      for (const layer of stack.layers) {
        const channels = planes.get(layer.id) ?? [];
        if (!channels.length) continue;
        const below = (layer.shading.shader.cpuFields ?? []).filter(field => field.name.startsWith("below.")).map(field => field.uniform);
        const fragment = planeFragment(layer.shading.shader.source, channels.map(channel => PLANE_VALUES[channel]), below);
        void this.planePass(`plane:${shaderIdentity(layer.shading.shader)}:${hash(fragment)}:${fragment.length}`, ATLAS_VERTEX, fragment, this.atlasIdle).catch(() => {});
      }
    } catch { /* the bake reports it */ }
    void this.planePass("stack-compose", ATLAS_VERTEX, STACK_COMPOSE_FRAGMENT, this.atlasIdle).catch(() => {});
    void this.planePass("stack-lattice", ATLAS_VERTEX, LATTICE_MAPPING_FRAGMENT, this.atlasIdle).catch(() => {});
    void this.atlasNormals(this.compileTarget()).catch(() => {});
  }

  /**
   * Bakes a layered slot over one receiver's atlas as a pass graph of PLANES, the GPU preview of the CPU's stack
   * (`Request::layers`; see "Layered slots" in the program README). Each material of the stack (`stack.base`, then each
   * layer) draws a plane node per channel it gives, holding the value unencoded; a layer's `below.<channel>` reads the
   * plane of the material beneath that last changed the channel (`StackLayer.reads`), or the channel's default where
   * none did. A plane is keyed by its material's program, what its value reads of that material's parameters, seeds,
   * images and CPU fields, the receiver's atlas and the features the material queries; its resolved key adds the
   * planes it reads. So editing, adding or removing a layer draws only that layer's planes and those above that read
   * them, and the base is drawn once under any stack. The result's maps are composed from the planes of the materials
   * that last changed each channel; the normals pass reads the height and offset planes.
   *
   * The base's INTRINSIC outputs (`Reads.intrinsic`) are drawn into shared lattice tiles when every chart of the
   * receiver lies on its pattern anchor's lattice (`latticeTiles`): a tile is keyed by the lattice and the values it
   * reads alone, so another receiver over the same stretch of the pattern, and the same receiver resized, read the
   * tiles already drawn, through a cheap per-receiver mapping that copies the texel of each lattice cell.
   *
   * `fields` holds the CPU fields of the base's plan by sampler uniform and of each layer's by `<layer id>/<uniform>`
   * (all but its `below.*` fields). Returns and throws as `bakeAtlas` does; `atlas.program` is `stackProgram`'s layout.
   */
  async bakeStack(base: MaterialShading, uniforms: Uniforms, stack: StackInput, atlas: AtlasBakeInput,
    previous?: AtlasMaps, options: AtlasBakeOptions = {}): Promise<{ maps: AtlasMaps; timings: AtlasBakeTimings } | null> {
    this.busy++;
    const holds: object[] = [];
    try { return await this.bakeStackNow(base, uniforms, stack, atlas, previous, options, holds); }
    catch (error) { if (error instanceof Abandoned) return null; throw error; }
    finally { this.release(holds); this.busy--; this.trim(); }
  }

  private async bakeStackNow(base: MaterialShading, uniforms: Uniforms, stack: StackInput, atlas: AtlasBakeInput,
    previous: AtlasMaps | undefined, options: AtlasBakeOptions, holds: object[]): Promise<{ maps: AtlasMaps; timings: AtlasBakeTimings } | null> {
    const { measure = false, bandTexels = (spec: NodeSpec) => this.bandTexelsFor(spec), fields = {} } = options;
    const reason = this.unavailable();
    if (reason) throw new Error(`GPU preview unavailable: ${reason}.`);
    const composite = atlas.program ?? this.stackProgram(base, stack);
    const own = this.atlasProgram(base);
    // (A layer need not give a base color: what it does not give passes through.)
    const unsupported = gpuBakeUnsupported(base, true) ?? composite.unsupported ?? stackUnsupported(base, stack);
    if (unsupported) throw new Error(`GPU preview does not support this program: ${unsupported}.`);
    const { width, height } = atlas.layout;
    if (width < 1 || height < 1 || width > 4096 || height > 4096) throw new Error("GPU preview size is out of range.");
    const unitLimit = this.renderer.capabilities.maxTextures;
    const bakeUnits = Math.max(factsOf(base.shader).samplers, ...stack.layers.map(layer => factsOf(layer.shading.shader).samplers), MAX_TILES);
    if (bakeUnits > unitLimit) throw new Error(`GPU preview needs ${bakeUnits} texture units to bake; this device has ${unitLimit}.`);
    const profile = `${GPU_PREVIEW_PROFILE}:${GPU_PREVIEW_VERSION}`;
    // The CPU fields of each material, as their keys name them; a layer's `below.*` fields are planes of this graph.
    const fieldIdsOf = (shading: MaterialShading, prefix: string): string[] => (shading.shader.cpuFields ?? []).map(field => {
      if (prefix && field.name.startsWith("below.")) return `${field.name}=plane`;
      const found = fields[`${prefix}${field.uniform}`];
      if (!found) throw new Error(`GPU preview needs the CPU field ${prefix}${field.name}.`);
      if (found.width !== width || found.height !== height) throw new Error("CPU field and atlas sizes differ.");
      return `${field.name}=${found.id}`;
    });
    const bindFor = (shading: MaterialShading, own: Uniforms, prefix: string) => (): Uniforms => {
      const bound: Uniforms = { ...own, msAtlas: { value: new THREE.Vector2(width, height) }, msGutter: { value: atlas.layout.gutter },
        msExtent: { value: new THREE.Vector2(atlas.extent[0], atlas.extent[1]) } };
      for (const field of shading.shader.cpuFields ?? []) {
        if (!(prefix && field.name.startsWith("below."))) bound[field.uniform] = { value: this.fieldTexture(fields[`${prefix}${field.uniform}`]) };
      }
      return bound;
    };
    const planes = stackPlanes(base, stack);
    const planeId = (producer: string, channel: string) => `${producer}/${channel}`;
    const signatures: string[] = [];
    const before: NodeSpec[] = [], after: NodeSpec[] = [];
    const passKeys: string[] = [];

    // The base: intrinsic channels from lattice tiles where the receiver allows it, the rest (and its live arguments)
    // evaluated over the atlas.
    const baseFieldIds = fieldIdsOf(base, "");
    signatures.push(GpuBaker.atlasKey(base, uniforms, atlas.layout, own, baseFieldIds, atlas.receiver));
    const baseHash = factsOf(base.shader).hash;
    const baseChannels = planes.get("stack.base") ?? [];
    const lattice = this.intrinsicTiles ? latticeTiles(atlas.layout) : null;
    const tiled = lattice ? baseChannels.filter(channel => outputIntrinsic(base.shader, channel)) : [];
    const evaluated = baseChannels.filter(channel => !tiled.includes(channel));
    const live = own.attachments.filter(name => name.startsWith("live"));
    const bindBase = bindFor(base, uniforms, "");
    if (tiled.length) {
      const { step, tiles } = lattice!;
      const fragment = intrinsicTileFragment(base.shader.source, tiled.map(channel => PLANE_VALUES[channel]));
      const passKey = `tile:${shaderIdentity(base.shader)}:${hash(fragment)}:${fragment.length}`;
      passKeys.push(passKey);
      const origins: [number, number][] = [];
      for (const [i, j] of tiles) {
        const origin: [number, number] = [TILE_START + i * TILE_CELLS, TILE_START + j * TILE_CELLS];
        origins.push(origin);
        const op = this.tileOp(passKey, fragment, origin, step, bindBase, holds);
        tiled.forEach((channel, output) => before.push({ id: `tile/${i},${j}/${channel}`, op, output,
          key: `${profile}:tile:${baseHash}:${channel}:${step}:${origin.join(",")}:${TILE_CELLS}:${GpuBaker.ownValuesKey(base, uniforms, outputReads(base.shader, [channel]))}`,
          target: GpuBaker.planeTarget(channel, TILE_CELLS, TILE_CELLS, `gpu-tile ${channel}`) }));
      }
      const op = this.latticeOp(atlas, origins);
      for (const channel of tiled) {
        before.push({ id: planeId("stack.base", channel), key: `lattice:1:${atlas.layout.key}:${channel}`, op, target: GpuBaker.planeTarget(channel, width, height),
          inputs: Object.fromEntries(tiles.map(([i, j], index) => [`tile${index}`, `tile/${i},${j}/${channel}`])) });
      }
    }
    if (evaluated.length || live.length) {
      const values = [...evaluated.map(channel => PLANE_VALUES[channel]), ...liveValues(own)];
      const fragment = planeFragment(base.shader.source, values, []);
      const passKey = `plane:${shaderIdentity(base.shader)}:${hash(fragment)}:${fragment.length}`;
      passKeys.push(passKey);
      const op = this.stackEvaluateOp("base", passKey, fragment, atlas.geometry, bindBase, holds, []);
      const liveReads = this.nodeKeys ? this.atlasReads(base.shader, own) : {};
      evaluated.forEach((channel, output) => before.push({ id: planeId("stack.base", channel), op, output, target: GpuBaker.planeTarget(channel, width, height),
        key: `${profile}:plane:${baseHash}:${atlas.layout.key}:${channel}:${GpuBaker.atlasInputsKey(base, uniforms, baseFieldIds, atlas.receiver, this.nodeKeys ? outputReads(base.shader, [channel]) : undefined)}` }));
      live.forEach((name, index) => before.push({ id: name, op, output: evaluated.length + index, target: GpuBaker.atlasTarget(name, width, height, own),
        key: `${profile}:stack-live:${baseHash}:${own.key}:${atlas.layout.key}:${name}:${GpuBaker.atlasInputsKey(base, uniforms, baseFieldIds, atlas.receiver, liveReads[name])}` }));
    }

    // Each layer, over the planes beneath it.
    const fallbacks = new Set<string>();
    for (const layer of stack.layers) {
      const shader = layer.shading.shader;
      const channels = planes.get(layer.id) ?? [];
      const fieldIds = fieldIdsOf(layer.shading, `${layer.id}/`);
      signatures.push(GpuBaker.atlasKey(layer.shading, layer.uniforms, atlas.layout, own, fieldIds, layer.receiver));
      if (!channels.length) continue;
      const below = (shader.cpuFields ?? []).filter(field => field.name.startsWith("below."));
      const fragment = planeFragment(shader.source, channels.map(channel => PLANE_VALUES[channel]), below.map(field => field.uniform));
      const passKey = `plane:${shaderIdentity(shader)}:${hash(fragment)}:${fragment.length}`;
      passKeys.push(passKey);
      const op = this.stackEvaluateOp("layer", passKey, fragment, atlas.geometry, bindFor(layer.shading, layer.uniforms, `${layer.id}/`), holds, below.map(field => field.uniform));
      const beneath = (field: (typeof below)[number]): string => {
        const channel = field.name.slice("below.".length), producer = layer.reads[`base.${channel}`];
        if (producer) return planeId(producer, channel);
        fallbacks.add(channel);
        return `fallback/${channel}`;
      };
      const hashed = factsOf(shader).hash;
      channels.forEach((channel, output) => {
        // The planes beneath that this channel's value reads (all of them where the record does not say).
        const reads = outputCpuFields(shader, channel);
        const inputs = Object.fromEntries(below.filter(field => !reads || reads.includes(field.name)).map(field => [field.uniform, beneath(field)]));
        after.push({ id: planeId(layer.id, channel), op, output, inputs, target: GpuBaker.planeTarget(channel, width, height),
          key: `${profile}:plane:${hashed}:${atlas.layout.key}:${channel}:${GpuBaker.atlasInputsKey(layer.shading, layer.uniforms, fieldIds, layer.receiver, this.nodeKeys ? outputReads(shader, [channel]) : undefined)}` });
      });
    }

    // The result: maps composed from the planes of the materials that last changed each channel, and the normals.
    const produced = (channel: string) => stack.channels[channel] && planes.get(stack.channels[channel])?.includes(channel) ? planeId(stack.channels[channel], channel) : undefined;
    const color = produced("base_color"), roughness = produced("roughness"), metalness = produced("metalness"), emissive = produced("emissive");
    const offset = produced("normal_offset");
    let heightId = produced("height");
    if (!heightId) { fallbacks.add("height"); heightId = "fallback/height"; }
    const compose = this.composeOp(atlas);
    after.push({ id: "base", key: `stack-compose:1:${atlas.layout.key}:base`, op: compose, output: 0, target: GpuBaker.atlasTarget("base", width, height, composite),
      inputs: color ? { color } : {} });
    after.push({ id: "properties", key: `stack-compose:1:${atlas.layout.key}:properties`, op: compose, output: 1, target: GpuBaker.atlasTarget("properties", width, height, composite),
      inputs: { ...(roughness ? { roughness } : {}), ...(metalness ? { metalness } : {}) } });
    if (emissive) after.push({ id: "emissive", key: `stack-compose:1:${atlas.layout.key}:emissive`, op: compose, output: 2, target: GpuBaker.atlasTarget("emissive", width, height, composite), inputs: { emissive } });
    const normalTarget = GpuBaker.atlasTarget("normal", width, height, composite);
    after.push(!stack.channels.height && !offset ? { id: "normal", key: "flat-normal:1", target: normalTarget, op: this.fillOp("normals", 0.5, 0.5, 1) }
      : { id: "normal", key: `atlas-normals:1:${atlas.layout.key}:false:${!!offset}`, target: normalTarget, op: this.atlasNormalsOp(atlas, false, !!offset),
        inputs: offset ? { height: heightId, offset } : { height: heightId } });
    // Channels no material beneath a layer gives: the default its `below.<channel>` falls back to.
    const fills: Record<string, [number, number, number]> = { base_color: [0.5, 0.5, 0.5], roughness: [1, 0, 0], opacity: [1, 0, 0] };
    const defaults: NodeSpec[] = [...fallbacks].map(channel => ({ id: `fallback/${channel}`, key: `stack-fallback:1:${channel}`,
      target: GpuBaker.planeTarget(channel, width, height), op: this.fillOp("fallback", ...(fills[channel] ?? [0, 0, 0])) }));
    const specs = [...before, ...defaults, ...after];
    // The planes, tiles and fallbacks are read by the composition and the normals pass alone: nothing shows them.
    if (!this.keepIntermediates) {
      const shown = new Set<string>(["base", "properties", "emissive", "normal", ...live]);
      for (const spec of specs) if (!shown.has(spec.id)) spec.transient = true;
    }
    const hadPasses = passKeys.every(key => this.planePasses.has(key));
    const signature = signatures.join("|");
    const result = await this.graph.realize(specs, previous?.nodes, {
      current: options.current, measure, bandTexels, inPlace: options.inPlace, allocate: options.allocate, beforeOverwrite: options.beforeOverwrite,
      // A value that changed while the bands were drawn is in some texels and not others.
      stable: () => [GpuBaker.atlasKey(base, uniforms, atlas.layout, own, baseFieldIds, atlas.receiver),
        ...stack.layers.map(layer => GpuBaker.atlasKey(layer.shading, layer.uniforms, atlas.layout, own, fieldIdsOf(layer.shading, `${layer.id}/`), layer.receiver))].join("|") === signature,
    });
    if (!result) return null;
    const nodes = result.instance;
    const liveTextures: Record<string, THREE.Texture> = {};
    for (const name of live) liveTextures[`gpu-live-${name.slice(4)}`] = nodes.texture(name)!;
    const maps: AtlasMaps = { profile: GPU_PREVIEW_PROFILE, key: `stack:${hash(signature)}:${composite.key}:${atlas.layout.key}`, width, height, layout: composite, nodes,
      baseColor: nodes.texture("base")!, properties: nodes.texture("properties")!,
      emissive: emissive ? nodes.texture("emissive") : undefined, normal: nodes.texture("normal")!, live: liveTextures };
    const { timings } = result;
    const evaluate = ["base", "layer", "tile", "lattice", "compose", "fallback"].reduce((sum, label) => sum + (timings.draw[label] ?? 0), 0);
    return { maps, timings: { compile: hadPasses ? 0 : timings.prepare, evaluate, normals: timings.draw.normals ?? 0, mips: timings.mips,
      total: timings.total, bands: result.bands, drawn: result.drawn, reused: result.reused, units: { bake: bakeUnits, limit: unitLimit } } };
  }

  /** The values of a program's own uniforms that `reads` names (all of them when it is undefined), for an intrinsic tile's key. */
  private static ownValuesKey(shading: MaterialShading, uniforms: Uniforms, reads: ReadonlySet<string> | undefined): string {
    const own = [...shading.shader.parameters, ...shading.shader.seeds].filter(entry => !reads || reads.has(entry.uniform))
      .map(entry => [entry.uniform, JSON.stringify(uniforms[entry.uniform]?.value)]);
    const images = shading.shader.images.filter(image => !reads || reads.has(image.uniform))
      .map(image => `${image.name}:${image.linear}:${(uniforms[image.uniform]?.value as THREE.Texture | undefined)?.uuid}`);
    return hash(JSON.stringify([own, images]));
  }

  /** The evaluate pass of one material of a stack: its plane program over the receiver's atlas, `below` bound to the planes beneath. */
  private stackEvaluateOp(label: string, key: string, fragment: string, geometry: THREE.BufferGeometry, bind: () => Uniforms, holds: object[], below: string[]): NodeOp {
    let pass: Pass | undefined, bound: Uniforms | undefined;
    return {
      label, pace: key,
      prepare: async () => { pass = await this.planePass(key, ATLAS_VERTEX, fragment, this.atlasIdle); this.hold(pass, holds); },
      draw: ({ node, output, rows, inputs }) => {
        // Assigned at every draw: another bake may have used the program since the last band.
        const uniform = pass!.material.uniforms;
        Object.assign(uniform, bound ??= bind());
        for (const role of below) uniform[role] = { value: inputs[role]?.texture ?? this.blank() };
        uniform.msOutput = { value: output };
        pass!.material.uniformsNeedUpdate = true;
        pass!.mesh.geometry = geometry;
        try { this.renderInto(node.target, pass!.scene, rows); } finally { pass!.mesh.geometry = this.atlasIdle; }
      },
    };
  }

  /** The draw of one intrinsic tile: the base's intrinsic outputs over `TILE_CELLS` square lattice cells from `origin`. */
  private tileOp(key: string, fragment: string, origin: [number, number], step: number, bind: () => Uniforms, holds: object[]): NodeOp {
    let pass: Pass | undefined, bound: Uniforms | undefined;
    return {
      label: "tile", pace: key,
      prepare: async () => { pass = await this.planePass(key, VERTEX, fragment, this.geometry); this.hold(pass, holds); },
      draw: ({ node, output, rows }) => {
        const uniform = pass!.material.uniforms;
        Object.assign(uniform, bound ??= bind());
        uniform.msTileOrigin = { value: new THREE.Vector2(...origin) };
        uniform.msStep = { value: step };
        uniform.msOutput = { value: output };
        pass!.material.uniformsNeedUpdate = true;
        this.renderInto(node.target, pass!.scene, rows);
      },
    };
  }

  /** The draw of a lattice mapping: each texel of a receiver copies its lattice cell from the tiles (`tile0`, ... inputs) at `origins`. */
  private latticeOp(atlas: { layout: AtlasLayout; geometry: THREE.BufferGeometry }, origins: [number, number][]): NodeOp {
    let pass: Pass | undefined;
    return {
      label: "lattice",
      prepare: async () => { pass = await this.planePass("stack-lattice", ATLAS_VERTEX, LATTICE_MAPPING_FRAGMENT, this.atlasIdle); },
      draw: ({ node, inputs, rows }) => {
        const uniform = pass!.material.uniforms;
        uniform.msAtlas = { value: new THREE.Vector2(atlas.layout.width, atlas.layout.height) };
        uniform.msGutter = { value: atlas.layout.gutter };
        uniform.msTiles = { value: origins.length };
        uniform.msTileOrigins = { value: Array.from({ length: MAX_TILES }, (_, index) => new THREE.Vector2(...(origins[index] ?? [0, 0]))) };
        for (let index = 0; index < MAX_TILES; index++) uniform[`msTile${index}`] = { value: inputs[`tile${index}`]?.texture ?? this.blank() };
        pass!.material.uniformsNeedUpdate = true;
        pass!.mesh.geometry = atlas.geometry;
        try { this.renderInto(node.target, pass!.scene, rows); } finally { pass!.mesh.geometry = this.atlasIdle; }
      },
    };
  }

  /** The draw of a stack's result maps from its planes (`STACK_COMPOSE_FRAGMENT`): output 0 base color, 1 properties, 2 emissive. */
  private composeOp(atlas: { layout: AtlasLayout; geometry: THREE.BufferGeometry }): NodeOp {
    let pass: Pass | undefined;
    return {
      label: "compose",
      prepare: async () => { pass = await this.planePass("stack-compose", ATLAS_VERTEX, STACK_COMPOSE_FRAGMENT, this.atlasIdle); },
      draw: ({ node, output, inputs, rows }) => {
        const uniform = pass!.material.uniforms;
        uniform.msAtlas = { value: new THREE.Vector2(atlas.layout.width, atlas.layout.height) };
        for (const [role, name] of [["color", "Color"], ["roughness", "Roughness"], ["metalness", "Metalness"], ["emissive", "Emissive"]]) {
          uniform[`ms${name}`] = { value: inputs[role]?.texture ?? this.blank() };
          uniform[`msHas${name}`] = { value: inputs[role] !== undefined };
        }
        uniform.msOutput = { value: output };
        pass!.material.uniformsNeedUpdate = true;
        pass!.mesh.geometry = atlas.geometry;
        try { this.renderInto(node.target, pass!.scene, rows); } finally { pass!.mesh.geometry = this.atlasIdle; }
      },
    };
  }

  /** The base color and normal texels of the last bake, for tests and diagnostics (not for display). */
  readBack(bake: GpuBake): { baseColor: Uint8Array; normal: Uint8Array; height: Float32Array; properties: Uint8Array } {
    const nodes = this.swatches.get(`${bake.width}x${bake.height}`)?.instance;
    if (!nodes) throw new Error("This bake's textures were replaced.");
    const count = bake.width * bake.height;
    const baseColor = new Uint8Array(count * 4), normal = new Uint8Array(count * 4), height = new Float32Array(count * 4);
    const read = (id: string, into: Uint8Array | Float32Array) => this.renderer.readRenderTargetPixels(nodes.node(id)!.target, 0, 0, bake.width, bake.height, into);
    // The base color attachment decodes sRGB on read in some drivers; reading it as bytes returns the stored encoding.
    read("base color", baseColor); read("normal", normal); read("height", height);
    const properties = new Uint8Array(count * 4);
    read("properties", properties);
    return { baseColor, normal, properties, height: height.filter((_, index) => index % 4 === 0) };
  }

  /** Minimum, maximum and mean of each argument of a bake's live textures (tests and diagnostics, not for display). */
  readLive(bake: GpuBake): { min: number; max: number; mean: number }[] {
    if (!bake.live) return [];
    const half = (bits: number): number => {
      const sign = bits & 0x8000 ? -1 : 1, exponent = (bits >> 10) & 0x1f, fraction = bits & 0x3ff;
      if (exponent === 0) return sign * 2 ** -14 * (fraction / 1024);
      if (exponent === 31) return fraction ? NaN : sign * Infinity;
      return sign * 2 ** (exponent - 15) * (1 + fraction / 1024);
    };
    const found: { min: number; max: number; mean: number }[] = [];
    // The bake's own live textures, in texture order.
    const nodes = this.drewInto.get(bake)?.instance;
    for (let index = 0; nodes?.node(`live${index}`); index++) {
      const target = nodes.node(`live${index}`)!.target;
      const bits = new Uint16Array(target.width * target.height * 4);
      this.renderer.readRenderTargetPixels(target, 0, 0, target.width, target.height, bits);
      for (let channel = 0; channel < 4; channel++) {
        let min = Infinity, max = -Infinity, sum = 0;
        for (let index = channel; index < bits.length; index += 4) { const value = half(bits[index]); min = Math.min(min, value); max = Math.max(max, value); sum += value; }
        found.push({ min, max, mean: sum / (bits.length / 4) });
      }
    }
    return found;
  }

  dispose(): void {
    this.renderer.domElement.removeEventListener("webglcontextlost", this.contextLost);
    this.renderer.domElement.removeEventListener("webglcontextrestored", this.contextRestored);
    for (const pass of this.passes.values()) pass.material.dispose();
    this.normalPass?.material.dispose();
    for (const pass of this.atlasPasses.values()) pass.material.dispose();
    this.atlasPasses.clear();
    this.atlasNormalPass?.material.dispose(); this.atlasNormalPass = undefined;
    for (const pass of this.opPasses.values()) pass.material.dispose();
    this.opPasses.clear();
    for (const pass of this.planePasses.values()) pass.material.dispose();
    this.planePasses.clear();
    this.blankTexture?.dispose(); this.blankTexture = undefined;
    this.atlasCompileTarget?.dispose(); this.atlasCompileTarget = undefined;
    this.failureCheck?.pass.material.dispose(); this.failureCheck?.target.dispose(); this.failureCheck = undefined;
    this.disposeTargets();
    // After the swatches gave up their nodes: the graph frees what no one holds.
    this.graph.dispose();
    this.geometry.dispose(); this.atlasIdle.dispose();
    this.passes.clear();
    this.normalPass = undefined;
  }
}

/**
 * The planes a stack bake draws, by stack material (`stack.base`, `stack.<k>`): the channels something reads (each
 * channel of the result, and each a layer reads beneath it), in the order of the material's outputs, so a material's
 * plane program is the same text whoever makes it (`bakeStack`, `precompileStack`). The maps store no opacity, like
 * an atlas bake's, so it is drawn only for a layer that reads it.
 */
function stackPlanes(base: MaterialShading, stack: StackInput): Map<string, string[]> {
  const wanted = new Map<string, Set<string>>();
  const want = (producer: string, channel: string) => { if (!wanted.has(producer)) wanted.set(producer, new Set()); wanted.get(producer)!.add(channel); };
  for (const [channel, producer] of Object.entries(stack.channels)) if (STACK_CHANNELS.includes(channel) && channel !== "opacity") want(producer, channel);
  for (const layer of stack.layers) for (const [input, producer] of Object.entries(layer.reads)) want(producer, input.slice("base.".length));
  const planes = new Map<string, string[]>();
  for (const [id, shading] of [["stack.base", base] as const, ...stack.layers.map(layer => [layer.id, layer.shading] as const)]) {
    planes.set(id, shading.shader.outputs.filter(output => wanted.get(id)?.has(output) && PLANE_VALUES[output]));
  }
  return planes;
}

/** What the evaluate pass writes into each live argument texture of a program's layout, as `atlasFragment` writes it. */
function liveValues(layout: AtlasProgramLayout): string[] {
  return layout.attachments.filter(name => name.startsWith("live")).map(name => {
    const first = Number(name.slice(4)) * 4;
    return `vec4(${[0, 1, 2, 3].map(channel => layout.liveChannels[first + channel] ?? "0.0").join(", ")})`;
  });
}

/** Why a layered slot cannot be composed on the GPU, or null (see `GpuBaker.bakeStack`). */
export function stackUnsupported(base: MaterialShading, stack: StackInput): string | null {
  if (!stack.layers.length) return "the slot has no layers";
  for (const [index, layer] of stack.layers.entries()) {
    if (!layer.shading.shader.source) return `weathering layer ${layer.id} has no GPU plan`;    if (layer.shading.shader.live.length) return `weathering layer ${layer.id} has live operations`;
    const unknown = (layer.shading.shader.cpuFields ?? []).find(field => field.name.startsWith("below.") && !PLANE_VALUES[field.name.slice("below.".length)]);
    if (unknown) return `weathering layer ${layer.id} reads ${unknown.name}, which the GPU stack does not hold`;
    for (const producer of Object.values(layer.reads)) {
      if (producer !== "stack.base" && !stack.layers.slice(0, index).some(other => other.id === producer)) return `weathering layer ${layer.id} reads ${producer}, which is not beneath it`;
    }
  }
  const outputs = (producer: string) => producer === "stack.base" ? base.shader.outputs : stack.layers.find(layer => layer.id === producer)?.shading.shader.outputs ?? [];
  for (const [channel, producer] of Object.entries(stack.channels)) {
    if (!STACK_CHANNELS.includes(channel) || !outputs(producer).includes(channel)) return `the stack's ${channel} comes from ${producer}, which does not give it`;
  }
  return null;
}

/**
 * What bake keys and unit counts need of a shader's source, read once per shader: its hash and the samplers it declares.
 * (Both walk the whole source, tens of kilobytes for a heavy program, and every bake asks for them several times.)
 */
const sourceFacts = new WeakMap<object, { source: string; hash: string; samplers: number }>();
function factsOf(shader: Pick<ProgramShader, "source" | "id" | "samplers">): { hash: string; samplers: number } {
  // The engine states both (`Shader::id`, `Shader::samplers`); a record without them is read from its source.
  if (shader.id && Number.isInteger(shader.samplers)) return { hash: shader.id, samplers: shader.samplers! };
  let facts = sourceFacts.get(shader);
  if (!facts || facts.source !== shader.source) {
    facts = { source: shader.source, hash: shader.id || hash(shader.source),
      samplers: Number.isInteger(shader.samplers) ? shader.samplers! : (shader.source.match(/uniform\s+(?:highp\s+)?sampler2D/g) ?? []).length };
    sourceFacts.set(shader, facts);
  }
  return facts;
}

/** Small stable string hash (FNV-1a) for cache keys; not a security boundary. */
function hash(text: string): string {
  let value = 0x811c9dc5;
  for (let index = 0; index < text.length; index++) value = Math.imul(value ^ text.charCodeAt(index), 0x01000193);
  return (value >>> 0).toString(16).padStart(8, "0");
}

/** The CPU atlas gutter in texels (`GUTTER` in `authoring/receiver.rs`). */
const ATLAS_GUTTER = 2;
/** The CPU atlas's smallest chart edge in texels (`MIN_CHART`). */
const MIN_CHART = 8;

/**
 * The atlas layout the CPU baker gives a swatch (`Atlas::new` in `authoring/receiver.rs`: a square
 * quad of two coplanar triangles packed as one chart), so a GPU bake can replace the CPU texture on
 * the mesh the CPU build produced. Returns null when the swatch has a layout this does not model;
 * `matchesMesh` checks the result against the mesh's actual UVs before it is ever shown.
 */
export function swatchDomain(extent: [number, number], resolution: number, mapping: BakeDomain["mapping"], gravity: number[]): BakeDomain | null {
  const [w, h] = extent;
  if (!(w > 0 && h > 0) || resolution < 16) return null;
  const longest = Math.max(w, h);
  const rotated = h < w;
  const size = rotated ? [w, h] : [h, w];
  // The baker bisects 48 times for the density that fits; the swatch always fits, so it ends just under the upper bound.
  let low = 0, high = resolution / longest;
  for (let step = 0; step < 48; step++) low = (low + high) * 0.5;
  const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);
  const extentOf = (length: number) => clamp(Math.ceil(length * low) + 2 * ATLAS_GUTTER, MIN_CHART, resolution);
  const width = extentOf(size[0]), height = extentOf(size[1]);
  const inner = [width - 2 * ATLAS_GUTTER, height - 2 * ATLAS_GUTTER];
  // A chart sampled coarser than its texels snaps to sample centers; that case is left to the CPU baker.
  if (inner.some((count, axis) => clamp(Math.ceil(size[axis] * low), 1, count) < count)) return null;
  const origin: [number, number, number] = rotated ? [0, 0, 0] : [w, 0, 0];
  const u: [number, number, number] = rotated ? [w, 0, 0] : [0, 0, h];
  const v: [number, number, number] = rotated ? [0, 0, h] : [-w, 0, 0];
  const uvOf = (p: number[]): [number, number] => [p[0] / w, p[2] / h];
  const o = uvOf(origin), ue = uvOf(u), ve = uvOf(v);
  return {
    width: resolution, height, gutter: ATLAS_GUTTER, origin, u, v,
    uvOrigin: o, uvU: ue, uvV: ve,
    normal: [0, -1, 0], mapping, extent: [w, h], objectOrigin: [0, 0, 0],
    boundsMin: [0, 0, 0], boundsMax: [w, 0, h],
    gravity: gravity as [number, number, number],
    // The CPU bake of a swatch sees its four open rim edges (kind 2) and nothing else: the diagonal is flat.
    features: [[0, 0, 0, w, 0, 0], [w, 0, 0, w, 0, h], [w, 0, h, 0, 0, h], [0, 0, h, 0, 0, 0]]
      .flatMap(edge => [edge[0], edge[1], edge[2], 2, edge[3], edge[4], edge[5], 0, 0, 0, 0, 0]),
  };
}

/** Whether `uvs` (a mesh's vertex positions and atlas UVs) are exactly this domain's layout, to a texel's thousandth. */
export function matchesMesh(domain: BakeDomain, positions: ArrayLike<number>, uvs: ArrayLike<number>): boolean {
  const count = positions.length / 3;
  if (count === 0 || uvs.length !== count * 2) return false;
  // Invert world = origin + u s + v t for the planar rectangle: project onto its two axes.
  const dot = (a: number[], b: ArrayLike<number>, offset = 0) => a[0] * b[offset] + a[1] * b[offset + 1] + a[2] * b[offset + 2];
  const uu = dot(domain.u, domain.u), vv = dot(domain.v, domain.v);
  const inner = [domain.width - 2 * domain.gutter, domain.height - 2 * domain.gutter];
  for (let vertex = 0; vertex < count; vertex++) {
    const relative = [positions[vertex * 3] - domain.origin[0], positions[vertex * 3 + 1] - domain.origin[1], positions[vertex * 3 + 2] - domain.origin[2]];
    const s = dot(domain.u, relative) / uu, t = dot(domain.v, relative) / vv;
    const x = (domain.gutter + s * inner[0]) / domain.width;
    const y = 1 - (domain.gutter + t * inner[1]) / domain.height;
    const tolerance = 0.001 / Math.max(domain.width, domain.height);
    if (Math.abs(x - uvs[vertex * 2]) > tolerance || Math.abs(y - uvs[vertex * 2 + 1]) > tolerance) return false;
  }
  return true;
}
