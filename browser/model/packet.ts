import type { FindingDiagnostic, ReviewFinding } from "./validation.ts";
import { MAX_SCENE_IMAGE_BYTES } from "./limits.ts";
/** Version-three preview data uses Z-up meters, sRGB material colors and linear RGB vertex multipliers. */
export interface SourceLocation { line: number; column: number; offset: number; end?: number }
export interface SceneMesh {
  id: string; objectId: string; source: SourceLocation; material: string | null; visible: boolean;
  vertexStart: number; vertexCount: number; indexStart: number; indexCount: number;
  bounds: [number[], number[]];
}
/**
 * An instanced batch (`superspec_model::batches::Batch`): one mesh drawn `instanceCount` times. Its ranges index the
 * packet's batch buffers (`ScenePacket.batches`), never the scene's own vertex and index arrays. `material` is a scene
 * material. `template` names the scene mesh the batch's mesh was taken from, when it was one: per-mesh metadata kept
 * beside the scene (a receiver's atlas layout, its shading inputs) is found under it; the mesh itself is not in the scene.
 * A batch is render-only unless `collision` is `instances`; it is never listed in `Manifest.colliders`.
 */
export interface SceneBatch {
  id: string; objectId: string; source: SourceLocation; material: string | null; template?: string; visible: boolean;
  collision: "none" | "instances";
  vertexStart: number; vertexCount: number; indexStart: number; indexCount: number;
  instanceStart: number; instanceCount: number;
  /** Where the batch's `instance` values and tints start, in instances; absent without them. */
  valueStart?: number; tintStart?: number;
  /**
   * Named attributes beyond those: scalars (`components` 1) or vectors (3), one per instance or per vertex of the
   * batch's mesh, from float `start` of `BatchBuffers.attributes`. In a fixed order: instance scalars, instance
   * vectors, vertex scalars, vertex vectors, each ascending by name.
   */
  attributes?: BatchAttribute[];
  /** Bounds of the mesh in its own space, and of every instance in the scene. */
  localBounds: [number[], number[]]; bounds: [number[], number[]];
  /** How a renderer draws the batch as foliage (`superspec_types::batch::Foliage`); absent for every other batch. */
  foliage?: FoliageLook;
}
/**
 * A batch's foliage look (`superspec_types::batch::Foliage`): one of a fixed set of shader families with parameters,
 * never shader code. Every level of one field of foliage is its own batch with the same `patch`. `material` names a
 * material of the producing project whose painted program colors the instances. Angles are degrees, lengths meters.
 */
export interface FoliageLook {
  family: FoliageFamily; patch: string; level: number; distance: number; keep: number; far: number;
  material?: string; profile: "painted" | "lit";
  bend: { angle: number; randomness: number };
  wind: { azimuth: number; angle: number; scale: number; speed: number; bias: number; flutter: number };
  rejection: { boundary: number; fill: number; grazing: number; grazingKeep: number };
  /** `palette`: highlight grass, four linear colors, one per blade as its tip color (see FoliageTip in batch.rs). */
  tip: { lift: number; highlight: [number, number, number]; amount: number; start: number; palette?: [number, number, number][] };
  /** The plant families' parameters (`superspec_types::batch::FoliageCanopy`); absent for grass. */
  canopy?: FoliageCanopy;
  /** The plant painted as one mass, drawn with this batch in place of its patch's leaves and hull. */
  mass?: FoliageMass;
}
/**
 * A plant's foliage painted as one mass (`superspec_types::batch::FoliageMass`, painted-mass.ts): its leaf clusters
 * (`[x, y, z, size]`) and leafy twigs in the batch mesh's space, the density field's kernel and cell (meters), the
 * painted core's level and the share of cards kept.
 */
