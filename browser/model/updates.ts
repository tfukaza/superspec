import { type BatchBuffers, type Manifest, type ScenePacket, batchByteLength, copyPacketWords, decodePacket, readBatches, validateManifestHeader, validatePacket } from "./packet.ts";

interface Header {
  manifest: Manifest;
  baseRevision: number | null;
  meshes: (number | null)[];
  assets: (number | null)[];
  retainedScene?: boolean;
  batchSpans?: [number, number][];
  foliageMassRefs?: [number, number][];
}
const batchChannels = ["positions", "normals", "uvs", "colors", "indices", "placements", "values", "tints", "attributes"] as const;
const updateMagic = 0x4c535531;

/** Everything delivered to the renderer, except the edited document identity and AML source ranges. */
function sameSceneMetadata(a: Manifest, b: Manifest): boolean {
  const source = (v: object) => "offset" in v && "line" in v && Object.keys(v).every(k => ["offset", "end", "line", "column"].includes(k));
  function equal(left: unknown, right: unknown, key?: string): boolean {
    // Hydrated canopy references and their potentially large point arrays share identity.
    if (Object.is(left, right)) return true;
    if (!left || !right || typeof left !== "object" || typeof right !== "object") return false;
    if (key === "source" && source(left) && source(right)) return true;
    if (Array.isArray(left) || Array.isArray(right)) return Array.isArray(left) && Array.isArray(right)
      && left.length === right.length && left.every((value, i) => equal(value, right[i]));
    const l = left as Record<string, unknown>, r = right as Record<string, unknown>;
    const keys = Object.keys(l);
    return keys.length === Object.keys(r).length && keys.every(k => Object.hasOwn(r, k) && equal(l[k], r[k], k));
  }
  const { revision: _ar, name: _an, id: _ai, ...left } = a;
  const { revision: _br, name: _bn, id: _bi, ...right } = b;
  return equal(left, right);
}

/** Older producers transfer whole batches. Check their little-endian bits before retaining a displayed revision. */
function sameBatchBytes(view: DataView, offset: number, packet: ScenePacket): boolean {
  if (!packet.batches) return true;
  const b = packet.batches;
  for (const channel of [b.positions, b.normals, b.uvs, b.colors, b.indices, b.placements, b.values, b.tints, b.attributes]) {
    const bits = new Uint32Array(channel.buffer, channel.byteOffset, channel.length);
    for (let i = 0; i < bits.length; i++, offset += 4) if (bits[i] !== view.getUint32(offset, true)) return false;
  }
  return true;
}

/** Owns the last accepted transport snapshot, independently of the visible scene's lifecycle. */
export class SceneUpdates {
  private previous?: ScenePacket;
  get revision() { return this.previous?.manifest.revision; }

  /** A rejected preview is a diagnostic scene packet and leaves the accepted update baseline intact. */
  decodePreview(buffer: ArrayBuffer): ScenePacket {
    return buffer.byteLength >= 4 && new DataView(buffer).getUint32(0) === updateMagic
      ? this.decode(buffer) : decodePacket(buffer);
  }

