import assert from "node:assert/strict";
import test from "node:test";
import { createRenderGltfLoader } from "../../adapters/three/src/gltfLoader.ts";
import { applyGraphicsVisibility, graphicsAllows } from "../../adapters/three/src/graphicsVisibility.ts";
import { MeshoptDecoder, MeshoptEncoder } from "meshoptimizer";
import { mobileTextureSize } from "./city-mobile-delivery.mjs";
import { mobileGeometry, mobileLodSidecar, selectMobileIndexTier } from "./mobile-geometry.mjs";
import { pruneMobileResources } from "./mobile-resources.mjs";

test("a renderer filters tagged entities before creating GLTF dependencies", async () => {
  const scene = JSON.stringify({ asset: { version: "2.0" }, scenes: [{ nodes: [0, 1] }], nodes: [
    { name: "structure" },
    { name: "decorations", children: [2], extras: { worldSpecMinGraphics: "low" } },
    { name: "unlabelled-prop" },
  ] });
  const low = await createRenderGltfLoader(undefined, { graphicsPreset: "very-low" }).parseAsync(scene, "");
  assert.ok(low.scene.getObjectByName("structure"));
  assert.equal(low.scene.getObjectByName("unlabelled-prop"), undefined);
  const full = await createRenderGltfLoader(undefined, { graphicsPreset: "medium" }).parseAsync(scene, "");
  assert.ok(full.scene.getObjectByName("unlabelled-prop"));
  assert.throws(() => graphicsAllows("unlisted", "low"), /minimum graphics preset/);
});

const tieredChain = (extras = {}) => ({ asset: { version: "2.0" }, scenes: [{ nodes: [0] }], nodes: [
  { name: "placement", children: [1], translation: [4, 0, 0] },
  { name: "near", extras: { worldSpecMinGraphics: "medium", ...extras }, extensions: { MSFT_lod: { ids: [2, 3, 4] } } },
  { name: "middle", extras: { worldSpecMinGraphics: "low" }, scale: [2, 2, 2] },
  { name: "coarse" },
  { name: "far" },
] });

test("a chain whose near level is above the preset continues from its first delivered level", async () => {
  const authored = { worldSpecLod: { version: 1, distances: [0, 10, 20, 30] } };
  const medium = tieredChain(authored);
  assert.deepEqual(applyGraphicsVisibility(medium, "medium"), { omittedNodes: 0, omittedPrimitives: 0 });
  assert.deepEqual(medium, tieredChain(authored));

  // Levels are near, middle, coarse, far starting at 0, 10, 20, 30 m. Without
  // the near level the middle one starts at the eye and the others keep theirs.
  const low = tieredChain(authored);
  applyGraphicsVisibility(low, "low");
  assert.deepEqual(low.nodes[0].children, [2]);
  assert.deepEqual(low.nodes[2].extensions.MSFT_lod.ids, [3, 4]);
  assert.deepEqual(low.nodes[2].extras.worldSpecLod, { version: 1, distances: [0, 20, 30] });
  assert.deepEqual(low.nodes[2].scale, [2, 2, 2]);
  assert.equal(low.nodes[1].extensions.MSFT_lod, undefined);
  const again = structuredClone(low);
  applyGraphicsVisibility(again, "low");
  assert.deepEqual(again, low);

  const lowest = tieredChain(authored);
  applyGraphicsVisibility(lowest, "very-low");
  assert.deepEqual(lowest.nodes[0].children, [3]);
  assert.deepEqual(lowest.nodes[3].extensions.MSFT_lod.ids, [4]);
  assert.deepEqual(lowest.nodes[3].extras.worldSpecLod.distances, [0, 30]);

  const root = tieredChain(authored);
  root.scenes[0].nodes = [1];
  applyGraphicsVisibility(root, "very-low");
  assert.deepEqual(root.scenes[0].nodes, [3]);

  const coverage = tieredChain({ MSFT_screencoverage: [.5, .2, .1, 0] });
  applyGraphicsVisibility(coverage, "very-low");
  assert.deepEqual(coverage.nodes[3].extras.MSFT_screencoverage, [.1, 0]);

  // A renderer draws the tier's own near level where the omitted one stood.
  const loaded = await createRenderGltfLoader(undefined, { graphicsPreset: "very-low" }).parseAsync(JSON.stringify(tieredChain(authored)), "");
  assert.equal(loaded.scene.getObjectByName("near"), undefined);
  assert.equal(loaded.scene.getObjectByName("coarse").parent.name, "placement");
  assert.equal(loaded.scene.getObjectByName("far"), undefined);

  const single = tieredChain(authored);
  single.nodes[1].extensions.MSFT_lod.ids = [2];
  applyGraphicsVisibility(single, "very-low");
  assert.deepEqual(single.nodes[0].children, []);
});

