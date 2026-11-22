import { writeImmutable } from "./immutable-file.mjs";
import { atlasChannels, packAtlasPng } from "./atlas-png.mjs";
import { attachMaterialLibrary } from "./material-library.mjs";
import { readDeclaredLod, appendDeclaredLod, declaredLodBytes } from "./declared-lod-pack.mjs";
import { readFile, writeFile, mkdir, open, rename, rm } from "node:fs/promises";
import { createHash, randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";
import { basename, dirname, resolve } from "node:path";
import sharp from "sharp";
import { attachPaintedLawns } from "./painted-lawn-pack.mjs";
import { markConcrete, appendConcreteDetail } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/concrete-detail.mjs";
import { finishPassageWall } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/wayfinding/wall-finish.mjs";
import {
  arcadeBattenLight, citySky, passageLampLight, planterCeilingLight, shopAccentLights, shopTubeLight,
} from "../../crates/engine/src/worldspec/themes/kowloon/materials/arcade-lighting.mjs";
import { finishArena } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/arena-finish.mjs";
import { finishDownstands } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/downstand-finish.mjs";
import { finishGrilleBlades } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/maintenance/grille-finish.mjs";
import { finishServicePipes } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/service-pipe-finish.mjs";
import { finishServiceDucts } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/service-duct-finish.mjs";
import { readMaterialEdges } from "./material-edges.mjs";
import { bakeStreetTree, finishStreetTreeLods } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/street-trees.mjs";
import { groupStreetInstances } from "./street-batches.mjs";
import { cityLighting, kowloonFarProfile, publishLitCity } from "./bounced-light.mjs";

import { Document, NodeIO, Logger, TextureInfo } from "@gltf-transform/core";
import { ALL_EXTENSIONS, KHRMaterialsEmissiveStrength } from "@gltf-transform/extensions";
import {
  copyToDocument,
  dedup,
  flatten,
  instance,
  join,
  prune,
  transformMesh,
  unpartition,
  quantize,
  weld,
} from "@gltf-transform/functions";

const [directory, destination, ...options] = process.argv.slice(2);
const blockCell = /^--block-proxy=(-?\d+,-?\d+)$/;
if (!destination || options.some(option => option !== "--compact" && !/^--far-profile=(standard|aggressive)$/.test(option) && !blockCell.test(option))
  || options.filter(option => option === "--compact").length > 1
  || options.filter(option => option.startsWith("--far-profile=")).length > 1
  || options.filter(option => blockCell.test(option)).length > 1)
  throw Error("Usage: node pack.mjs directory destination.glb [--compact] [--far-profile=standard|aggressive] [--block-proxy=X,Z]");
const blockProxyCell = options.find(option => blockCell.test(option))?.split("=")[1];
const compressionLevel = options.includes("--compact") ? 9 : 6;
const lodFarProfile = kowloonFarProfile(options.find(option => option.startsWith("--far-profile="))?.split("=")[1]);
const io = new NodeIO().registerExtensions(ALL_EXTENSIONS);
const result = new Document();
const metalBake = { Document, NodeIO, TextureInfo, copyToDocument, readMaterialEdges, sharp };
result.createExtension(KHRMaterialsEmissiveStrength);
const shopLightPositions = [];
const authoredLods = [];
// The Kowloon city record as it is, so the map looks the same in the game and in
// the WorldSpec viewer. Its sun puts a strip of daylight across the stoa's lane
// while leaving the shops under the awnings, and its `horizon` occlusion is the
// shader the game carries a copy of.
const lighting = await cityLighting();
const scene = result.createScene("Stoa").setExtras({
  worldSpec: true,
  worldSpecRender: { version: 1, effects: ["paintedShopSurfaces", "displayAntialiasing"], outlines: false },
  worldSpecLighting: lighting,
  worldSpecEnvironment: {
    version: 1,
    bounds: { minX: -32, minZ: -32, maxX: 32, maxZ: 32 },
    sky: citySky,
    fog: { near: 110, far: 390 },
  },
});
result.getRoot().setDefaultScene(scene);
const placements = JSON.parse(
  await readFile(resolve(directory, "placements.json")),
);

// A native room groups its separate fluorescent tubes into one primitive. Split
// connected triangles at welded positions so each point sits at one actual tube.
function tubeCenters(primitive) {
  const positions = primitive.getAttribute("POSITION");
  const indices = primitive.getIndices()?.getArray() ?? Array.from({ length: positions.getCount() }, (_, i) => i);
  const neighbours = new Map(), points = new Map();
  const keys = Array.from({ length: positions.getCount() }, (_, i) => {
    const p = positions.getElement(i, []);
    const key = p.map((v) => Math.round(v * 1e5)).join(",");
    points.set(key, p);
    if (!neighbours.has(key)) neighbours.set(key, new Set());
    return key;
  });
  for (let i = 0; i < indices.length; i += 3) {
    const triangle = [keys[indices[i]], keys[indices[i + 1]], keys[indices[i + 2]]];
    for (const a of triangle) for (const b of triangle) neighbours.get(a).add(b);
  }
  const centers = [], seen = new Set();
  for (const start of points.keys()) {
    if (seen.has(start)) continue;
    const stack = [start], min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
    while (stack.length) {
      const key = stack.pop();
      if (seen.has(key)) continue;
      seen.add(key);
      const p = points.get(key);
      for (let k = 0; k < 3; k++) {
        min[k] = Math.min(min[k], p[k]);
        max[k] = Math.max(max[k], p[k]);
      }
      stack.push(...neighbours.get(key));
    }
    centers.push(min.map((v, k) => (v + max[k]) / 2));
  }
  return centers;
}

for (const name of ["arena", ...new Set(placements.map((p) => p.prototype))]) {
  if (!name.startsWith("tree-")) {
    // Atlas baking is also a glTF-Transform consumer. Detect the original native
    // building first so a pre-existing baked file cannot conceal a dropped chain.
    const source = name.startsWith("building-")
      ? await readDeclaredLod(resolve(directory, name + ".glb")) ?? await readDeclaredLod(resolve(directory, name + "-pbr.glb"))
      : await readDeclaredLod(resolve(directory, name + ".glb"));
    if (source) {
      const positions = name === "arena" ? [{ position: [0, 0, 0], heading: 0 }] : placements.filter(p => p.prototype === name);
      authoredLods.push({ source, name, positions });
      continue;
    }
  }
  const doc = name.startsWith("tree-") ? await bakeStreetTree(resolve(directory,name + ".lsp"), name, { Document, sharp }) : await io.read(
    resolve(
      directory,
      name + (name.startsWith("building-") ? "-pbr.glb" : ".glb"),
    ),
  );
  doc.setLogger(new Logger(Logger.Verbosity.WARN));
  if (name.startsWith("tree-")) {
    const source = declaredLodBytes(finishStreetTreeLods(await io.writeBinary(doc)));
    if (source) {
      authoredLods.push({ source, name, positions: placements.filter(p => p.prototype === name) });
      continue;
    }
  }
  if (name.startsWith("grass-field-")) {
    for (const material of doc.getRoot().listMaterials()) {
      const mode = material.getAlphaMode();
      material.setName({ BLEND: "Soft olive grass", MASK: "Tall grass tips", OPAQUE: "Rolling olive turf" }[mode]);
      // The stamps are already prefiltered. Avoid sampling neighbouring atlas
      // cells through a coarse mip on fixed cards seen nearly edge-on.
      material.getBaseColorTextureInfo()?.setMinFilter(TextureInfo.MinFilter.LINEAR);
      material.setExtras({ ...material.getExtras(), grassField: true, ...(mode === "BLEND" ? { castsShadow: false } : {}) });
    }
  }
  if (name.startsWith("weed-")) {
    for (const material of doc.getRoot().listMaterials()) {
      const wash = material.getAlphaMode() === "BLEND";
      material.setName(wash ? "Weed underpainting" : "Weed blades");
      // The soft pigment lies below the blades. Only the blade geometry casts
      // a shadow, so a translucent support card cannot darken a whole tuft.
      if (wash) material.setExtras({ ...material.getExtras(), castsShadow: false });
    }
  }
  const proceduralWall = name.startsWith("procedural-wall-");
  if (name.startsWith("ceiling-twin-strip-")) {
    for (const material of doc.getRoot().listMaterials()) {
      material.setExtras({ ...material.getExtras(), worldSpecTwinStrip: true });
    }
  }
  if (name.startsWith("pipe-run-")) await finishServicePipes(doc, metalBake);
  if (name.startsWith("duct-run-")) await finishServiceDucts(doc, metalBake);
  if (name.startsWith("wall-extraction-grille-")) await finishGrilleBlades(doc, sharp);
  if (name === "arena" || proceduralWall) {
    await finishArena(doc, directory, { sharp, proceduralWall });
    if (!proceduralWall) {
      markConcrete(doc);
      await finishDownstands(doc, sharp);
    }
  }
  if (name.startsWith("building-")) {
    const source = await io.read(resolve(directory, name + ".glb"));
    markConcrete(doc, source);
    const instances = placements.filter((p) => p.prototype === name);
    for (const node of source.getRoot().listNodes()) {
      if (!node.getName().endsWith("/room/room/tube-light")) continue;
      // Native vertices are in prototype coordinates under the Z-up scene root.
      for (const primitive of node.getMesh().listPrimitives()) {
        for (const [u, v, h] of tubeCenters(primitive)) {
          for (const placement of instances) {
            const [x, y, z] = placement.position;
            const a = placement.heading * Math.PI / 180;
            shopLightPositions.push([
              x + u * Math.cos(a) - v * Math.sin(a), z + h,
              -(y + u * Math.sin(a) + v * Math.cos(a)),
            ]);
          }
        }
      }
    }
    const preserved = doc.getRoot().listScenes()[0].getExtras().nativeMeshes;
    if (!Array.isArray(preserved)) throw Error("Missing native mesh manifest for " + name);
    const sourceNodes = new Map(source.getRoot().listNodes().map((n) => [n.getName(), n]));
    const nodes = preserved.map((id) => {
      const node = sourceNodes.get(id);
      if (!node?.getMesh()) throw Error("Native/viewer mesh mismatch: " + id);
      return node;
    });
    // Keep the glyph atlas and its original UVs. Mipmapping individual tiny
    // letters a second time in a chart atlas erodes the strokes at street distance.
    const sheet = doc
      .createTexture("Kowloon type")
      .setMimeType("image/png")
      .setImage(
        await readFile(
          fileURLToPath(
            new URL(
              "../../assets/materials/lettering.png",
              import.meta.url,
            ),
          ),
        ),
      );
    const ink = doc
      .createMaterial("Enamel lettering")
      .setBaseColorTexture(sheet)
      .setDoubleSided(true)
      .setAlphaMode("MASK")
      .setAlphaCutoff(0.72)
      .setMetallicFactor(0)
      .setRoughnessFactor(0.8);
    const meshes = nodes.map((n) => n.getMesh());
    const copies = copyToDocument(doc, source, meshes);
    const root = doc.getRoot().listScenes()[0].listChildren()[0];
    for (const node of nodes) {
      const copy = copies.get(node.getMesh());
      const glyph = node.getName().endsWith("/lettering");
      for (const primitive of copy.listPrimitives()) {
        if (!glyph) {
          const material = primitive.getMaterial();
          if (material?.getName() !== "builtin:display-glass" && !material?.getBaseColorTexture()) throw Error("Missing catalog atlas: " + node.getName());
          if (material.getName() === "builtin:display-glass") material.setExtras({ ...material.getExtras(), castsShadow: false });
          continue;
        }
        primitive.setMaterial(ink);
        const pos = primitive.getAttribute("POSITION"),
          normal = primitive.getAttribute("NORMAL");
        for (let i = 0; i < pos.getCount(); i++) {
          const xyz = pos.getElement(i, []),
            n = normal.getElement(i, []);
          pos.setElement(
            i,
            xyz.map((v, j) => v + 0.006 * n[j]),
          );
        }
      }
      root.addChild(doc.createNode(node.getName()).setMesh(copy).setMatrix(node.getMatrix()));
    }
  }
  const passageTextures = name === "arena" ? await finishPassageWall(doc, "brush-78", TextureInfo) : [];
  // Native GLB includes a preview sun. Klali owns game lighting and shadow passes.
  for (const node of doc.getRoot().listNodes()) {
    if (node.getExtension("KHR_lights_punctual")) node.dispose();
    else node.setExtras({});
  }
  // Bake the native Z-up root into geometry before placing props in glTF coordinates.

  for (const mesh of doc.getRoot().listMeshes())
    for (const prim of mesh.listPrimitives()) {
      if (!prim.getMaterial()?.getBaseColorTexture() && !prim.getAttribute("TEXCOORD_1"))
        prim.setAttribute("TEXCOORD_0", null);
      prim.setAttribute("_SUPERSPEC_HDR_COLOR", null);
    }
  await doc.transform(flatten(), join(), weld());
  if (name === "arena")
    for (const texture of doc.getRoot().listTextures()) {
      if (passageTextures.includes(texture) || texture.getName().startsWith("Washroom concrete ") || texture.getName() === "Laid stone paving") continue;
      const image = await sharp(texture.getImage())
        .resize({
          width: 768,
          height: 768,
          fit: "inside",
          withoutEnlargement: true,
        })
        .jpeg({ quality: 88 })
        .toBuffer();
      texture.setImage(image).setMimeType("image/jpeg");
    }
  const meshes = doc.getRoot().listMeshes();
  for (const node of doc.getRoot().listNodes()) {
    if (node.getMesh()) transformMesh(node.getMesh(), node.getWorldMatrix());
  }
  const copies = copyToDocument(result, doc, meshes);
  const positions =
    name === "arena"
      ? [{ position: [0, 0, 0], heading: 0 }]
      : placements.filter((p) => p.prototype === name);
  for (const mesh of meshes) {
    const shared = copies.get(mesh);
    for (const {
      position: [x, y, z],
      heading, scale = 1,
    } of positions) {
      const a = (heading * Math.PI) / 360;
      scene.addChild(
        result
          .createNode(name)
          .setMesh(shared)
          .setTranslation([x, z, -y])
          .setScale([scale, scale, scale])
          .setRotation([0, Math.sin(a), 0, Math.cos(a)]),
      );
    }
  }
}
for (const placement of placements) {
  if (placement.planterLight) shopLightPositions.push(planterCeilingLight(placement));
  if (placement.lowerArcadeBatten) shopLightPositions.push(arcadeBattenLight(placement));
  if (placement.prototype.startsWith("passage-sign-lamp-"))
    shopLightPositions.push(passageLampLight(placement));
}
scene.setExtras({
  ...scene.getExtras(),
  shopLightPositions: [...shopLightPositions.map(light => Array.isArray(light) ? shopTubeLight(light) : light), ...shopAccentLights],
});
await result.transform(
  dedup(),
  quantize(),
  instance({ min: 2 }),
  groupStreetInstances,
  prune(),
  unpartition(),
);
const stockMaterials = new Set(["builtin:grocer-props", "builtin:shop-register", "builtin:snack-packet", "builtin:market-trays",
  "builtin:restaurant-food", "builtin:kitchen-metal", "builtin:restaurant-surfaces", "builtin:display-glass",
  "builtin:market-hero-props", "builtin:kitchen-worktop"]);
for (const mesh of result.getRoot().listMeshes()) {
  for (const primitive of mesh.listPrimitives()) {
    const material = primitive.getMaterial();
    if (stockMaterials.has(material?.getName()) || material?.getExtras().worldSpecOutline === false) {
      primitive.setExtras({ ...primitive.getExtras(), worldSpecOutline: false });
    }
  }
  if (mesh.listPrimitives().every(p => p.getExtras().worldSpecOutline === false)) {
    mesh.setExtras({ ...mesh.getExtras(), worldSpecOutline: false });
  }
}
// The stock atlas already supplies the crate pigment. Ease the second layer of
// vertex tint in the stoa's shade, with a lighter finish on sky-facing rails.
const stockAtlas = JSON.parse(await readFile(new URL(
  "../../crates/engine/src/worldspec/themes/kowloon/shop/assets/stock-appearances.json",
  import.meta.url,
)));
const crate = stockAtlas["bottle-crate"];
const crateRegions = [crate.source, ...crate.variants.map(variant => variant.uv)];
const finishedStock = new Set();
for (const mesh of result.getRoot().listMeshes()) {
  for (const primitive of mesh.listPrimitives()) {
    if (primitive.getMaterial()?.getName() !== "builtin:grocer-props") continue;
    const color = primitive.getAttribute("COLOR_0");
    if (finishedStock.has(color)) continue;
    finishedStock.add(color);
    const uv = primitive.getAttribute("TEXCOORD_0"), normal = primitive.getAttribute("NORMAL");
    for (let i = 0; i < color.getCount(); i++) {
      const [u, v] = uv.getElement(i, []);
      if (!crateRegions.some(([x0, y0, x1, y1]) => u >= x0 && u <= x1 && 1-v >= y0 && 1-v <= y1)) continue;
      const up = Math.max(0, normal.getElement(i, [])[1]);
      color.setElement(i, color.getElement(i, []).map(c => Math.min(1, 0.58 + 0.42 * c + 0.28 * up * up)));
    }
  }
}
// Browser canvas uses a fast PNG encoder. Deflate the generated PBR pages for delivery;
// native catalog/glyph images are copied byte-for-byte even if shared across materials.
function materialImages(material) {
  return [
    material.getBaseColorTexture(), material.getNormalTexture(), material.getMetallicRoughnessTexture(),
    material.getEmissiveTexture(), material.getOcclusionTexture(),
  ].filter(Boolean);
}
const atlasMaterials = result.getRoot().listMaterials();
const nativeImages = new Set(atlasMaterials.filter((m) => !m.getExtras().materialSpecNormalFrame).flatMap(materialImages));
const pages = new Map();
for (const texture of new Set(atlasMaterials.filter((m) => m.getExtras().materialSpecNormalFrame).flatMap(materialImages))) {
  if (nativeImages.has(texture) || texture.getMimeType() !== "image/png") continue;
  const channels = atlasChannels(texture);
  const digest = createHash("sha256").update(channels).update(texture.getImage()).digest("hex");
  if (!pages.has(digest)) pages.set(digest, { textures: [], channels });
  pages.get(digest).textures.push(texture);
}
const cache = resolve(directory, "atlas-png-cache");
await mkdir(cache, { recursive: true });
const codec = createHash("sha256").update(JSON.stringify([sharp.versions, compressionLevel, false, "used-channels-v1"])).digest("hex");
const queue = [...pages.entries()];
let saved = 0, reused = 0;
async function packedPage(digest, original, channels) {
  const path = resolve(cache, codec + "-" + digest + ".bin");
  const checksum = data => createHash("sha256").update(codec).update(digest).update(data).digest();
  let file;
  try {
    file = await open(path, "r");
    const { size } = await file.stat();
    if (size >= 32 && size <= original.length + 32) {
      const entry = await file.readFile();
      const data = entry.subarray(32);
      if (entry.length === size && checksum(data).equals(entry.subarray(0, 32))) {
        reused++;
        return data.length ? data : original;
      }
    }
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  } finally { await file?.close(); }

  const packed = await packAtlasPng(original, channels, compressionLevel);
  // An empty payload records that recompressing the original did not save bytes.
  const data = packed === original ? Buffer.alloc(0) : packed;
  const staging = path + "." + randomUUID() + ".tmp";
  try {
    await writeFile(staging, Buffer.concat([checksum(data), data]), { flag: "wx" });
    await rename(staging, path);
  } finally { await rm(staging, { force: true }); }
  return packed;
}
async function deflatePages() {
  while (queue.length) {
    const [digest, { textures, channels }] = queue.pop();
    const original = textures[0].getImage();
    const packed = await packedPage(digest, original, channels);
    if (packed === original) continue;
    saved += original.length - packed.length;
    for (const texture of textures) texture.setImage(packed);
  }
}
await Promise.all(Array.from({ length: Math.min(4, queue.length) }, deflatePages));
console.log("Verified lossless PNG packing for", pages.size, "unique atlas images;", reused, "reused;", saved, "bytes saved");
// Keep each network resource independently cacheable and within ordinary Git object limits.
// GLB permits external buffers. Resource URLs are relative to the entry document.
const glb = Buffer.from(await io.writeBinary(result));
const jsonLength = glb.readUInt32LE(12);
const json = JSON.parse(glb.toString("utf8", 20, 20 + jsonLength));
const payload = glb.subarray(28 + jsonLength);
const folder = basename(destination, ".glb") + "-data";
const resourceDir = resolve(dirname(destination), folder);
await mkdir(resourceDir, { recursive: true });
let start = 0,
  end = 0;
json.buffers = [];
async function flush() {
  const digest = createHash("sha256").update(payload.subarray(start,end)).digest("hex").slice(0,24);
  const file = "chunk-" + digest + ".bin";
  await writeImmutable(resolve(resourceDir, file), payload.subarray(start, end));
  json.buffers.push({ byteLength: end - start, uri: folder + "/" + file });
  start = end;
}
for (const view of [...json.bufferViews].sort(
  (a, b) => a.byteOffset - b.byteOffset,
)) {
  const offset = view.byteOffset ?? 0;
  if (end > start && offset + view.byteLength - start > 16 * 1024 * 1024) {
    end = offset;
    await flush();
  }
  view.buffer = json.buffers.length;
  view.byteOffset = offset - start;
  end = offset + view.byteLength;
}
await flush();
await appendConcreteDetail(json, folder, sharp, (name, bytes) => writeImmutable(resolve(resourceDir, name), bytes));
await attachPaintedLawns(json, directory, resourceDir, folder, placements);
await attachMaterialLibrary(json,destination,directory);
for (const { source, name, positions } of authoredLods)
  await appendDeclaredLod(json, source, { name, positions, resourceDir, folder });
const text = Buffer.from(JSON.stringify(json));
const length = (text.length + 3) & ~3;
const entry = Buffer.alloc(20 + length, 0x20);
entry.write("glTF");
entry.writeUInt32LE(2, 4);
entry.writeUInt32LE(entry.length, 8);
entry.writeUInt32LE(length, 12);
entry.write("JSON", 16);
text.copy(entry, 20);
await publishLitCity(destination, entry, { lodFarProfile, blockProxyCell });
