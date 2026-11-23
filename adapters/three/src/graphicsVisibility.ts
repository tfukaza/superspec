import { GRAPHICS_PRESETS, type GraphicsPreset } from "./graphicsSettings.ts";

type Extras = { worldSpecMinGraphics?: unknown;
  worldSpecLod?: { distances?: unknown[]; presetDistances?: Record<string, unknown> };
  MSFT_screencoverage?: unknown[]; cityLod?: unknown };
type Node = { mesh?: number; children?: number[]; extras?: Extras;
  extensions?: { MSFT_lod?: { ids: number[] }; EXT_mesh_gpu_instancing?: unknown } };
type Mesh = { primitives: { extras?: Extras }[] };
type Document = { scenes?: { nodes?: number[]; extras?: Extras }[]; nodes?: Node[]; meshes?: Mesh[] };

/** Untagged visual entities appear at every preset. Higher tiers include lower-tier entities. */
export function graphicsAllows(minimum: unknown, preset: GraphicsPreset): boolean {
  if (minimum === undefined) return true;
  const rank = GRAPHICS_PRESETS.findIndex(tier => tier.id === minimum);
  if (rank < 0) throw new Error("Invalid WorldSpec minimum graphics preset: " + String(minimum));
  return GRAPHICS_PRESETS.findIndex(tier => tier.id === preset) >= rank;
}

/** Apply the same scene admission in the browser and when producing smaller
 * packs. Preserve glTF indices for callers that still own external records. */
export function applyGraphicsVisibility(json: Document, preset: GraphicsPreset) {
  const nodes = json.nodes ?? [], meshes = json.meshes ?? [];
  const allowed = nodes.map(node => graphicsAllows(node.extras?.worldSpecMinGraphics, preset));
  let omittedNodes = 0, omittedPrimitives = 0;
  for (const value of allowed) if (!value) omittedNodes++;
  for (const mesh of meshes) {
    const kept = mesh.primitives.filter(part => graphicsAllows(part.extras?.worldSpecMinGraphics, preset));
    omittedPrimitives += mesh.primitives.length - kept.length;
    mesh.primitives = kept;
  }
  // A chain whose near level this preset omits continues from its first
  // delivered alternate. That node takes the owner's place in the hierarchy
  // with its own transform, and becomes the tier's canonical near rendition.
  const promoted = new Map<number, number>();
  for (const [i, node] of nodes.entries()) {
    const chain = node.extensions?.MSFT_lod;
    if (!chain || allowed[i]) continue;
    const first = chain.ids.findIndex(id => allowed[id] !== false);
    if (first < 0) continue;
    const heir = nodes[chain.ids[first]], rest = chain.ids.slice(first + 1), record = node.extras?.worldSpecLod;
    const later = (levels: unknown[]) => levels.slice(first + 1);
    heir.extras ??= {};
    if (record) heir.extras.worldSpecLod = { ...record,
      ...(record.distances ? { distances: [0, ...later(record.distances).slice(1)] } : {}) };
    if (node.extras?.MSFT_screencoverage) heir.extras.MSFT_screencoverage = later(node.extras.MSFT_screencoverage);
    if (rest.length) (heir.extensions ??= {}).MSFT_lod = { ids: rest };
    delete node.extensions!.MSFT_lod;
    promoted.set(i, chain.ids[first]);
  }
  const placed = (ids: number[]) => ids.map(id => promoted.get(id) ?? id).filter(id => allowed[id] !== false);
  for (const [i, node] of nodes.entries()) {
    if (!allowed[i] || (node.mesh !== undefined && meshes[node.mesh]?.primitives.length === 0)) {
      delete node.mesh;
      if (node.extensions) delete node.extensions.EXT_mesh_gpu_instancing;
    }
    if (node.children) node.children = placed(node.children);
    const chain = node.extensions?.MSFT_lod;
    if (chain) {
      const keep = chain.ids.map(id => allowed[id] !== false);
      if (keep.includes(false)) {
        const filter = (levels: unknown[]) => levels.filter((_, level) => level === 0 || keep[level - 1]);
        if (node.extras?.worldSpecLod?.distances)
          node.extras.worldSpecLod.distances = filter(node.extras.worldSpecLod.distances);
        if (node.extras?.MSFT_screencoverage) node.extras.MSFT_screencoverage = filter(node.extras.MSFT_screencoverage);
        chain.ids = chain.ids.filter((_, index) => keep[index]);
        if (!chain.ids.length) delete node.extensions!.MSFT_lod;
      }
    }
    // An author can state the switch distances a preset uses for the levels it
    // keeps. Other presets' entries stay, so filtering a pack twice is stable.
    const record = allowed[i] ? node.extras?.worldSpecLod : undefined;
    const authored = record?.presetDistances && Object.hasOwn(record.presetDistances, preset)
      ? record.presetDistances[preset] : undefined;
    if (record && authored !== undefined) {
      const levels = (node.extensions?.MSFT_lod?.ids.length ?? 0) + 1;
      if (!Array.isArray(authored) || authored.length !== levels || authored[0] !== 0
          || authored.some((d, k) => typeof d !== "number" || !Number.isFinite(d) || k > 0 && d <= authored[k - 1]))
        throw new Error(`worldSpecLod presetDistances for ${preset} must start at zero and strictly increase, one value per delivered level`);
      record.distances = [...authored];
    }
  }
  for (const scene of json.scenes ?? []) {
    if (scene.nodes) scene.nodes = placed(scene.nodes);
    // The external city approximation addresses primitive positions in the
    // unfiltered mesh. A filtered full pack uses its remaining core geometry.
    if (omittedPrimitives && scene.extras) delete scene.extras.cityLod;
  }
  return { omittedNodes, omittedPrimitives };
}
