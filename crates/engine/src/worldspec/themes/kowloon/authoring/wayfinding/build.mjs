import { readFile } from "node:fs/promises";
export async function buildWayfinding({ center, normal, right, width, height, host, art }, Document) {
  const doc = new Document();
  const buffer = doc.createBuffer();
  const uv = [
    [0, 1],
    [1, 1],
    [1, 0],
    [0, 0],
  ];
  const positions = uv.flatMap(([u, v]) =>
    center.map(
      (c, i) =>
        c + right[i] * (u - 0.5) * width + (i === 2 ? (0.5 - v) * height : 0),
    ),
  );
  const attribute = (type, data) =>
    doc.createAccessor().setBuffer(buffer).setType(type).setArray(data);
  const pigment = doc
    .createTexture("Arcade passage stencil pigment")
    .setMimeType("image/png")
    .setImage(await readFile(new URL(art, import.meta.url)));
  const ink = doc
    .createMaterial("Arcade passage painted wayfinding")
    .setBaseColorTexture(pigment)
    .setEmissiveTexture(pigment)
    .setEmissiveFactor([0.12, 0.12, 0.11])
    .setMetallicFactor(0)
    .setRoughnessFactor(0.95)
    .setAlphaMode("MASK")
    .setAlphaCutoff(0.34)
    .setExtras({ castsShadow: false, worldSpecOutline: false });
  const quad = doc
    .createPrimitive()
    .setMaterial(ink)
    .setAttribute("POSITION", attribute("VEC3", new Float32Array(positions)))
    .setAttribute(
      "NORMAL",
      attribute("VEC3", new Float32Array(Array(4).fill(normal).flat())),
    )
    .setAttribute("TEXCOORD_0", attribute("VEC2", new Float32Array(uv.flat())))
    .setIndices(attribute("SCALAR", new Uint16Array([0, 1, 2, 0, 2, 3])));
  doc.createScene("Lower arcade wayfinding").addChild(
    doc
      .createNode("Arcade passage on " + host)
      .setRotation([-Math.SQRT1_2, 0, 0, Math.SQRT1_2])
      .setMesh(doc.createMesh("Wall stencil").addPrimitive(quad)),
  );
  return doc;
}
