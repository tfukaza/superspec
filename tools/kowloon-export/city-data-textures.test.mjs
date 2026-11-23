import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, readdir, writeFile, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import sharp from "sharp";
import { DATA_RECIPE, cityCompanionUris, dataTextureKey, exclusiveDataTextures, readCityEntry, stageCityDataTextures } from "./city-data-textures.mjs";
import { entryBytes } from "./city-lod.mjs";

const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const material = index => ({ pbrMetallicRoughness: { metallicRoughnessTexture: { index } } });

test("embedded building texture names are not external city resources", () => {
  const json = {
    images: [{ name: "$generated/surfaces/tenement/0/normal.png", bufferView: 0, mimeType: "image/png" }],
    buffers: [{ uri: "city-data/chunk.bin" }],
    scenes: [{ extras: { cityLod: "city-data/lod.bin" } }],
  };
  assert.deepEqual([...cityCompanionUris(json)], ["city-data/chunk.bin", "city-data/lod.bin"]);
});

async function fixture(t, width = 1024, height = 1024) {
  const root = await mkdtemp(join(tmpdir(), "city-data-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const source = join(root, "source", "city.glb"), destination = join(root, "stage", "city.glb");
  await mkdir(dirname(source));
  const png = await sharp({ create: { width, height, channels: 3, background: { r: 0, g: 170, b: 40 } } }).png().toBuffer();
  await writeFile(join(dirname(source), "properties.png"), png);
  const json = { asset: { version: "2.0" }, scene: 0, scenes: [{ extras: {} }],
    images: [{ uri: "properties.png", mimeType: "image/png" }], textures: [{ source: 0, sampler: 0 }],
    samplers: [{ wrapS: 10497, wrapT: 10497 }], materials: [material(0)] };
  await writeFile(source, entryBytes(json));
  const calls = [];
  const encode = async manifest => {
    const { jobs } = JSON.parse(await readFile(manifest, "utf8")); calls.push(jobs);
    for (const job of jobs) {
      assert.equal(job.key, dataTextureKey(await readFile(job.input)));
      const header = Buffer.alloc(80);
      Buffer.from([0xab, 0x4b, 0x54, 0x58, 0x20, 0x32, 0x30, 0xbb, 13, 10, 26, 10]).copy(header);
      header.writeUInt32LE(width, 20); header.writeUInt32LE(height, 24); header.writeUInt32LE(1, 40);
      await writeFile(job.output, header);
    }
  };
  return { root, source, destination, json, png, calls, options: { source, destination, cacheDirectory: join(root, "cache"), encode } };
}

test("MR admission rejects other slots, shared images, extras and extension aliases", () => {
  const json = { images: [{}, {}, {}, {}, {}, {}], textures: Array.from({ length: 6 }, (_, source) => ({ source })),
    materials: Array.from({ length: 6 }, (_, index) => material(index)) };
  json.materials[0].normalTexture = { index: 0 };
  json.materials[1].extras = { unknownAlias: 1 };
  json.materials[2].extensions = { customTexture: { index: 2 } };
  json.textures.push({ source: 3 }); // another texture aliases the same image
  json.materials[4].occlusionTexture = { index: 4 };
  assert.deepEqual([...exclusiveDataTextures(json)], [[5, [5]]]);
});

test("staging appends optional Basis images, retains exact fallback and updates only LOD provenance", async t => {
  const f = await fixture(t), payload = Buffer.from([1, 8, 3, 7]);
  const manifest = { sourceSha256: hash(entryBytes(f.json)), primitives: [{ mesh: 0, levels: [] }] };
  const text = Buffer.from(JSON.stringify(manifest)), header = Buffer.alloc(12 + Math.ceil(text.length / 4) * 4);
  header.write("SLOD"); header.writeUInt32LE(1, 4); header.writeUInt32LE(text.length, 8); text.copy(header, 12);
  const sidecar = Buffer.concat([header, payload]);
  await mkdir(join(dirname(f.source), "city-data"));
  await writeFile(join(dirname(f.source), "city-data", "lod-original.bin"), sidecar);
  f.json.scenes[0].extras.cityLod = "city-data/lod-original.bin";
  const original = entryBytes(f.json);
  await writeFile(f.source, original);
  const report = await stageCityDataTextures(f.options);
  assert.equal(report.convertedImages, 1);
  assert.equal(f.calls.length, 1);
  const { json } = readCityEntry(await readFile(f.destination));
  assert.equal(json.textures[0].source, 0);
  assert.equal(json.textures[0].extensions.KHR_texture_basisu.source, 1);
  const artifact = await readFile(join(dirname(f.destination), json.images[1].uri));
  assert.equal(json.images[1].uri, "city-data/data-" + hash(artifact) + ".ktx2");
  assert.notEqual(report.records[0].key, report.records[0].artifactSha256);
  assert.ok(!json.extensionsRequired?.includes("KHR_texture_basisu"));
  const lod = await readFile(join(dirname(f.destination), json.scenes[0].extras.cityLod));
  const length = lod.readUInt32LE(8), updated = JSON.parse(lod.toString("utf8", 12, 12 + length));
  const withoutLod = structuredClone(json); delete withoutLod.scenes[0].extras.cityLod;
  assert.equal(updated.sourceSha256, hash(entryBytes(withoutLod)));
  assert.deepEqual(updated.primitives, manifest.primitives);
  assert.deepEqual(lod.subarray(12 + Math.ceil(length / 4) * 4), payload);
  delete json.textures[0].extensions; delete json.extensionsUsed; json.images.pop();
  json.scenes[0].extras.cityLod = f.json.scenes[0].extras.cityLod;
  assert.deepEqual(json, f.json, "core fallback records, materials, indices and samplers are unchanged");
  assert.deepEqual(await readFile(f.source), original);
  assert.deepEqual(await readFile(join(dirname(f.source), f.json.scenes[0].extras.cityLod)), sidecar);
  assert.deepEqual(await readFile(join(dirname(f.destination), "properties.png")), f.png);
  const corrupted = Buffer.from(sidecar);
  const wrong = JSON.stringify({ ...manifest, sourceSha256: "0".repeat(64) });
  Buffer.from(wrong).copy(corrupted, 12);
  await writeFile(join(dirname(f.source), f.json.scenes[0].extras.cityLod), corrupted);
  await assert.rejects(stageCityDataTextures(f.options), /provenance does not match/);
  assert.equal(f.calls.length, 1, "stale provenance is rejected before encoding");
});

test("content aliases in separate image records cannot change pigment or normal maps", async t => {
  const f = await fixture(t);
  await writeFile(join(dirname(f.source), "normal.png"), f.png);
  f.json.images.push({ uri: "normal.png", mimeType: "image/png" });
  f.json.textures.push({ source: 1 }); f.json.materials[0].normalTexture = { index: 1 };
  await writeFile(f.source, entryBytes(f.json));
  assert.equal((await stageCityDataTextures(f.options)).convertedImages, 0);
  assert.equal(f.calls.length, 0);
  assert.deepEqual(await readFile(f.destination), await readFile(f.source));
});

test("only large block-aligned atlases qualify", async t => {
  for (const dimensions of [[512, 512], [1025, 1024]]) {
    const f = await fixture(t, ...dimensions);
    assert.equal((await stageCityDataTextures(f.options)).convertedImages, 0);
    assert.equal(f.calls.length, 0);
  }
});

test("content keys include the codec recipe and staged output is idempotent", async t => {
  const f = await fixture(t);
  assert.equal(dataTextureKey(f.png), createHash("sha256").update(DATA_RECIPE).update(f.png).digest("hex"));
  assert.notEqual(dataTextureKey(f.png), dataTextureKey(Buffer.concat([f.png, Buffer.from([0])])));
  await stageCityDataTextures(f.options);
  const staged = await readFile(f.destination);
  const next = join(f.root, "again", "city.glb");
  const report = await stageCityDataTextures({ ...f.options, source: f.destination, destination: next });
  assert.equal(report.convertedImages, 0);
  assert.equal(f.calls.length, 1);
  assert.deepEqual(await readFile(next), staged);
});

test("failed encoding preserves existing published entries and rejects in-place staging", async t => {
  const f = await fixture(t);
  await mkdir(dirname(f.destination)); await writeFile(f.destination, "previous completed pack");
  await assert.rejects(stageCityDataTextures({ ...f.options, encode: async () => { throw Error("encoder failed"); } }), /encoder failed/);
  assert.equal(await readFile(f.destination, "utf8"), "previous completed pack");
  assert.deepEqual(await readdir(f.options.cacheDirectory), [], "failed run removes extracted inputs and manifest");
  assert.deepEqual(await readFile(f.source), entryBytes(f.json));
  await assert.rejects(stageCityDataTextures({ ...f.options, destination: join(dirname(f.source), "other.glb") }), /separate directory/);
  const alias = join(f.root, "source-alias");
  await symlink(dirname(f.source), alias, process.platform === "win32" ? "junction" : "dir");
  await assert.rejects(stageCityDataTextures({ ...f.options, destination: join(alias, "city.glb") }), /separate directory/);
  assert.deepEqual(await readFile(f.source), entryBytes(f.json));
});

test("embedded image bounds are checked before encoding or publishing", async t => {
  const f = await fixture(t);
  f.json.images[0] = { bufferView: 0, mimeType: "image/png" };
  f.json.buffers = [{ uri: "properties.png", byteLength: f.png.length }];
  f.json.bufferViews = [{ buffer: 0, byteOffset: f.png.length - 1, byteLength: 5 }];
  await writeFile(f.source, entryBytes(f.json));
  await assert.rejects(stageCityDataTextures(f.options), /buffer bounds/);
  assert.equal(f.calls.length, 0);
});

test("embedded BIN bytes remain exact when a non-LOD entry gains optional data images", async t => {
  const f = await fixture(t);
  f.json.images[0] = { bufferView: 0, mimeType: "image/png" };
  f.json.buffers = [{ byteLength: f.png.length }];
  f.json.bufferViews = [{ buffer: 0, byteOffset: 0, byteLength: f.png.length }];
  const tail = Buffer.alloc(8 + Math.ceil(f.png.length / 4) * 4);
  tail.writeUInt32LE(tail.length - 8, 0); tail.writeUInt32LE(0x004e4942, 4); f.png.copy(tail, 8);
  const original = Buffer.concat([entryBytes(f.json), tail]); original.writeUInt32LE(original.length, 8);
  await writeFile(f.source, original);
  await stageCityDataTextures(f.options);
  assert.deepEqual(readCityEntry(await readFile(f.destination)).tail, tail);
  assert.deepEqual(await readFile(f.source), original);
});
