import test from "node:test";
import assert from "node:assert/strict";
import { BufferGeometry, Float32BufferAttribute } from "three";
import { featureTexture, featureUniforms } from "../../browser/renderer/feature-texture.ts";
import { PLANE_VALUES, TILE_CELLS, TILE_START, atlasCoverage, atlasFragment, atlasLatticeKey, atlasLayouts, atlasPlaneLayoutKey, attachmentReads, bakeGeometry, carryIndexedTints, displayGeometry, latticeTiles, outputReads, planeFragment, programLayout, uniformAttachment, usesAtlasUniform,
  type ProjectLayouts } from "../../browser/renderer/atlas.ts";
import type { MaterialShading, Reads } from "../../browser/renderer/shade.ts";
import { hashFloats } from "../../browser/renderer/layout-digest.ts";

test("receiver feature textures distinguish reflected line endpoints", () => {
  // A source edge with both endpoint x coordinates mirrored used to collide at every FNV word seed.
  const left = Float32Array.of(3, 0, 2, 1, 4, 0, 2, 1, 0, -1, 0, 0);
  const right = left.slice(); right[0] = -3; right[4] = -4;
  assert.notEqual(hashFloats(left), hashFloats(right));
});

test("feature lookups preserve eligible segment order across named, geometric, overlapping and empty selectors", () => {
  const kinds = [6, 2, 5, 1, 6, 0, 8, 4, 2, 5, 3];
  const rows = kinds.flatMap((kind, id) => [id, 0, 0, kind, id, 0, 1, 25, 1, 0, 0, 1]);
  const texture = featureTexture(rows), uniforms = featureUniforms(texture, kinds.length);
  const data = texture.image.data as Float32Array;
  assert.equal(uniforms.ms_feature_count.value, kinds.length);
  assert.ok(uniforms.ms_feature_selectors.value >= 12);
  const selected = (selector: number, kind: number) =>
    [[0], [1], [2], [0, 2], [0, 1, 2], [3], [4], []][selector]?.includes(kind) ?? kind === selector - 3;
  for (let selector = 0; selector < uniforms.ms_feature_selectors.value; selector++) {
    const range = kinds.length * 12 + selector * 4;
    const seen = [];
    for (let i = data[range]; i < data[range + 1]; i++) {
      if (selected(selector, data[i * 12 + 3])) seen.push(data[i * 12]);
    }
    assert.deepEqual(seen, kinds.flatMap((kind, id) => selected(selector, kind) ? [id] : []), `selector ${selector}`);
  }
  texture.dispose();
  for (const count of [0, 1, 4096]) {
    const original = Array.from({ length: count * 12 }, (_, i) => i % 12 === 3 ? 2 : 0);
    const plain = featureTexture(original);
    assert.equal(featureUniforms(plain, count).ms_feature_selectors.value, 0);
    assert.equal(plain.image.height, Math.max(1, count));
    plain.dispose();
  }
});

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

test("an edit may reuse a spatially uniform output only over the same chart coverage", () => {
  const layout = atlasLayouts(layouts).get("quad")!;
  const resized = { ...layout, records: layout.records.slice() };
  resized.records[8] += 0.001;
  assert.equal(atlasCoverage(layout), atlasCoverage(resized));
  const repacked = { ...resized, records: resized.records.slice() };
  repacked.records[0] += 1;
  assert.notEqual(atlasCoverage(layout), atlasCoverage(repacked));
  const shader = shading({}, "", ["base_color", "roughness"]).shader;
  assert.equal(uniformAttachment(shader, "properties"), false);
  const flat: Reads = { parameters: ["ms_p0"], seed: false, images: [], edges: false, features: false,
    sample: [], cpuFields: [], planes: [], intrinsic: true };
  shader.reads = { outputs: { base_color: { ...flat, sample: ["surface"] }, roughness: flat } };
  assert.equal(uniformAttachment(shader, "properties"), true);
  assert.equal(uniformAttachment(shader, "base"), false);
  shader.reads.outputs.roughness = { ...flat, features: true };
  assert.equal(uniformAttachment(shader, "properties"), false);
});

