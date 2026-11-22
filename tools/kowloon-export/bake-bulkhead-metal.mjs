// Rebuild the portable catalog maps with the same MaterialSpec baker used for the room services.
import { Document, NodeIO } from "@gltf-transform/core";
import { copyToDocument } from "@gltf-transform/functions";
import sharp from "sharp";
import { resolve } from "node:path";
import { openBaker } from "../../tools/headless/host.mjs";
import { serviceMetalProgram } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/service-duct-finish.mjs";
import { bakeBulkheadReflection } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/bulkhead-reflection.mjs";

const out = process.argv[2];
if (!out) throw new Error("Specify the material image output directory.");
const host = openBaker();
try {
  const doc = new Document(),
    buffer = doc.createBuffer();
  const pos = doc
    .createAccessor()
    .setBuffer(buffer)
    .setType("VEC3")
    .setArray(new Float32Array([0, 0, 0, 1, 0, 0, 0, 0, 1]));
  const normal = doc
    .createAccessor()
    .setBuffer(buffer)
    .setType("VEC3")
    .setArray(new Float32Array([0, -1, 0, 0, -1, 0, 0, -1, 0]));
  const uv = doc
    .createAccessor()
    .setBuffer(buffer)
    .setType("VEC2")
    .setArray(new Float32Array([0, 0, 1, 0, 0, 1]));
  const receiver = doc
    .createMesh()
    .addPrimitive(
      doc
        .createPrimitive()
        .setAttribute("POSITION", pos)
        .setAttribute("NORMAL", normal)
        .setAttribute("TEXCOORD_0", uv),
    );
  const { material, uniforms } = await serviceMetalProgram(
    doc,
    receiver,
    host,
    { Document, NodeIO, copyToDocument },
    "bulkhead-spun.material.ts",
  );
  const size = 384;
  const bake = await host.baker.bake(material, uniforms, {
    width: size,
    height: size,
    gutter: 0,
    origin: [0, 0, 0],
    u: [1, 0, 0],
    v: [0, 0, 1],
    normal: [0, -1, 0],
    uvOrigin: [0, 0],
    uvU: [1, 0],
    uvV: [0, 1],
    extent: [1, 1],
    mapping: "world",
    objectOrigin: [0, 0, 0],
    boundsMin: [0, 0, 0],
    boundsMax: [1, 0, 1],
    gravity: [0, 0, -1],
    features: [],
  });
  if (!bake) throw new Error("Spun metal bake was cancelled.");
  const channels = host.baker.readBack(bake);
  for (const [channel, suffix] of [
    ["baseColor", ""],
    ["normal", "-normal"],
    ["properties", "-properties"],
  ]) {
    await sharp(Buffer.from(channels[channel]), { raw: { width: size, height: size, channels: 4 } })
      .flip()
      .removeAlpha()
      .png()
      .toFile(resolve(out, "bulkhead-spun-metal" + suffix + ".png"));
  }
  await bakeBulkheadReflection(sharp, resolve(out, "bulkhead-spun-metal-reflection.png"), size);
} finally {
  host.close();
}
