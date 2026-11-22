import * as THREE from "three";
import { GpuBaker, type IntrinsicDomain, type StackInput } from "../../browser/renderer/bake.ts";
import { atlasLayouts, bakeGeometry, type AtlasLayout, type ProjectLayouts } from "../../browser/renderer/atlas.ts";
import type { GraphInstance, NodeSpec, TargetSpec } from "../../browser/renderer/bake-graph.ts";
import type { MaterialShading, Reads } from "../../browser/renderer/shade.ts";

/**
 * The pass graph's node types on a real WebGL context, over synthetic targets: the separable blur (on any node) and
 * the intrinsic-plus-mapping shape (one shared intrinsic node read by a mapping node per receiver). Nothing here is a
 * material; `tests/bake-graph-browser.mjs` drives it and checks the texels against values computed in Node.
 */

const renderer = new THREE.WebGLRenderer({ canvas: document.querySelector("canvas")!, antialias: false });
const baker = new GpuBaker(renderer);
const graph = baker.graph;

const float = (name: string, width: number, height: number, filter: TargetSpec["filter"] = "nearest", type: THREE.TextureDataType = THREE.FloatType): TargetSpec =>
  ({ width, height, format: THREE.RGBAFormat, type, colorSpace: THREE.NoColorSpace, filter, wrap: THREE.ClampToEdgeWrapping, name });

function read(instance: GraphInstance, id: string): number[] {
  const { target } = instance.node(id)!;
  const texels = new Float32Array(target.width * target.height * 4);
  renderer.readRenderTargetPixels(target, 0, 0, target.width, target.height, texels);
  return [...texels];
}

// An impulse in red, a step in green and a ramp in blue; `msGain` scales them (a value in the node's key).
const PATTERN = `
uniform float msGain;
layout(location = 0) out vec4 msOut;
void main() {
  ivec2 p = ivec2(gl_FragCoord.xy);
  msOut = msGain * vec4(p == ivec2(20, 12) ? 1.0 : 0.0, p.x >= 16 ? 1.0 : 0.0, float(p.y) / 8.0, 1.0);
}`;

const WIDTH = 40, HEIGHT = 24;
function blurSpecs(gain: number, radius: number): NodeSpec[] {
  return [
    { id: "source", key: `pattern:${gain}`, target: float("pattern", WIDTH, HEIGHT), op: baker.shaderOp("pattern", PATTERN, () => ({ msGain: gain })) },
    ...baker.blurNodes("blurred", "source", float("blurred", WIDTH, HEIGHT), radius),
  ];
}

// The texture's own coordinate in red and green: an intrinsic bake whose value at a surface point is that point.
const COORDINATE = `
uniform vec2 msSize;
uniform float msGain;
layout(location = 0) out vec4 msOut;
void main() { msOut = vec4(msGain * gl_FragCoord.xy / msSize, 0.0, 1.0); }`;

/** One triangle record as `authoring/layout.rs` writes it (38 numbers), on the plane y = 0 facing -y. */
function record(rect: number[], turn: number, samples: number[], role: number, positions: number[][]): number[] {
  return [...rect, turn, ...samples, role, ...positions.flat(), ...positions.flatMap(() => [0, -1, 0]), ...positions.flatMap(point => [point[0], point[2]]), 0, 0, 0, 0, 0, 0];
}

/** A square of `size` meters at `x`, as two triangles sharing one 12 x 12 chart (gutter 2, 8 x 8 samples) of a 16 x 16 atlas. */
function square(x: number, size: number): AtlasLayout {
  const layouts: ProjectLayouts = { gutter: 2, record: 38, receivers: [{ id: "square", material: "m", width: 16, height: 16, density: 8 / size, triangles: 2, offset: 0 }],
    records: new Float32Array([
      ...record([0, 0, 12, 12], 0, [8, 8], 1, [[x, 0, 0], [x + size, 0, 0], [x, 0, size]]),
      ...record([0, 0, 12, 12], 2, [8, 8], 2, [[x + size, 0, size], [x, 0, size], [x + size, 0, 0]]),
    ]) };
  return atlasLayouts(layouts).get("square")!;
}

const DOMAIN: IntrinsicDomain = { origin: [0, 0], size: [4, 4], density: 16 };
const receiverUniforms = { msMapping: { value: 2 }, msObjectFromWorld: { value: new THREE.Matrix4() } };

