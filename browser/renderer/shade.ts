import * as THREE from "three";
import { applyLiveOperations } from "./live.ts";

/** A MaterialSpec program as GLSL (`Program::shader` in the engine), with what it needs bound. */
export interface ProgramShader {
  source: string;
  parameters: { name: string; uniform: string; type: "scalar" | "vec2" | "vec3" | "bool" }[];
  seeds: { name: string; uniform: string }[];
  images: { name: string; uniform: string; linear: boolean }[];
  outputs: string[];
  live: { id: string; arguments: Record<string, string> }[];
  features: boolean;
  /** Present on a hybrid plan: fields the CPU bakes, read by the shader from float textures. */
  cpuFields?: CpuField[];
  /** Outputs that read a CPU field. */
  cpuOutputs?: string[];
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
}
/** One material of a shaded scene: its shader, seed values and parameter defaults. */
export interface MaterialShading {
  shader: ProgramShader;
  seedValues: number[];
  parameters: { name: string; default: unknown }[];
  mapping: "uv" | "object" | "world";
}
/** One mesh's receiver inputs: bounds and features as rows of twelve numbers (see `shade_project`). */
export interface MeshShading {
  material: string;
  boundsMin: number[];
  boundsMax: number[];
  features: number[];
  objectToWorld?: number[] | null;
}
export interface SceneShading {
  materials: Record<string, MaterialShading>;
  meshes: Record<string, MeshShading>;
  gravity: number[];
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

const sourceIds = new Map<string, number>();

/**
 * A standard material whose channels a MaterialSpec program computes per pixel instead of reading
 * baked maps. `shared` holds the material's parameter, seed and image uniforms, so one update
 * reaches every mesh that shows it. Relief becomes a normal from the height's screen derivatives.
 */
export function programMaterial(shading: MaterialShading, mesh: MeshShading, gravity: number[], shared: Uniforms): THREE.MeshStandardMaterial {
  const material = new THREE.MeshStandardMaterial({ color: 0xffffff });
  const shader = shading.shader;
  const features = featureTexture(mesh.features);
  const objectFromWorld = new THREE.Matrix4();
  if (mesh.objectToWorld) objectFromWorld.fromArray(mesh.objectToWorld).invert();
  else objectFromWorld.makeTranslation(-mesh.boundsMin[0], -mesh.boundsMin[1], -mesh.boundsMin[2]);
  const uniforms: Uniforms = {
    ...shared,
    ms_features: { value: features }, ms_feature_count: { value: mesh.features.length / 12 },
    ms_gravity: { value: new THREE.Vector3(...(gravity as [number, number, number])) },
    ms_bounds_min: { value: new THREE.Vector3(...(mesh.boundsMin as [number, number, number])) },
    ms_bounds_max: { value: new THREE.Vector3(...(mesh.boundsMax as [number, number, number])) },
    msObjectFromWorld: { value: objectFromWorld },
    msMapping: { value: ["uv", "object", "world"].indexOf(shading.mapping) },
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
      .replace("#include <common>", "#include <common>\nvarying vec3 msWorld;\nvarying vec3 msNormal;\nvarying vec2 msUv;")
      .replace("#include <project_vertex>", `#include <project_vertex>
  msWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;
  msNormal = mat3(modelMatrix) * objectNormal;
  msUv = uv;`);
    program.fragmentShader = program.fragmentShader
      .replace("#include <common>", `#include <common>
varying vec3 msWorld;
varying vec3 msNormal;
varying vec2 msUv;
uniform mat4 msObjectFromWorld;
uniform int msMapping;
${shader.source}`)
      .replace("#include <clipping_planes_fragment>", `#include <clipping_planes_fragment>
  ms_world = msWorld;
  ms_object = (msObjectFromWorld * vec4(msWorld, 1.0)).xyz;
  ms_normal = normalize(msNormal);
  ms_uv = msUv;
  {
    vec3 at = msMapping == 2 ? ms_world : ms_object;
    vec3 facing = abs(msMapping == 1 ? mat3(msObjectFromWorld) * ms_normal : ms_normal);
    ms_surface = msMapping == 0 ? msUv : (facing.z >= max(facing.x, facing.y) ? at.xy : (facing.y >= facing.x ? at.xz : at.yz));
  }
  ms_footprint = max(length(dFdx(msWorld)), length(dFdy(msWorld)));
  ms_uv_footprint = max(length(dFdx(msUv)), length(dFdy(msUv)));
  ms_evaluate();
  diffuseColor = vec4(clamp(ms_base_color, 0.0, 1.0), clamp(ms_opacity, 0.0, 1.0));`)
      .replace("#include <roughnessmap_fragment>", "#include <roughnessmap_fragment>\n  roughnessFactor = clamp(ms_roughness, 0.0, 1.0);")
      .replace("#include <metalnessmap_fragment>", "#include <metalnessmap_fragment>\n  metalnessFactor = clamp(ms_metalness, 0.0, 1.0);")
      .replace("#include <emissivemap_fragment>", "#include <emissivemap_fragment>\n  totalEmissiveRadiance = ms_emissive;")
      .replace("#include <normal_fragment_maps>", `#include <normal_fragment_maps>
  {
    // Relief: the height's gradient over the surface, from screen derivatives.
    vec3 flat_normal = ms_normal;
    vec3 across = dFdx(msWorld), down = dFdy(msWorld);
    vec3 r1 = cross(down, flat_normal), r2 = cross(flat_normal, across);
    float det = dot(across, r1);
    vec3 shaded = abs(det) > 1e-20
      ? normalize(abs(det) * flat_normal - sign(det) * (dFdx(ms_height) * r1 + dFdy(ms_height) * r2)) : flat_normal;
    // receiverNormal turns the relief by the smallest rotation from the receiver normal to it.
    vec3 target = normalize(ms_receiver_normal);
    float cosine = dot(flat_normal, target);
    if (cosine > 1e-6 && cosine < 1.0 - 1e-7) {
      vec3 axis = cross(flat_normal, target);
      vec3 turned = cross(axis, shaded);
      shaded = normalize(shaded + turned + cross(axis, turned) / (1.0 + cosine));
    }
    vec3 bent = shaded + ms_normal_offset;
    if (dot(bent, bent) > 1e-12 && dot(bent, flat_normal) > 0.0) shaded = normalize(bent);
    normal = normalize((viewMatrix * vec4(shaded, 0.0)).xyz) * faceDirection;
  }`);
  };
  let id = sourceIds.get(shader.source);
  if (id === undefined) { id = sourceIds.size; sourceIds.set(shader.source, id); }
  const liveKey = material.customProgramCacheKey();
  material.customProgramCacheKey = () => `materialSpec-program:${id}:${liveKey}`;
  material.userData.liveOperations = live;
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
