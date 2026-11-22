import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
const grainSource = new URL("../../../../materialspec/program/examples/textures/concrete-fine-sand.png", import.meta.url);
const mineralSource = new URL("../../../../materialspec/program/examples/textures/concrete-plain.png", import.meta.url);
const resolution = 2048;
const tileMeters = 2;

function triangleCount(p) {
  return (p.getIndices() ?? p.getAttribute("POSITION")).getCount() / 3;
}

// Baking expands triangle corners and packs pages by surface, losing the ItemSpec path.
// Match the native balcony receiver by its actual triangles before marking its atlas material.
function triangleKey(p) {
  const index = p.getIndices(), vertices = p.getAttribute("POSITION"), triangles = [];
  for (let i = 0; i < triangleCount(p) * 3; i += 3) {
    const corners = [0, 1, 2].map(k => vertices.getElement(index ? index.getScalar(i + k) : i + k, [])
      .map(v => Math.round(v * 10000)).join(","));
    triangles.push(corners.sort().join(";"));
  }
  return triangles.sort().join("|");
}

export function markConcrete(doc, native) {
  if (!native) {
    for (const material of doc.getRoot().listMaterials()) {
      if (material.getName().startsWith("Arena concrete "))
        material.setExtras({ ...material.getExtras(), worldSpecConcrete: true });
    }
    return;
  }
  const faces = doc.getRoot().listMeshes().flatMap(m => m.listPrimitives());
  for (const node of native.getRoot().listNodes()) {
    if (!node.getName().endsWith("/balcony/balcony/concrete")) continue;
    for (const src of node.getMesh().listPrimitives()) {
      const key = triangleKey(src);
      const matches = faces.filter(face => triangleCount(face) === triangleCount(src) && triangleKey(face) === key);
      if (matches.length !== 1) throw Error("Expected one baked balcony receiver for " + node.getName());
      const face = matches[0], material = face.getMaterial().clone();
      face.setMaterial(material.setExtras({ ...material.getExtras(), worldSpecConcrete: true }));
    }
  }
}

async function highPass(sharp, source, size, blur) {
  const pixels = await sharp(fileURLToPath(source))
    .resize(size, size)
    .greyscale()
    .raw()
    .toBuffer();
  const low = await sharp(pixels, {
    raw: { width: size, height: size, channels: 1 },
  })
    .blur(blur)
    .greyscale()
    .raw()
    .toBuffer();
  return Int16Array.from(pixels, (value, i) => value - low[i]);
}

// Sparse 1–6 mm pinholes in the cement paste. Draw directly at final resolution,
// wrapping every footprint so that pits crossing a tile edge remain continuous.
function addPores(field) {
  let state = 0x3fb921c;
  function random() {
    state = (Math.imul(1664525, state) + 1013904223) >>> 0;
    return state / 4294967296;
  }
  const mask = resolution - 1;
  for (let i = 0; i < 18000; i++) {
    const cx = random() * resolution,
      cy = random() * resolution;
    const a = 0.65 + 2.55 * random() ** 2,
      b = a * (0.52 + 0.46 * random());
    const angle = 2 * Math.PI * random(),
      c = Math.cos(angle),
      s = Math.sin(angle);
    const notch = 2 * Math.PI * random(),
      depth = 9 + 23 * random();
    const reach = Math.ceil(a * 1.35 + 1);
    for (let y = Math.floor(cy) - reach; y <= cy + reach; y++) {
      for (let x = Math.floor(cx) - reach; x <= cx + reach; x++) {
        const dx = x + 0.5 - cx,
          dy = y + 0.5 - cy;
        const u = (dx * c + dy * s) / a,
          v = (dy * c - dx * s) / b;
        const r =
          Math.hypot(u, v) *
          (1 + 0.16 * Math.sin(3 * Math.atan2(v, u) + notch));
        const coverage = Math.max(0, Math.min(1, (1.12 - r) * Math.min(a, b)));
        field[(y & mask) * resolution + (x & mask)] -= depth * coverage;
      }
    }
  }
}

/** Add a mean-neutral, world-scaled grain tile above the authored wear atlas. */
export async function appendConcreteDetail(json, folder, sharp, publish) {
  const receivers = json.materials.filter(m => m.extras?.worldSpecConcrete || m.extras?.worldSpecAggregate);
  if (!receivers.length) return;
  const half = resolution / 2;
  const [sand, mineral] = await Promise.all([
    highPass(sharp, grainSource, resolution, 10),
    highPass(sharp, mineralSource, half, 2.2),
  ]);
  const field = new Float32Array(resolution ** 2);
  for (let y = 0; y < resolution; y++) {
    for (let x = 0; x < resolution; x++) {
      const i = y * resolution + x;
      field[i] = sand[i] * 2.1 + mineral[(x % half) * half + (y % half)] * 1.5;
    }
  }
  addPores(field);
  const center = field.reduce((s, v) => s + v, 0) / field.length;
  const residual = Buffer.allocUnsafe(field.length);
  for (let i = 0; i < field.length; i++)
    residual[i] = Math.round(
      Math.max(0, Math.min(255, 128 + field[i] - center)),
    );
  const encoded = await sharp(residual, {
    raw: { width: resolution, height: resolution, channels: 1 },
  })
    .toColourspace("b-w")
    .jpeg({ quality: 94 })
    .toBuffer();
  const pixels = await sharp(encoded).greyscale().raw().toBuffer();
  const mean = pixels.reduce((s, v) => s + v, 0) / (255 * pixels.length);
  const name =
    "chunk-" +
    createHash("sha256").update(encoded).digest("hex").slice(0, 24) +
    ".bin";
  await publish(name, encoded);
  const buffer =
    json.buffers.push({
      uri: folder + "/" + name,
      byteLength: encoded.length,
    }) - 1;
  const bufferView =
    json.bufferViews.push({
      buffer,
      byteOffset: 0,
      byteLength: encoded.length,
    }) - 1;
  const source =
    json.images.push({
      name: "Washroom concrete fine aggregate",
      bufferView,
      mimeType: "image/jpeg",
    }) - 1;
  json.samplers ??= [];
  const sampler =
    json.samplers.push({
      magFilter: 9729,
      minFilter: 9987,
      wrapS: 10497,
      wrapT: 10497,
    }) - 1;
  const texture = json.textures.push({ source, sampler }) - 1;
  json.scenes[json.scene ?? 0].extras.worldSpecConcreteDetail = {
    version: 1,
    texture,
    metersPerTile: tileMeters,
    mean,
    strength: 0.45,
    micro: { metersPerTile: 0.25, strength: 0.29 },
  };
  console.log(
    "Shared concrete detail:",
    encoded.length,
    "bytes,",
    resolution / tileMeters,
    "texels/m,",
    receivers.length,
    "materials",
  );
}
