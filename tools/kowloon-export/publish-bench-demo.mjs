import { writeImmutable } from "./immutable-file.mjs";
import { createHash, randomUUID } from "node:crypto";
import { copyFile, mkdir, rename, rm, writeFile } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { citySky, planterCeilingLight } from "../../crates/engine/src/worldspec/themes/kowloon/materials/arcade-lighting.mjs";
import { attachBounce, cityLighting } from "./bounced-light.mjs";
import { captureReflectionProbes } from "./bake-reflection-probes.mjs";
import { entryBytes } from "./city-lod.mjs";
import { Document, NodeIO, TextureInfo } from "@gltf-transform/core";
import { ALL_EXTENSIONS } from "@gltf-transform/extensions";
import { copyToDocument, unpartition, unweld } from "@gltf-transform/functions";
import { connectorFinishes, finishConnector } from "../../crates/engine/src/worldspec/themes/sci_fi_industrial/authoring/connector-finish.mjs";
import sharp from "sharp";
import { runNativeBinary } from "../headless/native-stdio.mjs";
import { readMaterialEdges } from "./material-edges.mjs";
import { finishServiceDucts } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/service-duct-finish.mjs";
import { finishServicePipes } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/service-pipe-finish.mjs";
import { appendConcreteDetail } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/concrete-detail.mjs";
import { concreteMaps, concreteResolution } from "../../crates/engine/src/worldspec/themes/sci_fi_industrial/authoring/concrete.ts";

const args = new Set(process.argv.slice(2));
if ([...args].some(arg => !["--diffuse-only", "--modern-industrial"].includes(arg)))
  throw Error("Usage: node tools/kowloon-export/publish-bench-demo.mjs [--diffuse-only] [--modern-industrial]");
const diffuseOnly = args.has("--diffuse-only");
const modern = args.has("--modern-industrial");
const bench = modern ? "modern-industrial-bench" : "kowloon-bench";
let source = fileURLToPath(new URL(`../../editor/demos/${bench}.aml`, import.meta.url));
const outputName = bench + (diffuseOnly ? "-diffuse.glb" : ".glb");
const destination = fileURLToPath(new URL("../../generated/demos/" + outputName, import.meta.url));
const directory = dirname(destination);
const dataDirectory = basename(destination, ".glb") + "-data";
const connectorAssets = new URL("../../crates/engine/src/worldspec/themes/sci_fi_industrial/items/", import.meta.url);
const connectorFiles = ["power-plug-body.glb", "power-socket.glb", "power-cable-bundle.glb"];
const signFiles = ["critical-power-01.glb", "critical-power-02.glb", "critical-power-03.glb"];
const modernMeshFiles = [...connectorFiles, ...signFiles];

function mapBenchTiles(doc) {
  for (const node of doc.getRoot().listNodes()) {
    for (const primitive of node.getMesh()?.listPrimitives() ?? []) {
      if (primitive.getMaterial()?.getName() !== "builtin:restaurant-tiles") continue;
      const positions = primitive.getAttribute("POSITION");
      const uv = primitive.getAttribute("TEXCOORD_0").clone();
      const floor = node.getName().startsWith("floor/");
      // Match the kitchen atlas's physical spans: 3.6 m across and ten 18 cm courses.
      for (let i = 0; i < positions.getCount(); i++) {
        const [x, y, z] = positions.getElement(i, []);
        uv.setElement(i, floor ? [x / 3.6, 1 - y / 1.8] : [y / 3.6, 1 - z / 1.8]);
      }
      primitive.setAttribute("TEXCOORD_0", uv);
    }
  }
}

