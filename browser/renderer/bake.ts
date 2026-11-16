import * as THREE from "three";
import type { MaterialShading } from "./shade.ts";
import { MAP_ANISOTROPY } from "./textures.ts";

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
layout(location = 0) out vec4 msBaseColor;
layout(location = 1) out vec4 msProperties;
layout(location = 2) out vec4 msEmissive;
layout(location = 3) out vec4 msHeight;
layout(location = 4) out vec4 msOffset;
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
  msBaseColor = vec4(clamp(ms_base_color, 0.0, 1.0), 1.0);
  msProperties = vec4(1.0, clamp(ms_roughness, 0.0, 1.0), clamp(ms_metalness, 0.0, 1.0), 1.0);
  msEmissive = vec4(max(ms_emissive, vec3(0.0)), 1.0);
  // Height in meters, with the world-space receiver_normal output beside it; the normal pass turns the relief by it.
  msHeight = vec4(ms_height, ms_receiver_normal);
  msOffset = vec4(ms_normal_offset, 0.0);
}
`;

/** The pass-1 inputs and per-texel setup, shared with the pass that evaluates live renderer arguments. */
const INPUT_UNIFORMS = PASS1_HEAD.slice(0, PASS1_HEAD.indexOf("layout(location = 0)"));
const TEXEL_SETUP = PASS1_MAIN.slice(0, PASS1_MAIN.indexOf("  // The base color target"));
/**
 * Texture units a viewer material uses besides live arguments when the viewer does not say (`BakeOptions.viewerUnits`):
 * base color, normal, roughness, metalness (the packed properties texture is bound once per sampler), emissive, the
 * environment and the sun's shadow map.
 */
const VIEWER_UNITS = 7;
/** Compiled programs kept per kind (bake programs, live-argument programs), least recently used dropped first. */
const KEPT_PASSES = 8;
const KEPT_LIVE_PASSES = 16;
/** Sources whose compile failed, remembered so an edit that keeps the source does not compile it again. */
const KEPT_FAILURES = 16;
/** Live argument textures one draw writes. */
const LIVE_PER_DRAW = 4;

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
bool msFailed = false;
vec3 relief(ivec2 p, ivec2 size) {
  vec4 center = texelFetch(msHeightMap, p, 0);
  float h = center.r;
  // Image right is +x; the next atlas row is the framebuffer row below. Clamped at the edge.
  float right = texelFetch(msHeightMap, ivec2(min(p.x + 1, size.x - 1), p.y), 0).r;
  float down = texelFetch(msHeightMap, ivec2(p.x, max(p.y - 1, 0)), 0).r;
  vec3 relief = normalize(vec3(-(right - h) / msStep.x, (down - h) / msStep.y, 1.0));
  vec3 n = msFrameZ;
  if (msOrient) {
    // orient_relief: the smallest rotation from the receiver normal to the receiver_normal output.
    vec3 target = normalize(center.gba);
    float cosine = dot(n, target);
    if (!(cosine > 1e-6)) msFailed = true;
    else if (cosine < 1.0 - 1e-6) {
      vec3 world = relief.x * msFrameX + relief.y * msFrameY + relief.z * n;
      vec3 axis = cross(n, target);
      vec3 kw = cross(axis, world);
      vec3 turned = world + kw + cross(axis, kw) / (1.0 + cosine);
      relief = normalize(vec3(dot(turned, msFrameX), dot(turned, msFrameY), dot(turned, n)));
    }
  }
  if (msBend) {
    // bend_shaded: add the world-space offset, renormalize, and hold the result just above the horizon.
    vec3 offset = texelFetch(msOffsetMap, p, 0).rgb;
    if (any(notEqual(offset, vec3(0.0)))) {
      vec3 world = relief.x * msFrameX + relief.y * msFrameY + relief.z * n + offset;
      if (dot(world, world) > 1e-24) {
        vec3 bent = normalize(world);
        vec3 local = vec3(dot(bent, msFrameX), dot(bent, msFrameY), dot(bent, n));
        if (local.z > 1e-6) relief = local;
        else {
          float sideways = length(local.xy);
          if (sideways >= 1e-12) relief = vec3(local.xy * (sqrt(1.0 - 0.02 * 0.02) / sideways), 0.02);
        }
      }
    }
  }
  return relief;
}
float dither(ivec2 texel, int channel, float amount) {
  uint h = uint(texel.x) * 0x9E3779B1u + uint(texel.y) * 0x85EBCA77u + uint(channel) * 0xC2B2AE3Du;
  h ^= h >> 15; h *= 0x2C1B3C6Du; h ^= h >> 12; h *= 0x297A2D39u; h ^= h >> 15;
  return (float(h) / 4294967296.0 - 0.5) * amount;
}
void main() {
  ivec2 size = textureSize(msHeightMap, 0);
  ivec2 p = ivec2(gl_FragCoord.xy);
  vec3 n = relief(p, size);
  vec3 before = p.x > 0 ? relief(p - ivec2(1, 0), size) : n;
  ivec2 texel = ivec2(p.x, size.y - 1 - p.y);
  vec3 encoded = vec3(0.0);
  for (int channel = 0; channel < 3; channel++) {
    // Full dither on plateaus (under a step per texel), none where the relief already varies.
    float amount = clamp(1.0 - abs(n[channel] - before[channel]) * 127.5, 0.0, 1.0);
    encoded[channel] = clamp(floor((n[channel] * 0.5 + 0.5) * 255.0 + dither(texel, channel, amount) + 0.5), 0.0, 255.0) / 255.0;
  }
  msNormalOut = vec4(encoded, msFailed ? 0.0 : 1.0);
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

type Uniforms = Record<string, THREE.IUniform>;

/** One shader's compiled bake material, shared by every bake of that source. */
interface Pass { material: THREE.ShaderMaterial; scene: THREE.Scene; }
interface Target { width: number; height: number; color: THREE.WebGLRenderTarget; normal: THREE.WebGLRenderTarget; flatNormal: boolean }

/**
 * Texels drawn per band. A draw longer than the driver's watchdog (typically a second or two) loses the
 * context, so large targets are drawn as horizontal bands with the browser given a turn between them.
 */
export const BAND_TEXELS = 1 << 17;
/** At most this many target sets (one per size) stay allocated: the low-resolution and full-size bake of a swatch. */
const KEPT_TARGETS = 3;
/** Live-argument targets kept (up to four textures each, per size). */
const KEPT_LIVE_TARGETS = 8;

/** One draw of live arguments: the textures `first`.. of a bake, with the program that writes them. */
interface LiveDraw { first: number; target: THREE.WebGLRenderTarget; pass: Pass }

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
  /** Target sets by size, least recently used first. */
  private targets = new Map<string, Target>();
  private featureData?: { key: string; texture: THREE.DataTexture };
  /** What each returned bake drew into, so a viewer can say which bake it displays. */
  private drewInto = new WeakMap<GpuBake, { target: Target; live: THREE.WebGLRenderTarget[] }>();
  /** The targets a viewer displays (`display`): never evicted, whatever else is baked or warmed. */
  private displayed = new Set<object>();
  /** Targets and passes bakes in flight are using, counted: never evicted or disposed under them. */
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
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    if (gl.getParameter(gl.MAX_DRAW_BUFFERS) < 5) return "fewer than five draw buffers";
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
    this.livePasses.clear();
    this.liveTargets.clear();
    this.displayed.clear();
    this.held.clear();
    this.targets.clear();
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
    if (!drew) return;
    this.displayed.add(drew.target);
    for (const target of drew.live) this.displayed.add(target);
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
    for (const [passes, kept] of [[this.passes, KEPT_PASSES], [this.livePasses, KEPT_LIVE_PASSES]] as const) {
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

  /** The fragment shader of pass 1 for a program's shader source (also used by diagnostics). */
  static pass1Source(source: string): string { return `${PASS1_HEAD}\n${source}\n${PASS1_MAIN}`; }

  /** The cache key of a bake: everything that changes a pixel, with the profile and version. */
  static key(shading: MaterialShading, uniforms: Uniforms, domain: BakeDomain, fieldIds: string[] = []): string {
    const values = Object.entries(uniforms).filter(([, uniform]) => !(uniform.value as { isTexture?: boolean } | null)?.isTexture)
      .map(([name, uniform]) => [name, JSON.stringify(uniform.value)]).sort();
    const images = shading.shader.images.map(image => `${image.name}:${image.linear}`);
    return `${GPU_PREVIEW_PROFILE}:${GPU_PREVIEW_VERSION}:${hash(shading.shader.source)}:${hash(JSON.stringify([values, images, domain, fieldIds]))}`;
  }

  private target(width: number, height: number): Target {
    const name = `${width}x${height}`;
    const kept = this.targets.get(name);
    if (kept) { this.targets.delete(name); this.targets.set(name, kept); return kept; }
    // A target on screen or in use by a bake in flight is never the one evicted (the set may exceed its size instead).
    for (const [other, candidate] of [...this.targets]) {
      if (this.targets.size < KEPT_TARGETS) break;
      if (!this.evictable(candidate)) continue;
      candidate.color.dispose(); candidate.normal.dispose(); this.targets.delete(other);
    }
    // One draw writes base color (sRGB), packed properties, emissive (HDR) and height (meters).
    const color = new THREE.WebGLRenderTarget(width, height, { count: 5, depthBuffer: false, stencilBuffer: false });
    const [base, properties, emissive, height1, offset] = color.textures;
    offset.format = THREE.RGBAFormat; offset.type = THREE.HalfFloatType; offset.colorSpace = THREE.NoColorSpace;
    offset.minFilter = offset.magFilter = THREE.NearestFilter; offset.generateMipmaps = false; offset.name = "gpu-preview normal offset";
    base.format = THREE.RGBAFormat; base.type = THREE.UnsignedByteType; base.colorSpace = THREE.SRGBColorSpace;
    properties.format = THREE.RGBAFormat; properties.type = THREE.UnsignedByteType; properties.colorSpace = THREE.NoColorSpace;
    emissive.format = THREE.RGBAFormat; emissive.type = THREE.HalfFloatType; emissive.colorSpace = THREE.NoColorSpace;
    height1.format = THREE.RGBAFormat; height1.type = THREE.FloatType; height1.colorSpace = THREE.NoColorSpace;
    for (const texture of [base, properties, emissive]) {
      // Mips are generated once after all bands (see `mips`), not by every draw.
      texture.minFilter = THREE.LinearMipmapLinearFilter; texture.magFilter = THREE.LinearFilter; texture.generateMipmaps = false;
      texture.wrapS = texture.wrapT = THREE.RepeatWrapping; texture.anisotropy = MAP_ANISOTROPY;
    }
    // Heights are read with texelFetch only.
    height1.minFilter = height1.magFilter = THREE.NearestFilter; height1.generateMipmaps = false;
    base.name = "gpu-preview base color"; properties.name = "gpu-preview properties";
    emissive.name = "gpu-preview emissive"; height1.name = "gpu-preview height";
    const normal = new THREE.WebGLRenderTarget(width, height, { depthBuffer: false, stencilBuffer: false,
      format: THREE.RGBAFormat, type: THREE.UnsignedByteType, colorSpace: THREE.NoColorSpace,
      minFilter: THREE.LinearMipmapLinearFilter, magFilter: THREE.LinearFilter, generateMipmaps: false });
    normal.texture.wrapS = normal.texture.wrapT = THREE.RepeatWrapping;
    normal.texture.anisotropy = MAP_ANISOTROPY;
    normal.texture.name = "gpu-preview normal";
    const created = { width, height, color, normal, flatNormal: false };
    this.targets.set(name, created);
    return created;
  }

  private disposeTargets(): void {
    for (const target of this.targets.values()) { target.color.dispose(); target.normal.dispose(); }
    this.targets.clear();
    this.featureData?.texture.dispose();
    this.featureData = undefined;
    for (const texture of this.fieldTextures.values()) texture.dispose();
    this.fieldTextures.clear();
    for (const pass of this.livePasses.values()) pass.material.dispose();
    for (const target of this.liveTargets.values()) target.dispose();
    this.livePasses.clear();
    this.liveTargets.clear();
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

  /** Draws `scene` into `target` in bands, giving the browser a turn between them; false if `current` went false. */
  private async draw(target: THREE.WebGLRenderTarget, scene: THREE.Scene, bandRows: number, current: () => boolean): Promise<number> {
    const bands = Math.ceil(target.height / bandRows);
    for (let band = 0; band < bands; band++) {
      if (band > 0) {
        await yieldToBrowser();
        if (!current()) return -1;
      }
      const before = this.renderer.getRenderTarget(), clear = this.renderer.autoClear;
      try {
        // Restored before every await: the viewer's own frames render with these settings in between.
        this.renderer.autoClear = false;
        target.scissorTest = bands > 1;
        if (bands > 1) target.scissor.set(0, band * bandRows, target.width, Math.min(bandRows, target.height - band * bandRows));
        this.renderer.setRenderTarget(target);
        this.renderer.render(scene, this.camera);
      } finally {
        target.scissorTest = false;
        this.renderer.setRenderTarget(before);
        this.renderer.autoClear = clear;
      }
    }
    return bands;
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
  private async pass(source: string, target: Target): Promise<Pass> {
    const cached = this.passes.get(source);
    if (cached) { this.passes.delete(source); this.passes.set(source, cached); return cached; }
    // One compile per source, however many bakes (a late first preview and the next edit) wait for it.
    const waiting = this.compiling.get(source);
    if (waiting) return waiting;
    const material = new THREE.ShaderMaterial({
      glslVersion: THREE.GLSL3,
      vertexShader: VERTEX,
      fragmentShader: GpuBaker.pass1Source(source),
      uniforms: {},
      depthTest: false, depthWrite: false, blending: THREE.NoBlending,
    });
    const started: Promise<Pass> = this.compiled(material, target.color, this.passes, source)
      .finally(() => { if (this.compiling.get(source) === started) this.compiling.delete(source); });
    this.compiling.set(source, started);
    return started;
  }

  private compiling = new Map<string, Promise<Pass>>();
  /** Why a source's program did not compile, by the key it would be cached under: it is not compiled again. */
  private failed = new Map<string, string>();
  /** Sources whose program has been drawn once: the driver finishes compiling at the first draw, so a program is warm only then. */
  private drawn = new Set<string>();

  /** Whether the program has been compiled and drawn once, so the next bake pays only for its pixels. */
  hasDrawn(shading: MaterialShading): boolean { return this.drawn.has(shading.shader.source); }

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
   * Compiles against the target it will draw into (three keys programs by the target's color space) and checks that the
   * program linked. three.js checks that only when a program is first used to draw, and then only logs it, so the link
   * status is read here: a program the driver rejects fails the bake instead of leaving the targets as they were. A
   * failure is remembered under `key`.
   */
  private async compiled(material: THREE.ShaderMaterial, target: THREE.WebGLRenderTarget, into: Map<string, Pass> | undefined, key: string): Promise<Pass> {
    const known = this.failed.get(key);
    if (known) { material.dispose(); throw new Error(known); }
    if (this.inject) material.fragmentShader += this.inject;
    const generation = this.generation;
    const scene = new THREE.Scene();
    const quad = new THREE.Mesh(this.geometry, material);
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
    const pass = { material, scene };
    into?.set(key, pass);
    return pass;
  }

  private async normals(target: THREE.WebGLRenderTarget): Promise<Pass> {
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
    const pass = await this.compiled(material, target, undefined, "normals");
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
    if (await this.draw(target, pass.scene, FAIL_GRID, () => true) < 0) return false;
    const cells = new Uint8Array(FAIL_GRID * FAIL_GRID * 4);
    this.renderer.readRenderTargetPixels(target, 0, 0, FAIL_GRID, FAIL_GRID, cells);
    for (let index = 0; index < cells.length; index += 4) if (cells[index] > 0) return true;
    return false;
  }

  private livePasses = new Map<string, Pass>();
  private liveTargets = new Map<string, THREE.WebGLRenderTarget>();

  /**
   * Evaluates the program's live renderer arguments (fields the shader already computes) into half-float textures, four
   * to a texel, so a viewer's lighting effects follow an edit instead of the last CPU build's baked arguments.
   * Null when `current` went false.
   */
  private async bakeLive(draws: LiveDraw[], shading: MaterialShading, bound: Uniforms, bandRows: number,
    current: () => boolean): Promise<NonNullable<GpuBake["live"]> | null> {
    const args = liveArguments(shading);
    const textures: Record<string, THREE.Texture> = {};
    const operations: LiveArgumentOperation[] = shading.shader.live.map(operation => ({ id: operation.id, arguments: {} }));
    for (const { first, target, pass } of draws) {
      Object.assign(pass.material.uniforms, bound);
      pass.material.uniformsNeedUpdate = true;
      if (await this.draw(target, pass.scene, bandRows, current) < 0) return null;
      target.textures.forEach((texture, slot) => { textures[`gpu-live-${first + slot}`] = texture; });
    }
    args.forEach((argument, index) => {
      operations[argument.operationIndex].arguments[argument.name] = { texture: `gpu-live-${Math.floor(index / 4)}`, channel: index % 4, scale: 1, offset: 0 };
    });
    return { key: JSON.stringify(operations.map(operation => [operation.id, Object.keys(operation.arguments)])), operations, textures };
  }

  /**
   * The targets and compiled programs the live-argument draws of a bake need, made before anything is drawn: a first
   * compile takes a while, and a viewer must not show half of a new bake during it. Everything is held for the bake.
   */
  private async prepareLive(shading: MaterialShading, domain: BakeDomain, holds: object[]): Promise<LiveDraw[]> {
    const args = liveArguments(shading);
    const count = Math.ceil(args.length / 4);
    const draws: LiveDraw[] = [];
    for (let first = 0; first < count; first += LIVE_PER_DRAW) {
      const here = Math.min(LIVE_PER_DRAW, count - first);
      const name = `${domain.width}x${domain.height}:${first}:${here}`;
      let target = this.liveTargets.get(name);
      if (target) this.liveTargets.delete(name);
      else {
        target = new THREE.WebGLRenderTarget(domain.width, domain.height, { count: here, depthBuffer: false, stencilBuffer: false,
          format: THREE.RGBAFormat, type: THREE.HalfFloatType, colorSpace: THREE.NoColorSpace,
          minFilter: THREE.LinearFilter, magFilter: THREE.LinearFilter, generateMipmaps: false });
      }
      this.liveTargets.set(name, target);
      this.hold(target, holds);
      // Least recently used first; one on screen or in use is skipped (the set may exceed its size instead).
      for (const [other, candidate] of [...this.liveTargets]) {
        if (this.liveTargets.size <= KEPT_LIVE_TARGETS) break;
        if (!this.evictable(candidate)) continue;
        candidate.dispose(); this.liveTargets.delete(other);
      }
      const outputs = Array.from({ length: here }, (_, slot) => `layout(location = ${slot}) out vec4 msLive${slot};`).join("\n");
      const writes = Array.from({ length: here }, (_, slot) => {
        const channels = [0, 1, 2, 3].map(channel => args[(first + slot) * 4 + channel]?.expression ?? "0.0");
        return `  msLive${slot} = vec4(${channels.join(", ")});`;
      }).join("\n");
      const key = `${shading.shader.source}#live:${first}:${here}:${hash(writes)}`;
      let pass = this.livePasses.get(key);
      if (pass) { this.livePasses.delete(key); this.livePasses.set(key, pass); }
      else {
        const material = new THREE.ShaderMaterial({ glslVersion: THREE.GLSL3, vertexShader: VERTEX,
          fragmentShader: `${INPUT_UNIFORMS}\n${outputs}\n${shading.shader.source}\n${TEXEL_SETUP}${writes}\n}\n`,
          uniforms: {}, depthTest: false, depthWrite: false, blending: THREE.NoBlending });
        pass = await this.compiled(material, target, this.livePasses, key);
      }
      this.hold(pass, holds);
      draws.push({ first, target, pass });
    }
    return draws;
  }

  /** Whether the shader source is already compiled, so a parameter or seed edit needs no recompile. */
  compiledFor(shading: MaterialShading): boolean {
    return this.passes.has(shading.shader.source);
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

  private async bakeNow(shading: MaterialShading, uniforms: Uniforms, domain: BakeDomain, options: BakeOptions, holds: object[]): Promise<GpuBake | null> {
    const { measure = false, bandTexels = BAND_TEXELS, fields = {} } = options;
    const reason = this.unavailable();
    if (reason) throw new Error(`GPU preview unavailable: ${reason}.`);
    const unsupported = gpuBakeUnsupported(shading, domain.features !== undefined);
    if (unsupported) throw new Error(`GPU preview does not support this program: ${unsupported}.`);
    if (domain.width < 1 || domain.height < 1 || domain.width > 4096 || domain.height > 4096) throw new Error("GPU preview size is out of range.");
    // A context lost (or restored) while this bake waits ends it like a newer edit would.
    const generation = this.generation;
    const current = () => generation === this.generation && (options.current?.() ?? true);
    const clock = () => performance.now();
    // Measuring forces the GPU to finish each pass, since draw calls return before they run: one texel is read from the
    // target the pass drew into, which waits for every draw into it.
    const settle = (target: THREE.WebGLRenderTarget) => {
      if (!measure) return;
      const type = target.textures[0].type;
      const texel = type === THREE.FloatType ? new Float32Array(4) : type === THREE.HalfFloatType ? new Uint16Array(4) : new Uint8Array(4);
      this.renderer.readRenderTargetPixels(target, 0, 0, 1, 1, texel, undefined, 0);
    };
    const started = clock();
    // Texture units: the baking program, and the viewer material that will show the maps (its maps, the environment,
    // shadow maps and the live argument textures), must each fit the device or the preview gives way to the CPU result.
    // Both are known from the program, so they are checked before anything is compiled or drawn.
    const unitLimit = this.renderer.capabilities.maxTextures;
    const bakeUnits = (shading.shader.source.match(/uniform\s+(?:highp\s+)?sampler2D/g) ?? []).length;
    if (bakeUnits > unitLimit) throw new Error(`GPU preview needs ${bakeUnits} texture units to bake; this device has ${unitLimit}.`);
    const viewerUnits = (options.viewerUnits ?? VIEWER_UNITS) + Math.ceil(liveArguments(shading).length / 4);
    if (viewerUnits > unitLimit) throw new Error(`GPU preview needs ${viewerUnits} texture units to show its maps and lighting arguments; this device has ${unitLimit}.`);
    const hadPass = this.compiledFor(shading);
    const outputs = shading.shader.outputs;
    const orient = outputs.includes("receiver_normal"), bend = outputs.includes("normal_offset");
    // Every program this bake draws is compiled before its first draw: a compile takes a while, the target may be the one
    // on screen, and a viewer frame in between must not show some maps of the new bake with others of the old.
    const targets = this.target(domain.width, domain.height);
    this.hold(targets, holds);
    const pass = await this.pass(shading.shader.source, targets);
    this.hold(pass, holds);
    const normals = await this.normals(targets.normal);
    if (orient) await this.failureChecker();
    const liveDraws = shading.shader.live.length ? await this.prepareLive(shading, domain, holds) : [];
    if (!current()) return null;
    targets.flatNormal = targets.flatNormal && !outputs.includes("height");
    const compiled = clock();

    const inner = [domain.width - 2 * domain.gutter, domain.height - 2 * domain.gutter];
    const length = (vector: number[]) => Math.hypot(vector[0], vector[1], vector[2]);
    const footprint = Math.max(length(domain.u) / inner[0], length(domain.v) / inner[1]);
    const uvFootprint = Math.max(Math.hypot(...domain.uvU) / inner[0], Math.hypot(...domain.uvV) / inner[1]);
    const vec3 = (value: number[]) => new THREE.Vector3(...(value as [number, number, number]));
    const vec2 = (value: number[]) => new THREE.Vector2(...(value as [number, number]));
    const bound: Uniforms = {
      ...uniforms,
      msAtlas: { value: new THREE.Vector2(domain.width, domain.height) },
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
    // CPU fields: float textures on the bake grid, read by texelFetch, so alignment is exact and nothing is filtered.
    const fieldIds: string[] = [];
    for (const wanted of shading.shader.cpuFields ?? []) {
      const given = fields[wanted.uniform];
      if (!given) throw new Error(`GPU preview needs the CPU field ${wanted.name}.`);
      const field = downsampleField(given, domain.width, domain.height);
      if (field.width !== domain.width || field.height !== domain.height) throw new Error("CPU field and bake grid sizes differ.");
      bound[wanted.uniform] = { value: this.fieldTexture(field) };
      fieldIds.push(`${wanted.name}=${field.id}`);
    }
    // Three keeps a reference to the material's uniform table, so rebinding assigns into it; the program is reused.
    Object.assign(pass.material.uniforms, bound);
    pass.material.uniformsNeedUpdate = true;
    const bindTime = clock();

    const bandRows = Math.max(1, Math.floor(bandTexels / domain.width));
    const bands = await this.draw(targets.color, pass.scene, bandRows, current);
    if (bands < 0) return null;
    settle(targets.color);
    const first = clock();
    const [baseColor, properties, emissive, heightMap, offsetMap] = targets.color.textures;
    this.mips([baseColor, properties, emissive]);
    settle(targets.color);
    const mipped = clock();
    // A program without a height output (or receiver-normal outputs) has a flat normal, drawn once per target.
    const flat = !outputs.includes("height") && !orient && !bend;
    let second = mipped;
    if (!(flat && targets.flatNormal)) {
      const uniform = normals.material.uniforms;
      uniform.msHeightMap.value = heightMap;
      uniform.msOffsetMap.value = offsetMap;
      (uniform.msStep.value as THREE.Vector2).set(length(domain.u) / inner[0], length(domain.v) / inner[1]);
      // The tangent frame the CPU atlas uses: z the receiver normal, x along image right, y up the image.
      const z = vec3(domain.normal).normalize();
      const x = vec3(domain.u).addScaledVector(z, -z.dot(vec3(domain.u))).normalize();
      const y = new THREE.Vector3().crossVectors(z, x);
      if (y.dot(vec3(domain.v)) > 0) y.negate();
      (uniform.msFrameX.value as THREE.Vector3).copy(x); (uniform.msFrameY.value as THREE.Vector3).copy(y);
      (uniform.msFrameZ.value as THREE.Vector3).copy(z);
      uniform.msOrient.value = orient; uniform.msBend.value = bend;
      normals.material.uniformsNeedUpdate = true;
      if (await this.draw(targets.normal, normals.scene, bandRows, current) < 0) return null;
      // A receiver normal facing away from the receiver is a bake error on the CPU; the preview then gives way to it.
      if (orient && await this.normalsFailed(targets.normal)) throw new Error("receiver_normal output must face the same side as the receiver");
      settle(targets.normal);
      second = clock();
      this.mips([targets.normal.texture]);
      targets.flatNormal = flat;
    }
    settle(targets.normal);
    const normalsDone = clock();
    let live: GpuBake["live"];
    if (liveDraws.length) {
      const baked = await this.bakeLive(liveDraws, shading, bound, bandRows, current);
      if (!baked) return null;
      live = baked;
      for (const draw of liveDraws) settle(draw.target);
    }
    const done = clock();
    const key = GpuBaker.key(shading, uniforms, domain, fieldIds);
    if (this.passes.has(shading.shader.source)) this.drawn.add(shading.shader.source);
    const bake: GpuBake = {
      profile: GPU_PREVIEW_PROFILE, key, width: domain.width, height: domain.height,
      baseColor, properties, emissive, normal: targets.normal.texture,
      timings: { compile: hadPass ? 0 : compiled - started, uniforms: bindTime - compiled, pass1: first - bindTime,
        mips: (mipped - first) + (normalsDone - second), pass2: second - mipped, total: done - started, bands, live: done - normalsDone },
      live,
      units: { bake: bakeUnits, viewer: viewerUnits, limit: unitLimit },
    };
    this.drewInto.set(bake, { target: targets, live: liveDraws.map(draw => draw.target) });
    return bake;
  }

  /** The base color and normal texels of the last bake, for tests and diagnostics (not for display). */
  readBack(bake: GpuBake): { baseColor: Uint8Array; normal: Uint8Array; height: Float32Array; properties: Uint8Array } {
    const targets = this.targets.get(`${bake.width}x${bake.height}`);
    if (!targets) throw new Error("This bake's textures were replaced.");
    const count = targets.width * targets.height;
    const baseColor = new Uint8Array(count * 4), normal = new Uint8Array(count * 4), height = new Float32Array(count * 4);
    // The base color attachment decodes sRGB on read in some drivers; reading it as bytes returns the stored encoding.
    this.renderer.readRenderTargetPixels(targets.color, 0, 0, targets.width, targets.height, baseColor, undefined, 0);
    this.renderer.readRenderTargetPixels(targets.normal, 0, 0, targets.width, targets.height, normal);
    this.renderer.readRenderTargetPixels(targets.color, 0, 0, targets.width, targets.height, height, undefined, 3);
    const properties = new Uint8Array(count * 4);
    this.renderer.readRenderTargetPixels(targets.color, 0, 0, targets.width, targets.height, properties, undefined, 1);
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
    for (const target of this.liveTargets.values()) {
      if (target.width !== bake.width || target.height !== bake.height) continue;
      for (let slot = 0; slot < target.textures.length; slot++) {
        const bits = new Uint16Array(target.width * target.height * 4);
        this.renderer.readRenderTargetPixels(target, 0, 0, target.width, target.height, bits, undefined, slot);
        for (let channel = 0; channel < 4; channel++) {
          let min = Infinity, max = -Infinity, sum = 0;
          for (let index = channel; index < bits.length; index += 4) { const value = half(bits[index]); min = Math.min(min, value); max = Math.max(max, value); sum += value; }
          found.push({ min, max, mean: sum / (bits.length / 4) });
        }
      }
    }
    return found;
  }

  dispose(): void {
    this.renderer.domElement.removeEventListener("webglcontextlost", this.contextLost);
    this.renderer.domElement.removeEventListener("webglcontextrestored", this.contextRestored);
    for (const pass of this.passes.values()) pass.material.dispose();
    this.normalPass?.material.dispose();
    this.failureCheck?.pass.material.dispose(); this.failureCheck?.target.dispose(); this.failureCheck = undefined;
    this.disposeTargets();
    this.geometry.dispose();
    this.passes.clear();
    this.normalPass = undefined;
  }
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