  /** Reconstructs one coherent revision. Invalid updates leave the prior snapshot available. */
  decode(buffer: ArrayBuffer): ScenePacket {
    const view = new DataView(buffer);
    if (buffer.byteLength < 8 || view.getUint32(0, false) !== updateMagic) throw new Error("Invalid scene update header.");
    const size = view.getUint32(4, true);
    if (size % 4 || size > buffer.byteLength - 8) throw new Error("Invalid scene update metadata length.");
    const header: Header = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(buffer, 8, size)));
    const { manifest } = header;
    validateManifestHeader(manifest);
    const prior = header.baseRevision === null ? undefined : this.previous;
    if (header.baseRevision !== null && (!Number.isSafeInteger(header.baseRevision)
      || prior?.manifest.revision !== header.baseRevision)) throw new Error("Scene update base revision is unavailable.");
    if (!Array.isArray(header.meshes) || !Array.isArray(header.assets) || header.meshes.length !== manifest.meshes.length || header.assets.length !== manifest.assets.length) throw new Error("Scene update references are incomplete.");
    if (header.foliageMassRefs !== undefined) {
      const refs = header.foliageMassRefs, batches = manifest.batches, previous = prior?.manifest.batches;
      if (!Array.isArray(refs) || !batches || !previous || refs.length > batches.length) throw new Error("Invalid retained canopy references.");
      for (const ref of refs) {
        if (!Array.isArray(ref) || ref.length !== 2 || !ref.every(Number.isSafeInteger)) throw new Error("Invalid retained canopy reference.");
        const batch = batches[ref[0]], old = previous[ref[1]];
        if (!batch?.foliage || !old?.foliage?.mass || old.id !== batch.id || batch.foliage.mass !== undefined)
          throw new Error("Retained canopy differs from its source batch.");
        batch.foliage.mass = old.foliage.mass;
      }
    }
    const spans = header.batchSpans;
    if (spans !== undefined && (!Array.isArray(spans) || spans.length !== batchChannels.length || !prior?.batches
      || !manifest.batchCounts || JSON.stringify(manifest.batchCounts) !== JSON.stringify(prior.manifest.batchCounts)
      || spans.some((span, i) => !Array.isArray(span) || span.length !== 2 || !span.every(Number.isSafeInteger)
        || span[0] < 0 || span[1] < 0 || span[0] + span[1] > prior.batches![batchChannels[i]].length))) {
      throw new Error("Invalid retained batch spans.");
    }
    const retainedBatches = spans ? spans.every(span => span[1] === 0) && buffer.byteLength === 8 + size
      : buffer.byteLength === 8 + size + batchByteLength(manifest.batchCounts);
    if (header.retainedScene === true && prior && manifest.success && prior.manifest.success
      && header.meshes.every((reference, index) => reference === index)
      && header.assets.every((reference, index) => reference === index)
      && retainedBatches && sameSceneMetadata(manifest, prior.manifest) && (spans || sameBatchBytes(view, 8 + size, prior))) {
      const retained = validatePacket({ ...prior, manifest, retainedFrom: prior.manifest.revision });
      this.previous = retained;
      return retained;
    }
    // A plant or light edit can arrive beside a fully detailed, untouched building.
    // Keep its contiguous CPU buffers when every range still occupies the same slot.
    const sameGeometry = prior && manifest.vertexCount === prior.manifest.vertexCount && manifest.indexCount === prior.manifest.indexCount
      && manifest.meshes.length === prior.manifest.meshes.length && manifest.meshes.every((mesh, i) => {
        const old = prior.manifest.meshes[i];
        return header.meshes[i] === i && old.id === mesh.id && old.vertexStart === mesh.vertexStart
          && old.vertexCount === mesh.vertexCount && old.indexStart === mesh.indexStart && old.indexCount === mesh.indexCount;
      });
    const sameImages = prior && manifest.assetByteCount === prior.manifest.assetByteCount && manifest.assets.length === prior.manifest.assets.length
      && manifest.assets.every((asset, i) => {
        const old = prior.manifest.assets[i];
        return header.assets[i] === i && old.mime === asset.mime && old.byteStart === asset.byteStart && old.byteLength === asset.byteLength;
      });
    const packet: ScenePacket = {
      manifest,
      positions: sameGeometry ? prior.positions : new Float32Array(manifest.vertexCount * 3),
      normals: sameGeometry ? prior.normals : new Float32Array(manifest.vertexCount * 3),
      uvs: sameGeometry ? prior.uvs : new Float32Array(manifest.vertexCount * 2),
      colors: sameGeometry ? prior.colors : new Float32Array(manifest.vertexCount * 3),
      indices: sameGeometry ? prior.indices : new Uint32Array(manifest.indexCount),
      assets: sameImages ? prior.assets : new Uint8Array(manifest.assetByteCount),
      assetIdentities: new Map(),
    };
    let cursor = 8 + size;
    /** Reads a bounded little-endian payload range into the reconstructed destination. */
    const read = (target: Float32Array | Uint32Array, start: number, count: number): void => {
      if (!Number.isSafeInteger(start) || !Number.isSafeInteger(count) || start < 0 || count < 0 || start + count > target.length || cursor + count * 4 > buffer.byteLength) throw new Error("Scene update range exceeds its payload.");
      copyPacketWords(view, cursor, target, start, count);
      cursor += count * 4;
    };
    if (!sameGeometry) for (const [index, mesh] of manifest.meshes.entries()) {
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
        if (!sameImages) packet.assets.set(prior!.assets.subarray(old.byteStart, old.byteStart + old.byteLength), asset.byteStart);
        const identity = prior!.assetIdentities?.get(old.id);
        if (identity) packet.assetIdentities!.set(asset.id, identity);
      }
    }
    if (spans) {
      const previous = prior!.batches!;
      const batches: BatchBuffers = { ...previous };
      batchChannels.forEach((name, index) => {
        const [start, count] = spans[index];
        if (!count) return;
        if (name === "indices") {
          batches.indices = previous.indices.slice();
          read(batches.indices, start, count);
        } else {
          batches[name] = previous[name].slice();
          read(batches[name], start, count);
        }
      });
      packet.batches = batches;
    } else if (manifest.batchCounts) {
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
