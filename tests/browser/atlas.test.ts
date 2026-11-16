import test from "node:test";
import assert from "node:assert/strict";
import { atlasLayouts, attachmentReads, bakeGeometry, displayGeometry, programLayout, type ProjectLayouts } from "../../browser/renderer/atlas.ts";
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

test("the engine's classes of live arguments lay out the pass without reading the source", () => {
  // The source says nothing a layout could be read from: only the record's classes and uniform declarations are used.
  const stated = shading({ spacing: "ms_f1", softness: "ms_f2", mask: "ms_f3", lift: "ms_f4", wide: "ms_f5" }, "");
  stated.shader.source = "void ms_evaluate() {}";
  stated.shader.uniforms = [{ name: "ms_p0", type: "float" }, { name: "ms_p1", type: "vec3" }];
  stated.shader.live[0].classes = { spacing: { constant: 0.25 }, softness: { parameters: ["ms_p0"], glsl: "mix(0.04, 0.48, ms_p0)" },
    mask: { varying: "ms_f3" }, lift: { varying: "ms_f3" }, wide: { varying: "ms_f5" } };
  const layout = programLayout(stated);
  const args = layout.liveOperations[0].arguments;
  assert.deepEqual(args.spacing, { value: 0.25 });
  assert.deepEqual(args.softness, { glsl: "mix(0.04, 0.48, ms_p0)" });
  assert.deepEqual(layout.liveParameters, [{ uniform: "ms_p0", type: "float" }]);
  assert.deepEqual(layout.liveChannels, ["ms_f3", "ms_f5"], "arguments that are one value share its channel");
  assert.deepEqual(args.lift, args.mask);
  assert.deepEqual(args.wide, { texture: "gpu-live-0", channel: 1, scale: 1, offset: 0 });
  assert.deepEqual(layout.attachments, ["base", "properties", "height", "live0"]);
  // The same record read from its source (no classes) gives the same layout, key included.
  const read = shading({ spacing: "ms_f1", softness: "ms_f2", mask: "ms_f3", lift: "ms_f4", wide: "ms_f5" },
    "  ms_f1 = 0.25;\n  ms_f2 = mix(0.04, 0.48, ms_p0);\n  ms_f3 = ms_noise(ms_world);\n  ms_f4 = ms_f3;\n  ms_f5 = (2.0 * ms_f3);");
  assert.deepEqual(programLayout(read), layout);
  // Classes that miss an argument are not trusted for the operation.
  const partial = structuredClone(stated);
  delete partial.shader.live[0].classes!.wide;
  assert.deepEqual(programLayout(partial).liveChannels, ["ms_f1", "ms_f2", "ms_f3", "ms_f4", "ms_f5"], "read from a source that assigns nothing: every argument varies");
});

test("each texture of the evaluate pass is said to read the uniforms its entry point names", () => {
  const program = shading({ mask: "ms_f3" }, "", ["base_color", "roughness", "height", "emissive"]);
  program.shader.seeds = [{ name: "grain", uniform: "ms_seed0" }];
  program.shader.images = [{ name: "decal", uniform: "ms_image0", linear: false }];
  program.shader.live[0].classes = { mask: { varying: "ms_f3" } };
  const layout = programLayout(program);
  assert.deepEqual(layout.attachments, ["base", "properties", "height", "emissive", "live0"]);
  // Without entry points (or stated reads) nothing is known: every texture may read anything.
  assert.deepEqual(attachmentReads(program.shader, layout), {});
  program.shader.entries = [
    { function: "ms_evaluate_height", outputs: ["height"], live: false, source: "" },
    { function: "ms_evaluate_base", outputs: ["base_color", "opacity"], live: false, source: "void ms_evaluate_base() { ms_f1 = ms_p1.x * texture(ms_image0, ms_uv).r; ms_base_color = vec3(ms_f1); }" },
    { function: "ms_evaluate_properties", outputs: ["roughness", "metalness"], live: false, source: "void ms_evaluate_properties() { ms_roughness = ms_p0; }" },
    { function: "ms_evaluate_relief", outputs: ["height", "receiver_normal"], live: false, source: "void ms_evaluate_relief() { ms_height = ms_noise(ms_world, ms_seed0); }" },
    { function: "ms_evaluate_live", outputs: [], live: true, source: "void ms_evaluate_live() { ms_f3 = ms_p0 * ms_noise(ms_world, ms_seed0); }" },
  ];
  const sets = (reads: ReturnType<typeof attachmentReads>) => Object.fromEntries(Object.entries(reads).map(([name, uniforms]) => [name, [...uniforms!].sort()]));
  assert.deepEqual(sets(attachmentReads(program.shader, layout)), {
    base: ["ms_image0", "ms_p1"], properties: ["ms_p0"], height: ["ms_seed0"], live0: ["ms_p0", "ms_seed0"],
  }, "the emissive texture has no entry point of its own here, so it stays keyed by everything; the height reads the relief entry, not the empty one");
  // A name that only begins like a uniform is not that uniform.
  program.shader.entries[2].source = "void ms_evaluate_properties() { ms_roughness = ms_p10; }";
  assert.deepEqual([...attachmentReads(program.shader, layout).properties!], []);
  // Reads the engine states take the place of the entry points' text; an output it does not list may read anything,
  // and one the program does not set reads nothing.
  program.shader.reads = { base_color: ["ms_p1"], roughness: ["ms_p0"], height: [], emissive: ["ms_p1", "ms_seed0"] };
  assert.deepEqual(sets(attachmentReads(program.shader, layout)), { base: ["ms_p1"], properties: ["ms_p0"], height: [], emissive: ["ms_p1", "ms_seed0"] },
    "the live fields are not listed, so the live texture is keyed by everything");
});

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
