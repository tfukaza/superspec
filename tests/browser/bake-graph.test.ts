import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { BakeGraph, targetBytes, type DrawStep, type GraphDevice, type NodeOp, type NodeSpec, type TargetSpec } from "../../browser/renderer/bake-graph.ts";

/** A device without a GPU: it records what the graph asks of it. */
function device() {
  const log: string[] = [];
  const state = { generation: 0 };
  const made: GraphDevice = {
    generation: () => state.generation,
    mips: textures => log.push(`mips ${textures.map(texture => texture.name).join(",")}`),
    yield: async () => { log.push("yield"); },
  };
  return { device: made, log, state };
}

const target = (name: string, width = 8, height = 8, filter: TargetSpec["filter"] = "nearest"): TargetSpec =>
  ({ width, height, format: THREE.RGBAFormat, type: THREE.UnsignedByteType, colorSpace: THREE.NoColorSpace, filter, wrap: THREE.ClampToEdgeWrapping, name });

/** An operation that records its draws: the node's name, its output, the textures it read and the rows. */
function op(label: string, log: string[], extra: Partial<NodeOp> = {}): NodeOp {
  return { label, draw: (step: DrawStep) => log.push(`${label} ${step.node.texture.name}#${step.output}${Object.entries(step.inputs).map(([role, node]) => ` ${role}=${node.texture.name}`).join("")}${step.rows ? ` rows ${step.rows.start}+${step.rows.count}` : ""}`), ...extra };
}

/** A bake like an atlas bake: two outputs of one program, and a normal made from the height. */
function bake(log: string[], key: string, evaluate = op("evaluate", log), normals = op("normals", log)): NodeSpec[] {
  return [
    { id: "base", key: `${key}:base`, target: target("base", 8, 8, "mips"), op: evaluate, output: 0 },
    { id: "height", key: `${key}:height`, target: target("height"), op: evaluate, output: 1 },
    { id: "normal", key: "normals", target: target("normal", 8, 8, "mips"), op: normals, inputs: { height: "height" } },
  ];
}

test("a node is drawn only when its key or an input's key changed", async () => {
  const { device: made, log } = device();
  const graph = new BakeGraph(made);
  const first = (await graph.realize(bake(log, "a")))!;
  assert.deepEqual(first.drawn, ["base", "height", "normal"]);
  // The evaluate outputs are one level, their mips built after it; the normal follows the height it reads.
  assert.deepEqual(log, ["evaluate base#0", "evaluate height#1", "mips base", "normals normal#0 height=height", "mips normal"]);
  assert.ok(first.instance.complete);

  // Nothing changed: nothing is drawn, and the same textures are held.
  log.length = 0;
  const same = (await graph.realize(bake(log, "a"), first.instance))!;
  assert.deepEqual(same.drawn, []);
  assert.equal(same.reused, 3);
  assert.deepEqual(log, []);
  assert.equal(same.instance.node("base"), first.instance.node("base"));
  assert.ok(first.instance.released, "the instance that was replaced is released");
  assert.equal(same.instance.node("base")!.users, 1);

  // Only the base color's key changed: the height and the normal made from it stay.
  const specs = bake(log, "a");
  specs[0].key = "b:base";
  const base = (await graph.realize(specs, same.instance))!;
  assert.deepEqual(base.drawn, ["base"]);
  assert.deepEqual(log, ["evaluate base#0", "mips base"]);
  // The height's key changed: the normal is drawn again though its own key did not change.
  log.length = 0;
  specs[1].key = "b:height";
  const height = (await graph.realize(specs, base.instance))!;
  assert.deepEqual(height.drawn, ["height", "normal"]);
  assert.equal(height.reused, 1);
  // Drawn over in place: the textures are the ones the first bake made.
  assert.equal(height.instance.node("normal"), first.instance.node("normal"));
});

