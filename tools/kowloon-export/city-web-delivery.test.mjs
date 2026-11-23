import assert from "node:assert/strict";
import test from "node:test";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import sharp from "sharp";
import { entryBytes, readCityEntry } from "./city-data-textures.mjs";
import { stageCityWebDelivery } from "./city-web-delivery.mjs";

test("web delivery rewrites aligned buffer views and keeps every decoded texel and non-image byte", async t => {
  const root = await mkdtemp(join(tmpdir(), "superspec-web-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(join(root, "core"));
  const source = join(root, "core", "city.glb"), destination = join(root, "web", "city.glb");
  const pixels = Buffer.alloc(128 * 128 * 4);
  for (let i = 0; i < pixels.length; i += 4) {
    pixels[i] = i >>> 4; pixels[i + 1] = 96; pixels[i + 2] = 180; pixels[i + 3] = i % 12 ? 255 : 0;
  }
  const png = await sharp(pixels, { raw: { width: 128, height: 128, channels: 4 } }).png({ compressionLevel: 0 }).toBuffer();
  const pad = Buffer.alloc((4 - png.length % 4) % 4);
  const geometry = Buffer.from([1, 0, 2, 0, 3, 0, 0, 0]);
  const combined = Buffer.concat([png, pad, geometry]);
  const glb = entryBytes({ asset: { version: "2.0" }, scenes: [{ nodes: [] }],
    buffers: [{ uri: "part.bin", byteLength: combined.length }],
    bufferViews: [{ buffer: 0, byteLength: png.length }, { buffer: 0, byteOffset: png.length + pad.length, byteLength: geometry.length }],
    images: [{ mimeType: "image/png", bufferView: 0 }], textures: [{ source: 0 }, { source: 0 }],
    accessors: [{ bufferView: 1, componentType: 5123, count: 4, type: "SCALAR" }],
  }, Buffer.alloc(0));
  await writeFile(source, glb);
  await writeFile(join(root, "core", "part.bin"), combined);
  const report = await stageCityWebDelivery(source, destination);
  assert.equal(report.converted, 1);
  assert.ok(report.deliveryBytes < report.originalBytes);
  const { json } = readCityEntry(await readFile(destination));
  assert.deepEqual(json.extensionsRequired, ["EXT_texture_webp"]);
  assert.deepEqual(json.textures, Array.from({ length: 2 }, () => ({ extensions: { EXT_texture_webp: { source: 0 } } })));
  const packed = await readFile(join(root, "web", json.buffers[0].uri));
  const offset = json.bufferViews[1].byteOffset;
  assert.equal(offset % 4, 0);
  assert.deepEqual(packed.subarray(offset), geometry);
  const decoded = await sharp(packed.subarray(0, json.bufferViews[0].byteLength)).ensureAlpha().raw().toBuffer();
  assert.deepEqual(decoded, pixels);
  assert.deepEqual(await readFile(join(root, "core", "part.bin")), combined);
  assert.deepEqual(await stageCityWebDelivery(source, destination), report);
});