export interface FoliageMass {
  sites: [number, number, number, number][]; twigs: [number, number, number][][];
  radius: number; cell: number; level: number; cardKeep: number;
}
/** Grass, or a plant's part: bark, leaf clusters, the canopy hull and its shadow proxy (roadmap stage 7). */
export type FoliageFamily = "grass" | "bark" | "leaves" | "hull" | "shadow";
export interface FoliageCanopy {
  facing: number; dab: number; cutout: number; highlights: number; highlight: [number, number, number];
  translucency: number; glint: number; wrap: number;
}
/** What a plant batch without `canopy` is drawn with (the record's defaults). */
export const DEFAULT_CANOPY: FoliageCanopy = { facing: 0.8, dab: 1.25, cutout: 0.4, highlights: 0.18, highlight: [0.62, 0.74, 0.3], translucency: 0.45, glint: 0.2, wrap: 0.5 };
/** Whether a delivered foliage look is one this viewer can draw: the stated family with every parameter finite. */
export function validFoliage(look: unknown): look is FoliageLook {
  const value = look as FoliageLook;
  const numbers = (record: unknown, names: string[]) => typeof record === "object" && record !== null
    && names.every(name => Number.isFinite((record as Record<string, unknown>)[name]));
  const canopy = value?.canopy;
  if (canopy !== undefined && !(numbers(canopy, ["facing", "dab", "cutout", "highlights", "translucency", "glint", "wrap"])
    && Array.isArray(canopy.highlight) && canopy.highlight.length === 3 && canopy.highlight.every(Number.isFinite))) return false;
  const mass = value?.mass;
  const points = (list: unknown, size: number) => Array.isArray(list) && list.every(point => Array.isArray(point) && point.length === size && point.every(Number.isFinite));
  if (mass !== undefined && !(numbers(mass, ["radius", "cell", "level", "cardKeep"]) && mass.radius > 0 && mass.cell > 0
    && points(mass.sites, 4) && mass.sites.length > 0 && Array.isArray(mass.twigs) && mass.twigs.every(twig => points(twig, 3)))) return false;
  return typeof value === "object" && value !== null && ["grass", "bark", "leaves", "hull", "shadow"].includes(value.family) && typeof value.patch === "string" && value.patch.length > 0
    && Number.isInteger(value.level) && value.level >= 0 && numbers(value, ["distance", "keep", "far"]) && value.keep > 0 && value.far > value.distance
    && (value.material === undefined || typeof value.material === "string") && ["painted", "lit"].includes(value.profile)
    && numbers(value.bend, ["angle", "randomness"]) && numbers(value.wind, ["azimuth", "angle", "scale", "speed", "bias", "flutter"]) && value.wind.scale > 0
    && numbers(value.rejection, ["boundary", "fill", "grazing", "grazingKeep"]) && numbers(value.tip, ["lift", "amount", "start"])
    && Array.isArray(value.tip.highlight) && value.tip.highlight.length === 3 && value.tip.highlight.every(Number.isFinite)
    && (value.tip.palette === undefined || (Array.isArray(value.tip.palette) && value.tip.palette.length === 4
      && value.tip.palette.every(color => Array.isArray(color) && color.length === 3 && color.every(Number.isFinite))));
}
export interface BatchAttribute { name: string; domain: "instance" | "vertex"; components: number; start: number }
/** `attributes` counts the floats of named attributes. */
export interface BatchCounts { vertices: number; indices: number; instances: number; values: number; tints: number; attributes?: number }
/** Numbers per placement: translation (3), rotation quaternion x y z w (4), scale (3). */
export const PLACEMENT_FLOATS = 10;
/** The data of every batch, in `Manifest.batches` order (`superspec_model::batches::Buffers`). */
export interface BatchBuffers {
  positions: Float32Array; normals: Float32Array; uvs: Float32Array; colors: Float32Array; indices: Uint32Array;
  placements: Float32Array; values: Float32Array; tints: Float32Array;
  /** Named attributes of every batch, end to end. */
  attributes: Float32Array;
}
/** The values of one named attribute of a batch. */
export function batchAttribute(buffers: BatchBuffers, batch: SceneBatch, attribute: BatchAttribute): Float32Array {
  const count = attribute.domain === "instance" ? batch.instanceCount : batch.vertexCount;
  return buffers.attributes.subarray(attribute.start, attribute.start + count * attribute.components);
}
/** Bytes of a packet's batch section for these counts. */
export function batchByteLength(counts: BatchCounts | undefined): number {
  return counts ? 4 * (counts.vertices * 11 + counts.indices + counts.instances * PLACEMENT_FLOATS + counts.values * 4 + counts.tints * 3 + (counts.attributes ?? 0)) : 0;
}
/** Reads a batch section (little-endian, as `Scene::packet` writes it) starting at `offset`. */
export function readBatches(view: DataView, offset: number, counts: BatchCounts): BatchBuffers {
  let cursor = offset;
  const floats = (count: number): Float32Array => {
    const values = new Float32Array(count);
    for (let i = 0; i < count; i++) values[i] = view.getFloat32(cursor + i * 4, true);
    cursor += count * 4;
    return values;
  };
  const positions = floats(counts.vertices * 3), normals = floats(counts.vertices * 3), uvs = floats(counts.vertices * 2), colors = floats(counts.vertices * 3);
  const indices = new Uint32Array(counts.indices);
  for (let i = 0; i < indices.length; i++) indices[i] = view.getUint32(cursor + i * 4, true);
  cursor += indices.byteLength;
  return { positions, normals, uvs, colors, indices, placements: floats(counts.instances * PLACEMENT_FLOATS), values: floats(counts.values * 4), tints: floats(counts.tints * 3),
    attributes: floats(counts.attributes ?? 0) };
}
export interface Manifest {
  architecture?: { metadata?: { kind: string; attributes: Record<string, unknown> }[] };
  schemaVersion: 3; revision: number; success: boolean; id: string; name: string;
  units: "meters"; axes: "right-handed-z-up";
  geometryValidation: "not-run" | "checked"; navigationValidation: "not-run" | "sampled";
  diagnostics: FindingDiagnostic[];
  reviews?: ReviewFinding[];
  materials: SceneMaterial[];
  assets: { id: string; mime: "image/png" | "image/jpeg" | "image/ktx2"; byteStart: number; byteLength: number }[];
  assetByteCount: number;
  meshes: SceneMesh[]; colliders: string[];
  /** Meshes that are decals: render-only sheets a few millimeters off their surface, drawn over it; absent without any. */
  decals?: string[];
  /** Instanced batches, ascending by identity, with the lengths of their buffers; both absent without batches. */
  batches?: SceneBatch[]; batchCounts?: BatchCounts;
  meshTangents?: Record<string, number[][]>;
  vertexCount: number; indexCount: number;
}
export interface SceneMaterial {
  id: string; color: string; repeat: number; roughness: number; metalness: number;
  emissive: string | null; emissiveStrength: number | null;
  texture: string | null; normalTexture: string | null; normalScale: number | null;
  metallicRoughnessTexture: string | null; heightTexture: string | null;
  heightScale: number | null; heightOffset: number | null; opacity: number | null; alphaCutoff: number | null;
  emissiveTexture?: string | null; occlusionTexture?: string | null; occlusionStrength?: number | null;
  alphaMode?: "OPAQUE" | "MASK" | "BLEND" | null; doubleSided?: boolean | null;
  baseColorFactor?: number[] | null; emissiveFactor?: number[] | null;
  /** Live renderer operations applied on top of the baked maps, in order. */
  live?: LiveOperation[];
  /** How the material is drawn; absent is lit. */
  shading?: "lit" | "painted";
  /** A painted material's per-pixel program; the maps above are its fallback (`LiveProgram` in `superspec_types::material`). */
  program?: LiveProgram;
}
/** A delivered per-pixel program: GLSL with `ms_evaluate()`, its uniform values and the render inputs a host binds. */
export interface LiveProgram {
  version: 1; source: string; painted: string; mapping: "uv" | "object" | "world" | "anchored"; domain: [number, number];
  uniforms: { uniform: string; type: "float" | "vec2" | "vec3" | "bool" | "uint"; value: number[] }[];
  requires: string[];
  inputs: { symbol: string; kind: "uniform" | "instance" | "vertex" | "derived" | "field"; glsl: string; name?: string; fallback?: string }[];
}
/** A live argument: one value, or a channel of a linear texture on UV0 decoded as byte/255 * scale + offset. */
export type LiveArgument = { value: number } | { texture: string; channel: number; scale: number; offset: number };
/** A live renderer operation recorded with a baked material (`extras.materialSpec` in GLB). */
export interface LiveOperation { id: string; stage: string; requires: string[]; arguments: Record<string, LiveArgument> }
export interface ScenePacket {
  manifest: Manifest; positions: Float32Array; normals: Float32Array; uvs: Float32Array; colors: Float32Array; indices: Uint32Array; assets: Uint8Array;
  assetIdentities?: Map<string, Promise<string>>;
  /** Present exactly when the manifest has batches. */
  batches?: BatchBuffers;
}