test("every operation is prepared before anything is drawn, and only when one of its nodes is drawn", async () => {
  const { device: made, log } = device();
  const graph = new BakeGraph(made);
  const evaluate = op("evaluate", log, { prepare: async () => { log.push("prepare evaluate"); } });
  const normals = op("normals", log, { prepare: async () => { log.push("prepare normals"); } });
  const first = (await graph.realize(bake(log, "a", evaluate, normals)))!;
  assert.deepEqual(log.slice(0, 3), ["prepare evaluate", "prepare normals", "evaluate base#0"]);
  log.length = 0;
  await graph.realize(bake(log, "a", evaluate, normals), first.instance);
  assert.deepEqual(log, [], "a bake that draws nothing compiles nothing");
});

test("bakes that want the same texels share the nodes, by users", async () => {
  const { device: made, log } = device();
  const graph = new BakeGraph(made);
  const one = (await graph.realize(bake(log, "a")))!;
  log.length = 0;
  const two = (await graph.realize(bake(log, "a")))!;
  assert.deepEqual(log, []);
  assert.equal(two.instance.node("base"), one.instance.node("base"));
  assert.equal(one.instance.node("base")!.users, 2);
  assert.equal(graph.bytes([one.instance, two.instance]), graph.bytes([one.instance]), "shared textures are counted once");
  assert.equal(graph.bytes([one.instance]), targetBytes(target("", 8, 8, "mips")) * 2 + targetBytes(target("")));

  // A node another bake holds is never drawn over: the changed bake gets new textures, the other keeps its own.
  const changed = (await graph.realize(bake(log, "b"), two.instance))!;
  assert.notEqual(changed.instance.node("base"), one.instance.node("base"));
  assert.equal(one.instance.node("base")!.users, 1);
  assert.ok(one.instance.complete);

  // Released nodes are kept: the same bake later draws nothing.
  graph.release(one.instance);
  graph.release(one.instance);
  assert.equal(graph.stats().idle, 3);
  log.length = 0;
  const again = (await graph.realize(bake(log, "a")))!;
  assert.deepEqual(log, []);
  assert.equal(graph.stats().idle, 0);
  assert.equal(again.instance.node("height")!.users, 1);
});

test("idle nodes are dropped least recently used first, by count and by memory", async () => {
  const { device: made, log } = device();
  const each = targetBytes(target(""));
  const single = (key: string): NodeSpec[] => [{ id: "only", key, target: target(key), op: op("draw", log) }];
  const disposed: string[] = [];
  const watch = (instance: { node(id: string): { target: THREE.WebGLRenderTarget; texture: THREE.Texture } | undefined }) => {
    const node = instance.node("only")!;
    node.target.addEventListener("dispose", () => disposed.push(node.texture.name));
  };
  const byCount = new BakeGraph(made, { idleNodes: 2, idleBytes: Infinity });
  for (const key of ["a", "b", "c"]) { const made = (await byCount.realize(single(key)))!; watch(made.instance); byCount.release(made.instance); }
  assert.deepEqual(disposed, ["a"]);
  assert.equal(byCount.stats().idle, 2);
  // Using a kept node again makes it the most recent.
  const b = (await byCount.realize(single("b")))!;
  byCount.release(b.instance);
  const d = (await byCount.realize(single("d")))!; watch(d.instance); byCount.release(d.instance);
  assert.deepEqual(disposed, ["a", "c"]);

  disposed.length = 0;
  const byBytes = new BakeGraph(made, { idleNodes: 100, idleBytes: each * 2 });
  for (const key of ["a", "b", "c"]) { const made = (await byBytes.realize(single(key)))!; watch(made.instance); byBytes.release(made.instance); }
  assert.deepEqual(disposed, ["a"]);
  assert.equal(byBytes.stats().idleBytes, each * 2);
});