async function restoreModernMeshMaterials(doc, io) {
  const objects = doc.getRoot().getExtras().superspec.architecture.objects;
  const placements = new Map(objects.filter(object => object.kind === "visual-mesh" && modernMeshFiles.includes(object.attributes.src))
    .map(object => [object.id, object]));
  const receivers = doc.getRoot().listNodes().filter(node => node.getMesh() && placements.has(node.getExtras().objectId));
  if (!receivers.length) return;
  const assets = new Map();
  const finishes = new Document();
  const maps = await connectorFinishes(finishes, sharp);

  for (const receiver of receivers) {
    const { matrix, attributes: { src } } = placements.get(receiver.getExtras().objectId);
    if (!assets.has(src)) {
      const asset = await io.read(fileURLToPath(new URL(src, connectorAssets)));
      if (connectorFiles.includes(src)) {
        await asset.transform(unweld());
        const textures = copyToDocument(asset, finishes, finishes.getRoot().listTextures());
        const channels = Object.fromEntries(Object.entries(maps).map(([name, channels]) =>
          [name, Object.fromEntries(Object.entries(channels).map(([channel, texture]) => [channel, textures.get(texture)]))]));
        finishConnector(asset, channels, { brightenPlug: src !== "power-socket.glb" });
      }
      if (src === "power-socket.glb") {
        for (const node of asset.getRoot().listNodes()) {
          if (node.getName().endsWith("| cam dog")) node.setExtras({ ...node.getExtras(), connectorLatch: "cam" });
          if (node.getName().endsWith("| safety plunger")) node.setExtras({ ...node.getExtras(), connectorLatch: "pin" });
        }
      }
      assets.set(src, asset);
    }
    const asset = assets.get(src);
    const roots = asset.getRoot().getDefaultScene().listChildren();
    const fallback = receiver.getMesh();
    const accessors = fallback.listPrimitives().flatMap(primitive => [...primitive.listAttributes(), primitive.getIndices()]);
    receiver.setMesh(null).setMatrix(matrix.flatMap((column, i) => [...column, i === 3 ? 1 : 0]));
    fallback.dispose();
    for (const accessor of new Set(accessors.filter(Boolean))) {
      if (accessor.listParents().every(parent => parent.propertyType === "Root")) accessor.dispose();
    }
    // Reverse the native export root's Y-up conversion before applying the resolved AML placement.
    const basis = doc.createNode("Imported asset axes").setRotation([Math.SQRT1_2, 0, 0, Math.SQRT1_2]);
    receiver.addChild(basis);
    const copies = copyToDocument(doc, asset, roots);
    for (const root of roots) basis.addChild(copies.get(root));
  }
  await doc.transform(unpartition());
}

await mkdir(resolve(directory, dataDirectory), { recursive: true });
if (modern) {
  const companions = resolve(directory, bench + "-source");
  await mkdir(companions, { recursive: true });
  const original = source;
  source = resolve(companions, "scene.aml");
  for (const [name, pixels] of concreteMaps()) {
    await sharp(pixels, { raw: { width: concreteResolution, height: concreteResolution, channels: 4 } })
      .removeAlpha().png().toFile(resolve(companions, name));
  }
  for (const name of modernMeshFiles) await copyFile(new URL(name, connectorAssets), resolve(companions, name));
  await copyFile(original, source);
}
const native = await runNativeBinary(process.env.RUST_CLI ?? "superspec", ["glb", source, "--textures", "png"]);
const io = new NodeIO().registerExtensions(ALL_EXTENSIONS);
const doc = await io.readBinary(native);
if (modern) {
  await restoreModernMeshMaterials(doc, io);
} else {
  mapBenchTiles(doc);
  await finishServiceDucts(doc, { Document, NodeIO, TextureInfo, copyToDocument, readMaterialEdges, sharp });
  await finishServicePipes(doc, { TextureInfo, sharp }, { bakeWeathering: false });
}
const glb = Buffer.from(await io.writeBinary(doc));
const binaryHeader = 20 + glb.readUInt32LE(12);
const json = JSON.parse(glb.toString("utf8", 20, binaryHeader));
const buffer = json.buffers[0];
if (json.buffers.length !== 1 || buffer.uri || glb.toString("ascii", binaryHeader + 4, binaryHeader + 8) !== "BIN\0")
  throw Error("Expected one embedded SuperSpec GLB buffer.");
const payload = glb.subarray(binaryHeader + 8, binaryHeader + 8 + buffer.byteLength);
if (payload.length !== buffer.byteLength) throw Error("Incomplete SuperSpec GLB buffer.");
const hash = createHash("sha256").update(payload).digest("hex").slice(0, 24);
buffer.uri = dataDirectory + "/model-" + hash + ".bin";
await writeImmutable(resolve(directory, buffer.uri), payload);

const lighting = await cityLighting();
// The city preset's occlusion radius spans much of this room.
lighting.ambient.intensity = 0.68;
lighting.environment.intensity = 0.26;
Object.assign(lighting.ambientOcclusion, { radius: 0.42, intensity: 0.90 });
const scene = json.scenes[json.scene ?? 0];
// Native mesh bounds are in domain metres. Use the AML fixture's actual lens
// centre so moving the fixture also moves the light and its baked contribution.
const lens = json.nodes.find(node => modern
  ? node.extras?.objectId === "light-diffuser" && node.mesh !== undefined
  : node.extras?.objectId === "city/bench-light" && node.name.endsWith("/lens"));
