import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, writeFile, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import sharp from "sharp";
import { attachCityTextureLods, exclusiveTextureLods, resizeTextureLod, stageCityTextureLods, textureLodKey } from "./city-texture-lod.mjs";
import { entryBytes } from "./city-lod.mjs";
import { readCityEntry } from "./city-data-textures.mjs";

const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const surface = (family, recipe = "stock-hardware", type = "Mixed surfaces with metal") => ({ worldSpecSurface: { family, recipe, surface: type } });

test("texture LOD admission protects concrete pigment/normals, masks, emission and unknown shared roles", () => {
  const json = { images: Array.from({ length: 12 }, () => ({})), textures: Array.from({ length: 12 }, (_, source) => ({ source })),
    materials: [
      { extras: surface("Shop inventory"), pbrMetallicRoughness: { baseColorTexture: { index: 0 }, metallicRoughnessTexture: { index: 2 } }, normalTexture: { index: 1 } },
      { extras: surface("Masonry", "architectural-concrete"), pbrMetallicRoughness: { baseColorTexture: { index: 3 }, metallicRoughnessTexture: { index: 5 } }, normalTexture: { index: 4 } },
      { extras: surface("Building atlases", "architectural-glazing", "Transparent surfaces"), alphaMode: "BLEND",
        pbrMetallicRoughness: { baseColorTexture: { index: 6 }, metallicRoughnessTexture: { index: 8 } }, normalTexture: { index: 7 } },
      { extras: surface("Building atlases"), pbrMetallicRoughness: { metallicRoughnessTexture: { index: 9 } }, occlusionTexture: { index: 9 } },
      { pbrMetallicRoughness: { metallicRoughnessTexture: { index: 10 } } },
      { extras: surface("Shop inventory"), alphaMode: "MASK", pbrMetallicRoughness: { baseColorTexture: { index: 11 } } },
    ] };
  assert.deepEqual([...exclusiveTextureLods(json).keys()], [0, 1, 2, 5, 6, 7, 8]);
  json.materials[0].extras.unknownTextureAlias = 1;
  json.textures.push({ source: 2 });
  assert.deepEqual([...exclusiveTextureLods(json).keys()], [0, 5, 6, 7, 8]);
  json.materials[0].extras.sourceFinishes = ["room / posters"];
  json.materials[2].emissiveTexture = { index: 6 };
  assert.deepEqual([...exclusiveTextureLods(json).keys()], [5]);
});

test("color mips filter in linear light while material data retains linear numeric channels", async () => {
  const pixels = Buffer.alloc(4 * 4 * 3);
  for (let y = 0; y < 4; y++) for (let x = 0; x < 4; x++) for (let c = 0; c < 3; c++) pixels[(y * 4 + x) * 3 + c] = (x + y) % 2 * 255;
  const png = await sharp(pixels, { raw: { width: 4, height: 4, channels: 3 } }).png().toBuffer();
  const colour = await sharp(await resizeTextureLod(png, { width: 1, height: 1, space: "srgb" })).raw().toBuffer();
  const data = await sharp(await resizeTextureLod(png, { width: 1, height: 1, space: "linear" })).raw().toBuffer();
  assert.ok(colour.every(value => Math.abs(value - 188) <= 1));
  assert.ok(data.every(value => Math.abs(value - 128) <= 1));
  assert.notEqual(textureLodKey(png, "srgb"), textureLodKey(png, "linear"));
});

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), "city-texture-lod-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const source = join(root, "source", "city.glb"), destination = join(root, "stage", "city.glb");
  await mkdir(dirname(source));
  const png = await sharp({ create: { width: 1024, height: 1024, channels: 4, background: { r: 80, g: 140, b: 60, alpha: .6 } } }).png().toBuffer();
  await writeFile(join(dirname(source), "pigment.png"), png);
  const json = { asset: { version: "2.0" }, scene: 0, scenes: [{ extras: {} }],
    images: [{ uri: "pigment.png", mimeType: "image/png" }], textures: [{ source: 0, sampler: 0, name: "source" }],
    samplers: [{ wrapS: 10497, wrapT: 33071 }],
    materials: [{ extras: surface("Shop inventory"), pbrMetallicRoughness: { baseColorTexture: { index: 0 } } }] };
  await writeFile(source, entryBytes(json));
  return { root, source, destination, json, png, options: { source, destination, cacheDirectory: join(root, "cache"), workers: 1 } };
}

test("the normal publisher can prepare texture LODs before committing its entry", async t => {
  const f = await fixture(t), before = await readFile(f.source);
  const { entry, report } = await attachCityTextureLods(f.source, before, { cacheDirectory: f.options.cacheDirectory });
  const json = readCityEntry(entry).json;
  assert.equal(report.pairs, 1);
  assert.deepEqual(json.scenes[0].extras.worldSpecTextureLod.pairs, [{ near: 0, far: 1 }]);
  assert.ok((await readFile(join(dirname(f.source), json.images[1].uri))).length > 0);
  assert.deepEqual(await readFile(f.source), before);
});