test("local chart corners keep their own indexed colors through a rigid move and reject a split face", () => {
  const source = new BufferGeometry(), display = new BufferGeometry();
  const corners = [0, 0, 0, 1, 0, 0, 0, 1, 0];
  source.setAttribute("position", new Float32BufferAttribute([...corners, ...corners], 3));
  source.setAttribute("color", new Float32BufferAttribute([1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1], 3));
  source.setIndex([0, 1, 2, 3, 4, 5]);
  display.setAttribute("position", new Float32BufferAttribute([...corners.slice(3), ...corners.slice(0, 3), ...corners], 3));
  assert.ok(carryIndexedTints(source, display));
  const colors = display.getAttribute("color");
  assert.deepEqual([...colors.array], [0,1,0, 0,0,1, 1,0,0, 0,0,0, 1,1,0, 1,1,1]);
  source.translate(7, 11, 0); display.translate(7, 11, 0);
  assert.ok(carryIndexedTints(source, display));
  assert.equal(display.getAttribute("color"), colors);
  display.getAttribute("position").setX(1, 7.125);
  assert.equal(carryIndexedTints(source, display), false);
});

test("receiver layouts expand to display and bake geometry", () => {
  const layout = atlasLayouts(layouts).get("quad")!;
  assert.equal(layout.triangles, 3);
  assert.match(layout.key, /^32x16:2:/);
  const display = displayGeometry(layout);
  assert.equal(display.getAttribute("position").count, 9);
  // The first triangle's second corner: its world position and its atlas UV, as the engine gave them.
  assert.deepEqual([...display.getAttribute("position").array.slice(3, 6)], [1, 0, 0]);
  assert.deepEqual([...display.getAttribute("uv").array.slice(2, 4)], [Math.fround(10 / 32), Math.fround(14 / 16)]);
  const placed = displayGeometry(layout, [8, -3.5, 1.25]);
  assert.deepEqual([...placed.getAttribute("position").array.slice(3, 6)], [9, -3.5, 1.25]);
  assert.deepEqual(placed.getAttribute("uv").array, display.getAttribute("uv").array);
  assert.deepEqual([...layout.records.slice(11, 14)], [1, 0, 0]);

  const bake = bakeGeometry(layout);
  const corners = bake.getAttribute("normal") as unknown as { offset: number; data: { array: Float32Array; stride: number } };
  const { array, stride } = corners.data;
  // Every chart rectangle is covered exactly once, gutters included: the pair's halves share the chart, the lone
  // triangle takes all of its own.
  assert.equal(bake.instanceCount, 3);
  assert.ok(Object.keys(bake.attributes).length <= 16);
  let covered = 0;
  // Each vertex carries its triangle's record; the pair's halves lie on their own sides of the shared edge.
  for (let instance = 0; instance < bake.instanceCount; instance++) {
    const start = instance * stride;
    const points = [...array.slice(start + corners.offset, start + corners.offset + 8), ...array.slice(start + 26, start + 28)];
    const rect = array.slice(start, start + 4), role = array[start + 7];
    covered += area(points, 2, bake.getIndex()!.array);
    for (let i = 0; i < points.length; i += 2) {
      const x = points[i], y = points[i + 1];
      assert.ok(x >= rect[0] && x <= rect[0] + rect[2] && y >= rect[1] && y <= rect[1] + rect[3], "inside its chart");
      const legs = (x - rect[0] - 2) / 8 + (y - rect[1] - 2) / 8;
      if (role === 1) assert.ok(legs <= 1 + 1e-6, "the first triangle's side");
      if (role === 2) assert.ok(legs >= 1 - 1e-6, "the second triangle's side");
    }
  }
  assert.equal(Math.round(covered), 12 * 12 * 2);
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
  // Reads the engine states (`ShaderReads`) take the place of the entry points' text; an output it does not list may
  // read anything, and one the program does not set reads nothing.
  const reads = (parameters: string[], more: Partial<Reads> = {}): Reads =>
    ({ parameters, seed: false, images: [], edges: false, features: false, sample: ["world"], cpuFields: [], planes: [], intrinsic: false, ...more });
  program.shader.reads = { outputs: { base_color: reads(["ms_p1"], { images: ["decal"] }), roughness: reads(["ms_p0"]), height: reads([]),
    emissive: reads(["ms_p1"], { seed: true }) } };
  assert.deepEqual(sets(attachmentReads(program.shader, layout)), { base: ["ms_image0", "ms_p1"], properties: ["ms_p0"], height: [], emissive: ["ms_p1", "ms_seed0"] },
    "a read seed is the named seeds' uniforms and an image its uniform; the live arguments are not listed, so the live texture is keyed by everything");
  // A live texture reads what every argument given one of its channels reads; other arguments' reads are not its own.
  program.shader.reads.live = { bandLuminance: { mask: reads(["ms_p0"], { seed: true }) } };
  assert.deepEqual(sets(attachmentReads(program.shader, layout)).live0, ["ms_p0", "ms_seed0"]);
  // An output of the texture that the record leaves out leaves the texture keyed by everything.
  delete program.shader.reads.outputs.roughness;
  assert.equal(attachmentReads(program.shader, layout).properties, undefined);
  // A record of another shape says nothing.
  program.shader.reads = { base_color: ["ms_p1"], live: ["ms_p0"] } as unknown as typeof program.shader.reads;
  assert.deepEqual(attachmentReads(program.shader, layout), {});
});

