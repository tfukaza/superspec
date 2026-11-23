import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { registerCityTextureLod, cityTextureBindings, disposeCityTextureLod } from "../../adapters/three/src/cityTextureLoader.ts";

// Real GLTFLoader parsing, with only browser image decoding replaced. No GPU or
// DOM is required; each loaded texture retains independently identifiable pixels.
Object.assign(globalThis, { self: globalThis });
function document() {
  return { asset: { version: "2.0" }, scene: 0,
    scenes: [{ nodes: [], extras: { worldSpecTextureLod: { version: 1, pairs: [{ near: 0, far: 1 }] } } }],
    images: [{ uri: "near.png" }, { uri: "far.png" }], samplers: [{ wrapS: 33071, wrapT: 33648, minFilter: 9987 }],
    textures: [{ source: 0, sampler: 0 }, { source: 1, sampler: 0 }],
    materials: [{ pbrMetallicRoughness: { baseColorTexture: { index: 0 } } }] } as any;
}
function fixture(options: { fail?: string; defer?: string; enabled?: boolean } = {}) {
  const requests: string[] = [], pending: (() => void)[] = [], loaded = new Map<string, THREE.Texture>();
  const manager = new THREE.LoadingManager();
  manager.addHandler(/\.png$/, { load(url: string, success: (texture: THREE.Texture) => void,
    _progress: unknown, failure: (error: Error) => void) {
    requests.push(url);
    const finish = () => {
      if (options.fail === url) { failure(new Error("fixture image failed")); return; }
      const texture = new THREE.DataTexture(new Uint8Array(url.startsWith("near") ? [1, 2, 3, 255] : [4, 5, 6, 255]), 1, 1);
      loaded.set(url, texture); success(texture);
    };
    if (options.defer === url) pending.push(finish); else finish();
  } } as unknown as THREE.Loader);
  const loader = new GLTFLoader(manager);
  if (options.enabled !== false) registerCityTextureLod(loader);
  return { loader, requests, pending, loaded, parse: (json: any) => loader.parseAsync(JSON.stringify(json), "") };
}

test("opt-in parsing binds coarse images without changing portable JSON, high pixels or sampling", async () => {
  const json = document();
  json.extensionsUsed = ["KHR_texture_transform"];
  json.materials[0].pbrMetallicRoughness.baseColorTexture = { index: 0, texCoord: 1,
    extensions: { KHR_texture_transform: { offset: [.2, .3], scale: [.5, .7], rotation: .4, texCoord: 2 } } };
  const f = fixture(), gltf = await f.parse(json);
  const material = await gltf.parser.getDependency("material", 0) as THREE.MeshStandardMaterial;
  assert.deepEqual(f.requests, ["far.png"]);
  assert.deepEqual(gltf.parser.json, json);
  assert.equal(material.map!.colorSpace, THREE.SRGBColorSpace);
  assert.equal(material.map!.channel, 2);
  assert.equal(material.map!.wrapS, THREE.ClampToEdgeWrapping);
  assert.equal(material.map!.wrapT, THREE.MirroredRepeatWrapping);
  material.map!.anisotropy = 12;
  material.map!.matrixAutoUpdate = false;
  material.map!.matrix.set(2, 0, .3, 0, 3, .4, 0, 0, 1);
  const [binding] = cityTextureBindings(gltf, material);
  assert.equal(binding.slot, "map");
  const high = await binding.loadHigh();
  assert.deepEqual(f.requests, ["far.png", "near.png"]);
  assert.notEqual(high, binding.low);
  const core = await gltf.parser.getDependency("texture", 0) as THREE.Texture;
  assert.notEqual(high, core);
  assert.equal(high.source, core.source);
  assert.deepEqual(Array.from((high.image as any).data), [1, 2, 3, 255]);
  for (const key of ["colorSpace", "channel", "wrapS", "wrapT", "minFilter", "magFilter", "anisotropy", "flipY", "rotation", "matrixAutoUpdate"] as const)
    assert.equal(high[key], binding.low[key], key);
  assert.deepEqual(high.offset, binding.low.offset); assert.deepEqual(high.repeat, binding.low.repeat);
  assert.deepEqual(high.matrix, binding.low.matrix);
  let coreDisposals = 0; core.addEventListener("dispose", () => coreDisposals++);
  high.dispose();
  const version = high.version;
  assert.equal(await binding.loadHigh(), high);
  assert.equal(high.version, version, "unchanged bindings do not schedule a resident image re-upload");
  material.map = high;
  assert.equal(cityTextureBindings(gltf, material)[0].low, binding.low);
  assert.equal(cityTextureBindings(gltf, material.clone())[0].low, binding.low);
  disposeCityTextureLod(gltf);
  assert.equal(coreDisposals, 0);
  assert.deepEqual(cityTextureBindings(gltf, material), []);
  await assert.rejects(binding.loadHigh(), { name: "AbortError" });
});

