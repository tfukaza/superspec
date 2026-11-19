import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { batchAttribute, decodePacket, PLACEMENT_FLOATS, type Manifest } from "../../browser/model/packet.ts";
import { SceneUpdates } from "../../browser/model/updates.ts";
import { Box3, Frustum, Matrix4, Vector3 } from "three";
import { FoliageVisibility } from "../../browser/renderer/foliage-visibility.ts";

test("foliage patch culling preserves tile levels across distance, view exits and reentry", () => {
  const tiles = new Map<number, Box3>();
  for (let x = 0; x < 256; x++) tiles.set(x, new Box3(new Vector3(x, 0, 0), new Vector3(x + 0.5, 0.5, 0.5)));
  const visibility = new FoliageVisibility(tiles, [[0, 0], [2, 8]], 24);
  const frustum = new Frustum().setFromProjectionMatrix(new Matrix4());
  let checks = 0;
  const intersects = Frustum.prototype.intersectsBox.bind(frustum);
  frustum.intersectsBox = box => { checks++; return intersects(box); };
  const eye = new Vector3();
  // Identity clip space sees the first two tiles, including the box touching its right plane.
  assert.equal(visibility.update(frustum, eye), true);
  assert.deepEqual([...visibility.chosen.values()], [0, 0, ...Array(254).fill(-1)]);
  assert.equal(visibility.update(frustum, eye), false);
  eye.x = -8;
  assert.equal(visibility.update(frustum, eye), true);
  assert.deepEqual([...visibility.chosen.values()].slice(0, 2), [2, 2]);
  eye.x = -23;
  assert.equal(visibility.update(frustum, eye), true);
  assert.deepEqual([...visibility.chosen.values()].slice(0, 2), [2, -1]);
  eye.x = -24;
  checks = 0;
  assert.equal(visibility.update(frustum, eye), true);
  assert.equal(visibility.update(frustum, eye), false);
  assert.equal(checks, 2, "one bounds check per frame for the whole distant patch");
  assert.ok([...visibility.chosen.values()].every(level => level === -1));
  eye.x = 0;
  frustum.setFromProjectionMatrix(new Matrix4().makeTranslation(4, 0, 0));
  checks = 0;
  assert.equal(visibility.update(frustum, eye), false);
  assert.equal(checks, 1, "one bounds check for the whole offscreen patch");
  frustum.setFromProjectionMatrix(new Matrix4());
  assert.equal(visibility.update(frustum, eye), true);
  assert.deepEqual([...visibility.chosen.values()], [0, 0, ...Array(254).fill(-1)]);
});

/**
 * Instanced batches in scene packets. The packets are assembled here, from the layout `Scene::packet` documents, so
 * the decoder is checked against the format rather than against the encoder; when the Rust exporter's fixtures exist
 * (`BATCH_FIXTURE_OUTPUT=generated/batches cargo test -p superspec-export --test batches`) they are decoded too.
 */

const source = { line: 1, column: 1, offset: 0 };
const half = Math.SQRT1_2;

