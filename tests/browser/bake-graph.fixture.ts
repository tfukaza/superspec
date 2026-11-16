import * as THREE from "three";
import { GpuBaker, type IntrinsicDomain } from "../../browser/renderer/bake.ts";
import { atlasLayouts, bakeGeometry, type AtlasLayout, type ProjectLayouts } from "../../browser/renderer/atlas.ts";
import type { GraphInstance, NodeSpec, TargetSpec } from "../../browser/renderer/bake-graph.ts";

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
async function realize(name: string, specs: NodeSpec[], bandTexels?: number): Promise<{ drawn: string[]; reused: number; bands: number }> {
  const result = await graph.realize(specs, held.get(name), { bandTexels });
  if (!result) throw new Error("the bake was abandoned");
  held.set(name, result.instance);
  return { drawn: result.drawn, reused: result.reused, bands: result.bands };
}

Object.assign(window, { driver: {
  unavailable: () => baker.unavailable(),
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
