import { writeImmutable } from "./immutable-file.mjs";
import { createHash, randomUUID } from "node:crypto";
import { readFile, rename, rm, writeFile } from "node:fs/promises";
import { basename, dirname, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { Matrix3, Matrix4, Quaternion, Vector3 } from "three";
import { MeshoptDecoder, MeshoptSimplifier } from "meshoptimizer";
import { GeometryReader } from "./city-lod.mjs";
import { entryBytes, localPath, readCityEntry, realDestination, retargetLod, verifiedLod } from "./city-data-textures.mjs";
import { bakeBlockAtlas } from "./block-proxy-atlas.mjs";

const matrix = (t, r, s) => new Matrix4().compose(new Vector3(...(t ?? [0,0,0])),
  new Quaternion(...(r ?? [0,0,0,1])), new Vector3(...(s ?? [1,1,1])));

async function transforms(json, reader, node) {
  const base = node.matrix ? new Matrix4().fromArray(node.matrix) : matrix(node.translation, node.rotation, node.scale);
  const attributes = node.extensions?.EXT_mesh_gpu_instancing?.attributes;
  if (!attributes) return [base];
  const columns = {}, semantics = { TRANSLATION: 3, ROTATION: 4, SCALE: 3 };
  let count;
  for (const [key, id] of Object.entries(attributes)) {
    if (!(key in semantics) || (count !== undefined && count !== json.accessors[id].count))
      throw Error("Unsupported block placement attributes.");
    count = json.accessors[id].count;
    columns[key] = await reader.read(id);
  }
  if (!count) throw Error("Empty block placement group.");
  return Array.from({ length: count }, (_, i) => {
    const at = key => columns[key]?.subarray(i * semantics[key], (i+1) * semantics[key]);
    return base.clone().multiply(matrix(at("TRANSLATION"), at("ROTATION"), at("SCALE")));
  });
}

function selectBlock(json, cell) {
  if (!/^-?\d+,-?\d+$/.test(cell)) throw Error("Block cell must be a pair of integer grid coordinates.");
  const scene = json.scenes[json.scene ?? 0];
  if (scene.extras?.worldSpecBlockLods) throw Error("This city already has block LODs.");
  const id = "Street " + cell;
  const sources = scene.nodes.filter(i => json.nodes[i].name === id && /^building-\d+$/.test(json.meshes[json.nodes[i].mesh]?.name));
  if (!sources.length) throw Error("No published building shells found in " + id);
  const animated = new Set((json.animations ?? []).flatMap(a => a.channels.map(c => c.target.node)));
  for (const i of sources) {
    const n = json.nodes[i];
    if (animated.has(i) || n.skin !== undefined || n.children?.length || n.weights ||
        Object.keys(n.extensions ?? {}).some(k => k !== "EXT_mesh_gpu_instancing") ||
        json.nodes.some(other => other.children?.includes(i) || other.extensions?.MSFT_lod?.ids?.includes(i)))
      throw Error("Block proxy requires independent static root nodes.");
    for (const p of json.meshes[n.mesh].primitives) {
      if ((p.mode ?? 4) !== 4 || p.targets || p.extensions || p.indices === undefined ||
          ["POSITION", "NORMAL", "TEXCOORD_0"].some(k => p.attributes[k] === undefined) ||
          Object.values(p.attributes).concat(p.indices).some(id => json.accessors[id].sparse))
        throw Error("Block proxy requires indexed static triangles with normals and UVs.");
    }
  }
  return { scene, id, sources };
}

function extendBounds(bounds, pos) {
  for (let i = 0; i < pos.length; i++) {
    const k = i % 3;
    bounds.min[k] = Math.min(bounds.min[k], pos[i]);
    bounds.max[k] = Math.max(bounds.max[k], pos[i]);
  }
}
const emptyBounds = () => ({ min: [Infinity, Infinity, Infinity], max: [-Infinity, -Infinity, -Infinity] });

// Reuse the producer's per-material far streams (including aggressive hardware
// tiers). A block must not restore geometry its individual buildings can shed.
function publishedIndices(indices, count, payload, record) {
  if (!record) return indices;
  if (record.nearCount !== indices.length || record.vertexCount !== count) throw Error("Block LOD source counts do not match.");
  const levels = [...record.levels, ...(record.detailLevels ?? []), ...(record.distantLevels ?? [])];
  const level = levels.reduce((best, l) => l.count < best.count ? l : best);
  if (level.count === indices.length && !level.bytes) return indices;
  if (![2,4].includes(level.componentBytes) || !Number.isSafeInteger(level.count) || level.count < 0 ||
      level.count > indices.length || level.count % 3 || !Number.isSafeInteger(level.offset) || level.offset < 0 ||
      !Number.isSafeInteger(level.bytes) || level.bytes < 0 || level.offset + level.bytes > payload.length)
    throw Error("Invalid published block LOD stream.");
  const result = level.componentBytes === 2 ? new Uint16Array(level.count) : new Uint32Array(level.count);
  if (result.length) MeshoptDecoder.decodeIndexBuffer(new Uint8Array(result.buffer), level.count, level.componentBytes,
    payload.subarray(level.offset, level.offset + level.bytes));
  if (result.some(index => index >= count)) throw Error("Block LOD references an absent vertex.");
  return result;
}

// A two-centimetre skin of rusty tiles can cover an entire roof. Geometric
// error alone happily removes it and exposes a different pigment underneath.
// Restore whole connected islands; mixing their old and new triangles would
// leave duplicate co-planar faces. Window panes likewise carry whole views.
export function retainPaintedSurfaces(indices, reduced, pos, colors, colorWidth, transparent) {
  if (transparent || reduced === indices) return indices;
  const count = pos.length / 3, owner = Int32Array.from({ length: count }, (_,i) => i);
  const find = index => {
    while (index !== owner[index]) { owner[index] = owner[owner[index]]; index = owner[index]; }
    return index;
  };
  for (const triangles of [indices, reduced]) for (let t = 0; t < triangles.length; t+=3) {
    const a = find(triangles[t]);
    owner[find(triangles[t+1])] = a; owner[find(triangles[t+2])] = a;
  }
  const keep = new Set();
  const paintedArea = new Float64Array(count);
  for (let t = 0; t < indices.length; t+=3) {
    const a = indices[t], c = a*colorWidth;
    const i = a*3, j = indices[t+1]*3, k = indices[t+2]*3;
    const ax = pos[j]-pos[i], ay = pos[j+1]-pos[i+1], az = pos[j+2]-pos[i+2];
    const bx = pos[k]-pos[i], by = pos[k+1]-pos[i+1], bz = pos[k+2]-pos[i+2];
    const up = az*bx-ax*bz;
    const area = .5*Math.hypot(ay*bz-az*by, up, ax*by-ay*bx);
    const component = find(a);
    if (colors) {
      const r = colors[c], g = colors[c+1], b = colors[c+2];
      if (Math.max(r,g,b) - Math.min(r,g,b) >= .065) paintedArea[component] += area;
    }
    // Keep the neutral tiles adjoining painted ones too. A flat opaque roof
    // underlay closely follows these faces but cannot reproduce their field.
    if (paintedArea[component] >= .06 || area >= .025 && up >= 2*area*.95) keep.add(component);
  }
  if (!keep.size) return reduced;
  const result = [];
  for (const triangles of [reduced, indices]) for (let t = 0; t < triangles.length; t+=3) {
    if (keep.has(find(triangles[t])) === (triangles === indices))
      result.push(triangles[t], triangles[t+1], triangles[t+2]);
  }
  return result;
}

async function appendPart(model, json, reader, primitive, transform, atlas, lod, record) {
  const { attributes, material } = primitive;
  const [indices, pos, normal, uv, colors, chartAspect] = await Promise.all([
    reader.read(primitive.indices, Uint32Array), reader.read(attributes.POSITION), reader.read(attributes.NORMAL),
    reader.read(attributes.TEXCOORD_0), attributes.COLOR_0 === undefined ? undefined : reader.read(attributes.COLOR_0),
    json.materials[material].extras?.materialSpecNormalFrame && attributes._MS_ASPECT !== undefined
      ? reader.read(attributes._MS_ASPECT) : undefined,
  ]);
  const count = pos.length / 3, colorWidth = colors ? colors.length / count : 0;
  const determinant = transform.determinant();
  if (!Number.isFinite(determinant) || Math.abs(determinant) < 1e-12) throw Error("Degenerate block placement transform.");
  const point = new Vector3(), normalTransform = new Matrix3().getNormalMatrix(transform);
  // Errors are in world metres. UV and normal error penalize loss of existing
  // atlas seams, window outlines and narrow changes in facade shading.
  const fields = new Float32Array(count * 5);
  for (let v = 0; v < count; v++) {
    point.fromArray(pos, 3*v).applyMatrix4(transform).toArray(pos, 3*v);
    point.fromArray(normal, 3*v).applyNormalMatrix(normalTransform).toArray(normal, 3*v);
    fields.set(normal.subarray(3*v, 3*v+3), 5*v);
    fields.set(uv.subarray(2*v, 2*v+2), 5*v+3);
    if (!(uv[2*v] >= 0 && uv[2*v] <= 1 && uv[2*v+1] >= 0 && uv[2*v+1] <= 1))
      throw Error("Block proxy requires charted UVs in [0,1].");
  }
  extendBounds(model.bounds, pos);
  const target = Math.max(3, 3 * Math.floor(indices.length / 3 * .018));
  const [simplified] = MeshoptSimplifier.simplifyWithAttributes(indices, pos, 3, fields, 5, [.45,.45,.45,3,3],
    null, target, .26, ["ErrorAbsolute", "Permissive", "Prune"]);
  const published = publishedIndices(indices, count, lod?.payload, record);
  const m = json.materials[material];
  const reduced = retainPaintedSurfaces(indices, published.length <= simplified.length ? published : simplified,
    pos, colors, colorWidth, m.alphaMode === "BLEND");
  model.nearTriangles += indices.length / 3;
  model.publishedFarTriangles += published.length / 3;
  const group = (m.alphaMode ?? "OPAQUE") + (m.doubleSided ? "/double" : "/front");
  if (!model.groups.has(group)) model.groups.set(group, []);
  const draw = model.groups.get(group), remap = new Map();
  for (let i = 0; i < reduced.length; i++) {
    // A mirrored source instance changes winding, the packed world-space mesh
    // has an identity transform so the change must be encoded in its indices.
    const v = reduced[determinant < 0 && i%3 !== 0 ? i + (i%3 === 1 ? 1 : -1) : i];
    if (!remap.has(v)) {
      remap.set(v, model.positions.length / 3);
      model.positions.push(pos[3*v], pos[3*v+1], pos[3*v+2]);
      model.normals.push(normal[3*v], normal[3*v+1], normal[3*v+2]);
      model.uvs.push(...atlas.uv(material, uv[2*v], uv[2*v+1]));
      model.aspects.push((chartAspect ? chartAspect[v] : 1) * atlas.height / atlas.width);
      for (let c = 0; c < 4; c++) model.colors.push(Math.round(255 * (colorWidth > c ? colors[v*colorWidth+c] : 1)));
    }
    draw.push(remap.get(v));
  }
}

// Publish all proxy-owned bytes under one immutable content identity. The
// original buffers, scene roots and canonical collision export stay intact.
async function attachResources(json, model, atlas, directory, folder) {
  const parts = [], buffer = json.buffers.length;
  let length = 0;
  const view = (bytes, target) => {
    const padding = (4-length%4)%4;
    if (padding) parts.push(Buffer.alloc(padding));
    const start = length + padding;
    parts.push(bytes); length = start + bytes.byteLength;
    return json.bufferViews.push({ buffer, byteOffset: start, byteLength: bytes.byteLength, ...(target && { target }) })-1;
  };
  const attribute = (data, Type, componentType, type, width, options = {}) => {
    const isIndex = type === "SCALAR" && (componentType === 5123 || componentType === 5125);
    const bufferView = view(Buffer.from(Type.from(data).buffer), isIndex ? 34963 : 34962);
    return json.accessors.push({ bufferView, componentType, count: data.length / width, type, ...options })-1;
  };
  const geomBounds = emptyBounds();
  extendBounds(geomBounds, model.positions);
  const attributes = {
    POSITION: attribute(model.positions, Float32Array, 5126, "VEC3", 3, geomBounds),
    NORMAL: attribute(model.normals, Float32Array, 5126, "VEC3", 3),
    TEXCOORD_0: attribute(model.uvs, Float32Array, 5126, "VEC2", 2),
    COLOR_0: attribute(model.colors, Uint8Array, 5121, "VEC4", 4, { normalized: true }),
    _MS_ASPECT: attribute(model.aspects, Float32Array, 5126, "SCALAR", 1),
  };
  const textures = {};
  const sampler = (json.samplers ??= []).push({ magFilter: 9729, minFilter: 9987, wrapS: 33071, wrapT: 33071 })-1;
  for (const [kind, data] of Object.entries(atlas.images)) {
    const source = (json.images ??= []).push({ name: "Block proxy " + kind, bufferView: view(data), mimeType: "image/png" })-1;
    textures[kind] = { index: (json.textures ??= []).push({ source, sampler })-1 };
  }
  const primitives = [];
  for (const [group, indices] of model.groups) {
    if (!indices.length) continue;
    const [alphaMode, side] = group.split("/");
    const surface = { name: "Block proxy " + group, doubleSided: side === "double", alphaMode,
      extras: { worldSpecBlockAtlas: true, materialSpecNormalFrame: true,
        worldSpecSurface: { family: "Building atlases", recipe: alphaMode === "BLEND" ? "architectural-glazing" : "opaque-building-atlas" } },
      pbrMetallicRoughness: { baseColorTexture: textures.color, metallicRoughnessTexture: textures.mr },
      emissiveTexture: textures.emission, emissiveFactor: [1,1,1], normalTexture: textures.normal };
    if (atlas.scale > 1) {
      surface.extensions = { KHR_materials_emissive_strength: { emissiveStrength: atlas.scale } };
      json.extensionsUsed ??= [];
      if (!json.extensionsUsed.includes("KHR_materials_emissive_strength")) json.extensionsUsed.push("KHR_materials_emissive_strength");
    }
    const material = json.materials.push(surface)-1, small = model.positions.length/3 <= 65536;
    primitives.push({ attributes, material, indices: attribute(indices, small ? Uint16Array : Uint32Array, small ? 5123 : 5125, "SCALAR", 1) });
  }
  const mesh = json.meshes.push({ name: "Distant building block", primitives })-1;
  const payload = Buffer.concat(parts), hash = createHash("sha256").update(payload).digest("hex").slice(0,24);
  const uri = folder + "/block-" + hash + ".bin", target = localPath(directory, uri);
  await writeImmutable(target, payload);
  json.buffers.push({ uri, byteLength: payload.length });
  return { mesh, bytes: payload.length };
}

export async function attachBlockProxy(destinationPath, bytes, { cell, distance = 45 }) {
  const directory = dirname(resolve(destinationPath)), folder = basename(destinationPath, ".glb") + "-data";
  if (!(Number.isFinite(distance) && distance > 0)) throw Error("Block distance must be positive metres.");
  const { json, tail } = readCityEntry(bytes);
  if (tail.length) throw Error("Block proxy requires a published city entry with external buffers.");
  for (const b of json.buffers) localPath(directory, b.uri);
  const { scene, id, sources } = selectBlock(json, cell);
  const lod = await verifiedLod(json, tail, directory);
  const streams = new Map(lod?.manifest.primitives.map(record => [record.mesh + "/" + record.primitive, record]));
  const reader = new GeometryReader(json, directory), ids = new Set();
  for (const i of sources) for (const p of json.meshes[json.nodes[i].mesh].primitives) ids.add(p.material);
  const atlas = await bakeBlockAtlas(json, directory, ids);
  const model = { positions: [], normals: [], uvs: [], colors: [], aspects: [], groups: new Map(), nearTriangles: 0, publishedFarTriangles: 0, bounds: emptyBounds() };
  await Promise.all([MeshoptSimplifier.ready, MeshoptDecoder.ready]);
  let nearPrimitives = 0, buildings = 0;
  for (const i of sources) {
    const node = json.nodes[i];
    for (const instance of await transforms(json, reader, node)) {
      buildings++;
      for (const [j, p] of json.meshes[node.mesh].primitives.entries()) {
        nearPrimitives++;
        await appendPart(model, json, reader, p, instance, atlas, lod, streams.get(node.mesh + "/" + j));
      }
    }
  }
  if (!model.positions.length) throw Error("Block proxy produced empty geometry.");
  const resource = await attachResources(json, model, atlas, directory, folder);
  const proxy = json.nodes.push({ name: id + " distant block", mesh: resource.mesh }) - 1;
  const block = { id, sources, proxy, bounds: model.bounds, distance };
  (scene.extras ??= {}).worldSpecBlockLods = { version: 1, blocks: [block] };
  await retargetLod(json, lod, directory, folder);
  return { entry: entryBytes(json, tail), summary: { ...block, buildings, nearPrimitives, nearTriangles: model.nearTriangles, publishedFarTriangles: model.publishedFarTriangles,
    proxyPrimitives: json.meshes[resource.mesh].primitives.length, proxyTriangles: [...model.groups.values()].reduce((s,a) => s + a.length/3, 0),
    tiles: atlas.tiles, atlasSize: [atlas.width, atlas.height], bytes: resource.bytes } };
}

async function main() {
  const usage = "Usage: node tools/kowloon-export/block-proxy.mjs source.glb sibling-output.glb --cell=X,Z [--distance=45]";
  const { positionals, values } = parseArgs({ allowPositionals: true, options: {
    cell: { type: "string" }, distance: { type: "string" }, help: { type: "boolean", short: "h" },
  } });
  if (values.help) {
    console.log(usage + "\nBake the building shells in one 28-metre glTF XZ street cell, e.g. --cell=-2,0."
      + "\nDistance is measured in metres from the block bounds at the reference 1080p view."
      + "\nKeep both entries and their companion files in the same directory. Preview with blockproxy=on.");
    return;
  }
  if (positionals.length !== 2 || !values.cell) throw Error(usage);
  const [source, destination] = await Promise.all(positionals.map(p => realDestination(resolve(p))));
  if (!relative(source, destination) || relative(dirname(source), dirname(destination))) throw Error("Use a separate sibling entry to retain the source and share its original companion URIs.");
  const original = await readFile(source);
  const { entry, summary } = await attachBlockProxy(destination, original, { cell: values.cell, distance: Number(values.distance ?? 45) });
  const pending = destination + "." + randomUUID() + ".tmp";
  try {
    await writeFile(pending, entry, { flag: "wx" });
    if (!(await readFile(source)).equals(original)) throw Error("Source entry changed during the block bake.");
    await rename(pending, destination);
  } finally { await rm(pending, { force: true }); }
  console.log(JSON.stringify(summary, null, 2));
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) await main();