test("arguments that share a live channel and CPU fields are among a texture's stated reads", () => {
  const program = shading({ mask: "ms_f3", lift: "ms_f3", wide: "ms_f5", flat: "ms_f6" }, "", ["base_color", "height"]);
  program.shader.live[0].classes = { mask: { varying: "ms_f3" }, lift: { varying: "ms_f3" }, wide: { varying: "ms_f5" }, flat: { parameters: ["ms_p0"], glsl: "ms_p0" } };
  program.shader.cpuFields = [
    { name: "runoff", uniform: "ms_cpu0", type: "scalar", parameters: [], seed: false, images: [], edges: true },
    { name: "stain", uniform: "ms_cpu1", type: "scalar", parameters: [], seed: false, images: [], edges: false, planes: ["wet"] },
    { name: "grime", uniform: "ms_cpu2", type: "scalar", parameters: ["ms_p1"], seed: false, images: [], edges: false },
  ];
  const layout = programLayout(program);
  const reads = (parameters: string[], more: Partial<Reads> = {}): Reads =>
    ({ parameters, seed: false, images: [], edges: false, features: false, sample: ["world"], cpuFields: [], planes: [], intrinsic: false, ...more });
  const operation = program.shader.live[0].id;
  program.shader.reads = {
    outputs: { base_color: reads([], { cpuFields: ["grime"] }), height: reads([], { edges: true, planes: ["wet"] }) },
    live: { [operation]: { mask: reads(["ms_p0"]), lift: reads(["ms_p1"]), wide: reads([], { cpuFields: ["runoff"], edges: true }), flat: reads(["ms_p0"]) } },
  };
  const found = attachmentReads(program.shader, layout);
  assert.deepEqual([...found.base!].sort(), ["ms_cpu2"]);
  assert.deepEqual([...found.height!].sort(), ["ms_cpu0", "ms_cpu1"], "edges and planes are the CPU fields that read them");
  assert.deepEqual([...found.live0!].sort(), ["ms_cpu0", "ms_p0", "ms_p1"], "both arguments of the shared channel, and the other channel of the texture");
  assert.deepEqual([...found.properties!], [], "outputs the program does not set read nothing");
  // An argument with a channel that the record leaves out leaves the texture keyed by everything.
  delete program.shader.reads.live![operation].lift;
  assert.equal(attachmentReads(program.shader, layout).live0, undefined);
});