test("pairs published through one image extension bind; mixed or unknown extensions keep the full texture", async () => {
  const requested: string[] = [];
  const ktx2 = { load(url: string, done: (texture: THREE.Texture) => void) {
    requested.push(url); done(new THREE.DataTexture(new Uint8Array(url.startsWith("near") ? [1, 2, 3, 255] : [4, 5, 6, 255]), 1, 1));
  } };
  const basis = (source: number) => ({ sampler: 0, extensions: { KHR_texture_basisu: { source } } });
  const parse = async (textures: any[], required = true) => {
    const json = document();
    json.images = [{ uri: "near.ktx2", mimeType: "image/ktx2" }, { uri: "far.ktx2", mimeType: "image/ktx2" }];
    json.textures = textures;
    json.extensionsUsed = ["KHR_texture_basisu"];
    if (required) json.extensionsRequired = ["KHR_texture_basisu"];
    requested.length = 0;
    const loader = registerCityTextureLod(new GLTFLoader());
    loader.setKTX2Loader(ktx2 as any);
    const gltf = await loader.parseAsync(JSON.stringify(json), "");
    const material = await gltf.parser.getDependency("material", 0) as THREE.MeshStandardMaterial;
    return { gltf, material, bindings: cityTextureBindings(gltf, material) };
  };
  // Both levels are KTX2: only the distant companion is fetched until the camera approaches.
  const streamed = await parse([basis(0), basis(1)]);
  assert.deepEqual(requested, ["far.ktx2"]);
  assert.equal(streamed.bindings.length, 1);
  assert.equal(streamed.material.map!.colorSpace, THREE.SRGBColorSpace);
  const high = await streamed.bindings[0].loadHigh();
  assert.deepEqual(requested, ["far.ktx2", "near.ktx2"]);
  assert.deepEqual(Array.from((high.image as any).data), [1, 2, 3, 255]);
  // An extension's image index is not an unknown reference to texture or image 1.
  assert.equal(streamed.material.map!.wrapT, THREE.MirroredRepeatWrapping);
  // An unrelated integer in an extra blocks texture 0 unless the publisher states its references are exclusive;
  // a key named for a texture blocks it either way.
  const extras = async (record: object, material: object) => {
    const json = document();
    Object.assign(json.scenes[0].extras.worldSpecTextureLod, record);
    json.scenes[0].extras.worldSpecLod = { distances: [0, 12] };
    json.materials[0].extras = material;
    const gltf = await fixture().parse(json);
    return cityTextureBindings(gltf, await gltf.parser.getDependency("material", 0) as THREE.MeshStandardMaterial).length;
  };
  assert.deepEqual([await extras({}, {}), await extras({ exclusive: true }, {}), await extras({ exclusive: true }, { worldSpecSurface: { detailTexture: 0 } })], [0, 1, 0]);
  // A companion in another codec, or a texture with a fallback source or a second extension, is not paired.
  for (const textures of [[basis(0), { source: 1, sampler: 0 }], [{ ...basis(0), source: 0 }, basis(1)],
    [{ sampler: 0, extensions: { KHR_texture_basisu: { source: 0 }, EXT_unknown: {} } }, basis(1)]]) {
    const plain = await parse(textures, false);
    assert.deepEqual(plain.bindings, []);
    assert.deepEqual(requested, ["near.ktx2"]);
  }
});