/** Checks metadata and allocation limits before decoding either complete packets or updates. */
export function validateManifestHeader(manifest: Manifest): void {
  if (manifest.schemaVersion !== 3 || manifest.units !== "meters" || manifest.axes !== "right-handed-z-up"
    || !["not-run", "checked"].includes(manifest.geometryValidation) || !["not-run", "sampled"].includes(manifest.navigationValidation)
    || typeof manifest.success !== "boolean" || typeof manifest.id !== "string" || typeof manifest.name !== "string"
    || !integer(manifest.revision) || !integer(manifest.vertexCount) || !integer(manifest.indexCount)
    || !Array.isArray(manifest.meshes) || !Array.isArray(manifest.materials) || !Array.isArray(manifest.colliders)
    || !integer(manifest.assetByteCount) || !Array.isArray(manifest.assets)
    || !Array.isArray(manifest.diagnostics)) throw new Error("Unsupported scene manifest.");
  if (manifest.vertexCount > 16_000_000 || manifest.indexCount > 48_000_000 || manifest.assetByteCount > MAX_SCENE_IMAGE_BYTES
    || manifest.meshes.length > 100_000 || manifest.assets.length > 16_384) throw new Error("Scene exceeds preview allocation limits.");
  const counts = manifest.batchCounts;
  if ((manifest.batches !== undefined && !Array.isArray(manifest.batches)) || Boolean(manifest.batches?.length) !== (counts !== undefined)
    || (counts !== undefined && (![counts.vertices, counts.indices, counts.instances, counts.values, counts.tints].every(integer)
      || manifest.batches!.length > 4096 || counts.vertices > 1_500_000 || counts.indices > 9_000_000 || counts.instances > 1_000_000
      || counts.values > counts.instances || counts.tints > counts.instances
      || (counts.attributes !== undefined && (!integer(counts.attributes) || counts.attributes > 64_000_000))))) throw new Error("Unsupported scene batches.");
}

