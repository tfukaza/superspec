import { writeImmutable } from "./immutable-file.mjs";
/** Preserve native authored LOD prototypes through the city packer's destructive
 * mesh transforms. This is the native static PNG/JPEG export contract, not a
 * general glTF importer: unsupported extensions or animated/skinned data fail. */
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { readCityEntry } from "./city-data-textures.mjs";
import { readPrototypeJson } from "./prototype-json.mjs";

const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const allowedExtensions = new Set(["MSFT_lod", "EXT_mesh_gpu_instancing", "KHR_materials_emissive_strength", "KHR_lights_punctual"]);
const tables = ["buffers", "bufferViews", "accessors", "images", "samplers", "textures", "materials", "meshes"];
const fail = text => { throw Error(`Authored LOD publication: ${text}`); };
const ref = (value, list, label) => {
  if (!Number.isInteger(value) || value < 0 || value >= (list?.length ?? 0)) fail(`invalid ${label} reference ${value}`);
  return value;
};
export const hasDeclaredLod = json => ["nodes", "materials"].some(key => json[key]?.some(record => record.extensions?.MSFT_lod));

function validate(json) {
  if (json.asset?.version !== "2.0") fail("expected a static glTF 2.0 export");
  if (json.scenes?.length !== 1 || (json.scene ?? 0) !== 0) fail("expected one default scene");
  if ((json.buffers?.length ?? 0) > 1 || json.buffers?.some(buffer => buffer.uri)) fail("native authored GLBs must contain one self-contained geometry buffer");
  for (const key of ["animations", "skins", "cameras"]) if (json[key]?.length) fail(`${key} are not supported`);
  for (const name of [...(json.extensionsUsed ?? []), ...(json.extensionsRequired ?? [])]) if (!allowedExtensions.has(name)) fail(`unsupported extension ${name}; export PNG/JPEG textures for city publication`);
  const walkExtensions = value => {
    if (!value || typeof value !== "object") return;
    for (const [key, child] of Object.entries(value)) {
      if (key === "extras") continue;
      if (key === "extensions") for (const name of Object.keys(child)) if (!allowedExtensions.has(name)) fail(`unsupported extension ${name}`);
      walkExtensions(child);
    }
  };
  walkExtensions(json);
  for (const [key, records] of Object.entries(json)) if (Array.isArray(records) && records.length > 1_000_000) fail(`oversized ${key} table`);
  const parents = new Map(), alternatives = new Set();
  for (const [index, node] of (json.nodes ?? []).entries()) {
    if (node.skin !== undefined || node.camera !== undefined || node.weights) fail("animated/skinned/camera nodes are not supported");
    if (node.mesh !== undefined) ref(node.mesh, json.meshes, "mesh");
    for (const child of node.children ?? []) {
      ref(child, json.nodes, "child node");
      if (parents.has(child)) fail("multiply parented child node");
      parents.set(child, index);
    }
    const attrs = node.extensions?.EXT_mesh_gpu_instancing?.attributes;
    if (attrs) for (const accessor of Object.values(attrs)) ref(accessor, json.accessors, "instance accessor");
    const ids = node.extensions?.MSFT_lod?.ids;
    if (ids) {
      if (!Array.isArray(ids) || !ids.length || ids.length > 15) fail("invalid node LOD chain");
      for (const id of ids) {
        ref(id, json.nodes, "LOD node");
        if (id === index || alternatives.has(id) || json.nodes[id].extensions?.MSFT_lod) fail("overlapping or nested node LOD chain");
        alternatives.add(id);
      }
      const record = node.extras?.worldSpecLod;
      if (record && (record.version !== 1 || record.distances?.length !== ids.length + 1
        || record.distances.some((d, i, all) => !Number.isFinite(d) || (i ? d <= all[i - 1] : d !== 0)))) fail("invalid authored LOD distances");
    }
  }
  const core = new Set(), active = new Set();
  function visit(id, reachable = false) {
    ref(id, json.nodes, "scene node");
    if (active.has(id)) fail("cyclic node graph");
    if (reachable) core.add(id);
    active.add(id);
    for (const child of json.nodes[id].children ?? []) visit(child, reachable);
    active.delete(id);
  }
  for (const id of json.scenes[0].nodes ?? []) visit(id, true);
  for (const id of alternatives) { if (core.has(id) || parents.has(id)) fail("lower LOD node is also in the core hierarchy"); visit(id); }
  for (const [index, material] of (json.materials ?? []).entries()) {
    const ids = material.extensions?.MSFT_lod?.ids;
    if (!ids) continue;
    if (!Array.isArray(ids) || !ids.length || ids.length > 16 || new Set(ids).size !== ids.length) fail("invalid material LOD chain");
    for (const id of ids) {
      ref(id, json.materials, "LOD material");
      if (id === index || json.materials[id].extensions?.MSFT_lod) fail("nested material LOD chain");
    }
  }
  for (const accessor of json.accessors ?? []) {
    if (accessor.sparse) fail("sparse accessors are not supported");
    ref(accessor.bufferView, json.bufferViews, "accessor buffer view");
  }
  for (const view of json.bufferViews ?? []) {
    ref(view.buffer, json.buffers, "buffer");
    if (!Number.isInteger(view.byteLength) || view.byteLength < 0 || !Number.isInteger(view.byteOffset ?? 0)
      || (view.byteOffset ?? 0) < 0 || (view.byteOffset ?? 0) + view.byteLength > json.buffers[view.buffer].byteLength) fail("buffer view exceeds its source");
  }
  for (const image of json.images ?? []) {
    if (!["image/png", "image/jpeg"].includes(image.mimeType)) fail("city GI requires native PNG/JPEG companions");
    if (image.uri) fail("native LOD images must be embedded buffer views");
    ref(image.bufferView, json.bufferViews, "image buffer view");
  }
  for (const texture of json.textures ?? []) {
    ref(texture.source, json.images, "texture image");
    if (texture.sampler !== undefined) ref(texture.sampler, json.samplers, "texture sampler");
  }
  const textures = value => {
    if (!value || typeof value !== "object") return;
    for (const [key, child] of Object.entries(value)) {
      if (key === "extras" || key === "MSFT_lod") continue;
      if (key.endsWith("Texture") && child?.index !== undefined) ref(child.index, json.textures, "material texture");
      else textures(child);
    }
  };
  for (const material of json.materials ?? []) {
    textures(material);
    for (const op of material.extras?.materialSpec?.live ?? []) for (const argument of Object.values(op.arguments ?? {}))
      if (argument.texture !== undefined) ref(argument.texture, json.textures, "live material texture");
  }
  for (const mesh of json.meshes ?? []) for (const primitive of mesh.primitives ?? []) {
    if (primitive.targets || mesh.weights) fail("morph targets are not supported");
    if (primitive.indices !== undefined) ref(primitive.indices, json.accessors, "index accessor");
    for (const accessor of Object.values(primitive.attributes ?? {})) ref(accessor, json.accessors, "vertex accessor");
    if (primitive.material !== undefined) ref(primitive.material, json.materials, "primitive material");
  }
}