test("metal/roughness slots deduplicate high clones and preserve normal/color separation", async () => {
  const json = document();
  json.images.push({ uri: "near-normal.png" }, { uri: "far-normal.png" }, { uri: "near-mr.png" }, { uri: "far-mr.png" });
  json.textures.push(...[2, 3, 4, 5].map(source => ({ source, sampler: 0 })));
  json.scenes[0].extras.worldSpecTextureLod.pairs.push({ near: 2, far: 3 }, { near: 4, far: 5 });
  json.materials[0].normalTexture = { index: 2 };
  json.materials[0].pbrMetallicRoughness.metallicRoughnessTexture = { index: 4 };
  const f = fixture(), gltf = await f.parse(json), material = await gltf.parser.getDependency("material", 0);
  assert.deepEqual(new Set(f.requests), new Set(["far.png", "far-normal.png", "far-mr.png"]));
  const bindings = cityTextureBindings(gltf, material);
  assert.equal(bindings.length, 4);
  assert.equal(bindings[2].loadHigh, bindings[3].loadHigh);
  assert.equal(bindings[0].loadHigh, cityTextureBindings(gltf, material.clone())[0].loadHigh);
  const high = await Promise.all(bindings.map(b => b.loadHigh()));
  assert.equal(high[2], high[3]);
  assert.notEqual(high[0], high[1]);
  assert.equal(high[0].colorSpace, THREE.SRGBColorSpace);
  assert.equal(high[1].colorSpace, THREE.NoColorSpace);
  assert.equal(high[2].colorSpace, THREE.NoColorSpace);
  assert.equal(f.requests.filter(uri => uri === "near-mr.png").length, 1);
  let disposals = 0;
  for (const texture of new Set(high)) texture.addEventListener("dispose", () => disposals++);
  for (const texture of new Set(high)) texture.dispose();
  assert.equal(disposals, 3);
  disposeCityTextureLod(gltf, { texturesReleased: true });
  assert.equal(disposals, 3);
  await assert.rejects(bindings[0].loadHigh(), { name: "AbortError" });
});

test("identical coarse image aliases preserve distinct full source and color-space identities", async () => {
  const json = document();
  json.images.push({ uri: "near-other.png" });
  json.textures.push({ source: 2, sampler: 0 }, { source: 1, sampler: 0 });
  json.scenes[0].extras.worldSpecTextureLod.pairs.push({ near: 2, far: 3 });
  json.materials[0].normalTexture = { index: 2 };
  const f = fixture(), gltf = await f.parse(json), material = await gltf.parser.getDependency("material", 0);
  const bindings = cityTextureBindings(gltf, material);
  assert.equal(bindings.length, 2);
  assert.notEqual(bindings[0].low, bindings[1].low);
  assert.equal(bindings[0].low.source, bindings[1].low.source);
  assert.equal(bindings[0].low.colorSpace, THREE.SRGBColorSpace);
  assert.equal(bindings[1].low.colorSpace, THREE.NoColorSpace);
  const highs = await Promise.all(bindings.map(binding => binding.loadHigh()));
  assert.notEqual(highs[0].source, highs[1].source);
  assert.equal(highs[0].source, f.loaded.get("near.png")!.source);
  assert.equal(highs[1].source, f.loaded.get("near-other.png")!.source);
  assert.deepEqual(f.requests, ["far.png", "near.png", "near-other.png"]);
  disposeCityTextureLod(gltf);
});

test("generic loader, absent metadata and malformed/mismatched pairs retain full resolution", async () => {
  const variants: ((json: any) => void)[] = [
    json => delete json.scenes[0].extras.worldSpecTextureLod,
    json => json.scenes[0].extras.worldSpecTextureLod.version = 9,
    json => json.scenes[0].extras.worldSpecTextureLod.pairs[0].far = 999,
    json => json.scenes[0].extras.worldSpecTextureLod.pairs.push({ near: 0, far: 1 }),
    json => json.textures[1].sampler = 999,
    json => json.textures[1].extensions = { KHR_texture_basisu: { source: 1 } },
  ];
  for (const change of variants) {
    const json = document(); change(json);
    const f = fixture(), gltf = await f.parse(json), material = await gltf.parser.getDependency("material", 0);
    assert.deepEqual(f.requests, ["near.png"]);
    assert.deepEqual(cityTextureBindings(gltf, material), []);
  }
  const f = fixture({ enabled: false }), gltf = await f.parse(document());
  const material = await gltf.parser.getDependency("material", 0);
  assert.deepEqual(f.requests, ["near.png"]);
  assert.deepEqual(cityTextureBindings(gltf, material), []);
});