test("a preset can state its own switch distances for the levels it delivers", () => {
  const authored = { worldSpecLod: { version: 1, distances: [0, 10, 20, 30],
    presetDistances: { low: [0, 7, 9], "very-low": [0, 5] } } };
  const medium = tieredChain(authored);
  applyGraphicsVisibility(medium, "medium");
  assert.deepEqual(medium.nodes[1].extras.worldSpecLod.distances, [0, 10, 20, 30]);
  const low = tieredChain(authored);
  applyGraphicsVisibility(low, "low");
  assert.deepEqual(low.nodes[2].extras.worldSpecLod.distances, [0, 7, 9]);
  // A staged low pack opened at a lower preset still finds that preset's entry.
  applyGraphicsVisibility(low, "very-low");
  assert.deepEqual(low.nodes[0].children, [3]);
  assert.deepEqual(low.nodes[3].extras.worldSpecLod.distances, [0, 5]);
  // The whole chain is delivered here, so the entry applies to all four levels.
  const whole = () => {
    const doc = tieredChain({ worldSpecLod: { version: 1, distances: [0, 10, 20, 30], presetDistances: { low: [0, 2, 4], "very-low": [0, 2, 4] } } });
    delete doc.nodes[1].extras.worldSpecMinGraphics;
    return doc;
  };
  assert.throws(() => applyGraphicsVisibility(whole(), "low"), /one value per delivered level/);
  // Only the middle level is above very low: near, coarse and far remain.
  const three = whole();
  applyGraphicsVisibility(three, "very-low");
  assert.deepEqual([three.nodes[1].extensions.MSFT_lod.ids, three.nodes[1].extras.worldSpecLod.distances], [[3, 4], [0, 2, 4]]);
  for (const distances of [[1, 5], [0, 5, 5], [0, "9"]]) {
    const invalid = tieredChain({ worldSpecLod: { version: 1, distances: [0, 10, 20, 30], presetDistances: { "very-low": distances } } });
    assert.throws(() => applyGraphicsVisibility(invalid, "very-low"), /presetDistances/);
  }
});

test("a mobile pack discards resources exclusive to omitted primitives and keeps shared and LOD resources", () => {
  const doc = {
    scenes: [{ nodes: [0], extras: { paintedLawn: { buffer: 3 }, worldSpecBounce: { bufferView: 8 } } }],
    nodes: [{ mesh: 0, extensions: { MSFT_lod: { ids: [1] } } }, { mesh: 1 }],
    meshes: [{ primitives: [
      { attributes: { POSITION: 0 }, indices: 1, material: 0 },
      { attributes: { POSITION: 0 }, indices: 2, material: 1, extras: { worldSpecMinGraphics: "low" } },
    ] }, { primitives: [{ attributes: { POSITION: 0 }, indices: 1, material: 2 }] }],
    materials: [
      { pbrMetallicRoughness: { baseColorTexture: { index: 0 } } },
      { normalTexture: { index: 1 } },
      { normalTexture: { index: 2 }, extras: { worldSpecSurface: { detailTexture: 0 } } },
    ],
    accessors: [{ bufferView: 0 }, { bufferView: 1 }, { bufferView: 2 }],
    textures: [{ source: 0 }, { source: 1 }, { source: 2 }],
    images: [{ bufferView: 3 }, { bufferView: 4 }, { bufferView: 5 }],
  };
  assert.deepEqual(applyGraphicsVisibility(doc, "very-low"), { omittedNodes: 0, omittedPrimitives: 1 });
  const resources = pruneMobileResources(doc);
  assert.equal(resources.prunedImages, 1);
  assert.deepEqual(doc.meshes[1].primitives[0], { attributes: { POSITION: 0 }, indices: 1, material: 1 });
  assert.equal(doc.materials[1].normalTexture.index, 1);
  assert.deepEqual(resources.views.map(view => view.bufferView).sort(), [0, 1, 3, 5, 8]);
  assert.deepEqual(resources.raw, [{ buffer: 3 }]);
});

