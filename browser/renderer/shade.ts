import * as THREE from "three";
import { applyLiveOperations } from "./live.ts";
import { INSTANCE_ATTRIBUTE } from "./batch.ts";
import { MAPPINGS, SURFACE_OF, anchorFromWorld, type PatternAnchor } from "./atlas.ts";
import { DEFAULT_LIGHTING, DEFAULT_PAINTED, type PaintedFinishing } from "./lighting.ts";
import { CAST_SHADOW } from "./shadow-style.ts";
import { programFieldPass, sceneFieldUniforms } from "./scene-fields.ts";
import type { LiveProgram } from "../model/packet.ts";

/**
 * Seconds of the viewer's time-driven render mode (`RustScene.setContinuous`), shared by every program material as
 * the uniform `msTime`. It stands still while the viewer renders on demand. No program reads it yet: the engine's
 * shader has no `time` input, and when it declares `ms_time` the material assigns it from this uniform.
 */
export const programTime: THREE.IUniform<number> = { value: 0 };

/**
 * The sun of the scene's lighting record, as every program material reads it (`sunDirection`, uniform
 * `msSunDirection`): the unit vector from the scene toward the sun, zero without a sun. `SceneLighting.set` keeps it,
 * so moving the sun redraws painted materials with no rebuild and no bake.
 */
export const programSun: THREE.IUniform<THREE.Vector3> = { value: new THREE.Vector3(...DEFAULT_LIGHTING.lights[0].direction) };

/** Painted materials made so far, to follow the scene's painted finishing (`setPaintedFinishing`). */
const paintedMaterials = new Set<WeakRef<THREE.Material>>();
let paintedFinishing: PaintedFinishing = DEFAULT_PAINTED;

/**
 * Applies the lighting record's `painted` setting to every painted material: whether exposure and the tone curve apply
 * to them (a global rendering setting, never one of a material). Ambient occlusion is a pass over the finished image;
 * it covers painted pixels whatever the setting (see `SceneLighting`, which reports the setting it cannot honor).
 */
export function setPaintedFinishing(finishing: PaintedFinishing): void {
  if (finishing.toneMapping === paintedFinishing.toneMapping && finishing.ambientOcclusion === paintedFinishing.ambientOcclusion) return;
  paintedFinishing = { ...finishing };
  for (const reference of paintedMaterials) {
    const material = reference.deref();
    if (!material) { paintedMaterials.delete(reference); continue; }
    material.toneMapped = finishing.toneMapping;
    material.needsUpdate = true;
  }
}

/**
 * Makes a material drawn with painted output follow the scene's painted finishing (`setPaintedFinishing`) from now on,
 * as program materials with `shading: painted` do: foliage's painted profile (foliage.ts) is one.
 */
export function followPaintedFinishing(material: THREE.Material): void {
  material.toneMapped = paintedFinishing.toneMapping;
  paintedMaterials.add(new WeakRef(material));
}

