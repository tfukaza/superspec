import type { ScenePacket } from "../../browser/model/packet.ts";
import type { AuthoringProject, Parameter } from "../../browser/runtime/authoring.ts";

/** Compare with the last accepted inputs: a newer source or finish edit can supersede an unfinished resize. */
export function onlyItemShapeChanged(previous: AuthoringProject, next: AuthoringProject, parameters: Parameter[]): boolean {
  for (const key of ["entry", "seed", "resolution", "profile", "inspection", "mapping", "domain", "gravity", "edges"] as const)
    if (JSON.stringify(previous[key]) !== JSON.stringify(next[key])) return false;
  // Project snapshots share immutable asset buffers. Checking their references avoids reading image bytes on an edit.
  for (const key of ["modules", "assets"] as const) {
    if (Object.keys(previous[key]).length !== Object.keys(next[key]).length) return false;
    if (Object.entries(previous[key]).some(([name, value]) => next[key][name] !== value)) return false;
  }
  let resized = false;
  for (const name of new Set([...Object.keys(previous.parameters), ...Object.keys(next.parameters)])) {
    const declaration = parameters.find(parameter => parameter.name === name);
    if (JSON.stringify(previous.parameters[name] ?? declaration?.default) === JSON.stringify(next.parameters[name] ?? declaration?.default)) continue;
    if (declaration?.role !== "geometry") return false;
    resized = true;
  }
  return resized;
}

/**
 * Stretch the accepted finish with a resized item while its exact maps bake.
 * The atlas rotates triangle corners and splits vertices. Match each triangle
 * in winding order against the new shape, then take its exact positions and
 * normals. A topology change or a non-affine part uses the compiler's shape.
 * The resident atlas, texels and indices stay shared with the accepted packet.
 */
export async function itemShapePreview(shape: ScenePacket, accepted: ScenePacket, current: () => boolean): Promise<ScenePacket> {
  const before = accepted.manifest, after = shape.manifest;
  if (before.meshes.length !== after.meshes.length || before.batches?.length || after.batches?.length
    || before.meshTangents || after.meshTangents
    || accepted.positions.byteLength + accepted.normals.byteLength > 16 * 1024 * 1024) return shape;
  const positions = accepted.positions.slice(), normals = accepted.normals.slice();
  const meshes = [];
  let slice = performance.now();
  for (let m = 0; m < before.meshes.length; m++) {
    const old = before.meshes[m], next = after.meshes[m];
    if (old.id !== next.id || old.objectId !== next.objectId || old.visible !== next.visible || old.indexCount !== next.indexCount) return shape;
    if (old.material === next.material && old.vertexCount !== next.vertexCount) return shape;
    const span = (bounds: number[][], axis: number) => bounds[1][axis] - bounds[0][axis];
    const scale = [0, 1, 2].map(k => span(old.bounds, k) ? span(next.bounds, k) / span(old.bounds, k) : 1);
    const tolerance = 1e-5 * Math.max(1, ...[0, 1, 2].map(k => span(next.bounds, k)));
    for (let i = 0; i < old.indexCount; i += 3) {
      if (i % 1536 === 0 && performance.now() - slice > 4) {
        await new Promise<void>(resolve => setTimeout(resolve, 0));
        if (!current()) return shape;
        slice = performance.now();
      }
      const a = old.indexStart + i, b = next.indexStart + i;
      let rotation = 0;
      for (; rotation < 3; rotation++) {
        let matches = true;
        for (let j = 0; j < 3 && matches; j++) {
          const u = 3 * (old.vertexStart + accepted.indices[a + j]);
          const v = 3 * (next.vertexStart + shape.indices[b + (rotation + j) % 3]);
          for (let k = 0; k < 3; k++) {
            const projected = next.bounds[0][k] + (accepted.positions[u + k] - old.bounds[0][k]) * scale[k];
            if (Math.abs(projected - shape.positions[v + k]) > tolerance) { matches = false; break; }
          }
        }
        if (matches) break;
      }
      if (rotation === 3) return shape;
      for (let j = 0; j < 3; j++) {
        const u = 3 * (old.vertexStart + accepted.indices[a + j]);
        const v = 3 * (next.vertexStart + shape.indices[b + (rotation + j) % 3]);
        // Catalog/imported geometry already carries the right UVs and tints. If
        // they changed, prefer its current mapping to the previous packet's.
        if (old.material === next.material) {
          if (accepted.indices[a + j] !== shape.indices[b + (rotation + j) % 3]) return shape;
          const s = u / 3 * 2, t = v / 3 * 2;
          if (accepted.uvs[s] !== shape.uvs[t] || accepted.uvs[s + 1] !== shape.uvs[t + 1]) return shape;
          for (let k = 0; k < 3; k++) if (accepted.colors[u + k] !== shape.colors[v + k]) return shape;
        }
        for (let k = 0; k < 3; k++) { positions[u + k] = shape.positions[v + k]; normals[u + k] = shape.normals[v + k]; }
      }
    }
    meshes.push({ ...old, bounds: next.bounds, source: next.source });
  }
  return { ...accepted, positions, normals, manifest: { ...before, meshes, revision: after.revision } };
}
