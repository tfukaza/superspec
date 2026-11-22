import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import sharp from "sharp";
import { attachBlockProxy, retainPaintedSurfaces } from "./block-proxy.mjs";
import { entryBytes, GeometryReader } from "./city-lod.mjs";
import { imageBytes, readCityEntry, retargetLod, verifiedLod } from "./city-data-textures.mjs";

async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), "superspec-block-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const positions = new Float32Array([0,0,0, 2,0,0, 0,3,0]);
  const normals = new Float32Array([0,0,1, 0,0,1, 0,0,1]);
  const uvs = new Float32Array([0,0, 1,0, 0,1]);
  const colors = new Uint8Array([128,64,32,255, 128,64,32,255, 128,64,32,255]);
  const aspects = new Float32Array([.7, .7, .7]);
  const indices = new Uint16Array([0,1,2]);
  const arrays = [positions, normals, uvs, colors, aspects, indices];
  const bytes = Buffer.concat(arrays.map(a => Buffer.from(a.buffer)));
  await writeFile(join(directory, "mesh.bin"), bytes);
  const rgb = Buffer.alloc(704*704*3);
  for (let i = 0; i < 704*704; i++) rgb.fill(i%2 ? 255 : 0, i*3, i*3+3);
  await writeFile(join(directory, "stripes.png"), await sharp(rgb, { raw: { width: 704, height: 704, channels: 3 } }).png().toBuffer());
  let offset = 0;
  const bufferViews = arrays.map(a => {
    const v = { buffer: 0, byteOffset: offset, byteLength: a.byteLength };
    offset += a.byteLength; return v;
  });
  const primitive = { indices: 5, attributes: { POSITION: 0, NORMAL: 1, TEXCOORD_0: 2, COLOR_0: 3, _MS_ASPECT: 4 } };
  const json = { asset: { version: "2.0" }, scene: 0, scenes: [{ nodes: [0,1], extras: {} }],
    nodes: [{ name: "Street -2,0", translation: [-4,2,0], scale: [-2,1,1], mesh: 0 }, { name: "Street -2,1", mesh: 0 }],
    meshes: [{ name: "building-1", primitives: [{ ...primitive, material: 0 }, { ...primitive, material: 1 }] }],
    materials: [{ extras: { materialSpecNormalFrame: true }, pbrMetallicRoughness: { baseColorTexture: { index: 0 }, metallicFactor: 0 } },
      { extras: { materialSpecNormalFrame: true }, alphaMode: "BLEND", doubleSided: true, pbrMetallicRoughness: { baseColorFactor: [1,.25,0,.4] } }],
    images: [{ uri: "stripes.png" }], textures: [{ source: 0 }],
    buffers: [{ uri: "mesh.bin", byteLength: bytes.length }], bufferViews,
    accessors: ["VEC3","VEC3","VEC2","VEC4","SCALAR","SCALAR"].map((type,i) => ({ type, bufferView: i, count: 3,
      componentType: [5126,5126,5126,5121,5126,5123][i], ...(i === 3 && { normalized: true }) })) };
  return { directory, json, bytes, destination: join(directory, "proxy.glb") };
}