/** A MaterialSpec program as GLSL (`Program::shader` in the engine), with what it needs bound. */
export interface ProgramShader {
  source: string;
  parameters: { name: string; uniform: string; type: "scalar" | "vec2" | "vec3" | "bool" }[];
  seeds: { name: string; uniform: string }[];
  images: { name: string; uniform: string; linear: boolean }[];
  outputs: string[];
  /**
   * `classes` says what each argument is to a host that bakes: a number, an expression of scalar parameter uniforms
   * alone, or a value that varies over the surface (the symbol of `arguments`).
   */
  live: { id: string; arguments: Record<string, string>; classes?: Record<string, { constant: number } | { parameters: string[]; glsl: string } | { varying: string }> }[];
  features: boolean;
  /** The zone names the source queries, in their kinds' order; a host binds `ms_zones` and `ms_zone_count` (see `zoneTexture`). */
  zones?: string[];
  /** An identity of `source`: equal sources have equal identities. */
  id?: string;
  /** Every uniform `source` declares, as GLSL declares it, and how many of them are samplers. */
  uniforms?: { name: string; type: string }[];
  samplers?: number;
  /**
   * Entry points besides `ms_evaluate()`, each for some of the outputs (and the live fields when `live`). A program
   * that calls one appends its `source` to the shader's (empty for `ms_evaluate_height`, which the shader's holds).
   */
  entries?: { function: string; outputs: string[]; live: boolean; source: string }[];
  /**
   * What each output and each live argument reads, through everything upstream of it (`ShaderReads` in the engine's
   * `program/glsl.rs`): by output name for every output the program sets, and by live operation id, then argument
   * name (absent when the program has no live operation). A bake's textures are keyed by them (`attachmentReads` in
   * atlas.ts is the one place that reads this); for a record without it that function derives them from `entries`.
   */
  reads?: { outputs: Record<string, Reads>; live?: Record<string, Record<string, Reads>> };
  /** Present on a hybrid plan: fields the CPU bakes, read by the shader from float textures. */
  cpuFields?: CpuField[];
  /** Outputs that read a CPU field. */
  cpuOutputs?: string[];
  /**
   * Render-only inputs the source reads (`Shader::render_inputs`): uniforms (`msTime`, `msSunDirection`), per-instance
   * and per-vertex attributes (`msInstanceValue`, `msInstance_<name>`, `msVertex_<name>`), varyings derived from what is
   * drawn (`msInstanceRoot`, `msInstanceTint`, `msInstanceRandom`, `msPlacedWorld`) and scene fields (`msField<k>` and
   * `msFieldRect<k>`, named by `name`). `fallback` is what a named attribute reads where the mesh has none.
   */
  renderInputs?: RenderInput[];
  /** For a painted material, the expression holding the color that reaches the screen after `ms_evaluate()`. */
  painted?: string;
  /** The cutoff of a cutout (`alphaCutout` as the opacity): the surface is drawn only where `ms_opacity` reaches it. */
  alphaCutoff?: number;
}
/** See `ProgramShader.renderInputs`. */
export interface RenderInput { symbol: string; kind: "uniform" | "instance" | "vertex" | "derived" | "field"; glsl: string; name?: string; fallback?: string }
/** Everything one value of a program reads (`Reads` in the engine's `program/glsl.rs`). */
export interface Reads {
  /** Parameter uniforms (`ProgramShader.parameters[].uniform`). */
  parameters: string[];
  /** Whether any named seed is read. */
  seed: boolean;
  /** Supplied images, by name. */
  images: string[];
  /** Whether the supplied source edges are read. */
  edges: boolean;
  /** Whether receiver features are read. */
  features: boolean;
  /** The parts of a sample read (`uv`, `surface`, `world`, `normal`, `bounds`, ...). */
  sample: string[];
  /** CPU fields read, by name; empty outside a plan. */
  cpuFields: string[];
  /** External inputs read, whose planes belong to one receiver. */
  planes: string[];
  /** Whether the value is the same at a surface position on every receiver. */
  intrinsic: boolean;
}
/** A field a hybrid shader reads from a texture the host bakes on the CPU, and what it must be baked again for. */
export interface CpuField {
  name: string;
  uniform: string;
  type: "scalar" | "vec2" | "vec3" | "bool";
  parameters: string[];
  seed: boolean;
  images: string[];
  edges: boolean;
  /** External inputs the field reads; absent when it reads none. */
  planes?: string[];
}
/** One material of a shaded scene: its shader, seed values and parameter defaults. */
export interface MaterialShading {
  shader: ProgramShader;
  seedValues: number[];
  parameters: { name: string; default: unknown }[];
  /** `anchored` measures the surface from each receiver's pattern anchor (`MeshShading.anchor`). */
  mapping: "uv" | "object" | "world" | "anchored";
}
/** One mesh's receiver inputs: bounds and features as rows of twelve numbers (see `shade_project`). */
export interface MeshShading {
  material: string;
  boundsMin: number[];
  boundsMax: number[];
  features: number[];
  objectToWorld?: number[] | null;
  /** The receiver's pattern anchor, for a material mapped `anchored`; without one the frame is the world's. */
  anchor?: PatternAnchor | null;
  /** The receiver's weathering zones as the material's shader queries them (`materialspec::program::pack`). */
  zones?: ZoneRows | null;
  /** The same for each layer of a layered slot, in layer order (null where a layer queries none). */
  layerZones?: (ZoneRows | null)[];
}
/** Zone rows of seven RGBA texels (segments, then their clips) and the number of segment rows. */
export interface ZoneRows { rows: number[]; count: number }
export interface SceneShading {
  materials: Record<string, MaterialShading>;
  meshes: Record<string, MeshShading>;
  gravity: number[];
  /** Physical size of the UV square for `uv` mapping, meters (the project's domain). */
  domain?: number[];
}
type Uniforms = Record<string, THREE.IUniform>;

function uniformValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.length === 2 ? new THREE.Vector2(...(value as [number, number])) : new THREE.Vector3(...(value as [number, number, number]));
  return value;
}

/** Sets a material's parameter uniforms from overrides, falling back to the recipe's defaults. */
export function setParameters(uniforms: Uniforms, material: MaterialShading, values: Record<string, unknown>): void {
  const defaults = new Map(material.parameters.map(parameter => [parameter.name, parameter.default]));
  for (const parameter of material.shader.parameters) {
    const value = uniformValue(values[parameter.name] ?? defaults.get(parameter.name));
    const uniform = uniforms[parameter.uniform] ??= { value };
    uniform.value = value;
  }
}

/** Sets a material's named seed uniforms from the engine's seed values (`seedValues`, in `shader.seeds` order). */
export function setSeeds(uniforms: Uniforms, material: MaterialShading): void {
  material.shader.seeds.forEach((seed, index) => {
    const uniform = uniforms[seed.uniform] ??= { value: material.seedValues[index] };
    uniform.value = material.seedValues[index];
  });
}

/** A mesh's features as a float texture: three texels per feature, one feature per row. */
function featureTexture(features: number[]): THREE.DataTexture {
  const rows = Math.max(1, features.length / 12);
  const data = new Float32Array(rows * 12);
  data.set(features);
  const texture = new THREE.DataTexture(data, 3, rows, THREE.RGBAFormat, THREE.FloatType);
  texture.minFilter = texture.magFilter = THREE.NearestFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return texture;
}

/** Texels of a zone row (`ZONE_ROW_TEXELS` in the engine). */
export const ZONE_ROW_TEXELS = 7;
/** A receiver's zone rows as a float texture, one row a texel row; at least one row, so the sampler is always bound. */
export function zoneTexture(zones: ZoneRows | null | undefined): THREE.DataTexture {
  const values = zones?.rows ?? [];
  const width = ZONE_ROW_TEXELS * 4;
  const rows = Math.max(1, values.length / width);
  const data = new Float32Array(rows * width);
  data.set(values);
  const texture = new THREE.DataTexture(data, ZONE_ROW_TEXELS, rows, THREE.RGBAFormat, THREE.FloatType);
  texture.minFilter = texture.magFilter = THREE.NearestFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return texture;
}

/**
 * What names a shader's source in caches of compiled programs and in bake keys: the engine's identity of it
 * (`ProgramShader.id`), or the source itself from an engine that states none. Equal sources have equal names.
 */
export function shaderIdentity(shader: Pick<ProgramShader, "source" | "id">): string {
  return shader.id ? `#${shader.id}` : shader.source;
}

/**
 * Program cache identities by shader source, for sources the engine gives no identity: most recently used last; a
 * source that left the list gets a new identity.
 */
const sourceIds = new Map<string, number>();
let nextSourceId = 0;
const KEPT_SOURCE_IDS = 64;

function sourceId(shader: ProgramShader): string {
  if (shader.id) return `#${shader.id}`;
  const source = shader.source;
  let id = sourceIds.get(source);
  if (id === undefined) id = nextSourceId++;
  else sourceIds.delete(source);
  sourceIds.set(source, id);
  while (sourceIds.size > KEPT_SOURCE_IDS) sourceIds.delete(sourceIds.keys().next().value!);
  return String(id);
}