/** A packet of one ordinary triangle, two batches and one asset, laid out by hand. */
function packet(change: (manifest: Manifest, numbers: { floats: number[]; indices: number[]; instances: number[] }) => void = () => {}, magic = "LSP3"): ArrayBuffer {
  const batch = (id: string, start: { vertex: number; index: number; instance: number }, count: number, extra: object) => ({
    id, objectId: id, source, material: null, visible: true, collision: "none" as const,
    vertexStart: start.vertex, vertexCount: 3, indexStart: start.index, indexCount: 3, instanceStart: start.instance, instanceCount: count,
    localBounds: [[0, 0, 0], [1, 1, 0]] as [number[], number[]], bounds: [[0, 0, 0], [9, 9, 9]] as [number[], number[]], ...extra,
  });
  const manifest: Manifest = {
    schemaVersion: 3, revision: 1, success: true, id: "scene", name: "Scene", units: "meters", axes: "right-handed-z-up",
    geometryValidation: "not-run", navigationValidation: "not-run", diagnostics: [], materials: [],
    assets: [{ id: "image", mime: "image/png", byteStart: 0, byteLength: 5 }], assetByteCount: 5,
    meshes: [{ id: "mesh", objectId: "mesh", source, material: null, visible: true, vertexStart: 0, vertexCount: 3, indexStart: 0, indexCount: 3, bounds: [[0, 0, 0], [1, 1, 0]] }],
    colliders: [], vertexCount: 3, indexCount: 3,
    batches: [batch("a", { vertex: 0, index: 0, instance: 0 }, 2, { tintStart: 0 }), batch("b", { vertex: 3, index: 3, instance: 2 }, 1, { valueStart: 0, tintStart: 2, template: "model/bolt" })],
    batchCounts: { vertices: 6, indices: 6, instances: 3, values: 1, tints: 3 },
  };
  // Positions, normals, UVs and colors of the two batch meshes, then their indices, then placements, values and tints.
  const triangle = [0, 0, 0, 1, 0, 0, 0, 1, 0], up = [0, 0, 1, 0, 0, 1, 0, 0, 1], uv = [0, 0, 1, 0, 0, 1], white = Array(9).fill(1);
  const numbers = {
    floats: [...triangle, ...triangle.map(value => value * 2), ...up, ...up, ...uv, ...uv, ...white, ...white],
    indices: [0, 1, 2, 0, 2, 1],
    instances: [
      1, 2, 3, 0, 0, 0, 1, 1, 1, 1,
      4, 5, 6, 0, 0, half, half, 2, 2, 2,
      7, 8, 9, half, 0, 0, half, 1, 1, 3,
      0.25, 0.5, 0.75, 1,
      1, 1, 1, 0.5, 0.5, 0.5, 2, 0, 0,
    ],
  };
  change(manifest, numbers);
  let json = new TextEncoder().encode(JSON.stringify(manifest));
  const padded = new Uint8Array(Math.ceil(json.length / 4) * 4).fill(32);
  padded.set(json); json = padded;
  const mesh = [...triangle, ...up, ...uv, ...white];
  const length = 8 + json.length + mesh.length * 4 + 12 + (numbers.floats.length + numbers.indices.length + numbers.instances.length) * 4 + 5;
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode(magic)); view.setUint32(4, json.length, true); bytes.set(json, 8);
  let at = 8 + json.length;
  const floats = (values: number[]) => { for (const value of values) { view.setFloat32(at, value, true); at += 4; } };
  const integers = (values: number[]) => { for (const value of values) { view.setUint32(at, value, true); at += 4; } };
  floats(mesh); integers([0, 1, 2]);
  floats(numbers.floats); integers(numbers.indices); floats(numbers.instances);
  bytes.set([9, 8, 7, 6, 5], at);
  return bytes.buffer;
}

test("a packet's batches lie between its indices and its assets and decode into their own buffers", () => {
  const scene = decodePacket(packet());
  const batches = scene.batches!;
  assert.equal(scene.manifest.batches!.length, 2);
  assert.deepEqual([...batches.positions.slice(9, 15)], [0, 0, 0, 2, 0, 0]);
  assert.deepEqual([...batches.indices], [0, 1, 2, 0, 2, 1]);
  assert.equal(batches.placements.length, 3 * PLACEMENT_FLOATS);
  assert.deepEqual([...batches.placements.slice(10, 13)], [4, 5, 6]);
  assert.deepEqual([...batches.values], [0.25, 0.5, 0.75, 1]);
  assert.deepEqual([...batches.tints.slice(6)], [2, 0, 0]);
  assert.equal(scene.manifest.batches![1].template, "model/bolt");
  // The scene's own mesh and the asset after the batch section are untouched.
  assert.deepEqual([...scene.positions], [0, 0, 0, 1, 0, 0, 0, 1, 0]);
  assert.deepEqual([...scene.assets], [9, 8, 7, 6, 5]);
  assert.equal(scene.manifest.colliders.length, 0, "a batch is never listed among the colliders");
});

test("packets without batches decode as before and state none", () => {
  const scene = decodePacket(packet((manifest, numbers) => {
    delete manifest.batches; delete manifest.batchCounts;
    numbers.floats = []; numbers.indices = []; numbers.instances = [];
  }));
  assert.equal(scene.batches, undefined);
  assert.deepEqual([...scene.assets], [9, 8, 7, 6, 5]);
});

test("malformed batches are refused before anything is uploaded", () => {
  const refused = (change: Parameters<typeof packet>[0], expected: RegExp) => assert.throws(() => decodePacket(packet(change)), expected);
  refused(manifest => { manifest.batches!.reverse(); }, /batch range or identity/i);
  refused(manifest => { manifest.batches![1].id = "mesh"; }, /batch range or identity/i);
  refused(manifest => { manifest.batches![1].instanceCount = 2; }, /batch range or identity/i);
  refused(manifest => { manifest.batches![0].material = "missing"; }, /batch range or identity/i);
  refused(manifest => { (manifest.batches![0] as { collision: string }).collision = "sometimes"; }, /batch range or identity/i);
  refused(manifest => { manifest.batches![1].valueStart = 1; }, /batch range or identity/i);
  refused(manifest => { delete manifest.batchCounts; }, /scene batches/i);
  refused(manifest => { manifest.batchCounts!.instances = 4; }, /lengths/i);
  refused(manifest => { manifest.batches!.pop(); }, /cover/i);
  refused((_, numbers) => { numbers.indices[5] = 3; }, /index exceeds/i);
  refused((_, numbers) => { numbers.instances[6] = 2; }, /placement/i);
  refused((_, numbers) => { numbers.instances[17] = 0; }, /placement/i);
  refused((_, numbers) => { numbers.instances[28] = -1; }, /placement/i);
  refused((_, numbers) => { numbers.instances[0] = Infinity; }, /numeric/i);
  refused((_, numbers) => { numbers.instances[34] = -0.5; }, /numeric/i);
  assert.throws(() => decodePacket(packet().slice(0, -4)), /length/);
});

