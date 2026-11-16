import { type Manifest, type ScenePacket, batchByteLength, readBatches, validateManifestHeader, validatePacket } from "./packet.ts";

interface Header { manifest: Manifest; baseRevision: number | null; meshes: (number | null)[]; assets: (number | null)[] }

/** Owns the last accepted transport snapshot, independently of the visible scene's lifecycle. */
export class SceneUpdates {
  private previous?: ScenePacket;

  /** Reconstructs one coherent revision. Invalid updates leave the prior snapshot available. */
  decode(buffer: ArrayBuffer): ScenePacket {
    const view = new DataView(buffer);
    if (buffer.byteLength < 8 || view.getUint32(0, false) !== 0x4c535531) throw new Error("Invalid scene update header.");
    const size = view.getUint32(4, true);
    if (size % 4 || size > buffer.byteLength - 8) throw new Error("Invalid scene update metadata length.");
    const header: Header = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(buffer, 8, size)));
    const { manifest } = header;
    validateManifestHeader(manifest);
    const prior = header.baseRevision === null ? undefined : this.previous;
    if (header.baseRevision !== null && prior?.manifest.revision !== header.baseRevision) throw new Error("Scene update base revision is unavailable.");
    if (!Array.isArray(header.meshes) || !Array.isArray(header.assets) || header.meshes.length !== manifest.meshes.length || header.assets.length !== manifest.assets.length) throw new Error("Scene update references are incomplete.");
    const packet: ScenePacket = {
      manifest, positions: new Float32Array(manifest.vertexCount * 3), normals: new Float32Array(manifest.vertexCount * 3),
      uvs: new Float32Array(manifest.vertexCount * 2), colors: new Float32Array(manifest.vertexCount * 3),
      indices: new Uint32Array(manifest.indexCount), assets: new Uint8Array(manifest.assetByteCount), assetIdentities: new Map(),
    };
    let cursor = 8 + size;
    /** Reads a bounded little-endian payload range into the reconstructed destination. */
    const read = (target: Float32Array | Uint32Array, start: number, count: number): void => {
      if (!Number.isSafeInteger(start) || !Number.isSafeInteger(count) || start < 0 || count < 0 || start + count > target.length || cursor + count * 4 > buffer.byteLength) throw new Error("Scene update range exceeds its payload.");
      for (let i = 0; i < count; i++) target[start + i] = target instanceof Uint32Array ? view.getUint32(cursor + i * 4, true) : view.getFloat32(cursor + i * 4, true);
      cursor += count * 4;
    };
    for (const [index, mesh] of manifest.meshes.entries()) {
      const reference = header.meshes[index];
      if (reference === null) {
        read(packet.positions, mesh.vertexStart * 3, mesh.vertexCount * 3);
        read(packet.normals, mesh.vertexStart * 3, mesh.vertexCount * 3);
        read(packet.uvs, mesh.vertexStart * 2, mesh.vertexCount * 2);
        read(packet.colors, mesh.vertexStart * 3, mesh.vertexCount * 3);
        read(packet.indices, mesh.indexStart, mesh.indexCount);
      } else {
        if (!Number.isSafeInteger(reference) || reference < 0) throw new Error("Invalid retained mesh reference.");
        const old = prior?.manifest.meshes[reference];
        if (!old || old.id !== mesh.id || old.vertexCount !== mesh.vertexCount || old.indexCount !== mesh.indexCount) throw new Error("Retained mesh shape differs from its source.");
        packet.positions.set(prior!.positions.subarray(old.vertexStart * 3, (old.vertexStart + old.vertexCount) * 3), mesh.vertexStart * 3);
        packet.normals.set(prior!.normals.subarray(old.vertexStart * 3, (old.vertexStart + old.vertexCount) * 3), mesh.vertexStart * 3);
        packet.uvs.set(prior!.uvs.subarray(old.vertexStart * 2, (old.vertexStart + old.vertexCount) * 2), mesh.vertexStart * 2);
        packet.colors.set(prior!.colors.subarray(old.vertexStart * 3, (old.vertexStart + old.vertexCount) * 3), mesh.vertexStart * 3);
        packet.indices.set(prior!.indices.subarray(old.indexStart, old.indexStart + old.indexCount), mesh.indexStart);
      }
    }
    for (const [index, asset] of manifest.assets.entries()) {
      const reference = header.assets[index];
      if (reference === null) {
        if (cursor + asset.byteLength > buffer.byteLength) throw new Error("Scene update image exceeds its payload.");
        packet.assets.set(new Uint8Array(buffer, cursor, asset.byteLength), asset.byteStart);
        cursor += asset.byteLength;
      } else {
        if (!Number.isSafeInteger(reference) || reference < 0) throw new Error("Invalid retained asset reference.");
        const old = prior?.manifest.assets[reference];
        if (!old || old.mime !== asset.mime || old.byteLength !== asset.byteLength) throw new Error("Retained asset shape differs from its source.");
        packet.assets.set(prior!.assets.subarray(old.byteStart, old.byteStart + old.byteLength), asset.byteStart);
        const identity = prior!.assetIdentities?.get(old.id);
        if (identity) packet.assetIdentities!.set(asset.id, identity);
      }
    }
    // Instanced batches come whole with every update, after everything else.
    if (manifest.batchCounts) {
      const length = batchByteLength(manifest.batchCounts);
      if (cursor + length > buffer.byteLength) throw new Error("Scene update batches exceed their payload.");
      packet.batches = readBatches(view, cursor, manifest.batchCounts);
      cursor += length;
    }
    if (cursor !== buffer.byteLength) throw new Error("Scene update has trailing data.");
    validatePacket(packet);
    this.previous = packet;
    return packet;
  }
}