/** Rejects malformed or unsupported packets before any renderer allocations. */
export function decodePacket(buffer: ArrayBuffer): ScenePacket {
  const view = new DataView(buffer);
  if (buffer.byteLength < 8 || view.getUint32(0, false) !== 0x4c535033) throw new Error("Invalid scene packet header.");
  const jsonLength = view.getUint32(4, true);
  if (jsonLength % 4 || jsonLength > buffer.byteLength - 8) throw new Error("Invalid manifest length.");
  const manifest: Manifest = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(buffer, 8, jsonLength)));
  validateManifestHeader(manifest);
  const payload = 8 + jsonLength;
  const batchBytes = batchByteLength(manifest.batchCounts);
  if (payload + manifest.vertexCount * 44 + manifest.indexCount * 4 + batchBytes + manifest.assetByteCount !== buffer.byteLength) throw new Error("Scene buffer lengths do not match the manifest.");
  const positions = new Float32Array(manifest.vertexCount * 3);
  const normals = new Float32Array(manifest.vertexCount * 3);
  const uvs = new Float32Array(manifest.vertexCount * 2);
  const colors = new Float32Array(manifest.vertexCount * 3);
  const indices = new Uint32Array(manifest.indexCount);
  // Explicit endian reads also support big-endian hosts; arrays own memory beyond the input's lifetime.
  for (let i = 0; i < positions.length; i++) {
    positions[i] = view.getFloat32(payload + i * 4, true);
    normals[i] = view.getFloat32(payload + positions.byteLength + i * 4, true);
    if (!Number.isFinite(positions[i]) || !Number.isFinite(normals[i])) throw new Error("Nonfinite mesh data.");
  }
  for (let i = 0; i < uvs.length; i++) {
    uvs[i] = view.getFloat32(payload + positions.byteLength * 2 + i * 4, true);
    if (!Number.isFinite(uvs[i])) throw new Error("Nonfinite UV data.");
  }
  const colorOffset = payload + positions.byteLength * 2 + uvs.byteLength;
  for (let i = 0; i < colors.length; i++) {
    colors[i] = view.getFloat32(colorOffset + i * 4, true);
    if (!Number.isFinite(colors[i]) || colors[i] < 0) throw new Error("Invalid linear vertex color.");
  }
  const indexOffset = colorOffset + colors.byteLength;
  for (let i = 0; i < indices.length; i++) indices[i] = view.getUint32(indexOffset + i * 4, true);
  // Instanced batches lie between the indices and the assets.
  const batchOffset = indexOffset + indices.byteLength;
  const batches = manifest.batchCounts ? readBatches(view, batchOffset, manifest.batchCounts) : undefined;
  const assets = new Uint8Array(buffer.slice(batchOffset + batchBytes));
  return validatePacket({ manifest, positions, normals, uvs, colors, indices, assets, ...(batches ? { batches } : {}) });
}

