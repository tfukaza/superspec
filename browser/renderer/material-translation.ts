import type { ScenePacket } from "../model/packet.ts";

export type MaterialOrigin = [number, number, number];

function equalValues(a: Float32Array | Uint32Array | Uint8Array, b: Float32Array | Uint32Array | Uint8Array): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  const left = a instanceof Float32Array ? new Uint32Array(a.buffer, a.byteOffset, a.length) : a;
  const right = b instanceof Float32Array ? new Uint32Array(b.buffer, b.byteOffset, b.length) : b;
  for (let i = 0; i < left.length; i++) if (left[i] !== right[i]) return false;
  return true;
}

/** The appearance project proves the material inputs. Independently check the packet that owns
 * visibility, colors, geometry and fallback assets before keeping a displayed building's GPU resources. */
export function rigidMaterialTranslation(a: ScenePacket, b: ScenePacket, owner: string, from: MaterialOrigin, to: MaterialOrigin): boolean {
  const old = a.manifest, next = b.manifest;
  if (old.batches?.length || next.batches?.length || !old.meshes.some(mesh => mesh.objectId === owner) || old.meshes.length !== next.meshes.length
    || old.id !== next.id || a.positions.length !== b.positions.length
    || !next.meshes.every((mesh, i) => {
      const prior = old.meshes[i];
      return mesh.id === prior.id && mesh.objectId === prior.objectId
        && mesh.visible === prior.visible && mesh.material === prior.material && mesh.worldSpecOutline === prior.worldSpecOutline
        && mesh.vertexStart === prior.vertexStart && mesh.vertexCount === prior.vertexCount
        && mesh.indexStart === prior.indexStart && mesh.indexCount === prior.indexCount;
    })) return false;
  for (const name of ["decals", "meshTangents", "materials", "assets", "lighting"] as const) {
    if (old[name] !== next[name] && JSON.stringify(old[name]) !== JSON.stringify(next[name])) return false;
  }
  for (const channel of ["normals", "uvs", "colors", "indices", "assets"] as const) {
    if (!equalValues(a[channel], b[channel])) return false;
  }
  const delta = to.map((v, i) => v - from[i]);
  const translated = delta.some(value => value !== 0);
  for (const mesh of next.meshes) {
    const end = (mesh.vertexStart + mesh.vertexCount) * 3;
    const moving = translated && mesh.objectId === owner;
    for (let i = mesh.vertexStart * 3; i < end; i++) {
      const before = a.positions[i], after = b.positions[i];
      if (!moving) {
        if (!Object.is(after, before)) return false;
        continue;
      }
      // The compiler rounds each final position independently to f32. The sum of both
      // rounding errors bounds the possible residual from translating the previous packet.
      if (Math.abs((after - before) - delta[i % 3]) > (Math.abs(before) + Math.abs(after)) * 2 ** -24 + 2 ** -149) return false;
    }
  }
  return true;
}
