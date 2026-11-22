import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { decodePacket, type ScenePacket } from "../../browser/model/packet.ts";
import { SceneUpdates } from "../../browser/model/updates.ts";
import { rigidMaterialTranslation } from "../../browser/renderer/material-translation.ts";

const executable = resolve(process.env.RUST_CLI ?? `target/release/superspec${process.platform === "win32" ? ".exe" : ""}`);
const native = decodePacket(Uint8Array.from(execFileSync(executable, ["scene", "tests/fixtures/aml/rust-materials.aml"], { env: { ...process.env, PATH: "" } })).buffer);

test("retaining a local material frame requires one rigid owner and identical material, geometry and asset inputs", () => {
  const meshes = native.manifest.meshes.map(mesh => ({ ...mesh, objectId: "building" }));
  const before = { ...native, manifest: { ...native.manifest, meshes } };
  const delta: [number, number, number] = [0.125, -0.2, 0];
  const moved = { ...before, positions: before.positions.map((value, i) => value + delta[i % 3]) };
  const accepts = (packet: ScenePacket) => rigidMaterialTranslation(before, packet, "building", [0, 0, 0], delta);
  assert.ok(accepts(moved));
  const deformed = moved.positions.slice();
  deformed[0] += 0.001;
  assert.equal(accepts({ ...moved, positions: deformed }), false);
  assert.equal(accepts({ ...moved, colors: moved.colors.map((value, i) => i ? value : 1 - value) }), false);
  assert.equal(accepts({ ...moved, assets: Uint8Array.of(255, ...moved.assets.subarray(0, 15)) }), false);
  assert.equal(accepts({ ...moved, manifest: { ...moved.manifest, meshes: meshes.map((m, i) => i ? m : { ...m, objectId: "neighbor" }) } }), false);
  const neighbor = { ...meshes[0], objectId: "neighbor" };
  const mixed = { ...before, manifest: { ...before.manifest, meshes: [neighbor, ...meshes.slice(1)] } };
  const withNeighbor = { ...mixed, positions: moved.positions.slice() };
  withNeighbor.positions.set(before.positions.subarray(0, neighbor.vertexCount * 3));
  assert.ok(rigidMaterialTranslation(mixed, withNeighbor, "building", [0, 0, 0], delta));
  withNeighbor.positions[0] += 1;
  assert.equal(rigidMaterialTranslation(mixed, withNeighbor, "building", [0, 0, 0], delta), false);
});

/** Encodes a fixture packet's ranges in the public incremental transport layout. */
function update(packet: ScenePacket, revision: number, base: number | null, retain: boolean, retainedScene = false): ArrayBuffer {
  const manifest = { ...packet.manifest, revision };
  const header = { manifest, baseRevision: base, retainedScene, meshes: manifest.meshes.map((_, i) => retain ? i : null), assets: manifest.assets.map((_, i) => retain ? i : null) };
  const json = new TextEncoder().encode(JSON.stringify(header));
  const size = Math.ceil(json.length / 4) * 4;
  const bytes = new Uint8Array(8 + size + (retain ? 0 : manifest.vertexCount * 44 + manifest.indexCount * 4 + manifest.assetByteCount));
  bytes.set(new TextEncoder().encode("LSU1"));
  const view = new DataView(bytes.buffer);
  view.setUint32(4, size, true); bytes.fill(32, 8, 8 + size); bytes.set(json, 8);
  let cursor = 8 + size;
  if (!retain) {
    for (const mesh of manifest.meshes) {
      for (const [array, start, count] of [
        [packet.positions, mesh.vertexStart * 3, mesh.vertexCount * 3], [packet.normals, mesh.vertexStart * 3, mesh.vertexCount * 3],
        [packet.uvs, mesh.vertexStart * 2, mesh.vertexCount * 2], [packet.colors, mesh.vertexStart * 3, mesh.vertexCount * 3],
        [packet.indices, mesh.indexStart, mesh.indexCount],
      ] as const) {
        for (let i = start; i < start + count; i++, cursor += 4) {
          if (array instanceof Uint32Array) view.setUint32(cursor, array[i], true);
          else view.setFloat32(cursor, array[i], true);
        }
      }
    }
    bytes.set(packet.assets, cursor);
  }
  return bytes.buffer;
}

test("scene updates retain exact buffers and recover after a bad base revision", () => {
  const decoder = new SceneUpdates();
  const first = decoder.decode(update(native, 1, null, false));
  first.assetIdentities!.set(first.manifest.assets[0].id, Promise.resolve("unchanged-image"));
  assert.throws(() => decoder.decode(update(native, 2, 99, true)), /base revision/);
  const next = decoder.decode(update(native, 2, 1, true));
  for (const key of ["positions", "normals", "uvs", "colors", "indices", "assets"] as const) assert.deepEqual(next[key], native[key]);
  assert.equal(next.assetIdentities!.get(next.manifest.assets[0].id), first.assetIdentities!.get(first.manifest.assets[0].id));
  assert.throws(() => decoder.decode(update(native, 3, 2, true).slice(0, -1)), /metadata length/);
  assert.equal(decoder.decode(update(native, 3, 2, true)).manifest.revision, 3);
});

test("source-only deltas share admitted buffers and check their rendering and collision baseline", () => {
  const decoder = new SceneUpdates();
  const original = decoder.decode(update(native, 1, null, false));
  const manifest = structuredClone(original.manifest);
  manifest.name = "Renamed";
  manifest.meshes[0].source.offset++;
  delete manifest.meshes[0].source.end;
  const rename = decoder.decode(update({ ...original, manifest }, 2, 1, true, true));
  assert.equal(rename.retainedFrom, 1);
  assert.equal(rename.positions, original.positions);
  assert.equal(rename.assets, original.assets);
  manifest.meshes[0].visible = !manifest.meshes[0].visible;
  const changed = decoder.decode(update({ ...original, manifest }, 3, 2, true, true));
  assert.equal(changed.retainedFrom, undefined);
  assert.equal(changed.manifest.meshes[0].visible, manifest.meshes[0].visible);
  manifest.colliders = ["absent"];
  assert.throws(() => decoder.decode(update({ ...original, manifest }, 4, 3, true, true)));
  assert.equal(decoder.revision, 3);
});
