import test from "node:test";
import assert from "node:assert/strict";
import { deflateRawSync } from "node:zlib";
import { crc32, readArchive, writeArchive } from "../../editor/studio/archive.ts";
import { openProject, projectFile, type StudioProject } from "../../editor/studio/project.ts";

const bytes = (length: number, seed: number) => Uint8Array.from({ length }, (_, i) => (i * 31 + seed) & 255);
const project: StudioProject = {
  schemaVersion: 1, kind: "material", entry: "main.ms", modules: { "main.ms": "material 1;\n" }, parameters: {}, seed: 3,
  resolution: 64, mapping: "uv", inspection: "final", domain: [1, 1], base: { kind: "glb", name: "model.glb" },
  target: { scope: "mesh", enabled: false }, edges: [], gravity: [0, 0, -1],
  assets: { "model.glb": bytes(5000, 1), "textures/a b.png": bytes(70, 2), "../outside.png": bytes(9, 3) },
} as StudioProject;

test("CRC-32 matches the standard check value", () => {
  assert.equal(crc32(new TextEncoder().encode("123456789")), 0xcbf43926);
});

test("small projects stay JSON and large ones become reopenable archives", async () => {
  assert.match(projectFile(project).name, /\.project\.json$/);
  const saved = projectFile(project, 0);
  assert.equal(saved.name, "material.studio.zip");
  const archive = new Uint8Array(await saved.content.arrayBuffer());
  // Saving is deterministic: the same project gives the same bytes.
  assert.deepEqual(new Uint8Array(await projectFile(project, 0).content.arrayBuffer()), archive);
  const entries = await readArchive(archive, () => {});
  // Plain relative names keep their paths; others get neutral entry names.
  assert.deepEqual([...entries.keys()], ["project.json", "assets/model.glb", "assets/textures/a b.png", "assets/companion-2"]);
  const reopened = await openProject(saved.content, "material");
  assert.deepEqual(Object.keys(reopened.assets), Object.keys(project.assets));
  for (const [name, asset] of Object.entries(project.assets)) assert.deepEqual(reopened.assets[name], asset);
  assert.equal(reopened.seed, 3);
  await assert.rejects(openProject(saved.content, "item"), /not a supported/);
});

test("archives re-zipped with deflate open; damaged or incomplete ones are rejected", async () => {
  const document = new TextEncoder().encode(JSON.stringify({ ...project, assets: { "model.glb": "assets/model.glb" } }));
  const model = bytes(5000, 1);
  // A deflated archive, as an ordinary zip tool writes it.
  const deflated = await writeDeflated([{ name: "project.json", bytes: document }, { name: "assets/model.glb", bytes: model }]);
  assert.deepEqual((await openProject(new Blob([deflated]), "material")).assets["model.glb"], model);
  const stored = new Uint8Array(await writeArchive([{ name: "project.json", bytes: document }, { name: "assets/model.glb", bytes: model }]).arrayBuffer());
  const damaged = stored.slice();
  damaged[stored.length - 200] ^= 1;
  await assert.rejects(openProject(new Blob([damaged]), "material"), /checksum/);
  const missing = new Uint8Array(await writeArchive([{ name: "project.json", bytes: document }]).arrayBuffer());
  await assert.rejects(openProject(new Blob([missing]), "material"), /missing companion model\.glb/);
  await assert.rejects(readArchive(stored, name => { if (name === "assets/model.glb") throw new Error("too large"); }), /too large/);
});

/** Writes an archive with deflated entries, as general-purpose zip tools do. */
async function writeDeflated(entries: { name: string; bytes: Uint8Array }[]): Promise<Uint8Array> {
  const parts: Uint8Array[] = [];
  const directory: Uint8Array[] = [];
  let offset = 0;
  for (const { name, bytes } of entries) {
    const encoded = new TextEncoder().encode(name);
    const data = deflateRawSync(bytes);
    const local = new Uint8Array(30 + encoded.length);
    const header = new DataView(local.buffer);
    header.setUint32(0, 0x04034b50, true); header.setUint16(4, 20, true); header.setUint16(8, 8, true);
    header.setUint32(14, crc32(bytes), true); header.setUint32(18, data.length, true); header.setUint32(22, bytes.length, true);
    header.setUint16(26, encoded.length, true); local.set(encoded, 30);
    const central = new Uint8Array(46 + encoded.length);
    const entry = new DataView(central.buffer);
    entry.setUint32(0, 0x02014b50, true); entry.setUint16(6, 20, true); entry.setUint16(10, 8, true);
    entry.setUint32(16, crc32(bytes), true); entry.setUint32(20, data.length, true); entry.setUint32(24, bytes.length, true);
    entry.setUint16(28, encoded.length, true); entry.setUint32(42, offset, true); central.set(encoded, 46);
    parts.push(local, data); directory.push(central);
    offset += local.length + data.length;
  }
  const size = directory.reduce((sum, part) => sum + part.length, 0);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true); end.setUint16(8, entries.length, true); end.setUint16(10, entries.length, true);
  end.setUint32(12, size, true); end.setUint32(16, offset, true);
  return new Uint8Array(await new Blob([...parts, ...directory, end.buffer] as BlobPart[]).arrayBuffer());
}
