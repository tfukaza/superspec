import { writeImmutable } from "./immutable-file.mjs";
// Bake from the packed visual geometry so instance placements, atlas UVs and vertex tints
// match gameplay. The large triangles supply the diffuse reflectors at probe resolution.
import { readFile, rename, rm, writeFile } from "node:fs/promises";
import { createHash, randomUUID } from "node:crypto";
import { basename, dirname, resolve } from "node:path";
import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
import { Matrix4, Quaternion, Vector3 } from "three";
import sharp from "sharp";
import { attachCityLod, entryBytes, GeometryReader } from "./city-lod.mjs";
import { attachCityTextureLods } from "./city-texture-lod.mjs";
import { citySky } from "../../crates/engine/src/worldspec/themes/kowloon/materials/arcade-lighting.mjs";

const areaSamples = [
  [2 / 3, 1 / 6, 1 / 6],
  [1 / 6, 2 / 3, 1 / 6],
  [1 / 6, 1 / 6, 2 / 3],
  [1 / 3, 1 / 3, 1 / 3],
];
const linear = (v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
function transform(t = [0, 0, 0], r = [0, 0, 0, 1], s = [1, 1, 1]) {
  return new Matrix4().compose(new Vector3(...t), new Quaternion(...r), new Vector3(...s));
}

async function placements(json, reader, node, base) {
  const attrs = node.extensions?.EXT_mesh_gpu_instancing?.attributes;
  if (!attrs) return [base];
  const rows = {};
  for (const [id, accessor] of Object.entries(attrs)) rows[id] = await reader.read(accessor);
  const count = json.accessors[Object.values(attrs)[0]].count;
  return Array.from({ length: count }, (_, i) =>
    base
      .clone()
      .multiply(
        transform(
          rows.TRANSLATION?.subarray(3 * i, 3 * i + 3),
          rows.ROTATION?.subarray(4 * i, 4 * i + 4),
          rows.SCALE?.subarray(3 * i, 3 * i + 3),
        ),
      ),
  );
}

export async function reflectors(json, directory, minimumReflectorArea) {
  const reader = new GeometryReader(json, directory),
    images = new Map(),
    surfaces = [];
  async function imageFor(texture) {
    if (!texture) return;
    const id = json.textures[texture.index].source;
    if (!images.has(id)) {
      const image = json.images[id],
        view = json.bufferViews[image.bufferView];
      const uri = json.buffers[view.buffer].uri;
      const buffer = await readFile(resolve(directory, uri));
      const offset = view.byteOffset ?? 0;
      images.set(
        id,
        await sharp(buffer.subarray(offset, offset + view.byteLength))
          .resize(128, 128, { fit: "fill" })
          .removeAlpha()
          .toColourspace("srgb")
          .raw()
          .toBuffer(),
      );
    }
    return images.get(id);
  }
  const byMesh = new Map();
  async function visit(id, parent) {
    const node = json.nodes[id];
    const local = node.matrix ? new Matrix4().fromArray(node.matrix) : transform(node.translation, node.rotation, node.scale);
    const world = parent.clone().multiply(local);
    if (node.mesh !== undefined) {
      if (!byMesh.has(node.mesh)) byMesh.set(node.mesh, []);
      byMesh.get(node.mesh).push(...await placements(json, reader, node, world));
    }
    for (const child of node.children ?? []) await visit(child, world);
  }
  for (const root of json.scenes[json.scene ?? 0].nodes) await visit(root, new Matrix4());
  for (const [mesh, transforms] of byMesh) {
    const largest = Math.max(...transforms.map((m) => m.getMaxScaleOnAxis()));
    for (const prim of json.meshes[mesh].primitives) {
      if ((prim.mode ?? 4) !== 4) continue;
      const mat = json.materials[prim.material];
      if ((mat.alphaMode ?? "OPAQUE") !== "OPAQUE") continue;
      // The grime bands subdivide these structural walls into small triangles.
      const minimumArea = mat.extras?.worldSpecProceduralWall ? 1e-6 : minimumReflectorArea;
      const pos = await reader.read(prim.attributes.POSITION);
      const indices = prim.indices === undefined ? null : await reader.read(prim.indices, Uint32Array);
      const pbr = mat.pbrMetallicRoughness ?? {},
        factor = pbr.baseColorFactor ?? [1, 1, 1, 1];
      const tex = pbr.baseColorTexture;
      const uv = tex && (await reader.read(prim.attributes["TEXCOORD_" + (tex.texCoord ?? 0)]));
      const col = prim.attributes.COLOR_0 === undefined ? null : await reader.read(prim.attributes.COLOR_0);
      const components = col && col.length / (pos.length / 3);
      const a = new Vector3(),
        b = new Vector3(),
        c = new Vector3(),
        ab = new Vector3(),
        ac = new Vector3();
      let pixels;
      for (let i = 0; i < (indices?.length ?? pos.length / 3); i += 3) {
        const ia = indices?.[i] ?? i,
          ib = indices?.[i + 1] ?? i + 1,
          ic = indices?.[i + 2] ?? i + 2;
        a.fromArray(pos, 3 * ia);
        b.fromArray(pos, 3 * ib);
        c.fromArray(pos, 3 * ic);
        const area = ab.subVectors(b, a).cross(ac.subVectors(c, a)).length() * 0.5;
        if (area * largest * largest < minimumArea) continue;
        const corners = [ia, ib, ic],
          color = [0, 0, 0];
        if (uv) pixels ??= await imageFor(tex);
        // The centroids of four equal-area subtriangles sample the textured reflector.
        for (const weights of areaSamples) {
          const attribute = (data, width, k) =>
            corners.reduce((sum, j, n) => sum + weights[n] * data[width * j + k], 0);
          const pixel = (k) => {
            const value = attribute(uv, 2, k);
            return Math.floor((value - Math.floor(value)) * 128) % 128;
          };
          const offset = uv ? 3 * (128 * pixel(1) + pixel(0)) : 0;
          for (let k = 0; k < 3; k++) {
            const pigment = pixels ? linear(pixels[offset + k] / 255) : 1;
            color[k] += 0.25 * factor[k] * pigment * (col ? attribute(col, components, k) : 1);
          }
        }
        for (const matrix of transforms) {
          const vertices = [a, b, c].map((point) => {
            const { x, y, z } = point.clone().applyMatrix4(matrix);
            return [x, -z, y];
          });
          surfaces.push({ vertices, color });
        }
      }
    }
  }
  return surfaces;
}

async function runBaker(request) {
  const exe = process.env.SUPERSPEC_GI ?? "superspec-gi";
  const child = spawn(exe, [], { windowsHide: true, stdio: ["pipe", "pipe", "inherit"] });
  const chunks = [];
  child.stdout.on("data", (part) => chunks.push(part));
  const done = new Promise((ok, fail) => {
    child.once("error", fail);
    child.stdin.on("error", fail);
    child.once("close", (code) => (code === 0 ? ok() : fail(Error("WorldSpec GI bake exited " + code))));
  });
  child.stdin.end(JSON.stringify(request));
  await done;
  return Buffer.concat(chunks);
}

export async function cityLighting() {
  const source = new URL("../../crates/engine/src/worldspec/themes/kowloon/city-lighting.json", import.meta.url);
  return JSON.parse(await readFile(source, "utf8"));
}

function spotEmitters(lights = []) {
  const toDomain = ([x, y, z]) => [x, -z, y];
  return lights.filter(light => light.kind === "arcade-batten" || light.kind === "planter-ceiling").map(light => {
    const direction = light.target.map((v, i) => v - light.position[i]);
    return {
      position: toDomain(light.position),
      direction: toDomain(direction),
      intensity: [16, 8, 0].map(shift => linear(((light.color >>> shift) & 255) / 255) * light.candela),
      range: light.range,
      outer_cosine: Math.cos(light.angle),
      inner_cosine: Math.cos(light.angle * (1 - light.penumbra)),
    };
  });
}

export async function attachBounce(destination, entry, lighting, {
  origin = [-48, -48, -4], dimensions = [65, 65, 17], spacing = 1.5, minimumReflectorArea = 0.45,
  strokeMeters = 0.36, spotBounceScale = 1, bake = runBaker,
} = {}) {
  const json = JSON.parse(entry.toString("utf8", 20, 20 + entry.readUInt32LE(12)));
  const extras = json.scenes[json.scene ?? 0].extras;
  if (lighting) {
    extras.worldSpecLighting = lighting;
    const environment = extras.worldSpecEnvironment ?? extras.klali;
    if (environment) environment.sky = citySky;
  }
  const sun = extras.worldSpecLighting.lights.find((l) => l.role === "sun" && l.kind === "directional");
  const surfaces = await reflectors(json, dirname(destination), minimumReflectorArea);
  const bytes = await bake({
    origin,
    dimensions,
    spacing,
    surfaces,
    spots: spotEmitters(extras.shopLightPositions).map(spot => ({
      ...spot, intensity: spot.intensity.map(channel => channel * spotBounceScale),
    })),
    sun_direction: sun.direction,
    sun_radiance: sun.color.map((c) => c * sun.intensity),
  });
  if (bytes.length !== dimensions.reduce((a, b) => a * b) * 18)
    throw Error("Incomplete WorldSpec irradiance volume");
  const digest = createHash("sha256").update(bytes).digest("hex").slice(0, 24);
  const uri = basename(destination, ".glb") + "-data/bounce-" + digest + ".bin";
  await writeImmutable(resolve(dirname(destination), uri), bytes);
  const bufferView = extras.worldSpecBounce?.bufferView ?? json.bufferViews.length;
  const buffer = json.bufferViews[bufferView]?.buffer ?? json.buffers.length;
  json.buffers[buffer] = { uri, byteLength: bytes.length };
  json.bufferViews[bufferView] = { buffer, byteLength: bytes.length };
  extras.worldSpecBounce = { version: 1, origin, dimensions, spacing, bufferView, strokeMeters };
  console.log("WorldSpec bounce:", surfaces.length, "reflectors,", bytes.length, "bytes,", uri);
  return entryBytes(json);
}

/** Kowloon's reviewed far hardware chain is part of normal publication. The
 * neutral sidecar helper still defaults to standard for explicit comparisons. */
export function kowloonFarProfile(value, environment = process.env.SUPERSPEC_CITY_LOD_FAR) {
  const profile = value ?? environment ?? "aggressive";
  if (!["standard", "aggressive"].includes(profile)) throw RangeError("City LOD far profile must be standard or aggressive.");
  return profile;
}

// Publish the entry only after its content-addressed buffers have been written.
export async function publishLitCity(destination, source, {checkCurrent = false, lighting, lodFarProfile, blockProxyCell,
  textureLods = process.env.SUPERSPEC_CITY_TEXTURE_LODS === "1"} = {}) {
  const farProfile = kowloonFarProfile(lodFarProfile);
  let entry = await attachCityLod(destination, await attachBounce(destination, source, lighting), { farProfile });
  if (textureLods) ({ entry } = await attachCityTextureLods(destination, entry));
  if (blockProxyCell !== undefined) {
    const { attachBlockProxy } = await import("./block-proxy.mjs");
    const proxy = await attachBlockProxy(destination, entry, { cell: blockProxyCell });
    entry = proxy.entry;
    console.log("WorldSpec block proxy:", proxy.summary);
  }
  const staging = destination + "." + randomUUID() + ".tmp";
  try {
    await writeFile(staging, entry, {flag: "wx"});
    if (checkCurrent && !(await readFile(destination)).equals(source))
      throw Error("The map changed during the bake. Re-run against the new export.");
    await rename(staging, destination);
  } finally {
    await rm(staging, {force: true});
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const options = process.argv.slice(3);
  if (!process.argv[2] || options.some(option => option !== "--theme-lighting" && !/^--far-profile=(standard|aggressive)$/.test(option)))
    throw Error("Usage: node bounced-light.mjs MAP.glb [--theme-lighting] [--far-profile=standard|aggressive]");
  const destination = resolve(process.argv[2]);
  await publishLitCity(destination, await readFile(destination), {
    checkCurrent: true,
    lighting: process.argv.includes("--theme-lighting") ? await cityLighting() : undefined,
    lodFarProfile: options.find(option => option.startsWith("--far-profile="))?.split("=")[1],
  });
}