/**
 * What a program material binds for the render-only inputs its shader reads (`ProgramShader.renderInputs`), given the
 * names of the attributes its geometry has:
 * - `msSunDirection` is the shared `programSun`, `msTime` the shared `programTime` (bound by `programMaterial`);
 * - named attributes (`msInstance_<name>`, `msVertex_<name>`) pass from the geometry to the fragment shader through a
 *   varying of their own; a geometry without one reads the value its call is typed by (`fallback`), as the CPU's
 *   default bindings do;
 * - derived values: `msInstanceRoot` is where the drawn instance's origin stands, through `instanceMatrix` (the mesh's
 *   origin outside a batch); `msInstanceTint` the instance's color (white without one); `msInstanceRandom` the
 *   instance's `random` attribute, or a hash of its index (0 outside a batch); `msPlacedWorld` the drawn point through
 *   the instance's placement, where `ms_world` stays in the batch's own space, the space its one bake is made in;
 * - scene fields are the shared uniforms of `sceneFieldUniforms`.
 */
export function renderInputs(shader: Pick<ProgramShader, "renderInputs">, attributes: readonly string[]): {
  uniforms: Uniforms; vertexDeclarations: string; vertexAssignments: string; fragmentDeclarations: string;
} {
  const uniforms: Uniforms = {};
  const vertexDeclarations: string[] = [], vertexAssignments: string[] = [], fragmentDeclarations: string[] = [];
  const has = new Set(attributes);
  for (const input of shader.renderInputs ?? []) {
    const { symbol, glsl } = input;
    if ((input.kind === "instance" || input.kind === "vertex") && symbol !== "msInstanceValue") {
      if (has.has(symbol)) {
        vertexDeclarations.push(`attribute ${glsl} ${symbol};`, `varying ${glsl} v${symbol};`);
        vertexAssignments.push(`  v${symbol} = ${symbol};`);
        fragmentDeclarations.push(`varying ${glsl} v${symbol};`, `#define ${symbol} v${symbol}`);
      } else fragmentDeclarations.push(`#define ${symbol} (${input.fallback ?? `${glsl}(0.0)`})`);
    } else if (input.kind === "derived") {
      vertexDeclarations.push(`varying ${glsl} ${symbol};`);
      fragmentDeclarations.push(`varying ${glsl} ${symbol};`);
      if (symbol === "msInstanceRoot") vertexAssignments.push(`#ifdef USE_INSTANCING
  msInstanceRoot = (modelMatrix * instanceMatrix * vec4(0.0, 0.0, 0.0, 1.0)).xyz;
#else
  msInstanceRoot = (modelMatrix * vec4(0.0, 0.0, 0.0, 1.0)).xyz;
#endif`);
      else if (symbol === "msInstanceTint") vertexAssignments.push(`#ifdef USE_INSTANCING_COLOR
  msInstanceTint = instanceColor;
#else
  msInstanceTint = vec3(1.0);
#endif`);
      else if (symbol === "msInstanceRandom") {
        if (has.has("msInstance_random")) {
          vertexDeclarations.push("attribute float msInstance_random;");
          vertexAssignments.push("  msInstanceRandom = msInstance_random;");
        } else vertexAssignments.push(`#ifdef USE_INSTANCING
  msInstanceRandom = float((uint(gl_InstanceID) * 2654435761u) >> 8u) * (1.0 / 16777216.0);
#else
  msInstanceRandom = 0.0;
#endif`);
      } else if (symbol === "msPlacedWorld") vertexAssignments.push(`#ifdef USE_INSTANCING
  msPlacedWorld = (modelMatrix * instanceMatrix * vec4(transformed, 1.0)).xyz;
#else
  msPlacedWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;
#endif`);
    } else if (input.kind === "field" && input.name) {
      const field = sceneFieldUniforms(input.name);
      uniforms[symbol] = glsl === "sampler2D" ? field.texture : field.rect;
    }
  }
  // An attribute both named and derived (`random`) is declared once.
  const once = (lines: string[]) => [...new Set(lines)].join("\n");
  return { uniforms, vertexDeclarations: once(vertexDeclarations), vertexAssignments: vertexAssignments.join("\n"), fragmentDeclarations: once(fragmentDeclarations) };
}

