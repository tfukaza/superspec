import type { Manifest, SceneMesh, ScenePacket } from "../browser/model/packet.ts";
import type { StudioModel } from "../browser/runtime/authoring.ts";

type Bounds = [number[], number[]];
export type MaterialOrigin = [number, number, number];
interface ContactContext { bounds: Bounds; neighbors: SceneMesh[]; origin: MaterialOrigin }

export interface MaterialSliceIdentity {
  index: number; key: string; contactsCovered: boolean; owner?: string; origin?: MaterialOrigin;
}

/** Every scene material input, including the other owners' nearby contact triangles,
 * must match the displayed solve. A single horizontal frame translation may differ. */
export function movedBuildingFrame(previous: MaterialSliceIdentity[], next: MaterialSliceIdentity[]) {
  if (previous.length !== next.length) return;
  let moved: { owner: string; from: MaterialOrigin; to: MaterialOrigin } | undefined;
  let unchanged: typeof moved;
  for (const [i, after] of next.entries()) {
    const before = previous[i];
    if (!before.contactsCovered || !after.contactsCovered || before.index !== after.index || before.key !== after.key || before.owner !== after.owner) return;
    if (!before.origin && !after.origin) continue;
    if (!before.origin || !after.origin || !after.owner || before.origin[2] !== after.origin[2]) return;
    if (before.origin[0] === after.origin[0] && before.origin[1] === after.origin[1]) {
      unchanged ??= { owner: after.owner, from: before.origin, to: after.origin };
      continue;
    }
    if (moved) return;
    moved = { owner: after.owner, from: before.origin, to: after.origin };
  }
  return moved ?? unchanged;
}

/** All nonlocal texture planes (AO, shadows, sunlight and custom scene services) need
 * the original set of contributors. Compiled base.* planes in a layer come from this receiver. */
export function allowsSeparateContactSolve(materials: unknown, items?: unknown): boolean {
  type Inputs = { requiredInputs?: string[]; layers?: { metadata?: Inputs }[] };
  const local = (description: Inputs | undefined, layer = false): boolean =>
    !!description && Array.isArray(description.requiredInputs)
    && description.requiredInputs.every(input => !input.startsWith("plane:") || (layer && input.startsWith("plane:base.")))
    && (description.layers ?? []).every(entry => local(entry.metadata, true));
  // resolve_items emits sceneReachDropped (including zero) on the item report whenever
  // named features can reach other items beyond the triangles they touch.
  if (items && typeof items === "object" && Object.values(items).some(item =>
    item && typeof item === "object" && "sceneReachDropped" in item)) return false;
  return typeof materials === "object" && materials !== null && Object.values(materials).every(entry => local(entry));
}

/** A sole reanchored building still needs the world solver when its materials use
 * a scene plane or its features reach out to other items. */
export function needsWorldMaterialSolve(parts: readonly { frame?: unknown; optional?: boolean; metadata: { materials?: unknown; items?: unknown } }[]): boolean {
  return parts.some(part => (parts.length > 1 || part.frame || part.optional) && !allowsSeparateContactSolve(part.metadata.materials, part.metadata.items));
}

/** A building and its neighboring props can use separate material frames when each side
 * can supply the other's potential contact triangles. Nonlocal planes are checked after compilation. */
export function splitBuildingSlice(slice: StudioModel[], owners: Map<string, string>, manifest: Manifest): StudioModel[][] {
  if (slice.length < 2) return [slice];
  const building = slice.find(model => model.entry === "kowloon-building.item.json");
  if (!building) return [slice];
  const frame = buildingMaterialFrame([building], owners, manifest);
  if (!frame) return [slice];
  const rest = slice.filter(model => model !== building);
  return adjacentBuildingContext(rest, owners, manifest) ? [[building], rest] : [slice];
}

/** Keep the complete contact solve for a mixed slice. A building with its own slice can
 * use the same horizontal material coordinates as the published building prototype.
 * Preserve world elevation for ground-dependent wear. Collect neighboring contact
 * candidates, including native AML surfaces that are outside the ItemSpec slice. */
export function buildingMaterialFrame(slice: StudioModel[], owners: Map<string, string>, manifest: Manifest):
  (ContactContext & { model: StudioModel; owner: string }) | undefined {
  if (slice.length !== 1) return;
  const model = slice[0], transform = model.transform;
  if (model.entry !== "kowloon-building.item.json" || !transform) return;
  const owner = owners.get(model.id);
  if (!owner) return;
  const [x, y, z] = transform[3];
  const context = contactContext(new Set([owner]), manifest, [x, y, 0]);
  if (!context) return;
  return { ...context, owner, model: { ...model, transform: [...transform.slice(0, 3), [0, 0, z]] } };
}