test("unlit, point and line materials retain their authored source when another triangle material streams the same image", async () => {
  for (const mode of ["unlit", 0, 1, 2, 3]) {
    const json = document(), f = fixture();
    json.materials.push(structuredClone(json.materials[0]));
    json.accessors = [{ componentType: 5126, count: 3, type: "VEC3", min: [0, 0, 0], max: [0, 0, 0] }];
    const primitive = (material: number) => ({ material, attributes: { POSITION: 0 } });
    const uses = [primitive(0), { ...primitive(0), mode: mode === "unlit" ? 4 : mode }, primitive(1)];
    if (mode === "unlit") {
      json.extensionsUsed = ["KHR_materials_unlit"];
      json.materials[0].extensions = { KHR_materials_unlit: {} };
    }
    json.meshes = uses.map(primitive => ({ primitives: [primitive] }));
    json.nodes = uses.map((_, mesh) => ({ mesh }));
    json.scenes[0].nodes = [0, 1, 2];
    const gltf = await f.parse(json);
    const portable = await gltf.parser.getDependency("material", 0);
    const managed = await gltf.parser.getDependency("material", 1);
    assert.deepEqual(cityTextureBindings(gltf, portable), []);
    assert.equal(portable.map.source, f.loaded.get("near.png")!.source);
    assert.equal(managed.map.source, f.loaded.get("far.png")!.source);
    const [binding] = cityTextureBindings(gltf, managed);
    const high = await binding.loadHigh();
    assert.equal(high.source, portable.map.source);
    assert.deepEqual(f.requests.sort(), ["far.png", "near.png"]);
    disposeCityTextureLod(gltf);
  }
});

test("unsupported slot/extension/extras references and image aliases cannot change through a pair", async () => {
  const variants: ((json: any) => void)[] = [
    json => json.materials[0].occlusionTexture = { index: 0 },
    json => json.materials[0].emissiveTexture = { index: 0 },
    json => json.materials[0].normalTexture = { index: 0 },
    json => json.materials[0].extensions = { TEST_extension: { customTexture: { index: 0 } } },
    json => json.materials[0].extras = { detailTexture: 0 },
    json => json.materials[0].extensions = { TEST_extension: { opaqueAlias: 0 } },
    json => {
      json.textures.push({ source: 0, sampler: 0 });
      json.scenes[0].extras.worldSpecTextureLod.pairs[0].near = 2;
      json.materials[0].pbrMetallicRoughness.baseColorTexture.index = 2;
      json.materials[0].extras = { imageAlias: 0 };
    },
    json => { json.textures.push({ source: 0, sampler: 0 }); json.materials[0].occlusionTexture = { index: 2 }; },
    json => json.materials.push({ pbrMetallicRoughness: { baseColorTexture: { index: 1 } } }),
  ];
  for (const change of variants) {
    const json = document(); change(json);
    const f = fixture(), gltf = await f.parse(json), material = await gltf.parser.getDependency("material", 0);
    assert.ok(!f.requests.includes("far.png"));
    assert.deepEqual(cityTextureBindings(gltf, material), []);
  }
});

test("a failed coarse image falls back to the standard image without exposing a broken binding", async t => {
  t.mock.method(console, "error", () => {});
  const f = fixture({ fail: "far.png" }), gltf = await f.parse(document());
  const material = await gltf.parser.getDependency("material", 0) as THREE.MeshStandardMaterial;
  assert.deepEqual(f.requests, ["far.png", "near.png"]);
  assert.ok(material.map);
  assert.deepEqual(Array.from((material.map.image as any).data), [1, 2, 3, 255]);
  assert.deepEqual(cityTextureBindings(gltf, material), []);
});

test("parse registries remain independent and teardown cancels late high-image completion", async () => {
  const f = fixture({ defer: "near.png" }); registerCityTextureLod(f.loader);
  const first = await f.parse(document()), second = await f.parse(document());
  const a = await first.parser.getDependency("material", 0), b = await second.parser.getDependency("material", 0);
  assert.notEqual(a.map, b.map);
  assert.deepEqual(cityTextureBindings(first, b), []);
  const pending = cityTextureBindings(first, a)[0].loadHigh();
  // getDependency starts its loader in a promise continuation.
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.pending.length, 1);
  disposeCityTextureLod(first);
  f.pending.shift()!();
  await assert.rejects(pending, { name: "AbortError" });
  const other = cityTextureBindings(second, b)[0].loadHigh();
  await new Promise(resolve => setImmediate(resolve));
  f.pending.shift()!();
  assert.ok(await other);
  disposeCityTextureLod(second);
});
