import assert from "node:assert/strict";
import test from "node:test";
import { Document, NodeIO } from "@gltf-transform/core";
import { applyGraphicsVisibility } from "../../adapters/three/src/graphicsVisibility.ts";
import { STREET_TREE_LEVELS, finishStreetTreeLods, sprigSubset } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/street-trees.mjs";
import { readCityEntry } from "./city-data-textures.mjs";
import { declaredLodBytes } from "./declared-lod-pack.mjs";

test("a thinned canopy keeps the whole crown's leaf area in fewer, larger leaves", () => {
  // A crown that grew 8000 leaves publishes 5500 of them. Leaf area goes with the
  // square of its scale, so count * scale^2 stays at the 8000 unit leaves grown.
  const published = sprigSubset(8000, 5500, 5500), thinned = sprigSubset(8000, 5500, 2200);
  assert.deepEqual(thinned.indices, Array.from({ length: 2200 }, (_, i) => i));
  for (const level of [published, thinned])
    assert.ok(Math.abs(level.indices.length * level.growth ** 2 - 8000) < 1e-6);
  assert.ok(Math.abs(thinned.growth / published.growth - Math.sqrt(2.5)) < 1e-12);
  // A crown with fewer leaves than a level asks for draws them all, unscaled.
  assert.deepEqual(sprigSubset(1200, 1200, 2200), { indices: Array.from({ length: 1200 }, (_, i) => i), growth: 1 });
});

test("the distant rendition draws its leaves at a stated scale and the nearer ones hold the whole leaf area", () => {
  // Three times a published leaf, which is itself sqrt(8000 / 5500) of a grown one.
  const rim = sprigSubset(8000, 5500, STREET_TREE_LEVELS.far.sprigs, STREET_TREE_LEVELS.far.scale);
  assert.equal(rim.indices.length, 400);
  assert.ok(Math.abs(rim.growth - 3 * Math.sqrt(8000 / 5500)) < 1e-12);
  // 400 leaves at 3x carry 400 * 9 / 5500 of the published leaf area: about two thirds, over the crown mass.
  assert.ok(Math.abs(rim.indices.length * (rim.growth / Math.sqrt(8000 / 5500)) ** 2 / 5500 - 0.6545) < 1e-3);
  // Low and Very Low state no scale, so their leaves hold all of it.
  assert.equal(STREET_TREE_LEVELS.low.scale, undefined);
  assert.equal(STREET_TREE_LEVELS.veryLow.scale, undefined);
  assert.ok(STREET_TREE_LEVELS.veryLow.sprigs < STREET_TREE_LEVELS.low.sprigs && STREET_TREE_LEVELS.far.sprigs < STREET_TREE_LEVELS.veryLow.sprigs);
});

test("each preset starts the painted tree from its own rendition and keeps Medium's chain", async () => {
  const doc = new Document(), scene = doc.createScene();
  doc.getRoot().setDefaultScene(scene);
  const names = ["tree/lod/2", "tree/lod/low", "tree/lod/very-low", "tree/lod/distant"];
  const { low, veryLow, unreached } = STREET_TREE_LEVELS;
  scene.addChild(doc.createNode("tree").setExtras({ worldSpecMinGraphics: "medium", worldSpecPendingTreeLod: names,
    worldSpecLod: { version: 1, distances: [0, 45, ...unreached], presetDistances: { low: low.distances, "very-low": veryLow.distances } } }));
  doc.createNode(names[0]).setExtras({ worldSpecMinGraphics: "medium" });
  doc.createNode(names[1]).setExtras({ worldSpecMinGraphics: "low" });
  doc.createNode(names[2]);
  doc.createNode(names[3]);
  const finished = finishStreetTreeLods(await new NodeIO().writeBinary(doc));
  assert.ok(declaredLodBytes(finished));
  const read = () => readCityEntry(Buffer.from(finished)).json;
  const chain = preset => {
    const json = read();
    applyGraphicsVisibility(json, preset);
    const [owner] = json.scenes[0].nodes, node = json.nodes[owner];
    return { levels: [owner, ...node.extensions?.MSFT_lod?.ids ?? []].map(id => json.nodes[id].name), distances: node.extras.worldSpecLod.distances };
  };
  const authored = read();
  assert.ok(!authored.nodes[0].extras.worldSpecPendingTreeLod);
  // Medium and above: the published pair, then levels no city view reaches.
  for (const preset of ["medium", "high", "very-high"]) {
    const full = chain(preset);
    assert.deepEqual(full.levels, ["tree", ...names]);
    assert.deepEqual(full.distances.slice(0, 2), [0, 45]);
    assert.ok(full.distances[2] >= 1000);
  }
  assert.deepEqual(chain("low"), { levels: names.slice(1), distances: [0, 43, 71] });
  assert.deepEqual(chain("very-low"), { levels: names.slice(2), distances: [0, 73] });
});
