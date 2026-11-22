import assert from "node:assert/strict";
import test from "node:test";
import { Document } from "@gltf-transform/core";
import { KHRMaterialsClearcoat } from "@gltf-transform/extensions";
import sharp from "sharp";
import { atlasChannels, packAtlasPng } from "./atlas-png.mjs";

test("data pages retain every RGB sample and shared color pages retain cutout alpha", async () => {
  const doc = new Document(), page = doc.createTexture(), material = doc.createMaterial();
  material.setNormalTexture(page).setOcclusionTexture(page);
  const rgba = Buffer.alloc(128 * 128 * 4);
  for (let pixel = 0; pixel < rgba.length / 4; pixel++) {
    rgba.set([128, pixel % 29, 255, Math.floor(pixel / 31) % 256], pixel * 4);
  }
  const source = await sharp(rgba, { raw: { width: 128, height: 128, channels: 4 } }).png({ compressionLevel: 0 }).toBuffer();
  const packed = async () => packAtlasPng(source, atlasChannels(page), 6);
  const data = await packed();
  assert.equal((await sharp(data).metadata()).channels, 3);
  assert.deepEqual(await sharp(data).raw().toBuffer(), await sharp(source).removeAlpha().raw().toBuffer());
  const clearcoat = doc.createExtension(KHRMaterialsClearcoat).createClearcoat().setClearcoatNormalTexture(page);
  assert.equal(atlasChannels(page), "rgba");
  clearcoat.dispose();
  doc.createMaterial().setAlphaMode("MASK").setBaseColorTexture(page);
  assert.deepEqual(await sharp(await packed()).raw().toBuffer(), rgba);
  assert.equal(atlasChannels(doc.createTexture()), "rgba");
});