/** Include all other slices' and native AML surfaces' nearby triangles in these models' world-space contact solve. */
export function adjacentBuildingContext(models: StudioModel[], owners: Map<string, string>, manifest: Manifest) {
  return contactContext(new Set(models.map(model => owners.get(model.id))), manifest, [0, 0, 0]);
}

function contactContext(owners: Set<string | undefined>, manifest: Manifest, origin: MaterialOrigin): ContactContext | undefined {
  const bounds: Bounds = [[Infinity, Infinity, Infinity], [-Infinity, -Infinity, -Infinity]];
  const neighbors: SceneMesh[] = [];
  for (const mesh of [...manifest.meshes, ...(manifest.batches ?? [])]) {
    if (!mesh.visible) continue;
    if (!owners.has(mesh.objectId)) {
      neighbors.push(mesh);
      continue;
    }
    // Other placements of a shared template can have different neighbors.
    if ("instanceCount" in mesh) return;
    for (let axis = 0; axis < 3; axis++) {
      bounds[0][axis] = Math.min(bounds[0][axis], mesh.bounds[0][axis]);
      bounds[1][axis] = Math.max(bounds[1][axis], mesh.bounds[1][axis]);
    }
  }
  if (!Number.isFinite(bounds[0][0])) return;
  // Two millimeters conservatively covers the native receiver's 1 mm contact test.
  const nearby = neighbors.filter(({ bounds: [low, high] }) => [0, 1, 2].every(axis =>
    high[axis] + 0.002 >= bounds[0][axis] && low[axis] - 0.002 <= bounds[1][axis]));
  // The contact import below reads ordinary, placed triangle buffers only. Retain the
  // existing full project path when input bounds include other kinds or very large geometry.
  if (nearby.some(mesh => "instanceCount" in mesh) || nearby.reduce((n, mesh) => n + mesh.indexCount, 0) > 300_000) return;
  return { bounds, neighbors: nearby, origin };
}

/** A geometry-only GLB for the existing native/Wasm contact resolver. Its triangles have
 * no material assignments and never enter the displayed packet or its collision. */
export function buildingContactGeometry(frame: ContactContext, packet: ScenePacket): Uint8Array | undefined {
  if (!frame.neighbors.length) return;
  const points: number[] = [];
  const min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
  const [low, high] = frame.bounds;
  const pos = packet.positions;
  for (const mesh of frame.neighbors) {
    for (let index = mesh.indexStart; index < mesh.indexStart + mesh.indexCount; index += 3) {
      const a = (mesh.vertexStart + packet.indices[index]) * 3;
      const b = (mesh.vertexStart + packet.indices[index + 1]) * 3;
      const c = (mesh.vertexStart + packet.indices[index + 2]) * 3;
      if ([0, 1, 2].some(i => Math.max(pos[a+i], pos[b+i], pos[c+i]) < low[i] - 0.002
        || Math.min(pos[a+i], pos[b+i], pos[c+i]) > high[i] + 0.002)) continue;
      for (const at of [a,b,c]) {
        // glTF is Y-up. Subtract only the horizontal material origin.
        const p = [Math.fround(pos[at] - frame.origin[0]), pos[at+2], Math.fround(frame.origin[1] - pos[at+1])];
        points.push(...p);
        for (let i = 0; i < 3; i++) { min[i] = Math.min(min[i], p[i]); max[i] = Math.max(max[i], p[i]); }
      }
    }
  }
  if (!points.length) return;
  const vertices = new Float32Array(points);
  const json = new TextEncoder().encode(JSON.stringify({
    asset: { version: "2.0" }, scene: 0, scenes: [{ nodes: [0] }], nodes: [{ mesh: 0 }],
    meshes: [{ primitives: [{ attributes: { POSITION: 0 } }] }],
    buffers: [{ byteLength: vertices.byteLength }], bufferViews: [{ buffer: 0, byteLength: vertices.byteLength }],
    accessors: [{ bufferView: 0, componentType: 5126, count: points.length / 3, type: "VEC3", min, max }],
  }));
  const jsonSize = (json.length + 3) & ~3;
  const bytes = new Uint8Array(28 + jsonSize + vertices.byteLength);
  const header = new DataView(bytes.buffer);
  header.setUint32(0, 0x46546c67, true); header.setUint32(4, 2, true); header.setUint32(8, bytes.length, true);
  header.setUint32(12, jsonSize, true); header.setUint32(16, 0x4e4f534a, true);
  bytes.set(json, 20); bytes.fill(32, 20 + json.length, 20 + jsonSize);
  header.setUint32(20 + jsonSize, vertices.byteLength, true); header.setUint32(24 + jsonSize, 0x004e4942, true);
  bytes.set(new Uint8Array(vertices.buffer), 28 + jsonSize);
  return bytes;
}