test("a bake may be refused its memory, and leaves nothing behind", async () => {
  const { device: made, log } = device();
  const graph = new BakeGraph(made);
  const held = (await graph.realize(bake(log, "a")))!;
  let asked = 0;
  await assert.rejects(graph.realize(bake(log, "b"), undefined, { allocate: bytes => { asked = bytes; throw new Error("over the allowance"); } }), /over the allowance/);
  // The normal's key differs too (its height does); all three textures were asked for.
  assert.equal(asked, graph.bytes([held.instance]));
  assert.equal(held.instance.node("base")!.users, 1);
  assert.equal(graph.stats().nodes, 3);
});

test("large nodes are drawn in bands, a level's nodes together, and an abandoned bake leaves its textures incomplete", async () => {
  const { device: made, log } = device();
  const graph = new BakeGraph(made);
  const first = (await graph.realize(bake(log, "a"), undefined, { bandTexels: 32 }))!;
  assert.equal(first.bands, 2);
  assert.deepEqual(log, ["evaluate base#0 rows 0+4", "evaluate height#1 rows 0+4", "yield", "evaluate base#0 rows 4+4", "evaluate height#1 rows 4+4", "mips base",
    "normals normal#0 height=height rows 0+4", "yield", "normals normal#0 height=height rows 4+4", "mips normal"]);

  // Abandoned after the first band, drawing over its own textures: they hold a mix, and say so.
  let turns = 0;
  let warned = 0;
  const abandoned = await graph.realize(bake(log, "b"), first.instance, { bandTexels: 32, current: () => turns++ < 0, beforeOverwrite: () => warned++ });
  assert.equal(abandoned, null);
  assert.equal(warned, 1);
  assert.equal(first.instance.released, false, "the previous instance is still the caller's");
  assert.equal(first.instance.complete, false);
  assert.equal(first.instance.node("base")!.key, "");
  assert.equal(first.instance.node("base")!.users, 1);
  // The next bake draws what is incomplete (here everything: the height changed).
  log.length = 0;
  const next = (await graph.realize(bake(log, "b"), first.instance))!;
  assert.deepEqual(next.drawn, ["base", "height", "normal"]);
  assert.ok(next.instance.complete);

  // Not in place: the textures on screen stay whole while their replacements are drawn, and survive an abandoned bake.
  turns = 0;
  const kept = await graph.realize(bake(log, "c"), next.instance, { bandTexels: 32, inPlace: false, current: () => turns++ < 0 });
  assert.equal(kept, null);
  assert.ok(next.instance.complete);
  const replaced = (await graph.realize(bake(log, "c"), next.instance, { inPlace: false }))!;
  assert.notEqual(replaced.instance.node("base"), next.instance.node("base"));
  assert.ok(next.instance.released);
});

test("values that change while a bake waits abandon it", async () => {
  const { device: made, log } = device();
  const graph = new BakeGraph(made);
  let stable = true;
  made.yield = async () => { stable = false; };
  assert.equal(await graph.realize(bake(log, "a"), undefined, { bandTexels: 32, stable: () => stable }), null);
  assert.equal(graph.stats().nodes, 0, "nothing incomplete is kept");
});

test("an operation that fails leaves the previous bake as it was", async () => {
  const { device: made, log } = device();
  const graph = new BakeGraph(made);
  const first = (await graph.realize(bake(log, "a")))!;
  const failing = op("normals", log, { finish: () => { throw new Error("receiver_normal faces away"); } });
  await assert.rejects(graph.realize(bake(log, "b", undefined, failing), first.instance, { inPlace: false }), /faces away/);
  assert.ok(first.instance.complete);
  assert.equal(first.instance.node("normal")!.users, 1);
});

test("a lost context takes every node", async () => {
  const { device: made, log, state } = device();
  const graph = new BakeGraph(made);
  const first = (await graph.realize(bake(log, "a")))!;
  state.generation++;
  graph.forget();
  log.length = 0;
  const again = (await graph.realize(bake(log, "a"), first.instance))!;
  assert.deepEqual(again.drawn, ["base", "height", "normal"]);
  assert.notEqual(again.instance.node("base"), first.instance.node("base"));
});

