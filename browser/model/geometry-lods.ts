import { MAX_GEOMETRY_LOD_GROUPS, MAX_GEOMETRY_LOD_LEVELS } from "./limits.ts";
export interface GeometryLodLevel { distance: number; meshes?: string[]; batches?: string[]; screenCoverage?: number }
export interface GeometryLodGroup { id: string; levels: GeometryLodLevel[] }
type Source = { geometryLods?: GeometryLodGroup[]; meshes: { id: string; visible: boolean }[]; batches?: { id: string; visible: boolean }[] };

/** Validate independently of any rendering policy or collision selection. */
export function validateGeometryLods(source: Source): void {
  const groups = source.geometryLods;
  if (groups === undefined) return;
  if (!Array.isArray(groups) || groups.length > MAX_GEOMETRY_LOD_GROUPS) throw Error("Invalid geometry LOD groups.");
  const meshes = new Map(source.meshes.map(mesh => [mesh.id, mesh.visible]));
  const batches = new Map((source.batches ?? []).map(batch => [batch.id, batch.visible]));
  const identities = new Set<string>(), members = new Set<string>();
  for (const group of groups) {
    if (!group || typeof group.id !== "string" || !group.id || identities.has(group.id)
      || !Array.isArray(group.levels) || group.levels.length < 2 || group.levels.length > MAX_GEOMETRY_LOD_LEVELS) throw Error("Invalid geometry LOD group identity or levels.");
    identities.add(group.id);
    const coverage = group.levels[0]?.screenCoverage !== undefined;
    for (const [i, level] of group.levels.entries()) {
      if (!level || !Number.isFinite(level.distance) || (i === 0 ? level.distance !== 0 : level.distance <= group.levels[i - 1].distance)
        || (level.screenCoverage !== undefined) !== coverage || coverage && (!Number.isFinite(level.screenCoverage)
          || level.screenCoverage! < 0 || level.screenCoverage! > 1 || i < group.levels.length - 1 && level.screenCoverage === 0
          || i > 0 && level.screenCoverage! >= group.levels[i - 1].screenCoverage!)) throw Error("Invalid geometry LOD selection thresholds.");
      const meshIds = level.meshes ?? [], batchIds = level.batches ?? [];
      if (!Array.isArray(meshIds) || !Array.isArray(batchIds) || !meshIds.length && !batchIds.length) throw Error("Empty geometry LOD level.");
      for (const [ids, known] of [[meshIds, meshes], [batchIds, batches]] as const) for (const id of ids) {
        if (typeof id !== "string" || known.get(id) !== true || members.has(id)) throw Error("Unknown, hidden or overlapping geometry LOD member.");
        members.add(id);
      }
    }
  }
}
