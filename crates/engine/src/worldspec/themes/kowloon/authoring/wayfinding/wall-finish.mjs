import { readFile } from "node:fs/promises";
const name = "Ochre arcade passage plaster";

export async function finishPassageWall(doc, owner, TextureInfo) {
  const parts = doc
    .getRoot()
    .listNodes()
    .filter((node) => node.getExtras().objectId === owner)
    .flatMap((node) => node.getMesh()?.listPrimitives() ?? [])
    .filter((part) => part.getMaterial()?.getName() === "m-yellow");
  if (!parts.length) return [];

  const low = [Infinity, Infinity, Infinity];
  const high = [-Infinity, -Infinity, -Infinity];
  for (const part of parts) {
    const positions = part.getAttribute("POSITION");
    const min = positions.getMin([]),
      max = positions.getMax([]);
    for (let i = 0; i < 3; i++) {
      low[i] = Math.min(low[i], min[i]);
      high[i] = Math.max(high[i], max[i]);
    }
  }
  const width = high[0] - low[0],
    height = high[2] - low[2];
  if (!(width > 0 && height > 0))
    throw Error("Passage wall has no paintable area.");

  const textures = await Promise.all(
    ["base", "normal", "properties"].map(async (role) => {
      const image = await readFile(
        new URL("./ochre-" + role + ".png", import.meta.url),
      );
      return doc
        .createTexture(name + " " + role)
        .setMimeType("image/png")
        .setImage(image);
    }),
  );
  const [base, normal, properties] = textures;
  const material = doc
    .createMaterial(name)
    .setExtras({worldSpecSmoothBounce: true})
    .setBaseColorTexture(base)
    .setNormalTexture(normal)
    .setMetallicFactor(0)
    .setRoughnessFactor(1)
    .setMetallicRoughnessTexture(properties);
  for (const info of [
    material.getBaseColorTextureInfo(),
    material.getNormalTextureInfo(),
    material.getMetallicRoughnessTextureInfo(),
  ]) {
    info
      .setWrapS(TextureInfo.WrapMode.CLAMP_TO_EDGE)
      .setWrapT(TextureInfo.WrapMode.CLAMP_TO_EDGE);
  }
  for (const part of parts) {
    const pos = part.getAttribute("POSITION"),
      normals = part.getAttribute("NORMAL");
    const uv = new Float32Array(pos.getCount() * 2);
    for (let i = 0; i < pos.getCount(); i++) {
      const [x, y, z] = pos.getElement(i, []);
      const [nx, , nz] = normals.getElement(i, []);
      // Both broad faces share the same placement of wear along the casting.
      // The narrow returns draw from the adjoining front edge at metre scale.
      const inset = high[1] - y;
      let u = (high[0] - x) / width,
        v = (high[2] - z) / height;
      if (Math.abs(nx) > 0.9) u = nx > 0 ? inset / width : 1 - inset / width;
      if (Math.abs(nz) > 0.9) v = nz > 0 ? inset / height : 1 - inset / height;
      uv.set([u, v], 2 * i);
    }
    const chart = doc
      .createAccessor(name + " UV")
      .setBuffer(pos.getBuffer())
      .setType("VEC2")
      .setArray(uv);
    // The GLTF renderer derives the paint's tangent frame from this projection.
    // Discard an optional tangent exported for the former box chart.
    part
      .setAttribute("TANGENT", null)
      .setAttribute("TEXCOORD_0", chart)
      .setMaterial(material);
  }
  return textures;
}