test("a batch's foliage look is decoded when it is one the viewer can draw, and refused otherwise", () => {
  // The look as `superspec_types::batch::Foliage` serializes it with every default stated.
  const look = {
    family: "grass", patch: "lawn", level: 1, distance: 7, keep: 0.5, far: 22, material: "painted-grass", profile: "painted",
    bend: { angle: 0, randomness: 0.4 }, wind: { azimuth: 30, angle: 14, scale: 2.5, speed: 1.2, bias: 0.6, flutter: 2 },
    rejection: { boundary: 0.22, fill: 0, grazing: 15, grazingKeep: 0.5 }, tip: { lift: 1.08, highlight: [0.62, 0.72, 0.28], amount: 0.35, start: 0.55 },
  };
  const scene = decodePacket(packet(manifest => { manifest.batches![0].foliage = structuredClone(look) as never; }));
  assert.deepEqual(scene.manifest.batches![0].foliage, look);
  assert.equal(scene.manifest.batches![1].foliage, undefined);
  const refused = (change: (foliage: Record<string, unknown>) => void) => assert.throws(() => decodePacket(packet(manifest => {
    const foliage = structuredClone(look) as Record<string, unknown>;
    change(foliage);
    manifest.batches![0].foliage = foliage as never;
  })), /foliage look/i);
  refused(foliage => { foliage.family = "custom"; });
  refused(foliage => { foliage.far = 5; });
  refused(foliage => { foliage.profile = "toon"; });
  refused(foliage => { (foliage.wind as Record<string, number>).scale = 0; });
  refused(foliage => { (foliage.tip as Record<string, unknown>).highlight = [1, 1]; });
});

const fixtures = resolve("generated/batches");
test("the Rust exporter's packet and update decode to the batch it describes", { skip: !existsSync(resolve(fixtures, "batches.lsp3")) && "no fixtures: run the export crate's batch tests with BATCH_FIXTURE_OUTPUT=generated/batches" }, () => {
  const owned = (name: string) => Uint8Array.from(readFileSync(resolve(fixtures, name))).buffer;
  for (const scene of [decodePacket(owned("batches.lsp3")), new SceneUpdates().decode(owned("batches.lsu1"))]) {
    const [batch] = scene.manifest.batches!;
    assert.equal(batch.id, "bolts");
    assert.equal(batch.collision, "none");
    assert.equal(batch.instanceCount, 3);
    assert.equal(batch.material, "steel");
    assert.deepEqual([...scene.batches!.placements.slice(0, 10)], [1, 2, 3, 0, 0, 0, 1, 1, 1, 1]);
    assert.deepEqual([...scene.batches!.tints.slice(3, 6)], [0.5, 0.25, 0.125]);
    assert.deepEqual([...scene.batches!.values.slice(0, 4)], [0.25, 0, 0, 1]);
    // Bounds of every instance: the triangle's box under each placement, worked out by hand for the first axis.
    assert.ok(Math.abs(batch.bounds[0][0] - -6) < 1e-5 && Math.abs(batch.bounds[1][0] - 2) < 1e-5, JSON.stringify(batch.bounds));
    assert.equal(scene.manifest.meshes.length, 1, "the batch's mesh is not among the scene's meshes");
    // Named attributes, in delivery order, with their values.
    assert.deepEqual(batch.attributes!.map(attribute => [attribute.name, attribute.domain, attribute.components]),
      [["variation", "instance", 1], ["shadeNormal", "instance", 3], ["heightRatio", "vertex", 1]]);
    assert.deepEqual([...batchAttribute(scene.batches!, batch, batch.attributes![1])], [0, 0, 1, 0, 1, 0, 1, 0, 0]);
    assert.deepEqual([...batchAttribute(scene.batches!, batch, batch.attributes![2])], [0, 0.5, 1]);
  }
});
