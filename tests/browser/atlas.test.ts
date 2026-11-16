import test from "node:test";
import assert from "node:assert/strict";
import { atlasLayouts, bakeGeometry, displayGeometry, programLayout, type ProjectLayouts } from "../../browser/renderer/atlas.ts";
import type { MaterialShading } from "../../browser/renderer/shade.ts";

/** One triangle record as `authoring/layout.rs` writes it (38 numbers). */
function record(rect: number[], turn: number, samples: number[], role: number, positions: number[][], atlasUvs: number[][]): number[] {
  return [...rect, turn, ...samples, role, ...positions.flat(), ...positions.flatMap(() => [0, -1, 0]), ...positions.flatMap(point => [point[0], point[2]]), ...atlasUvs.flat()];
}

/** A 1 m square as two triangles sharing one 12 x 12 chart (gutter 2) of a 16 x 16 atlas, and a lone triangle beside it. */
const layouts: ProjectLayouts = {
  gutter: 2, record: 38,
  receivers: [{ id: "quad", material: "m", width: 32, height: 16, density: 8, triangles: 3, offset: 0 }],
  records: new Float32Array([
    ...record([0, 0, 12, 12], 0, [8, 8], 1, [[0, 0, 0], [1, 0, 0], [0, 0, 1]], [[2 / 32, 14 / 16], [10 / 32, 14 / 16], [2 / 32, 6 / 16]]),
    ...record([0, 0, 12, 12], 2, [8, 8], 2, [[1, 0, 1], [0, 0, 1], [1, 0, 0]], [[10 / 32, 6 / 16], [2 / 32, 6 / 16], [10 / 32, 14 / 16]]),
    ...record([12, 0, 12, 12], 0, [8, 8], 0, [[2, 0, 0], [3, 0, 0], [2, 0, 1]], [[14 / 32, 14 / 16], [22 / 32, 14 / 16], [14 / 32, 6 / 16]]),
  ]),
};

/** Area of an indexed triangle list in the xy plane. */
function area(positions: ArrayLike<number>, stride: number, indices: ArrayLike<number>): number {
  let sum = 0;
  for (let index = 0; index < indices.length; index += 3) {
    const [a, b, c] = [0, 1, 2].map(corner => [positions[indices[index + corner] * stride], positions[indices[index + corner] * stride + 1]]);
    sum += Math.abs((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / 2;
  }
  return sum;
}

test("receiver layouts expand to display and bake geometry", () => {
  const layout = atlasLayouts(layouts).get("quad")!;
  assert.equal(layout.triangles, 3);
  assert.match(layout.key, /^32x16:2:/);
  const display = displayGeometry(layout);
  assert.equal(display.getAttribute("position").count, 9);
  // The first triangle's second corner: its world position and its atlas UV, as the engine gave them.
  assert.deepEqual([...display.getAttribute("position").array.slice(3, 6)], [1, 0, 0]);
  assert.deepEqual([...display.getAttribute("uv").array.slice(2, 4)], [Math.fround(10 / 32), Math.fround(14 / 16)]);

  const bake = bakeGeometry(layout);
  const vertices = bake.getAttribute("position") as unknown as { data: { array: Float32Array; stride: number } };
  const { array, stride } = vertices.data;
  // Every chart rectangle is covered exactly once, gutters included: the pair's halves share the chart, the lone
  // triangle takes all of its own.
  assert.equal(Math.round(area(array, stride, bake.getIndex()!.array)), 12 * 12 * 2);
  // Each vertex carries its triangle's record; the pair's halves lie on their own sides of the shared edge.
  for (let vertex = 0; vertex < array.length / stride; vertex++) {
    const [x, y] = array.slice(vertex * stride, vertex * stride + 2);
    const rect = array.slice(vertex * stride + 3, vertex * stride + 7), role = array[vertex * stride + 10];
    assert.ok(x >= rect[0] && x <= rect[0] + rect[2] && y >= rect[1] && y <= rect[1] + rect[3], "inside its chart");
    const legs = (x - rect[0] - 2) / 8 + (y - rect[1] - 2) / 8;
    if (role === 1) assert.ok(legs <= 1 + 1e-6, "the first triangle's side");
    if (role === 2) assert.ok(legs >= 1 - 1e-6, "the second triangle's side");
  }
});

test("a different record size is refused", () => {
  assert.throws(() => atlasLayouts({ ...layouts, record: 40 }), /records have 40 numbers/);
});

/** A shader as the engine emits one: every field assigned once, live arguments named by field. */
function shading(live: Record<string, string>, body: string, outputs = ["base_color", "height"]): MaterialShading {
  return {
    shader: { source: `uniform float ms_p0;\nuniform vec3 ms_p1;\nfloat ms_f1; float ms_f2; float ms_f3; float ms_f4; float ms_f5; float ms_f6;\nvoid ms_evaluate() {\n${body}\n}`,
      parameters: [{ name: "gloss", uniform: "ms_p0", type: "scalar" }, { name: "tint", uniform: "ms_p1", type: "vec3" }], seeds: [], images: [], outputs,
      live: [{ id: "bandLuminance", arguments: live }], features: false },
    seedValues: [], parameters: [], mapping: "world",
  };
}

test("live arguments cost a texture channel only when they vary over the surface", () => {
  const layout = programLayout(shading(
    { spacing: "ms_f1", softness: "ms_f2", mask: "ms_f3", lift: "ms_f4", again: "ms_f5", tinted: "ms_f6" },
    "  ms_f1 = 0.25;\n  ms_f2 = mix(0.04, 0.48, ms_p0);\n  ms_f3 = ms_noise(ms_world);\n  ms_f4 = ms_f3;\n  ms_f5 = (1.0 - ms_f3);\n  ms_f6 = ms_p1.x;"));
  const args = layout.liveOperations[0].arguments;
  assert.deepEqual(args.spacing, { value: 0.25 }, "a constant is a value");
  assert.deepEqual(args.softness, { glsl: "mix(0.04, 0.48, ms_p0)" }, "a function of scalar parameters is evaluated by the material");
  assert.deepEqual(layout.liveParameters, [{ uniform: "ms_p0", type: "float" }]);
  // A field that repeats another shares its channel; an expression of a varying field is a channel of its own, and so
  // is anything that reads a parameter this does not hand to the material (a vector's component).
  assert.deepEqual(layout.liveChannels, ["ms_f3", "ms_f5", "ms_f6"]);
  assert.deepEqual(args.mask, { texture: "gpu-live-0", channel: 0, scale: 1, offset: 0 });
  assert.deepEqual(args.lift, args.mask);
  assert.deepEqual(args.again, { texture: "gpu-live-0", channel: 1, scale: 1, offset: 0 });
  assert.deepEqual(layout.attachments, ["base", "properties", "height", "live0"]);
});

test("the evaluate pass writes only what the program has outputs for", () => {
  const plain = programLayout(shading({}, "", ["base_color"]));
  assert.deepEqual(plain.attachments, ["base", "properties", "height"]);
  assert.equal(plain.orient, false);
  const full = programLayout(shading({}, "", ["base_color", "emissive", "normal_offset", "receiver_normal"]));
  assert.deepEqual(full.attachments, ["base", "properties", "height", "emissive", "offset"]);
  assert.equal(full.orient, true);
  assert.notEqual(plain.key, full.key);
});
