import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, readdir, rename, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as THREE from "three";
import { MeshoptDecoder } from "meshoptimizer";
import { attachCityLod, cityDetailPolicy, cityFarPolicy, entryBytes } from "./city-lod.mjs";
import { kowloonFarProfile } from "./bounced-light.mjs";

test("canonical Kowloon publication selects the reviewed far profile with explicit standard overrides", () => {
  assert.equal(kowloonFarProfile(undefined, null), "aggressive");
  assert.equal(kowloonFarProfile(undefined, "standard"), "standard");
  assert.equal(kowloonFarProfile("standard", "aggressive"), "standard");
  assert.equal(kowloonFarProfile("aggressive", "standard"), "aggressive");
  assert.throws(() => kowloonFarProfile("unknown", undefined), /profile must be standard or aggressive/);
});

test("detail policy follows semantic surface metadata independently of display names", () => {
  const extras = { worldSpecSurface: { family: "Building atlases", surface: "Mixed surfaces with metal" } };
  assert.deepEqual(cityDetailPolicy({ name: "Building hardware", extras }), cityDetailPolicy({ name: "Renamed finish", extras }));
  assert.equal(cityDetailPolicy({ name: "building-3 / Architectural hardware / sheet 408" }), undefined);
  const inventory = cityDetailPolicy({ extras: { worldSpecSurface: { family: "Shop inventory" } } });
  assert.ok(!inventory.flags.includes("Prune"));
});

test("aggressive far policy only admits opaque non-emissive semantic building hardware", () => {
  const surface = { family: "Building atlases", surface: "Mixed surfaces with metal", recipe: "weathered-steel" };
  const hardware = { extras: { worldSpecSurface: surface } };
  const policy = cityFarPolicy(hardware);
  assert.deepEqual(policy.levels.map(level => level.errorMeters), [0.7, 1.4]);
  assert.deepEqual(cityFarPolicy({ ...hardware, name: "An arbitrary display name" }), policy);
  for (const forbidden of [
    { name: "Architectural hardware" },
    { extras: { worldSpecSurface: { ...surface, family: "Shop inventory" } } },
    { extras: { worldSpecSurface: { ...surface, family: "Masonry" } } },
    { extras: { worldSpecSurface: { ...surface, recipe: "sign-lamp-flutes" } } },
    { extras: { worldSpecSurface: { ...surface, surface: "Transparent surfaces" } } },
    { ...hardware, alphaMode: "MASK" }, { ...hardware, alphaMode: "BLEND" },
    { ...hardware, emissiveTexture: { index: 0 } },
    { ...hardware, emissiveFactor: [0, 0.1, 0] },
    { ...hardware, extensions: { KHR_materials_emissive_strength: { emissiveStrength: 1 } } },
    { extras: { ...hardware.extras, cityDetailKeep: true } },
    { extras: { ...hardware.extras, sourceFinishes: ["room / posters", "steel"] } },
    { extras: { ...hardware.extras, sourceFinishes: ["tube-light / glass"] } },
    { extras: { ...hardware.extras, sourceFinishes: "unknown" } },
  ]) assert.equal(cityFarPolicy(forbidden), undefined);
  assert.deepEqual(cityFarPolicy({ ...hardware, emissiveFactor: [0, 0, 0] }), policy);
  assert.deepEqual(cityFarPolicy({ extras: { ...hardware.extras, sourceFinishes: ["steel / braces"] } }), policy);
});