test("the near stream stays within one small error bound for every material and keeps protected detail", () => {
  const fine = { bytes: 10, count: 300, errorMeters: .02 };
  const cheap = { bytes: 10, count: 120, errorMeters: .035 };
  const broken = { bytes: 10, count: 30, errorMeters: .11 };
  const record = { levels: [fine, broken], detailLevels: [broken, cheap] };
  const hardware = { family: "Building atlases", surface: "Mixed surfaces with metal", recipe: "weathered-steel" };
  const budget = { nearErrorMeters: .035 };
  assert.equal(selectMobileIndexTier({}, { extras: { worldSpecSurface: hardware } }, record, budget), cheap);
  assert.equal(selectMobileIndexTier({}, { name: "Awning cloth" }, record, budget), cheap);
  assert.equal(selectMobileIndexTier({}, {}, record, { nearErrorMeters: 0 }), undefined);
  assert.equal(selectMobileIndexTier({}, { extras: { cityDetailKeep: true } }, record, budget), undefined);
  assert.equal(selectMobileIndexTier({}, { emissiveFactor: [1, 1, 1] }, record, budget), undefined);
});

test("a weed's shadowless wash takes its own bound and other blended surfaces keep every triangle", () => {
  const soft = { bytes: 10, count: 300, errorMeters: .013 }, softer = { bytes: 10, count: 90, errorMeters: .042 };
  const sparse = { bytes: 10, count: 30, errorMeters: .079 };
  const record = { levels: [soft, softer, sparse] };
  const wash = { alphaMode: "BLEND", extras: { castsShadow: false,
    worldSpecSurface: { family: "Vegetation", recipe: "Weed underpainting" } } };
  // 1.3 cm fits a 2 cm bound; 4.2 cm needs the 5 cm one; 7.9 cm fits neither.
  assert.equal(selectMobileIndexTier({}, wash, record, { nearErrorMeters: 0, washErrorMeters: .02 }), soft);
  assert.equal(selectMobileIndexTier({}, wash, record, { nearErrorMeters: .035, washErrorMeters: .05 }), softer);
  assert.equal(selectMobileIndexTier({}, wash, record, { nearErrorMeters: .035 }), undefined);
  const budget = { nearErrorMeters: .1, washErrorMeters: .1 };
  assert.equal(selectMobileIndexTier({}, { alphaMode: "BLEND" }, record, budget), undefined);
  assert.equal(selectMobileIndexTier({}, { ...wash, extras: { ...wash.extras, castsShadow: true } }, record, budget), undefined);
  assert.equal(selectMobileIndexTier({}, { ...wash, extras: { ...wash.extras, grassField: true } }, record, budget), undefined);
  assert.equal(selectMobileIndexTier({}, { ...wash, alphaMode: "MASK" }, record, budget), undefined);
  // An opaque surface is not widened by the wash bound.
  assert.equal(selectMobileIndexTier({}, {}, record, { nearErrorMeters: .02, washErrorMeters: .1 }), soft);
});