function mappingSpecs(gain: number, x: number, size: number): NodeSpec[] {
  const layout = square(x, size);
  const atlas = { layout, geometry: bakeGeometry(layout), extent: [1, 1] as [number, number] };
  return [
    // Keyed by the "program" and its value alone: no receiver is in it.
    { id: "intrinsic", key: `coordinate:${gain}`, target: float("intrinsic", 64, 64, "mips", THREE.HalfFloatType),
      op: baker.shaderOp("intrinsic", COORDINATE, () => ({ msSize: new THREE.Vector2(64, 64), msGain: gain })) },
    { id: "mapped", key: GpuBaker.mappingKey(atlas, receiverUniforms, DOMAIN), target: float("mapped", 16, 16), op: baker.mappingOp(atlas, receiverUniforms, DOMAIN), inputs: { source: "intrinsic" } },
  ];
}

const held = new Map<string, GraphInstance>();

/** A planar wall and an ordinary return with different coordinates, drawn into the same material atlas. */
async function mixedWallMaps(tiled: boolean, returnX: number, mode = 2, anchorOffset = 0, compact = true, width = 24, faceWidth = 1,
  zoneLayers?: { shortcut: boolean; counts: number[]; changeDuringBake?: boolean }) {
  const points = [[0, 0, 0], [1, 0, 0], [0, 0, 1]];
  const face = record([0, 0, 12, 12], 0, [8, 8], 3, points.map(([x, y, z]) => [x * faceWidth, y, z]));
  face.splice(32, 6, 2 / width, 10 / 12, (2 + 8 * faceWidth) / width, 10 / 12, 2 / width, 2 / 12);
  const edge = record([12, 0, 12, 12], 0, [8, 8], 0, points.map(([x, y, z]) => [x + returnX, y, z]));
  const step = 0.125;
  const layout = atlasLayouts({ gutter: 2, record: 38, records: new Float32Array([...face, ...edge]),
    receivers: [{ id: "mix", material: "base", density: 8, offset: 0, triangles: 2, width, height: 12,
      surface: [0, 0, step / 2, step / 2, step, 0, 0, step, step, 1] }] }).get("mix")!;
  const inputs = "vec3 ms_world, ms_object, ms_normal; vec2 ms_surface, ms_uv; float ms_footprint, ms_uv_footprint; ivec2 ms_texel;";
  const reads: Reads = { parameters: [], seed: false, images: [], edges: false, features: false,
    sample: ["surface", "footprint"], cpuFields: [], planes: [], intrinsic: true };
  const base: MaterialShading = {
    parameters: [], seedValues: [], mapping: "world",
    shader: { source: inputs + "vec3 ms_base_color, ms_emissive; float ms_metalness; void ms_evaluate(){ ms_base_color=vec3(ms_surface.x * 0.1 + 0.3, ms_surface.y * 0.2 + 0.2, 0.1); ms_metalness=ms_surface.y * 0.05 + 0.5; ms_emissive=vec3(0.02); }",
      parameters: [], seeds: [], images: [], features: false, outputs: ["base_color", "metalness", "emissive"], live: [],
      reads: { outputs: { base_color: reads, metalness: reads, emissive: compact ? { ...reads, sample: [] } : { ...reads, intrinsic: false } } } },
  };
  const layer: MaterialShading = { ...base, shader: { ...base.shader, outputs: ["roughness"], reads: undefined,
    cpuFields: [{ name: "below.emissive", uniform: "msBelow", type: "vec3", parameters: [], seed: false, images: [], edges: false }],
    source: inputs + "uniform highp sampler2D msBelow; float ms_roughness; void ms_evaluate(){ ms_roughness = clamp(ms_world.x * 0.2 + texelFetch(msBelow, ms_texel, 0).x * 12.5, 0.0, 1.0); }" } };
  baker.intrinsicTiles = tiled;
  const uniforms = { ...receiverUniforms, msMapping: { value: mode }, msAnchorFromWorld: { value: new THREE.Matrix4().makeTranslation(anchorOffset, 0, 0) } };
  const geometry = bakeGeometry(layout);
  const stack: StackInput = { layers: [{ id: "stack.1", shading: layer, uniforms, reads: { "base.emissive": "stack.base" }, receiver: "" }],
    channels: { base_color: "stack.base", metalness: "stack.base", emissive: "stack.base", roughness: "stack.1" } };
  if (zoneLayers) {
    const planes = [["base_color", "vec3", "xyz"], ["roughness", "float", "x"], ["normal_offset", "vec3", "xyz"]] as const;
    const cpuFields = planes.map(([channel, type]) => ({
      name: "below." + channel, uniform: "prev_" + channel, type: type === "float" ? "scalar" as const : type,
      parameters: [], seed: false, images: [], edges: false,
    }));
    const shading: MaterialShading = { ...base, shader: { ...base.shader, reads: undefined, parameters: [], zones: ["zone"],
      outputs: planes.map(([channel]) => channel), passthroughWithoutZones: zoneLayers.shortcut, cpuFields,
      source: inputs + "uniform int ms_zone_count;\n" + planes.map(([channel, type]) =>
        `uniform highp sampler2D prev_${channel}; ${type} ms_${channel};`).join("\n") + "\nvoid ms_evaluate(){\n" +
        planes.map(([channel, type, components]) =>
          `ms_${channel} = texelFetch(prev_${channel}, ms_texel, 0).${components} + ${type}(ms_zone_count > 0 ? 0.1 : 0.0);`).join("\n") + "}",
    } };
    for (const count of zoneLayers.counts) {
      const id = "stack." + (stack.layers.length + 1), inputs: Record<string, string> = {};
      for (const [channel] of planes) {
        if (stack.channels[channel]) inputs["base." + channel] = stack.channels[channel];
        stack.channels[channel] = id;
      }
      stack.layers.push({ id, shading, uniforms: { ...uniforms, ms_zone_count: { value: count } }, reads: inputs, receiver: "" });
    }
  }
  let changed = false;
  const current = () => {
    if (zoneLayers?.changeDuringBake && !changed) {
      const count = stack.layers.at(-1)!.uniforms.ms_zone_count;
      count.value = count.value ? 0 : 1;
      changed = true;
    }
    return true;
  };
  const baked = await baker.bakeStack(base, uniforms, stack, { layout, geometry, extent: [1, 1], receiver: "" }, undefined, { current });
  if (zoneLayers?.changeDuringBake) {
    geometry.dispose();
    if (baked) baker.releaseAtlasMaps(baked.maps);
    if (baked || !changed) throw Error("A changed zone binding must abandon the stale graph");
    return null;
  }
  if (!baked) throw Error("The wall bake was abandoned");
  try {
    const bytes = (id: string) => {
      const samples = new Uint8Array(width * 12 * 4);
      renderer.readRenderTargetPixels(baked.maps.nodes.node(id)!.target, 0, 0, width, 12, samples);
      return [...samples];
    };
    const emission = new Uint16Array(width * 12 * 4);
    renderer.readRenderTargetPixels(baked.maps.nodes.node("emissive")!.target, 0, 0, width, 12, emission);
    return { base: bytes("base"), properties: bytes("properties"), normal: zoneLayers && bytes("normal"),
      emissive: [...emission], drawn: baked.timings.drawn };
  } finally { geometry.dispose(); baker.releaseAtlasMaps(baked.maps); }
}
async function realize(name: string, specs: NodeSpec[], bandTexels?: number): Promise<{ drawn: string[]; reused: number; bands: number }> {
  const result = await graph.realize(specs, held.get(name), { bandTexels });
  if (!result) throw new Error("the bake was abandoned");
  held.set(name, result.instance);
  return { drawn: result.drawn, reused: result.reused, bands: result.bands };
}

Object.assign(window, { driver: {
  unavailable: () => baker.unavailable(),
  mixedWallMaps,
  blur: async (gain: number, radius: number, bandTexels?: number) => {
    const made = await realize("blur", blurSpecs(gain, radius), bandTexels);
    return { ...made, width: WIDTH, height: HEIGHT, source: read(held.get("blur")!, "source"), rows: read(held.get("blur")!, "blurred:rows"), blurred: read(held.get("blur")!, "blurred") };
  },
  mapping: async (receiver: string, gain: number, x: number, size: number) => {
    const made = await realize(receiver, mappingSpecs(gain, x, size));
    const instance = held.get(receiver)!;
    return { ...made, mapped: read(instance, "mapped"), intrinsicUsers: instance.node("intrinsic")!.users, intrinsic: instance.node("intrinsic")!.texture.uuid };
  },
  stats: () => ({ ...graph.stats(), ...graph.totals }),
} });