test("optional far tiers preserve prior streams, source attributes and gameplay meshes", async () => {
  const directory = await mkdtemp(join(tmpdir(), "superspec-far-lod-"));
  try {
    await mkdir(join(directory, "hardware-data"));
    const geometry = new THREE.TorusGeometry(1, 0.15, 12, 128);
    const arrays = [geometry.attributes.position.array, geometry.attributes.normal.array,
      geometry.attributes.uv.array, Uint32Array.from(geometry.index.array)];
    let offset = 0;
    const bufferViews = arrays.map(array => {
      const result = { buffer: 0, byteOffset: offset, byteLength: array.byteLength };
      offset += array.byteLength;
      return result;
    });
    const bytes = Buffer.concat(arrays.map(array => Buffer.from(array.buffer)));
    const bufferPath = join(directory, "hardware-data", "mesh.bin");
    await writeFile(bufferPath, bytes);
    const primitive = { attributes: { POSITION: 0, NORMAL: 1, TEXCOORD_0: 2 }, indices: 3, material: 0 };
    const surface = { family: "Building atlases", surface: "Mixed surfaces with metal", recipe: "weathered-steel" };
    const json = {
      asset: { version: "2.0" }, scene: 0, scenes: [{ nodes: [0, 1, 2], extras: {} }],
      nodes: [{ mesh: 0, scale: [2, 2, 2] }, { mesh: 1 }, { mesh: 2 }],
      meshes: [{ name: "building-0", primitives: [primitive] },
        { name: "arena", primitives: [primitive] },
        { name: "city/prototype/lamp", primitives: [{ ...primitive, material: 1 }] }],
      materials: [{ extras: { worldSpecSurface: surface } },
        { extras: { worldSpecSurface: surface }, emissiveTexture: { index: 0 }, emissiveFactor: [1, 1, 1] }],
      buffers: [{ uri: "hardware-data/mesh.bin", byteLength: bytes.length }], bufferViews,
      accessors: arrays.map((array, index) => ({ bufferView: index, componentType: index === 3 ? 5125 : 5126,
        count: index === 3 ? array.length : array.length / (index === 2 ? 2 : 3),
        type: index === 3 ? "SCALAR" : index === 2 ? "VEC2" : "VEC3" })),
    };
    const original = entryBytes(json);
    async function publish(farProfile) {
      const entry = await attachCityLod(join(directory, "hardware.glb"), original, { farProfile, workers: 1 });
      const exported = JSON.parse(entry.toString("utf8", 20, 20 + entry.readUInt32LE(12)));
      const sidecar = await readFile(join(directory, exported.scenes[0].extras.cityLod));
      delete exported.scenes[0].extras.cityLod;
      assert.deepEqual(exported, json);
      const length = sidecar.readUInt32LE(8), start = 12 + Math.ceil(length / 4) * 4;
      return { entry, sidecar, start, manifest: JSON.parse(sidecar.toString("utf8", 12, 12 + length)) };
    }
    const standard = await publish("standard"), aggressive = await publish("aggressive");
    assert.equal(aggressive.manifest.sourceSha256, standard.manifest.sourceSha256);
    assert.deepEqual(aggressive.manifest.primitives.map(record => record.mesh), [0, 2], "gameplay mesh has no LOD overrides");
    for (let i = 0; i < standard.manifest.primitives.length; i++) {
      const before = standard.manifest.primitives[i], after = aggressive.manifest.primitives[i];
      for (const key of ["levels", "detailLevels"]) for (let n = 0; n < before[key].length; n++) {
        const a = before[key][n], b = after[key][n];
        assert.deepEqual({ ...a, offset: 0 }, { ...b, offset: 0 });
        assert.deepEqual(standard.sidecar.subarray(standard.start + a.offset, standard.start + a.offset + a.bytes),
          aggressive.sidecar.subarray(aggressive.start + b.offset, aggressive.start + b.offset + b.bytes));
      }
    }
    assert.equal(aggressive.manifest.primitives[1].detailLevels.length, 5, "emitting lamp keeps its prior tiers");
    const selected = aggressive.manifest.primitives[0].detailLevels;
    assert.equal(selected.length, 7);
    assert.ok(selected[5].count < selected[4].count);
    assert.ok(selected[6].count <= selected[5].count);
    assert.ok(selected[5].errorMeters > 0.35 && selected[5].errorMeters <= 0.7);
    assert.ok(selected[6].errorMeters <= 1.4);
    assert.deepEqual((await publish("aggressive")).sidecar, aggressive.sidecar, "repeated publication is deterministic");
    assert.deepEqual(await readFile(bufferPath), bytes);
    const files = await readdir(join(directory, "hardware-data"));
    await assert.rejects(publish("unknown"), /profile must be standard or aggressive/);
    assert.deepEqual(await readdir(join(directory, "hardware-data")), files);
    geometry.dispose();
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("inventory LOD keeps the original compatibility streams and both stock bodies", async () => {
  const directory = await mkdtemp(join(tmpdir(), "superspec-city-lod-"));
  try {
    await mkdir(join(directory, "stock-data"));
    const cylinder = new THREE.CylinderGeometry(0.1, 0.1, 0.2, 32, 8);
    const vertices = cylinder.attributes.position.count;
    const positions = [], normals = [], uvs = [], indices = [];
    for (const [copy, x] of [-1, 1].entries()) {
      for (let i = 0; i < vertices; i++) {
        positions.push(cylinder.attributes.position.getX(i) + x, cylinder.attributes.position.getY(i), cylinder.attributes.position.getZ(i));
        normals.push(cylinder.attributes.normal.getX(i), cylinder.attributes.normal.getY(i), cylinder.attributes.normal.getZ(i));
        uvs.push(cylinder.attributes.uv.getX(i), cylinder.attributes.uv.getY(i));
      }
      for (const index of cylinder.index.array) indices.push(index + copy * vertices);
    }
    const arrays = [new Float32Array(positions), new Float32Array(normals), new Float32Array(uvs), new Uint32Array(indices)];
    let offset = 0;
    const bufferViews = arrays.map(array => {
      const view = { buffer: 0, byteOffset: offset, byteLength: array.byteLength };
      offset += array.byteLength;
      return view;
    });
    const source = Buffer.concat(arrays.map(array => Buffer.from(array.buffer)));
    await writeFile(join(directory, "stock-data", "mesh.bin"), source);
    const json = {
      asset: { version: "2.0" }, scene: 0, scenes: [{ nodes: [0], extras: {} }],
      nodes: [{ mesh: 0 }], meshes: [{ name: "city/prototype/stock", primitives: [{ attributes: { POSITION: 0, NORMAL: 1, TEXCOORD_0: 2 }, indices: 3, material: 0 }] }],
      materials: [{ name: "builtin:market-hero-props", extras: { worldSpecSurface: { family: "Shop inventory" } } }],
      buffers: [{ uri: "stock-data/mesh.bin", byteLength: source.length }], bufferViews,
      accessors: [
        { bufferView: 0, componentType: 5126, count: vertices * 2, type: "VEC3" },
        { bufferView: 1, componentType: 5126, count: vertices * 2, type: "VEC3" },
        { bufferView: 2, componentType: 5126, count: vertices * 2, type: "VEC2" },
        { bufferView: 3, componentType: 5125, count: indices.length, type: "SCALAR" },
      ],
    };
    const result = await attachCityLod(join(directory, "stock.glb"), entryBytes(json));
    const exported = JSON.parse(result.toString("utf8", 20, 20 + result.readUInt32LE(12)));
    const sidecarPath = join(directory, exported.scenes[0].extras.cityLod);
    const sidecar = await readFile(sidecarPath);
    delete exported.scenes[0].extras.cityLod;
    assert.deepEqual(exported, json);
    assert.deepEqual(await readFile(join(directory, "stock-data", "mesh.bin")), source);
    const length = sidecar.readUInt32LE(8), start = 12 + Math.ceil(length / 4) * 4;
    const records = JSON.parse(sidecar.toString("utf8", 12, 12 + length)).primitives;
    assert.equal(records.length, 1);
    assert.equal(records[0].levels.length, 2);
    assert.equal(records[0].detailLevels.length, 5);
    await MeshoptDecoder.ready;
    for (const level of records[0].detailLevels) {
      assert.ok(level.count > 0 && level.count < indices.length);
      const decoded = new Uint16Array(level.count);
      MeshoptDecoder.decodeIndexBuffer(new Uint8Array(decoded.buffer), level.count, 2, sidecar.subarray(start + level.offset, start + level.offset + level.bytes));
      assert.ok(decoded.every(index => index < vertices * 2));
      assert.ok(decoded.some(index => index < vertices), "left stock body survives");
      assert.ok(decoded.some(index => index >= vertices), "right stock body survives");
      assert.ok(level.errorMeters >= 0 && level.errorMeters <= 0.35);
    }
    await rename(sidecarPath, sidecarPath + ".backup");
    await mkdir(sidecarPath);
    await writeFile(join(sidecarPath, "sentinel"), "existing");
    const files = await readdir(join(directory, "stock-data"));
    await assert.rejects(attachCityLod(join(directory, "stock.glb"), entryBytes(json)), /different staged companion/);
    assert.deepEqual(await readdir(join(directory, "stock-data")), files);
    assert.equal(await readFile(join(sidecarPath, "sentinel"), "utf8"), "existing");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("bounded parallel simplification publishes the same ordered indices as a serial build", async () => {
  const directory = await mkdtemp(join(tmpdir(), "superspec-parallel-lod-"));
  try {
    await mkdir(join(directory, "grid-data"));
    const grid = new THREE.PlaneGeometry(8, 12, 160, 160);
    const positions = grid.attributes.position.array;
    const indices = Uint32Array.from(grid.index.array);
    const geometry = Buffer.concat([Buffer.from(positions.buffer), Buffer.from(indices.buffer)]);
    const uri = "grid-data/surface.bin";
    await writeFile(join(directory, uri), geometry);
    // Distinct policies over shared geometry exceed the parallel work threshold.
    const json = {
      asset: { version: "2.0" }, scene: 0, scenes: [{ nodes: [0], extras: {} }], nodes: [{ mesh: 0 }],
      meshes: [{ name: "city/prototype/grid", primitives: [0, 1, 0, 2].map(material => ({
        attributes: { POSITION: 0 }, indices: 1, material,
      })) }],
      materials: [{}, { extras: { worldSpecSurface: { family: "Shop inventory" } } },
        { extras: { worldSpecSurface: { family: "Building atlases", surface: "Mixed surfaces with metal", recipe: "weathered-steel" } } }],
      buffers: [{ uri, byteLength: geometry.length }],
      bufferViews: [
        { buffer: 0, byteLength: positions.byteLength },
        { buffer: 0, byteOffset: positions.byteLength, byteLength: indices.byteLength },
      ],
      accessors: [
        { bufferView: 0, componentType: 5126, type: "VEC3", count: positions.length / 3 },
        { bufferView: 1, componentType: 5125, type: "SCALAR", count: indices.length },
      ],
    };
    async function publish(workers) {
      const entry = await attachCityLod(join(directory, "grid.glb"), entryBytes(json), { workers, farProfile: "aggressive" });
      const manifest = JSON.parse(entry.toString("utf8", 20, 20 + entry.readUInt32LE(12)));
      const sidecar = await readFile(join(directory, manifest.scenes[0].extras.cityLod));
      return { entry, sidecar };
    }
    const serial = await publish(1);
    const sidecarUri = JSON.parse(serial.entry.toString("utf8", 20, 20 + serial.entry.readUInt32LE(12))).scenes[0].extras.cityLod;
    const files = await readdir(join(directory, "grid-data"));
    json.accessors[1].count--; // Malformed triangle count fails inside the worker.
    await assert.rejects(publish(2), error => /mesh 0, primitive [013]/.test(error.message) && error.cause instanceof Error);
    assert.deepEqual(await readdir(join(directory, "grid-data")), files);
    assert.deepEqual(await readFile(join(directory, sidecarUri)), serial.sidecar);
    json.accessors[1].count++;
    const parallel = await publish(2);
    assert.deepEqual(parallel.entry, serial.entry);
    assert.deepEqual(parallel.sidecar, serial.sidecar);
    const records = JSON.parse(parallel.sidecar.toString("utf8", 12, 12 + parallel.sidecar.readUInt32LE(8))).primitives;
    assert.deepEqual(records.map(record => [record.primitive, (record.detailLevels ?? record.distantLevels).length]),
      [[0, 1], [1, 5], [2, 1], [3, 7]]);
    let offset = 0;
    for (const record of records) for (const level of [...record.levels, ...(record.detailLevels ?? record.distantLevels)]) {
      assert.equal(level.offset, offset);
      offset += level.bytes;
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