test("a compacted mobile primitive keeps its farther levels, addressed at its final position", async () => {
  await MeshoptEncoder.ready;
  const encode = indices => MeshoptEncoder.encodeIndexBuffer(new Uint8Array(Uint16Array.from(indices).buffer), indices.length, 2);
  // Six vertices; the near promotion uses 0-3, the far level reaches vertex 5.
  const promoted = encode([0, 1, 2, 0, 2, 3]), far = encode([0, 1, 5]);
  const payload = Buffer.concat([promoted, far]);
  const level = (offset, stream, count, errorMeters) => ({ offset, bytes: stream.length, count, componentBytes: 2, errorMeters });
  const lod = { payload, manifest: { primitives: [{ mesh: 1, primitive: 0, vertexCount: 6, nearCount: 9,
    levels: [level(0, promoted, 6, .03), level(promoted.length, far, 3, .3)] }] } };
  const vertices = Buffer.alloc(6 * 12), indices = Buffer.from(Uint16Array.from([0, 1, 2, 0, 2, 3, 0, 3, 4]).buffer);
  for (let vertex = 0; vertex < 6; vertex++) vertices.writeFloatLE(vertex, vertex * 12);
  const json = {
    meshes: [{ primitives: [] }, { primitives: [{ attributes: { POSITION: 0 }, indices: 1, material: 0 }] }],
    materials: [{}],
    accessors: [{ bufferView: 0, componentType: 5126, type: "VEC3", count: 6 }, { bufferView: 1, componentType: 5123, type: "SCALAR", count: 9 }],
    bufferViews: [{ buffer: 0, byteLength: vertices.length, byteStride: 12, target: 34962 }, { buffer: 0, byteLength: indices.length, target: 34963 }],
  };
  const { lods, trianglesRemoved, verticesRemoved, replacement } = await mobileGeometry(json, lod, async view => [vertices, indices][view],
    { nearErrorMeters: .035 });
  assert.deepEqual([trianglesRemoved, verticesRemoved], [1, 1]);
  // Vertex 5 survives because the far level needs it; vertex 4 does not.
  assert.deepEqual([...new Float32Array(new Uint8Array(replacement.get(0).bytes).buffer)].filter((_, i) => i % 3 === 0), [0, 1, 2, 3, 5]);
  json.meshes.shift();
  const sidecar = mobileLodSidecar(json, lods);
  const [record] = sidecar.manifest.primitives;
  assert.deepEqual([record.mesh, record.primitive, record.vertexCount, record.nearCount], [0, 0, 5, 6]);
  // The promoted stream is now the drawn geometry itself, so it carries no bytes.
  assert.deepEqual(record.levels[0], { offset: 0, bytes: 0, count: 6, componentBytes: 2, errorMeters: .03 });
  await MeshoptDecoder.ready;
  const decoded = new Uint16Array(3);
  MeshoptDecoder.decodeIndexBuffer(new Uint8Array(decoded.buffer), 3, 2, sidecar.payload.subarray(record.levels[1].offset, record.levels[1].offset + record.levels[1].bytes));
  assert.deepEqual([...decoded].sort(), [0, 1, 4]);
  assert.equal(record.levels[1].errorMeters, .3);
});

test("mobile textures scale with their authored size and keep small images and strip edges legible", () => {
  const half = { textureScale: .5, maxDimension: 1024 }, quarter = { textureScale: .25, maxDimension: 512 };
  assert.deepEqual(mobileTextureSize(2048, 2048, half), { width: 1024, height: 1024 });
  assert.deepEqual(mobileTextureSize(2048, 2048, quarter), { width: 512, height: 512 });
  assert.deepEqual(mobileTextureSize(4096, 4096, half), { width: 1024, height: 1024 });
  assert.deepEqual(mobileTextureSize(4096, 32, quarter), { width: 512, height: 16 });
  assert.deepEqual(mobileTextureSize(196, 101, quarter), { width: 128, height: 66 });
  assert.deepEqual(mobileTextureSize(109, 19, quarter), { width: 109, height: 19 });
});