/**
 * A delivered program (a painted material's `program` in a packet or GLB, `LiveProgram` in the engine) as the shading
 * `programMaterial` draws: its source, its uniforms with their values, the render inputs it reads and its painted color.
 */
export function deliveredProgram(program: LiveProgram): { shading: MaterialShading; uniforms: Uniforms } {
  const shader: ProgramShader = { source: program.source, parameters: [], seeds: [], images: [], outputs: [], live: [], features: false,
    renderInputs: program.inputs, painted: program.painted };
  const uniforms: Uniforms = {};
  for (const { uniform, type, value } of program.uniforms) {
    uniforms[uniform] = { value: type === "bool" ? value[0] !== 0 : type === "vec2" ? new THREE.Vector2(value[0], value[1])
      : type === "vec3" ? new THREE.Vector3(value[0], value[1], value[2]) : value[0] };
  }
  return { shading: { shader, seedValues: [], parameters: [], mapping: program.mapping }, uniforms };
}

/** Whether a program's relief can be evaluated per pixel: the engine emits a height-only entry point when it has a height output. */
export function hasPixelRelief(shading: MaterialShading): boolean {
  return shading.shader.source.includes("void ms_evaluate_height()");
}

/**
 * A standard material whose channels a MaterialSpec program computes per pixel instead of reading
 * baked maps. `shared` holds the material's parameter, seed and image uniforms, so one update
 * reaches every mesh that shows it. Relief becomes a normal from the height's differences to the
 * neighbouring pixels' surface points (`"pixel"`), or from the height's screen derivatives
 * (`"quad"`): those step in 2x2 pixel blocks, but the program compiles in about half the time, so
 * a viewer shows it first and swaps to the per-pixel program once that is ready.
 *
 * `noise` chooses the form of the program the engine emits (see `glsl.rs`): `"looped"` evaluates
 * the lattice noises in loops, which compiles several times faster and draws several times
 * slower when a program has many; `"inline"` has each noise where it is used. Both draw the same
 * picture, so a viewer shows the looped form first and compiles the inline one behind it.
 *
 * `instanced` makes the material for an instanced batch (batch.ts). The program is then evaluated in the batch's own
 * space, the space its maps are baked in: `ms_world`, `ms_object`, the normal and the footprints are those of the
 * batch's mesh, the same for every instance, so the per-pixel look and the one bake agree and nothing in the program
 * depends on where an instance stands. Only the finished normal is turned by the instance's placement, for lighting.
 * Per-instance values reach the fragment shader as `msInstanceValue` (the `instance` input, zero without one); a
 * program that declares `ms_instance` gets it assigned.
 */