/** Called before glTF-Transform reads a prototype; no declared chain means no change. */
export async function readDeclaredLod(source) {
  const json = await readPrototypeJson(source, 64 * 1024 * 1024);
  if (!hasDeclaredLod(json)) return;
  validate(json);
  const bytes = await readFile(source);
  validate(readCityEntry(bytes).json);
  return { source: resolve(source), sourceSha256: hash(bytes) };
}

/** Owner-generated in-memory GLB, used by the painted street-tree bake. */
export function declaredLodBytes(bytes) {
  bytes = Buffer.from(bytes);
  const { json } = readCityEntry(bytes);
  if (!hasDeclaredLod(json)) return;
  validate(json);
  if (json.buffers?.some(buffer => buffer.uri)) fail("in-memory authored exports must be self-contained");
  return { bytes, sourceSha256: hash(bytes) };
}

async function immutableBuffer(resourceDir, folder, bytes) {
  const digest = hash(bytes), file = `authored-${digest}.bin`, output = resolve(resourceDir, file);
  await writeImmutable(output, bytes);
  return { byteLength: bytes.length, uri: `${folder}/${file}` };
}

/** Appended after theme transforms/material rebinding. Authored maps and topology
 * stay exact; per-placement node graphs share immutable geometry/material tables. */
export async function appendDeclaredLod(target, source, { positions, name, resourceDir, folder }) {
  const bytes = source.bytes ?? await readFile(source.source);
  if (hash(bytes) !== source.sourceSha256) fail("source entry changed during publication");
  const { json, tail } = readCityEntry(bytes);
  validate(json);
  if (!positions.length) return;
  for (const position of positions) if (position.position?.length !== 3 || position.position.some(n => !Number.isFinite(n))
    || !Number.isFinite(position.heading) || !Number.isFinite(position.scale ?? 1) || (position.scale ?? 1) <= 0) fail("invalid prototype placement");
  let binary;
  if (tail.length) {
    if (tail.length < 8 || tail.readUInt32LE(4) !== 0x004e4942 || tail.readUInt32LE(0) !== tail.length - 8) fail("expected one native BIN chunk");
    binary = tail.subarray(8);
  }
  const resources = [];
  for (const [index, buffer] of (json.buffers ?? []).entries()) {
    const data = index === 0 ? binary : undefined;
    if (!data || !Number.isInteger(buffer.byteLength) || data.length < buffer.byteLength || data.length > buffer.byteLength + 3) fail("incomplete native geometry buffer");
    resources.push(await immutableBuffer(resourceDir, folder, data.subarray(0, buffer.byteLength)));
  }
  const offsets = Object.fromEntries(tables.map(key => [key, target[key]?.length ?? 0]));
  const copies = structuredClone(json);
  copies.buffers = resources;
  for (const view of copies.bufferViews ?? []) view.buffer += offsets.buffers;
  for (const accessor of copies.accessors ?? []) accessor.bufferView += offsets.bufferViews;
  for (const image of copies.images ?? []) image.bufferView += offsets.bufferViews;
  for (const texture of copies.textures ?? []) {
    texture.source += offsets.images;
    if (texture.sampler !== undefined) texture.sampler += offsets.samplers;
  }
  const textures = value => {
    if (!value || typeof value !== "object") return;
    for (const [key, child] of Object.entries(value)) {
      if (key === "extras" || key === "MSFT_lod") continue;
      if (key.endsWith("Texture") && child?.index !== undefined) child.index += offsets.textures;
      else textures(child);
    }
  };
  for (const material of copies.materials ?? []) {
    textures(material);
    if (material.extensions?.MSFT_lod) material.extensions.MSFT_lod.ids = material.extensions.MSFT_lod.ids.map(id => id + offsets.materials);
    for (const op of material.extras?.materialSpec?.live ?? []) for (const argument of Object.values(op.arguments ?? {}))
      if (argument.texture !== undefined) argument.texture += offsets.textures;
  }
  for (const mesh of copies.meshes ?? []) {
    mesh.extras = { ...mesh.extras, worldSpecDeclaredLod: true };
    for (const primitive of mesh.primitives) {
      if (primitive.indices !== undefined) primitive.indices += offsets.accessors;
      for (const key of Object.keys(primitive.attributes)) primitive.attributes[key] += offsets.accessors;
      if (primitive.material !== undefined) primitive.material += offsets.materials;
    }
  }
  for (const key of tables) if (copies[key]?.length) (target[key] ??= []).push(...copies[key]);
  target.nodes ??= [];
  const scene = target.scenes[target.scene ?? 0];
  for (const { position: [x, y, z], heading, scale = 1 } of positions) {
    const offset = target.nodes.length, nodes = structuredClone(json.nodes);
    for (const node of nodes) {
      if (node.mesh !== undefined) node.mesh += offsets.meshes;
      if (node.children) node.children = node.children.map(id => id + offset);
      if (node.extensions?.MSFT_lod) node.extensions.MSFT_lod.ids = node.extensions.MSFT_lod.ids.map(id => id + offset);
      const attrs = node.extensions?.EXT_mesh_gpu_instancing?.attributes;
      if (attrs) for (const key of Object.keys(attrs)) attrs[key] += offsets.accessors;
      // The city owns lighting; the native preview light is not part of the prop.
      if (node.extensions?.KHR_lights_punctual) delete node.extensions.KHR_lights_punctual;
    }
    target.nodes.push(...nodes);
    const a = heading * Math.PI / 360;
    scene.nodes.push(target.nodes.length);
    target.nodes.push({ name, translation: [x, z, -y], rotation: [0, Math.sin(a), 0, Math.cos(a)], scale: [scale, scale, scale],
      children: json.scenes[0].nodes.map(id => id + offset), extras: { worldSpecAuthoredLod: { version: 1, sourceSha256: source.sourceSha256 } } });
  }
  for (const key of ["extensionsUsed", "extensionsRequired"]) {
    const additions = (json[key] ?? []).filter(name => name !== "KHR_lights_punctual");
    if (additions.length) target[key] = [...new Set([...(target[key] ?? []), ...additions])];
  }
}