test("nodes must be listed after the nodes they read", () => {
  const log: string[] = [];
  const specs = bake(log, "a");
  assert.throws(() => BakeGraph.resolve([specs[2], specs[1]]), /reads height, which is not listed before it/);
  assert.throws(() => BakeGraph.resolve([specs[0], specs[0]]), /twice/);
  // The resolved key names the target's format too: the same program output at another size is another texture.
  const small = BakeGraph.resolve([{ ...specs[0], target: target("base", 4, 4, "mips") }]).get("base");
  assert.notEqual(small, BakeGraph.resolve(specs).get("base"));
});

/**
 * The stage 2 shape: an INTRINSIC node keyed by the program and its values alone, read by a MAPPING node per receiver
 * whose key holds the receiver's geometry. (The keys here stand for `GpuBaker.intrinsicKey` and `GpuBaker.mappingKey`.)
 */
test("an intrinsic node keyed without receiver geometry is shared by per-receiver mapping nodes", async () => {
  const { device: made, log } = device();
  const graph = new BakeGraph(made);
  const intrinsicOp = op("intrinsic", log), mappingOp = op("mapping", log);
  const receiver = (name: string, parameters: string, geometry: string): NodeSpec[] => [
    { id: "intrinsic", key: `concrete:${parameters}`, target: target("intrinsic", 16, 16, "mips"), op: intrinsicOp },
    { id: "base", key: `mapping:${geometry}`, target: target(`base of ${name}`, 8, 8, "mips"), op: mappingOp, inputs: { source: "intrinsic" } },
  ];
  const wall = (await graph.realize(receiver("wall", "p1", "wall-3m")))!;
  const slab = (await graph.realize(receiver("slab", "p1", "slab")))!;
  // One intrinsic bake for both receivers; a mapping each.
  assert.deepEqual(log.filter(line => !line.startsWith("mips")), ["intrinsic intrinsic#0", "mapping base of wall#0 source=intrinsic", "mapping base of slab#0 source=intrinsic"]);
  assert.equal(wall.instance.node("intrinsic"), slab.instance.node("intrinsic"));
  assert.equal(wall.instance.node("intrinsic")!.users, 2);

  // The wall is resized: its mapping is drawn again, the intrinsic bake is reused and the slab is untouched.
  log.length = 0;
  const taller = (await graph.realize(receiver("wall", "p1", "wall-4m"), wall.instance))!;
  assert.deepEqual(taller.drawn, ["base"]);
  assert.deepEqual(log.filter(line => !line.startsWith("mips")), ["mapping base of wall#0 source=intrinsic"]);
  assert.equal(graph.dirty(receiver("slab", "p1", "slab"), slab.instance), 0);

  // A parameter changes: the intrinsic bake is drawn once (into a new texture: the slab still shows the old one),
  // and each receiver's mapping once.
  log.length = 0;
  const wall2 = (await graph.realize(receiver("wall", "p2", "wall-4m"), taller.instance))!;
  const slab2 = (await graph.realize(receiver("slab", "p2", "slab"), slab.instance))!;
  assert.deepEqual(log.filter(line => !line.startsWith("mips")), ["intrinsic intrinsic#0", "mapping base of wall#0 source=intrinsic", "mapping base of slab#0 source=intrinsic"]);
  assert.equal(wall2.instance.node("intrinsic"), slab2.instance.node("intrinsic"));
  assert.notEqual(wall2.instance.node("intrinsic"), wall.instance.node("intrinsic"));
  // The old intrinsic texture lost its last user and waits, so going back to the value draws only the mappings.
  assert.equal(graph.stats().idle, 1);
  log.length = 0;
  const back = (await graph.realize(receiver("wall", "p1", "wall-4m"), wall2.instance))!;
  assert.deepEqual(back.drawn, ["base"]);
});