export function programMaterial(shading: MaterialShading, mesh: MeshShading, gravity: number[], shared: Uniforms,
  mode: "pixel" | "quad" = "pixel", noise: "looped" | "inline" = "looped", instanced = false, attributes: readonly string[] = []): THREE.MeshStandardMaterial {
  const material = new THREE.MeshStandardMaterial({ color: 0xffffff });
  // Both shaders read the define; three.js names instancing to the vertex shader alone.
  if (instanced) material.defines = { ...material.defines, MS_INSTANCED: "" };
  const shader = shading.shader;
  const inputs = renderInputs(shader, attributes);
  // A painted material shows its program's color without the light loop; the scene's painted finishing decides
  // whether exposure and the tone curve still apply.
  if (shader.painted) {
    material.defines = { ...material.defines, MS_PAINTED: "" };
    material.toneMapped = paintedFinishing.toneMapping;
    paintedMaterials.add(new WeakRef(material));
  }
  // A cutout is drawn where the program's opacity reaches its cutoff (the baked look does the same, scene-bake.ts).
  if (typeof shader.alphaCutoff === "number") { material.alphaTest = shader.alphaCutoff; material.alphaToCoverage = true; }
  // Otherwise a program that states an opacity (glass) is drawn blended, as its baked look is.
  else if (shader.outputs.includes("opacity")) { material.transparent = true; material.depthWrite = false; }
  const features = featureTexture(mesh.features);
  const relief = mode === "pixel" && hasPixelRelief(shading);
  const objectFromWorld = new THREE.Matrix4();
  if (mesh.objectToWorld) objectFromWorld.fromArray(mesh.objectToWorld).invert();
  else objectFromWorld.makeTranslation(-mesh.boundsMin[0], -mesh.boundsMin[1], -mesh.boundsMin[2]);
  const uniforms: Uniforms = {
    ...shared,
    ms_features: { value: features }, ms_feature_count: { value: mesh.features.length / 12 },
    ...(shader.zones?.length ? { ms_zones: { value: zoneTexture(mesh.zones) }, ms_zone_count: { value: mesh.zones?.count ?? 0 } } : {}),
    ms_gravity: { value: new THREE.Vector3(...(gravity as [number, number, number])) },
    ms_bounds_min: { value: new THREE.Vector3(...(mesh.boundsMin as [number, number, number])) },
    ms_bounds_max: { value: new THREE.Vector3(...(mesh.boundsMax as [number, number, number])) },
    msObjectFromWorld: { value: objectFromWorld },
    // Read under the anchored mapping alone: the receiver's pattern anchor frame (the world's without an anchor).
    msAnchorFromWorld: { value: new THREE.Matrix4().fromArray(anchorFromWorld(mesh.anchor)) },
    msMapping: { value: MAPPINGS.indexOf(shading.mapping) },
    msReliefSamples: { value: 2 },
    msTime: programTime,
    msSunDirection: programSun,
    msFieldPass: programFieldPass,
    ...inputs.uniforms,
  };
  material.addEventListener("dispose", () => features.dispose());
  // Live operations read the program's fields directly; they are installed first so the program
  // runs before them in the fragment shader.
  const live = applyLiveOperations(material, shader.live.map(operation => ({
    id: operation.id,
    arguments: Object.fromEntries(Object.entries(operation.arguments).map(([name, glsl]) => [name, { glsl }])),
  })) as never, () => null);
  const previous = material.onBeforeCompile;
  material.onBeforeCompile = (program, renderer) => {
    previous.call(material, program, renderer);
    Object.assign(program.uniforms, uniforms);
    program.vertexShader = program.vertexShader
      .replace("#include <common>", `#include <common>
varying vec3 msWorld;
varying vec3 msNormal;
varying vec2 msUv;
#ifdef MS_INSTANCED
attribute vec4 ${INSTANCE_ATTRIBUTE};
varying vec4 msInstanceValue;
varying mat3 msInstanceTurn;
#endif
${inputs.vertexDeclarations}`)
      .replace("#include <project_vertex>", `#include <project_vertex>
  msWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;
  msNormal = mat3(modelMatrix) * objectNormal;
  msUv = uv;
${inputs.vertexAssignments}
#ifdef MS_INSTANCED
  {
    // What takes a normal of the batch's mesh to the instance: the placement's rotation over its scale (its columns
    // are perpendicular, so each is divided by its squared length).
    mat3 placed = mat3(instanceMatrix);
    msInstanceTurn = mat3(placed[0] / dot(placed[0], placed[0]), placed[1] / dot(placed[1], placed[1]), placed[2] / dot(placed[2], placed[2]));
    msInstanceValue = ${INSTANCE_ATTRIBUTE};
  }
#endif`);
    program.fragmentShader = program.fragmentShader
      .replace("#include <common>", `#include <common>
varying vec3 msWorld;
varying vec3 msNormal;
varying vec2 msUv;
uniform mat4 msObjectFromWorld;
uniform mat4 msAnchorFromWorld;
uniform int msMapping;
uniform int msReliefSamples;
uniform float msTime;
uniform int msFieldPass;
#ifdef MS_INSTANCED
varying vec4 msInstanceValue;
varying mat3 msInstanceTurn;
#endif
${inputs.fragmentDeclarations}
${noise === "inline" ? "#define MS_INLINE_NOISE\n" : ""}${shader.source}
// The height one pixel across and one pixel down on the surface, minus the height here.
float msHeightAcross = 0.0, msHeightDown = 0.0;
// Places the program's inputs at a point of the surface; the normal and footprints stay the pixel's.
void msPlace(vec3 world, vec2 uv) {
  ms_world = world;
  ms_object = (msObjectFromWorld * vec4(world, 1.0)).xyz;
  ms_uv = uv;
  ${SURFACE_OF("ms_world", "ms_object", "ms_normal", "uv", "ms_surface")}
}`)
      .replace("#include <clipping_planes_fragment>", `#include <clipping_planes_fragment>
  ms_normal = normalize(msNormal);
  ms_footprint = max(length(dFdx(msWorld)), length(dFdy(msWorld)));
  ms_uv_footprint = max(length(dFdx(msUv)), length(dFdy(msUv)));
${relief ? `  // Screen derivatives of a computed value are shared by a 2x2 pixel block, so the relief would step in
  // blocks; the height is evaluated at the neighbouring surface points instead. These run first: they
  // overwrite the program's fields, which the full evaluation below then sets for this pixel.
  {
    vec3 worldAcross = dFdx(msWorld), worldDown = dFdy(msWorld);
    vec2 uvAcross = dFdx(msUv), uvDown = dFdy(msUv);
    // A loop whose count the compiler cannot see, so the height code is compiled once, not twice.
    for (int i = 0; i < msReliefSamples; i++) {
      msPlace(msWorld + (i == 0 ? worldAcross : worldDown), msUv + (i == 0 ? uvAcross : uvDown));
      ms_evaluate_height();
      if (i == 0) msHeightAcross = ms_height; else msHeightDown = ms_height;
    }
  }
` : ""}  msPlace(msWorld, msUv);
${/\bms_time\b/.test(shader.source) ? "  ms_time = msTime;\n" : ""}${/\bms_instance\b/.test(shader.source) ? "#ifdef MS_INSTANCED\n  ms_instance = msInstanceValue;\n#endif\n" : ""}${/\bms_cast_shadow\b/.test(shader.source) ? "  ms_cast_shadow = msCastShadow();\n" : ""}  ms_evaluate();
${relief ? "  msHeightAcross -= ms_height; msHeightDown -= ms_height;" : "  msHeightAcross = dFdx(ms_height); msHeightDown = dFdy(ms_height);"}
  diffuseColor = vec4(clamp(ms_base_color, 0.0, 1.0), clamp(ms_opacity, 0.0, 1.0));`)
      .replace("#include <roughnessmap_fragment>", "#include <roughnessmap_fragment>\n  roughnessFactor = clamp(ms_roughness, 0.0, 1.0);")
      .replace("#include <metalnessmap_fragment>", "#include <metalnessmap_fragment>\n  metalnessFactor = clamp(ms_metalness, 0.0, 1.0);")
      .replace("#include <emissivemap_fragment>", "#include <emissivemap_fragment>\n  totalEmissiveRadiance = ms_emissive;")
      .replace("#include <normal_fragment_maps>", `#include <normal_fragment_maps>
  {
    // Relief: the height's gradient over the surface, from its differences to the neighbouring pixels.
    vec3 flat_normal = ms_normal;
    vec3 across = dFdx(msWorld), down = dFdy(msWorld);
    vec3 r1 = cross(down, flat_normal), r2 = cross(flat_normal, across);
    float det = dot(across, r1);
    vec3 shaded = abs(det) > 1e-20
      ? normalize(abs(det) * flat_normal - sign(det) * (msHeightAcross * r1 + msHeightDown * r2)) : flat_normal;
    // receiverNormal turns the relief by the smallest rotation from the receiver normal to it.
    vec3 target = normalize(ms_receiver_normal);
    float cosine = dot(flat_normal, target);
    if (cosine > 1e-6 && cosine < 1.0 - 1e-7) {
      vec3 axis = cross(flat_normal, target);
      vec3 turned = cross(axis, shaded);
      shaded = normalize(shaded + turned + cross(axis, turned) / (1.0 + cosine));
    }
    // normalOffset bends the result (bend_shaded in the engine): one that would face away from the
    // receiver is held just above its horizon, and an offset that cancels the normal changes nothing.
    vec3 bent = shaded + ms_normal_offset;
    if (any(notEqual(ms_normal_offset, vec3(0.0))) && dot(bent, bent) > 1e-24) {
      bent = normalize(bent);
      float facing = dot(bent, flat_normal);
      if (facing > 1e-6) shaded = bent;
      else {
        vec3 sideways = bent - facing * flat_normal;
        float flat_length = length(sideways);
        if (flat_length >= 1e-12) shaded = sideways * (sqrt(1.0 - 0.02 * 0.02) / flat_length) + 0.02 * flat_normal;
      }
    }
#ifdef MS_INSTANCED
    shaded = normalize(msInstanceTurn * shaded);
#endif
    normal = normalize((viewMatrix * vec4(shaded, 0.0)).xyz) * faceDirection;
  }`)
      // The cast shadow a painted shade band folds in, in the scene's shadow style (shadow-style.ts).
      .replace("#include <shadowmap_pars_fragment>", `#include <shadowmap_pars_fragment>\n${/\bms_cast_shadow\b/.test(shader.source) ? CAST_SHADOW : ""}`)
      // A painted material's color reaches the screen as authored: the light loop's result is replaced. A field pass
      // draws the color a scene field is made of, unlit (scene-fields.ts).
      .replace("#include <opaque_fragment>", `${shader.painted ? `  outgoingLight = ${shader.painted};\n` : ""}#include <opaque_fragment>`)
      .replace("#include <tonemapping_fragment>", `  if (msFieldPass == 1) gl_FragColor = vec4(ms_base_color, 1.0);
#include <tonemapping_fragment>`);
  };
  const id = sourceId(shader);
  const liveKey = material.customProgramCacheKey();
  // Which render inputs a mesh supplies changes the program (an attribute is a varying or its fallback).
  const bound = inputs.vertexDeclarations + inputs.fragmentDeclarations;
  material.customProgramCacheKey = () => `materialSpec-program:${id}:${relief ? "pixel" : "quad"}:${noise}:${liveKey}${bound ? `:${bound}` : ""}`;
  material.userData.liveOperations = live;
  // The program's inputs for this mesh (the shared ones and the receiver's own), for a bake of the same program.
  material.userData.programUniforms = uniforms;
  // The scene fields it samples, which the viewer then draws from the scene's ground (scene-fields.ts).
  material.userData.sceneFields = (shader.renderInputs ?? []).filter(input => input.kind === "field" && input.glsl === "sampler2D").map(input => input.name);
  material.needsUpdate = true;
  return material;
}

/** Decodes an image for a program: repeating, mipmapped, rows as the evaluator reads them. */
export async function programTexture(bytes: Uint8Array, linear: boolean): Promise<THREE.Texture> {
  const bitmap = await createImageBitmap(new Blob([bytes as Uint8Array<ArrayBuffer>]), { imageOrientation: "flipY", premultiplyAlpha: "none", colorSpaceConversion: "none" });
  const texture = new THREE.Texture(bitmap);
  texture.flipY = false;
  texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  texture.colorSpace = linear ? THREE.NoColorSpace : THREE.SRGBColorSpace;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.magFilter = THREE.LinearFilter;
  texture.anisotropy = 8;
  texture.needsUpdate = true;
  return texture;
}