if (!lens) throw Error("The bench light diffuser is missing from the exported AML.");
const { min, max } = json.accessors[json.meshes[lens.mesh].primitives[0].attributes.POSITION];
const x = (min[0] + max[0]) / 2, y = (min[1] + max[1]) / 2;
const fixtures = modern ? [-0.25, 0.25].map(fraction => {
  const emitterX = x + fraction * (max[0] - min[0]);
  return {
    kind: "planter-ceiling", fixture: "ceiling-twin-strip",
    position: [emitterX, min[2] - 0.015, -y], target: [emitterX, 0, -y],
    floorY: 0, ceilingY: max[2], cameraFloorCull: false,
    color: 0xe3efff, candela: 24, range: 2.82, angle: 1.02, penumbra: 0.85,
  };
}) : [{
  ...planterCeilingLight({
    position: [x, y, max[2] + 0.08],
    planterLight: { fixture: "ceiling-bulkhead", floorY: 0, undersideY: -0.19 },
  }),
  candela: 70, cameraFloorCull: false,
}];
if (modern) {
  Object.assign(lighting.environment, { intensity: 0.42 });
  Object.assign(lighting.ambient, { intensity: 1.12, sky: [0.93, 0.96, 1], ground: [0.55, 0.57, 0.60] });
  const sun = lighting.lights.find(light => light.role === "sun");
  Object.assign(sun, { color: [1, 0.94, 0.81], intensity: 2.30 });
  sun.shadow.tintStrength = 0;
}
Object.assign(scene.extras ??= {}, {
  worldSpec: true,
  worldSpecLighting: lighting,
  worldSpecEnvironment: { version: 1, sky: modern ? { zenith: [0.22, 0.34, 0.50], horizon: [1.50, 1.62, 1.78] } : citySky },
  worldSpecRender: { version: 1, effects: modern ? ["displayAntialiasing", "specularOutlineMask"] : ["displayAntialiasing"],
    outlines: modern, ...(modern ? { bloom: false } : {}) },
  shopLightPositions: fixtures,
  ...(modern ? { modernIndustrialCableDrag: 1 } : {}),
});
// The WorldSpec viewer prefers the portable record when the compiler supplied one.
if (json.extras?.superspec) json.extras.superspec.lighting = lighting;
for (const material of json.materials) {
  if (!modern && material.name === "builtin:concrete")
    (material.extras ??= {}).worldSpecConcrete = true;
  const pbr = material.pbrMetallicRoughness;
  const surface = material.extras?.worldSpecSurface;
  if (modern && (material.extras?.materialSpecEdge || material.extras?.materialSpec?.live?.some(op => op.id === "virtualSky"))) continue;
  if (surface?.reflection || material.extras?.worldSpecDuct) continue;
  if (material.name?.includes("restaurant-tiles") || (pbr?.metallicFactor ?? 0) >= 0.3 || pbr?.metallicRoughnessTexture) {
    (material.extras ??= {}).worldSpecSurface = { ...surface, reflection: "box" };
  }
}
if (!modern) await appendConcreteDetail(json, dataDirectory, sharp, (name, bytes) => writeImmutable(resolve(directory, dataDirectory, name), bytes));

const floorEnd = modern ? Math.max(...json.nodes.filter(node => node.extras?.objectId === "floor")
  .flatMap(node => json.meshes[node.mesh].primitives.map(primitive => json.accessors[primitive.attributes.POSITION].max[1]))) : 3;
const staging = resolve(directory, "bench-capture-" + randomUUID() + ".glb");
try {
  const bounced = await attachBounce(destination, entryBytes(json), undefined, {
    origin: [-0.55, -0.6, -0.35], dimensions: [42, modern ? Math.ceil((floorEnd + 0.9) * 10) + 1 : 40, 37],
    spacing: 0.10, minimumReflectorArea: 0.005,
    // Close receivers can dominate the baker's 64-ray estimate of fixture bounce.
    strokeMeters: 0, spotBounceScale: 0.30,
    bake: request => runNativeBinary(process.env.SUPERSPEC_GI ?? "superspec-gi", [], JSON.stringify(request)),
  });
  await writeFile(staging, bounced);
  if (!diffuseOnly) {
    const probe = { id: "bench-corner", center: [1.45, 1.35, -0.93], min: [-0.4, -0.2, -floorEnd - 0.1], max: [3.35, 2.85, 0.16], fade: 0.25 };
    const published = JSON.parse(bounced.toString("utf8", 20, 20 + bounced.readUInt32LE(12)));
    const panorama = { version: 1, width: 256, height: 128, probes: [] };
    published.scenes[published.scene ?? 0].extras.worldSpecReflectionProbes = await captureReflectionProbes(staging, [probe], panorama, 256);
    await writeFile(staging, entryBytes(published));
  }
  await rename(staging, destination);
  console.log("Published " + destination);
} finally {
  await rm(staging, { force: true });
}