test("atlas planes key the object's frame, physical UV extent and local grime only where the shader reads them", () => {
  const program = shading({}, "", ["base_color", "height"]), shader = program.shader;
  const plain: Reads = { parameters: [], seed: false, images: [], edges: false, features: false, sample: ["surface", "footprint"], cpuFields: [], planes: [], intrinsic: true };
  shader.reads = { outputs: { base_color: plain, height: { ...plain, sample: ["object", "bounds", "gravity"], features: true, intrinsic: false } } };
  const { base, height } = attachmentReads(shader, programLayout(program), true);
  assert.deepEqual(outputReads(shader, ["base_color"], true), base);
  for (const mode of [0, 1, 2, 3]) {
    assert.equal(usesAtlasUniform(base, mode, "msMapping"), true);
    assert.equal(usesAtlasUniform(base, mode, "msObjectFromWorld"), mode === 1);
    assert.equal(usesAtlasUniform(base, mode, "msAnchorFromWorld"), mode === 3);
    assert.equal(usesAtlasUniform(base, mode, "msExtent"), mode === 0);
    assert.equal(usesAtlasUniform(height, mode, "msObjectFromWorld"), true);
    assert.equal(usesAtlasUniform(height, mode, "msMapping"), false);
    for (const name of ["ms_gravity", "ms_bounds_min", "ms_bounds_max", "ms_features", "ms_feature_count", "ms_feature_selectors", "ms_zone_count"]) {
      assert.equal(usesAtlasUniform(base, mode, name), false);
      assert.equal(usesAtlasUniform(height, mode, name), true);
    }
  }
  shader.reads.outputs.base_color = { ...plain, sample: ["future-input"] };
  assert.equal(attachmentReads(shader, programLayout(program), true).base, undefined);
  assert.equal(usesAtlasUniform(undefined, 2, "msObjectFromWorld"), true);
  assert.equal(usesAtlasUniform(undefined, 2, "ms_features"), true);
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

test("a layer's plane program reads the planes beneath it by framebuffer row, and nothing else is turned over", () => {
  const source = "uniform highp sampler2D ms_cpu0;\nuniform highp sampler2D ms_cpu1;\nvoid ms_evaluate() {\n  ms_f0 = texelFetch(ms_cpu0, ms_texel, 0).xyz;\n  ms_f1 = texelFetch(ms_cpu1, ms_texel, 0).x;\n}";
  const fragment = planeFragment(source, [PLANE_VALUES.base_color], ["ms_cpu0"]);
  assert.match(fragment, /msPlaneAt\(ms_cpu0, msPlaneTexel\(ms_texel\)\)/);
  // A CPU field the host bakes keeps the engine's rows, from the atlas's upper edge.
  assert.match(fragment, /texelFetch\(ms_cpu1, ms_texel, 0\)/);
  assert.match(fragment, /int\(msAtlas\.y\) - 1 - texel\.y/);
  assert.throws(() => planeFragment("void ms_evaluate() { ms_f0 = texture(ms_cpu0, ms_uv).xyz; }", [PLANE_VALUES.base_color], ["ms_cpu0"]), /form the GPU stack does not know/);
  // Without planes beneath, the program is the evaluate pass's (an atlas bake's text, with plane values).
  assert.equal(planeFragment(source, ["X"], []), atlasFragment(source, { ...programLayout(shading({}, "", ["base_color"])), attachments: ["base"] }).replace("vec4(clamp(ms_base_color, 0.0, 1.0), 1.0)", "X"));
});

test("a receiver of whole flat faces on a lattice reads the intrinsic tiles its cells lie in", () => {
  // One 20 x 12 chart (gutter 2) of a face on a 1/128 m lattice whose first inner texel is cell (3, 5).
  const step = 1 / 128, face = record([0, 0, 20, 12], 0, [16, 8], 3, [[0, 0, 0], [1, 0, 0], [0, 0, 1]], [[2 / 32, 14 / 16], [18 / 32, 14 / 16], [2 / 32, 6 / 16]]);
  const surface = [0, 0, 3.5 * step, 5.5 * step, step, 0, 0, step, step, 1];
  const lattice: ProjectLayouts = { gutter: 2, record: 38, receivers: [{ id: "wall", material: "m", width: 32, height: 16, density: 128, triangles: 1, offset: 0, surface }], records: new Float32Array(face) };
  const tiles = latticeTiles(atlasLayouts(lattice).get("wall")!)!;
  assert.equal(tiles.step, Math.fround(step));
  assert.equal(tiles.mixed, false);
  // Cells 0 to 21 across and 2 to 15 down (the gutter included, one cell either side): all in the first tile.
  assert.deepEqual(tiles.tiles, [[0, 0]]);
  // Moved to straddle a tile's edge, it reads two.
  const moved = { ...lattice, receivers: [{ ...lattice.receivers[0], surface: [0, 0, (TILE_CELLS + TILE_START - 5.5) * step, 5.5 * step, step, 0, 0, step, step, 1] }] };
  assert.deepEqual(latticeTiles(atlasLayouts(moved).get("wall")!)!.tiles, [[0, 0], [1, 0]]);
  const mixed = { ...lattice, receivers: [{ ...lattice.receivers[0], triangles: 2 }], records: new Float32Array([...face, ...layouts.records.slice(-38)]) };
  assert.deepEqual(latticeTiles(atlasLayouts(mixed).get("wall")!), { ...tiles, mixed: true });
  // A chart off the lattice (a lone triangle) reads none.
  assert.equal(latticeTiles(atlasLayouts(layouts).get("quad")!), null);
});

test("a face can reveal another part of the same lattice rectangle without redrawing its intrinsic material", () => {
  const surface = [0, 0, 0.0625, 0.0625, 0.125, 0, 0, 0.125, 0.125, 1];
  const face = record([0, 0, 12, 12], 0, [8, 8], 3,
    [[0, 0, 0], [0.75, 0, 0], [0, 0, 1]], [[2 / 32, 14 / 16], [8 / 32, 14 / 16], [2 / 32, 6 / 16]]);
  const enlarged = face.slice();
  enlarged[11] = 0.875;
  enlarged[34] = 9 / 32;
  const bevel = [...layouts.records.slice(-38)];
  const layout = (records: number[], map = surface) => atlasLayouts({
    ...layouts, receivers: [{ ...layouts.receivers[0], triangles: records.length / 38, surface: map }], records: Float32Array.from(records),
  }).get("quad")!;
  const original = layout([...face, ...bevel]), resized = layout([...enlarged, ...bevel]);
  const faceDraws = (receiver: ReturnType<typeof layout>) => {
    const geometry = bakeGeometry(receiver);
    const kind = geometry.getAttribute("aSurface1");
    const draws = Array.from({ length: geometry.instanceCount }, (_, i) => kind.getW(i));
    geometry.dispose();
    return draws;
  };
  assert.deepEqual(faceDraws(original), [2, 0], "the backing rectangle already covers its first triangle");
  assert.deepEqual(faceDraws(layout([...face, ...enlarged, ...bevel])), [2, 1, 0], "subsequent triangles keep their own records");
  assert.notEqual(original.key, resized.key);
  assert.equal(atlasLatticeKey(original), atlasLatticeKey(resized));
  assert.equal(atlasCoverage(original), atlasCoverage(resized));
  const outside = enlarged.slice();
  outside[34] = 14 / 32;
  const malformed = layout([...outside, ...bevel]);
  assert.deepEqual(faceDraws(malformed), [2, 1, 0], "a first triangle extending outside its rectangle still draws");
  assert.equal(atlasCoverage(malformed), malformed.key);
  assert.equal(latticeTiles(malformed), null);
  // The same geometry with another pattern origin reads different cells.
  assert.notEqual(atlasLatticeKey(original), atlasLatticeKey(layout([...face, ...bevel], [0, 0, 0.1875, ...surface.slice(3)])));
  const movedBevel = bevel.slice();
  movedBevel[11] += 0.001;
  assert.notEqual(atlasLatticeKey(original), atlasLatticeKey(layout([...face, ...movedBevel])));
  // Word-wise FNV collides for exactly these two sign-bit flips even with two seeds.
  const mirrored = bevel.slice();
  mirrored[8] *= -1;
  mirrored[18] *= -1;
  const reflected = layout([...face, ...mirrored]);
  assert.notEqual(original.key, reflected.key);
  assert.notEqual(atlasLatticeKey(original), atlasLatticeKey(reflected));
  assert.equal(atlasCoverage(original), atlasCoverage(reflected));
  const shader = shading({}, "", ["base_color"]).shader;
  const read: Reads = { parameters: [], images: [], seed: false, edges: false, features: false, sample: [],
    cpuFields: ["below.base_color"], planes: ["base.base_color"], intrinsic: false };
  shader.reads = { outputs: { base_color: read } };
  assert.equal(atlasPlaneLayoutKey(original, shader, "base_color"), atlasPlaneLayoutKey(resized, shader, "base_color"));
  read.sample = ["world"];
  assert.notEqual(atlasPlaneLayoutKey(original, shader, "base_color"), atlasPlaneLayoutKey(resized, shader, "base_color"));
  read.sample = []; read.features = true;
  assert.notEqual(atlasPlaneLayoutKey(original, shader, "base_color"), atlasPlaneLayoutKey(resized, shader, "base_color"));
});
