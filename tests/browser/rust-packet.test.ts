import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { decodePacket, type Manifest } from "../../browser/model/packet.ts";

const executable = resolve(process.env.RUST_CLI ?? `target/release/superspec${process.platform === "win32" ? ".exe" : ""}`);
const native = execFileSync(executable, ["scene", "tests/fixtures/aml/rust-stage1.aml"], { env: { ...process.env, PATH: "" } });

/** Gives the decoder an owned buffer with no Node Buffer offset. */
function packet(): ArrayBuffer { return Uint8Array.from(native).buffer; }

test("Rust scene packet exposes geometry, source identity and separate collision selection", () => {
  const scene = decodePacket(packet());
  assert.equal(scene.manifest.meshes.filter(mesh => mesh.visible && mesh.id !== "door").length, 4);
  for (const id of ["ground", "wall", "bench", "plinth"]) assert.ok(scene.manifest.colliders.includes(`$collision/${id}`));
  assert.equal(scene.manifest.meshes.find(mesh => mesh.id === "wall")?.source.line, 7);
  assert.equal(scene.manifest.meshes.find(mesh => mesh.id === "$collision/wall")?.objectId, "wall");
  assert.equal(scene.positions.length, scene.manifest.vertexCount * 3);
  assert.equal(scene.colors.length, scene.manifest.vertexCount * 3);
  assert.ok(scene.colors.every(value => value === 1), "architectural vertices retain neutral white tints");
});

const textured = execFileSync(executable, ["scene", "tests/fixtures/aml/rust-materials.aml"], { env: { ...process.env, PATH: "" } });

/** Reframes a real native payload after deliberately corrupting its manifest. */
function alteredManifest(change: (manifest: Manifest) => void): ArrayBuffer {
  const original = Uint8Array.from(textured);
  const jsonLength = new DataView(original.buffer).getUint32(4, true);
  const manifest: Manifest = JSON.parse(new TextDecoder().decode(original.subarray(8, 8 + jsonLength)));
  change(manifest);
  const json = new TextEncoder().encode(JSON.stringify(manifest));
  const padded = Math.ceil(json.length / 4) * 4;
  const output = new Uint8Array(8 + padded + original.length - 8 - jsonLength);
  output.set(original.subarray(0, 4));
  new DataView(output.buffer).setUint32(4, padded, true);
  output.fill(32, 8, 8 + padded);
  output.set(json, 8);
  output.set(original.subarray(8 + jsonLength), 8 + padded);
  return output.buffer;
}

test("textured packets reject invalid assets, UVs and PBR channels before upload", () => {
  const scene = decodePacket(Uint8Array.from(textured).buffer);
  assert.equal(scene.manifest.assets.length, 1);
  assert.equal(scene.assets.length, scene.manifest.assets[0].byteLength);
  for (const offset of [-1, 1]) {
    assert.throws(() => decodePacket(alteredManifest(m => { m.assets[0].byteStart += offset; })), /asset range/i);
  }
  assert.throws(() => decodePacket(alteredManifest(m => { m.assets.push({ ...m.assets[0] }); })), /asset range/i);
  assert.throws(() => decodePacket(alteredManifest(m => { m.assets[0].byteLength--; })), /asset ranges/i);
  assert.throws(() => decodePacket(alteredManifest(m => { m.materials[0].normalTexture = "missing.png"; })), /channels/);
  assert.throws(() => decodePacket(alteredManifest(m => { m.materials[0].roughness = 1.1; })), /material/);
  assert.throws(() => decodePacket(alteredManifest(m => { m.materials[0].normalScale = -1; })), /channels/);
  assert.throws(() => decodePacket(alteredManifest(m => { m.meshes[0].objectId = ""; })), /mesh/i);
  assert.throws(() => decodePacket(Uint8Array.from(textured).buffer.slice(0, -1)), /length/);
  const uv = Uint8Array.from(textured).buffer;
  const view = new DataView(uv);
  view.setFloat32(8 + view.getUint32(4, true) + scene.manifest.vertexCount * 24, Infinity, true);
  assert.throws(() => decodePacket(uv), /Nonfinite UV/);
});

test("invalid versions, truncated payloads, nonfinite positions and invalid indices fail before upload", () => {
  assert.throws(() => decodePacket(packet().slice(0, 7)), /header/);
  assert.throws(() => decodePacket(packet().slice(0, -4)), /length/);
  const magic = packet(); new DataView(magic).setUint32(0, 0);
  assert.throws(() => decodePacket(magic), /header/);
  const nonfinite = packet(); const view = new DataView(nonfinite);
  view.setFloat32(8 + view.getUint32(4, true), NaN, true);
  assert.throws(() => decodePacket(nonfinite), /Nonfinite/);
  const index = packet(); new DataView(index).setUint32(index.byteLength - 4, 0xffffffff, true);
  assert.throws(() => decodePacket(index), /index/);
  const version = packet(); const bytes = new Uint8Array(version);
  const text = new TextDecoder().decode(bytes);
  const offset = text.indexOf('"schemaVersion":3') + '"schemaVersion":'.length;
  bytes[offset] = "4".charCodeAt(0);
  assert.throws(() => decodePacket(version), /manifest/);
});

test("linear vertex colors preserve ranges, own their bytes and leave texture payloads intact", () => {
  const original = decodePacket(Uint8Array.from(textured).buffer);
  const buffer = Uint8Array.from(textured).buffer;
  const view = new DataView(buffer);
  const offset = 8 + view.getUint32(4, true) + original.manifest.vertexCount * 32;
  for (let i = 0; i < original.colors.length; i++) view.setFloat32(offset + i * 4, [0, .5, 1.5][i % 3], true);
  const decoded = decodePacket(buffer);
  assert.deepEqual([...decoded.colors.slice(0, 6)], [0, .5, 1.5, 0, .5, 1.5]);
  assert.deepEqual(decoded.indices, original.indices);
  assert.deepEqual(decoded.assets, original.assets);
  for (const value of [NaN, Infinity, -.1]) {
    view.setFloat32(offset, value, true);
    assert.throws(() => decodePacket(buffer), /vertex color/);
    assert.equal(decoded.colors[0], 0, "decoded color memory remains independently owned");
  }
});