test("staged ordinary glTF companions preserve full-resolution bindings and SLOD payload provenance", async t => {
  const f = await fixture(t), payload = Buffer.from([4, 1, 9, 7]);
  const manifest = { sourceSha256: hash(entryBytes(f.json)), primitives: [] };
  const text = Buffer.from(JSON.stringify(manifest)), header = Buffer.alloc(12 + Math.ceil(text.length / 4) * 4);
  header.write("SLOD"); header.writeUInt32LE(1, 4); header.writeUInt32LE(text.length, 8); text.copy(header, 12);
  const sidecar = Buffer.concat([header, payload]);
  await mkdir(join(dirname(f.source), "city-data"));
  await writeFile(join(dirname(f.source), "city-data", "lod-original.bin"), sidecar);
  f.json.scenes[0].extras.cityLod = "city-data/lod-original.bin";
  const original = entryBytes(f.json); await writeFile(f.source, original);
  const report = await stageCityTextureLods(f.options);
  assert.equal(report.pairs, 1); assert.equal(report.uniqueImages, 1); assert.equal(report.reused, 0);
  const staged = await readFile(f.destination), { json } = readCityEntry(staged);
  assert.deepEqual(json.scenes[0].extras.worldSpecTextureLod, { version: 1, pairs: [{ near: 0, far: 1 }] });
  assert.deepEqual(json.textures[0], f.json.textures[0]);
  assert.deepEqual(json.textures[1], { ...f.json.textures[0], source: 1 });
  assert.deepEqual(json.materials, f.json.materials);
  const far = await readFile(join(dirname(f.destination), json.images[1].uri));
  const metadata = await sharp(far).metadata();
  assert.equal(metadata.width, 256); assert.equal(metadata.height, 256); assert.equal(metadata.channels, 4);
  assert.ok(json.images[1].uri.includes(hash(far)));
  const lod = await readFile(join(dirname(f.destination), json.scenes[0].extras.cityLod));
  const length = lod.readUInt32LE(8), revised = JSON.parse(lod.toString("utf8", 12, 12 + length));
  const noLod = structuredClone(json); delete noLod.scenes[0].extras.cityLod;
  assert.equal(revised.sourceSha256, hash(entryBytes(noLod)));
  assert.deepEqual(lod.subarray(12 + Math.ceil(length / 4) * 4), payload);
  const next = await stageCityTextureLods(f.options);
  assert.equal(next.reused, 1); assert.deepEqual(await readFile(f.destination), staged);
  assert.deepEqual(await readFile(f.source), original);
  assert.deepEqual(await readFile(join(dirname(f.source), f.json.scenes[0].extras.cityLod)), sidecar);
  assert.deepEqual(await readFile(join(dirname(f.destination), "pigment.png")), f.png);
});

test("encoded-content aliases with protected AO remain at full resolution", async t => {
  const f = await fixture(t);
  await writeFile(join(dirname(f.source), "ao.png"), f.png);
  f.json.images.push({ uri: "ao.png", mimeType: "image/png" }); f.json.textures.push({ source: 1 });
  f.json.materials[0].occlusionTexture = { index: 1 };
  await writeFile(f.source, entryBytes(f.json));
  const report = await stageCityTextureLods(f.options);
  assert.equal(report.pairs, 0);
  assert.deepEqual(await readFile(f.destination), await readFile(f.source));
});

test("failed resize and source aliases preserve the prior pack; stale SLOD fails before work", async t => {
  const f = await fixture(t);
  await mkdir(dirname(f.destination)); await writeFile(f.destination, "last complete publication");
  await assert.rejects(stageCityTextureLods({ ...f.options, resize: async () => { throw Error("resize failed"); } }), /resize failed/);
  assert.equal(await readFile(f.destination, "utf8"), "last complete publication");
  await assert.rejects(stageCityTextureLods({ ...f.options, destination: join(dirname(f.source), "another.glb") }), /separate directory/);
  const alias = join(f.root, "source-alias");
  await symlink(dirname(f.source), alias, process.platform === "win32" ? "junction" : "dir");
  await assert.rejects(stageCityTextureLods({ ...f.options, destination: join(alias, "city.glb") }), /separate directory/);
  const manifest = Buffer.from(JSON.stringify({ sourceSha256: "0".repeat(64), primitives: [] }));
  const sidecar = Buffer.alloc(12 + Math.ceil(manifest.length / 4) * 4);
  sidecar.write("SLOD"); sidecar.writeUInt32LE(1, 4); sidecar.writeUInt32LE(manifest.length, 8); manifest.copy(sidecar, 12);
  await writeFile(join(dirname(f.source), "stale.bin"), sidecar);
  f.json.scenes[0].extras.cityLod = "stale.bin"; await writeFile(f.source, entryBytes(f.json));
  await assert.rejects(stageCityTextureLods(f.options), /provenance does not match/);
  assert.equal(await readFile(f.destination, "utf8"), "last complete publication");
});

test("a concurrent source publication cannot replace the previously staged entry", async t => {
  const f = await fixture(t);
  await mkdir(dirname(f.destination)); await writeFile(f.destination, "previous pack");
  await assert.rejects(stageCityTextureLods({ ...f.options, resize: async (bytes, settings) => {
    f.json.scenes[0].extras.revision = "changed by another publisher";
    await writeFile(f.source, entryBytes(f.json));
    return resizeTextureLod(bytes, settings);
  } }), /Source entry changed/);
  assert.equal(await readFile(f.destination, "utf8"), "previous pack");
  const retry = await stageCityTextureLods(f.options);
  assert.equal(retry.reused, 1, "completed immutable image work is reusable after the failed entry publication");
  assert.equal(readCityEntry(await readFile(f.destination)).json.scenes[0].extras.revision, "changed by another publisher");
});
