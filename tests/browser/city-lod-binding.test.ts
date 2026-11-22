import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { loadCityLods } from "../../adapters/three/src/cityLods.ts";

const entryUrl = "https://viewer.example/city.glb";

async function sharedMeshes(order: number[]) {
  // glTF accessors without a bufferView initialize to zero. Geometry sharing
  // here goes through GLTFLoader's ordinary and GPU-instanced mesh paths.
  const primitive = { attributes: { POSITION: 0 }, indices: 1 };
  const data = {
    asset: { version: "2.0" }, scene: 0, scenes: [{ nodes: order, extras: { cityLod: "city-data/lod-cafe.bin" } }],
    extensionsUsed: ["EXT_mesh_gpu_instancing"],
    nodes: [{ mesh: 0, name: "first" }, { mesh: 1, name: "second",
      extensions: { EXT_mesh_gpu_instancing: { attributes: { TRANSLATION: 2 } } } }, { mesh: 1, name: "repeat" }],
    meshes: [{ primitives: [primitive] }, { primitives: [primitive] }],
    accessors: [
      { componentType: 5126, count: 4, type: "VEC3", min: [0, 0, 0], max: [0, 0, 0] },
      { componentType: 5123, count: 6, type: "SCALAR" },
      { componentType: 5126, count: 2, type: "VEC3" },
    ],
  };
  const gltf = await new GLTFLoader().parseAsync(JSON.stringify(data), "");
  const mesh = (name: string) => gltf.scene.getObjectByName(name) as THREE.Mesh;
  const first = mesh("first"), second = mesh("second"), repeat = mesh("repeat");
  assert.ok(second instanceof THREE.InstancedMesh);
  assert.equal(first.geometry, second.geometry);
  assert.equal(second.geometry, repeat.geometry);
  return { gltf, first, second, repeat, original: first.geometry };
}

function record(mesh: number, counts: number[]) {
  return { mesh, primitive: 0, vertexCount: 4, nearCount: 6,
    levels: counts.map(count => ({ count, offset: 0, bytes: 0, componentBytes: 2, errorMeters: mesh + 0.25 })) };
}

function sidecar(primitives: ReturnType<typeof record>[]) {
  const manifest = new TextEncoder().encode(JSON.stringify({ primitives }));
  const buffer = new ArrayBuffer(12 + Math.ceil(manifest.length / 4) * 4);
  const data = new Uint8Array(buffer);
  data.set([83, 76, 79, 68, 1, 0, 0, 0]); data.set(manifest, 12);
  new DataView(buffer).setUint32(8, manifest.length, true);
  return new Response(buffer);
}

test("city LOD chains follow authored primitive identity in either scene or sidecar order", async t => {
  let records = [record(0, [6, 0]), record(1, [0, 0])];
  t.mock.method(globalThis, "fetch", () => sidecar(records));
  for (const sidecarOrder of [records, [...records].reverse()]) for (const order of [[0, 1, 2], [2, 1, 0]]) {
    records = sidecarOrder;
    const { gltf, first, second, repeat, original } = await sharedMeshes(order);
    const lods = await loadCityLods(gltf, entryUrl);
    assert.equal(lods.size, 2);
    assert.deepEqual(lods.get(first.geometry)?.map(level => level.index!.count), [6, 0]);
    assert.deepEqual(lods.get(second.geometry)?.map(level => level.index!.count), [0, 0]);
    assert.equal(lods.get(second.geometry)?.[1].userData.cityLodErrorMeters, 1.25);
    assert.equal(second.geometry, repeat.geometry, "placements of the same authored primitive retain shared geometry");
    for (const mesh of [first, second]) {
      assert.equal(mesh.geometry.attributes.position, original.attributes.position);
      assert.equal(mesh.geometry.index, original.index);
    }
  }
});

test("an alias without a sidecar record keeps its original full-detail geometry", async t => {
  t.mock.method(globalThis, "fetch", () => sidecar([record(0, [6, 0])]));
  const { gltf, first, second, repeat, original } = await sharedMeshes([1, 0, 2]);
  const lods = await loadCityLods(gltf, entryUrl);
  assert.ok(lods.has(first.geometry));
  assert.equal(second.geometry, original); assert.equal(repeat.geometry, original);
  assert.equal(lods.has(original), false);
});

test("a malformed second chain leaves every placement and its canonical metadata untouched", async t => {
  t.mock.method(console, "warn", () => {});
  t.mock.method(globalThis, "fetch", () => sidecar([record(0, [6, 0]), record(1, [4, 0])]));
  const { gltf, first, second, repeat, original } = await sharedMeshes([0, 1, 2]);
  const lods = await loadCityLods(gltf, entryUrl);
  assert.equal(lods.size, 0);
  for (const mesh of [first, second, repeat]) assert.equal(mesh.geometry, original);
  assert.equal(original.userData.cityLodErrorMeters, undefined);
});