/** Applies the same identity, range and channel admission to reconstructed updates. */
export function validatePacket(packet: ScenePacket): ScenePacket {
  const { manifest, positions, normals, uvs, colors, indices, assets } = packet;
  validateManifestHeader(manifest);
  if (positions.length !== manifest.vertexCount * 3 || normals.length !== positions.length || uvs.length !== manifest.vertexCount * 2
    || colors.length !== positions.length || indices.length !== manifest.indexCount || assets.length !== manifest.assetByteCount) throw new Error("Scene arrays do not match their manifest.");
  if (![positions, normals, uvs, colors].every(array => array.every(Number.isFinite)) || colors.some(value => value < 0)) throw new Error("Invalid scene numeric data.");
  const assetIds = new Set<string>();
  let assetEnd = 0;
  for (const asset of manifest.assets) {
    if (typeof asset.id !== "string" || assetIds.has(asset.id) || !["image/png", "image/jpeg", "image/ktx2"].includes(asset.mime)
      || asset.byteStart !== assetEnd || !integer(asset.byteLength) || asset.byteLength === 0
      || assetEnd + asset.byteLength > assets.length) throw new Error("Invalid asset range or identity.");
    assetEnd += asset.byteLength; assetIds.add(asset.id);
  }
  if (assetEnd !== assets.length) throw new Error("Incomplete asset ranges.");
  const materialIds = new Set<string>();
  for (const material of manifest.materials) {
    if (typeof material.id !== "string" || materialIds.has(material.id) || !/^#[0-9a-f]{6}$/i.test(material.color)
      || !factor(material.roughness) || !factor(material.metalness)) throw new Error("Invalid material.");
    if (!Number.isFinite(material.repeat) || material.repeat <= 0
      || [material.texture, material.normalTexture, material.metallicRoughnessTexture, material.heightTexture, material.emissiveTexture ?? null, material.occlusionTexture ?? null].some(id => id !== null && !assetIds.has(id))
      || (material.occlusionStrength != null && !factor(material.occlusionStrength))
      || (material.alphaMode != null && !["OPAQUE", "MASK", "BLEND"].includes(material.alphaMode))
      || (material.doubleSided != null && typeof material.doubleSided !== "boolean")
      || (material.baseColorFactor != null && (material.baseColorFactor.length !== 3 || !material.baseColorFactor.every(factor)))
      || (material.emissiveFactor != null && (material.emissiveFactor.length !== 3 || !material.emissiveFactor.every(value => Number.isFinite(value) && value >= 0)))
      || (material.emissive !== null && !/^#[0-9a-f]{6}$/i.test(material.emissive))
      || [material.emissiveStrength, material.normalScale, material.heightScale].some(v => v !== null && (!Number.isFinite(v) || v < 0))
      || (material.heightOffset !== null && !Number.isFinite(material.heightOffset))
      || (material.opacity !== null && !factor(material.opacity))
      || (material.alphaCutoff !== null && (material.alphaMode != null ? !Number.isFinite(material.alphaCutoff) || material.alphaCutoff < 0 : !factor(material.alphaCutoff) || material.alphaCutoff === 0 || material.alphaCutoff === 1))) throw new Error("Invalid material channels.");
    if ((material.shading != null && !["lit", "painted"].includes(material.shading))
      || (material.program != null && (material.program.version !== 1 || typeof material.program.source !== "string"
        || !material.program.source.includes("void ms_evaluate()") || typeof material.program.painted !== "string"
        || !Array.isArray(material.program.uniforms) || !material.program.uniforms.every(uniform => Array.isArray(uniform.value) && uniform.value.every(Number.isFinite))
        || !Array.isArray(material.program.inputs)))) throw new Error("Invalid material program.");
    materialIds.add(material.id);
  }
  const meshIds = new Set<string>();
  let vertices = 0, elements = 0;
  for (const mesh of manifest.meshes) {
    if (typeof mesh.id !== "string" || meshIds.has(mesh.id) || typeof mesh.objectId !== "string" || !mesh.objectId || typeof mesh.visible !== "boolean"
      || !location(mesh.source) || !integer(mesh.vertexCount) || !integer(mesh.indexCount)
      || mesh.vertexStart !== vertices || mesh.indexStart !== elements || mesh.indexCount % 3
      || vertices + mesh.vertexCount > manifest.vertexCount || elements + mesh.indexCount > manifest.indexCount
      || (mesh.material !== null && !materialIds.has(mesh.material))
      || !Array.isArray(mesh.bounds) || mesh.bounds.length !== 2
      || mesh.bounds.some(point => !Array.isArray(point) || point.length !== 3 || !point.every(Number.isFinite))) throw new Error("Invalid mesh range or identity.");
    for (let i = elements; i < elements + mesh.indexCount; i++) {
      if (indices[i] >= mesh.vertexCount) throw new Error("Mesh index exceeds its vertex range.");
    }
    vertices += mesh.vertexCount; elements += mesh.indexCount; meshIds.add(mesh.id);
  }
  if (vertices !== manifest.vertexCount || elements !== manifest.indexCount
    || manifest.colliders.some(id => !meshIds.has(id)) || new Set(manifest.colliders).size !== manifest.colliders.length
    || (!manifest.success && (vertices !== 0 || elements !== 0 || manifest.diagnostics.length === 0))) throw new Error("Incomplete scene manifest.");
  validateBatches(packet, meshIds, materialIds);
  if (manifest.meshTangents !== undefined) {
    if (!manifest.meshTangents || typeof manifest.meshTangents !== "object" || Array.isArray(manifest.meshTangents)) throw new Error("Invalid tangent metadata.");
    const counts = new Map(manifest.meshes.map(mesh => [mesh.id, mesh.vertexCount]));
    for (const [id, tangents] of Object.entries(manifest.meshTangents)) {
      if (!Array.isArray(tangents) || tangents.length !== counts.get(id)
        || tangents.some(tangent => !Array.isArray(tangent) || tangent.length !== 4 || !tangent.every(Number.isFinite)
          || Math.abs(tangent[3]) !== 1 || Math.hypot(tangent[0], tangent[1], tangent[2]) < 1e-10)) throw new Error("Invalid authored tangent basis.");
    }
  }
  for (const diagnostic of manifest.diagnostics) {
    if (typeof diagnostic.code !== "string" || typeof diagnostic.message !== "string" || !location(diagnostic.source)
      || !Array.isArray(diagnostic.objects) || !diagnostic.objects.every(id => typeof id === "string")) throw new Error("Invalid source diagnostic.");
  }
  return packet;
}

/** Batch records and buffers must agree as `superspec_model::batches::check` requires, with valid placements. */
function validateBatches(packet: ScenePacket, meshIds: ReadonlySet<string>, materialIds: ReadonlySet<string>): void {
  const { manifest, batches } = packet;
  const counts = manifest.batchCounts;
  if (!counts) { if (batches) throw new Error("Scene batch data has no manifest."); return; }
  if (!batches || batches.positions.length !== counts.vertices * 3 || batches.normals.length !== counts.vertices * 3
    || batches.uvs.length !== counts.vertices * 2 || batches.colors.length !== counts.vertices * 3 || batches.indices.length !== counts.indices
    || batches.placements.length !== counts.instances * PLACEMENT_FLOATS || batches.values.length !== counts.values * 4
    || batches.tints.length !== counts.tints * 3 || batches.attributes.length !== (counts.attributes ?? 0)) throw new Error("Scene batch arrays do not match their manifest.");
  if (![batches.positions, batches.normals, batches.uvs, batches.colors, batches.placements, batches.values, batches.tints, batches.attributes].every(array => array.every(Number.isFinite))
    || batches.colors.some(value => value < 0) || batches.tints.some(value => value < 0)) throw new Error("Invalid batch numeric data.");
  let vertices = 0, elements = 0, instances = 0, values = 0, tints = 0, named = 0, previous: string | undefined;
  const bounds = (value: unknown): boolean => Array.isArray(value) && value.length === 2
    && value.every(point => Array.isArray(point) && point.length === 3 && point.every(Number.isFinite));
  for (const batch of manifest.batches!) {
    if (typeof batch.id !== "string" || !batch.id || meshIds.has(batch.id) || (previous !== undefined && !(previous < batch.id))
      || typeof batch.objectId !== "string" || !batch.objectId || typeof batch.visible !== "boolean" || !location(batch.source)
      || !["none", "instances"].includes(batch.collision) || (batch.material !== null && !materialIds.has(batch.material))
      || (batch.template !== undefined && (typeof batch.template !== "string" || !batch.template))
      || batch.vertexStart !== vertices || batch.indexStart !== elements || batch.instanceStart !== instances
      || !integer(batch.vertexCount) || !integer(batch.indexCount) || !integer(batch.instanceCount)
      || batch.vertexCount === 0 || batch.indexCount === 0 || batch.indexCount % 3 || batch.instanceCount === 0
      || vertices + batch.vertexCount > counts.vertices || elements + batch.indexCount > counts.indices || instances + batch.instanceCount > counts.instances
      || (batch.valueStart !== undefined && batch.valueStart !== values) || (batch.tintStart !== undefined && batch.tintStart !== tints)
      || !bounds(batch.localBounds) || !bounds(batch.bounds)) throw new Error("Invalid batch range or identity.");
    if (batch.foliage !== undefined && !validFoliage(batch.foliage)) throw new Error("Invalid batch foliage look.");
    for (let i = elements; i < elements + batch.indexCount; i++) if (batches.indices[i] >= batch.vertexCount) throw new Error("Batch index exceeds its mesh.");
    for (let i = instances; i < instances + batch.instanceCount; i++) {
      const at = i * PLACEMENT_FLOATS, p = batches.placements;
      if (Math.abs(Math.hypot(p[at + 3], p[at + 4], p[at + 5], p[at + 6]) - 1) > 1e-3 || !(p[at + 7] > 0 && p[at + 8] > 0 && p[at + 9] > 0)) throw new Error("Invalid batch placement.");
    }
    if (batch.attributes !== undefined && (!Array.isArray(batch.attributes) || batch.attributes.length > 24)) throw new Error("Invalid batch attributes.");
    const names = new Set<string>();
    for (const attribute of batch.attributes ?? []) {
      const key = `${attribute.domain}:${attribute.name}`;
      if (typeof attribute.name !== "string" || !/^[A-Za-z][A-Za-z0-9]{0,31}$/.test(attribute.name) || names.has(key)
        || !["instance", "vertex"].includes(attribute.domain) || ![1, 3].includes(attribute.components) || attribute.start !== named) throw new Error("Invalid batch attributes.");
      names.add(key);
      named += attribute.components * (attribute.domain === "instance" ? batch.instanceCount : batch.vertexCount);
    }
    previous = batch.id;
    vertices += batch.vertexCount; elements += batch.indexCount; instances += batch.instanceCount;
    if (batch.valueStart !== undefined) values += batch.instanceCount;
    if (batch.tintStart !== undefined) tints += batch.instanceCount;
  }
  if (vertices !== counts.vertices || elements !== counts.indices || instances !== counts.instances || values !== counts.values || tints !== counts.tints || named !== (counts.attributes ?? 0)) throw new Error("Batch records do not cover their buffers.");
}

/** Counts and revisions are unsigned, exactly representable integers. */
function integer(value: number): boolean { return Number.isSafeInteger(value) && value >= 0; }
/** PBR factors must be finite and normalized. */
function factor(value: number): boolean { return Number.isFinite(value) && value >= 0 && value <= 1; }
/** Source offsets are UTF-16, while line and column numbers start at one. */
function location(value: SourceLocation): boolean {
  return !!value && integer(value.line) && value.line > 0 && integer(value.column) && value.column > 0
    && integer(value.offset) && (value.end === undefined || (integer(value.end) && value.end >= value.offset));
}
