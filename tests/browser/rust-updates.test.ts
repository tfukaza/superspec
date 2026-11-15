import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { decodePacket, type ScenePacket } from "../../browser/model/packet.ts";
import { SceneUpdates } from "../../browser/model/updates.ts";

const executable = resolve(process.env.RUST_CLI ?? `target/release/superspec${process.platform === "win32" ? ".exe" : ""}`);
const native = decodePacket(Uint8Array.from(execFileSync(executable, ["scene", "tests/fixtures/aml/rust-materials.aml"], { env: { ...process.env, PATH: "" } })).buffer);

/** Encodes a fixture packet's ranges in the public incremental transport layout. */
function update(packet: ScenePacket, revision: number, base: number | null, retain: boolean): ArrayBuffer {
  const manifest = { ...packet.manifest, revision };
  const header = { manifest, baseRevision: base, meshes: manifest.meshes.map((_, i) => retain ? i : null), assets: manifest.assets.map((_, i) => retain ? i : null) };
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