test("block publication retains the fallback, transformed silhouette, winding, chart normals, pigment and blending", async t => {
  const { directory, json, bytes, destination } = await fixture(t);
  await retargetLod(json, { manifest: { primitives: [] }, payload: Buffer.alloc(0) }, directory, "source-data");
  const input = entryBytes(json), options = { cell: "-2,0" };
  const { entry, summary } = await attachBlockProxy(destination, input, options);
  assert.deepEqual((await attachBlockProxy(destination, input, options)).entry, entry);
  const result = readCityEntry(entry).json, block = result.scenes[0].extras.worldSpecBlockLods.blocks[0];
  assert.deepEqual(result.scenes[0].nodes, [0,1]);
  assert.deepEqual(result.nodes.slice(0,2), json.nodes);
  assert.deepEqual(result.materials.slice(0,2), json.materials);
  assert.deepEqual(block.sources, [0]);
  assert.deepEqual(block.bounds, { min: [-8,2,0], max: [-4,5,0] });
  assert.ok(!result.scenes[0].nodes.includes(block.proxy));
  assert.ok(await verifiedLod(result, Buffer.alloc(0), directory));
  assert.equal(summary.proxyPrimitives, 2);
  const draws = result.meshes[result.nodes[block.proxy].mesh].primitives;
  const [opaque, glass] = draws.map(p => result.materials[p.material]);
  assert.equal(opaque.extras.worldSpecSurface.recipe, "opaque-building-atlas");
  assert.equal(glass.extras.worldSpecSurface.recipe, "architectural-glazing");
  assert.equal(opaque.extras.worldSpecBlockAtlas, true);
  assert.equal(glass.alphaMode, "BLEND");
  assert.deepEqual(opaque.pbrMetallicRoughness.baseColorTexture, glass.pbrMetallicRoughness.baseColorTexture);
  const reader = new GeometryReader(result, directory), a = draws[0].attributes;
  const coords = await reader.read(a.POSITION), indices = await reader.read(draws[0].indices);
  const corners = Array.from(indices, index => coords.subarray(index*3, index*3+3));
  const [p,q,r] = corners;
  assert.ok((q[0]-p[0])*(r[1]-p[1])-(q[1]-p[1])*(r[0]-p[0]) > 0, "mirrored face points +Z");
  assert.equal(result.bufferViews[result.accessors[a._MS_ASPECT].bufferView].target, 34962);
  assert.ok(Math.abs((await reader.read(a._MS_ASPECT))[0] - .35) < 1e-6, "rectangular atlas retains metric chart aspect");
  assert.ok(Math.abs((await reader.read(a.COLOR_0))[0] - 128/255) < 1e-6);
  const tex = result.textures[opaque.pbrMetallicRoughness.baseColorTexture.index];
  const png = await imageBytes(result, Buffer.alloc(0), directory, result.images[tex.source]);
  const { data, info } = await sharp(png).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  const sample = (x,y) => [...data.subarray((y*info.width+x)*4, (y*info.width+x)*4+4)];
  assert.ok(Math.abs(sample(96,96)[0]-188) <= 2, "black and white pigment filters in linear light");
  assert.deepEqual(sample(288,96), [255,137,0,102], "linear pigment tint and original glass opacity");
  assert.deepEqual(sample(193,96), sample(200,96), "gutter belongs to its own sheet");
  assert.deepEqual(await readFile(join(directory, "mesh.bin")), bytes);
});

test("thin painted roof tiles retain their visible side without reintroducing distant hardware", () => {
  const positions = new Float32Array([0,0,0, 1,0,0, 0,0,1, 1,0,1, 1,0,0, 2,0,0, 1,0,1]);
  const colors = new Float32Array([.35,.09,.05, .35,.09,.05, .35,.09,.05, .35,.09,.05, .1,.1,.1, .1,.1,.1, .1,.1,.1]);
  const source = new Uint32Array([0,1,2, 1,2,3, 4,5,6]), reduced = new Uint32Array([0,1,3]);
  assert.deepEqual(retainPaintedSurfaces(source, reduced, positions, colors, 3, false), [0,1,2, 1,2,3]);
  assert.equal(retainPaintedSurfaces(source, reduced, positions, colors, 3, true), source);
});

test("neutral 29 cm roof tiles survive on top of an opaque underlay", () => {
  const positions = new Float32Array([0,.02,0, 0,.02,.29, .29,.02,0, .29,.02,.29]);
  const source = new Uint32Array([0,1,2, 1,3,2]);
  assert.deepEqual(retainPaintedSurfaces(source, new Uint32Array(), positions, undefined, 0, false), [...source]);
});

test("ineligible sources and invalid UV charts fail before an entry is published", async t => {
  const { json, destination } = await fixture(t);
  await assert.rejects(attachBlockProxy(destination, entryBytes(json), { cell: "3,9" }), /No published building/);
  await assert.rejects(attachBlockProxy(destination, entryBytes(json), { cell: "-2,0", distance: -1 }), /distance/);
  const animated = structuredClone(json);
  animated.animations = [{ channels: [{ target: { node: 0 } }] }];
  await assert.rejects(attachBlockProxy(destination, entryBytes(animated), { cell: "-2,0" }), /independent static/);
  json.accessors[2].byteOffset = 0;
  json.accessors[2].bufferView = 0;
  await assert.rejects(attachBlockProxy(destination, entryBytes(json), { cell: "-2,0" }), /charted UVs/);
});
