import type { FindingDiagnostic, ReviewFinding } from "./validation.ts";
import { MAX_SCENE_IMAGE_BYTES } from "./limits.ts";
/** Version-three preview data uses Z-up meters, sRGB material colors and linear RGB vertex multipliers. */
export interface SourceLocation { line: number; column: number; offset: number; end?: number }
export interface SceneMesh {
  id: string; objectId: string; source: SourceLocation; material: string | null; visible: boolean;
  vertexStart: number; vertexCount: number; indexStart: number; indexCount: number;
  bounds: [number[], number[]];
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
}
/** A live argument: one value, or a channel of a linear texture on UV0 decoded as byte/255 * scale + offset. */
export type LiveArgument = { value: number } | { texture: string; channel: number; scale: number; offset: number };
/** A live renderer operation recorded with a baked material (`extras.materialSpec` in GLB). */
export interface LiveOperation { id: string; stage: string; requires: string[]; arguments: Record<string, LiveArgument> }
export interface ScenePacket {
  manifest: Manifest; positions: Float32Array; normals: Float32Array; uvs: Float32Array; colors: Float32Array; indices: Uint32Array; assets: Uint8Array;
  assetIdentities?: Map<string, Promise<string>>;
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
  if (payload + manifest.vertexCount * 44 + manifest.indexCount * 4 + manifest.assetByteCount !== buffer.byteLength) throw new Error("Scene buffer lengths do not match the manifest.");
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
  const assets = new Uint8Array(buffer.slice(indexOffset + indices.byteLength));
  return validatePacket({ manifest, positions, normals, uvs, colors, indices, assets });
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

/** Counts and revisions are unsigned, exactly representable integers. */
function integer(value: number): boolean { return Number.isSafeInteger(value) && value >= 0; }
/** PBR factors must be finite and normalized. */
function factor(value: number): boolean { return Number.isFinite(value) && value >= 0 && value <= 1; }
/** Source offsets are UTF-16, while line and column numbers start at one. */
function location(value: SourceLocation): boolean {
  return !!value && integer(value.line) && value.line > 0 && integer(value.column) && value.column > 0
    && integer(value.offset) && (value.end === undefined || (integer(value.end) && value.end >= value.offset));
}
