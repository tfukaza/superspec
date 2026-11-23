import type { AuthoringProject, StudioMaterial, StudioModel } from "../browser/runtime/authoring.ts";
import type { ScenePacket } from "../browser/model/packet.ts";
import { projectLighting } from "../browser/renderer/lighting.ts";
import catalog from "../crates/engine/src/worldspec/themes/kowloon/materials/street-materials.json";
import reference from "../crates/engine/src/worldspec/themes/kowloon/materials/reference-building.json";
import families from "../crates/engine/src/worldspec/themes/kowloon/materials/building-families.json";

const raw = import.meta.glob<string>(
  [
    "../crates/engine/src/worldspec/themes/kowloon/items/*.item.json",
    "../crates/engine/src/worldspec/themes/kowloon/materials/*.material.ts",
    "../crates/engine/src/materialspec/program/examples/kowloon-*.material.ts",
  ],
  { eager: true, query: "?raw", import: "default" },
);
const modules = Object.fromEntries(
  Object.entries(raw).map(([path, source]) => [path.split("/").pop()!, source]),
);
type CityNode = {
  id: string;
  kind: string;
  children?: { attributes: { points: [number, number][] } }[];
  attributes: {
    position: number[];
    step?: number[];
    heading?: number;
    count?: number;
    length?: number;
    width?: number;
    footprint?: [number, number][];
    floors?: number;
    depth?: number;
    height?: number;
    seed?: number;
    model?: string;
    banner?: boolean;
    "ground-shops"?: boolean;
    pavement?: boolean;
    "front-trade"?: string;
    "graphics-detail"?: number;
    "variant-budget"?: number;
  };
};
type Role = keyof typeof catalog.roles;
function roleOf(kind: string): kind is Role {
  return Object.hasOwn(catalog.roles, kind);
}
export function modelsOf(packet: ScenePacket) {
  const models: StudioModel[] = [],
    owners = new Map<string, string>();
  // A repeated building design is delivered as batches of its first copy (crates/engine/src/worldspec/city.rs): later
  // copies that are wholly instanced own nothing in the packet, so they are left out of the shading project, and the
  // batches are shaded as their template, the first copy.
  const present = new Set([...packet.manifest.meshes.map(mesh => mesh.objectId),
    ...(packet.manifest.batches ?? []).filter(batch => batch.template).map(batch => batch.objectId)]);
  for (const section of packet.manifest.architecture?.metadata ?? []) {
    if (section.kind !== "city") continue;
    for (const node of (section as unknown as { children: CityNode[] }).children) {
      if (!roleOf(node.kind)) continue;
      const a = node.attributes,
        count = a.count ?? 1,
        angle = ((a.heading ?? 0) * Math.PI) / 180;
      for (let i = 0; i < count; i++) {
        const owner = "city/" + node.id + (count > 1 ? "/" + (i + 1) : "");
        if (!present.has(owner)) continue;
        const id = "p" + models.length;
        owners.set(id, owner);
        const params: Record<string, unknown> = {};
        if (node.kind === "building") {
          if (a.model === "reference-corner") Object.assign(params, reference);
          else {
            const seed = a.seed ?? a.floors ?? 4, w = a.width ?? 9.8, d = a.depth ?? 7;
            Object.assign(params, families[seed % families.length], {
              seed, footprint: a.footprint ?? [[0, 0], [w, 0], [w, d], [0, d]],
            });
            if (node.children?.length) params.courtyards = node.children.map(hole => hole.attributes.points);
            if (a["ground-shops"] !== undefined) params.groundShops = a["ground-shops"];
            if (a.pavement !== undefined) params.pavement = a.pavement;
            if (a["front-trade"] !== undefined)
              for (const [plan, value] of [["shopPlans", "open"], ["tradePlans", a["front-trade"]]] as const) {
                // The family rows are shared module data: copy before changing the front shop.
                const rows = structuredClone((params[plan] as string[][] | undefined) ?? [[]]);
                rows[0][0] = value;
                params[plan] = rows;
              }
          }
          // The shading project regenerates the building from these parameters, so it must draw the
          // level the compiled scene drew (crates/engine/src/worldspec/themes/kowloon/city.rs).
          if (a["graphics-detail"] !== undefined) params.graphicsDetail = a["graphics-detail"];
          // Likewise the variant budget: another budget picks other catalogue shapes.
          if (a["variant-budget"] !== undefined) params.variantBudget = a["variant-budget"];
        } else {
          for (const key of ["width", "depth", "height"] as const)
            if (a[key] !== undefined) params[key] = a[key];
        }
        for (const key of ["floors", "length", "banner"] as const)
          if (a[key] !== undefined) params[key] = a[key];
        models.push({
          id,
          kind: "item",
          entry: "kowloon-" + node.kind + ".item.json",
          parameters: params as StudioModel["parameters"],
          ...catalog.roles[node.kind],
          transform: [
            [Math.cos(angle), Math.sin(angle), 0],
            [-Math.sin(angle), Math.cos(angle), 0],
            [0, 0, 1],
            a.position.map((v, k) => v + i * (a.step?.[k] ?? 0)),
          ],
        });
      }
    }
  }
  return { models, owners };
}

export function cityProject(slice: StudioModel[], packet: ScenePacket, assets: AuthoringProject["assets"]): AuthoringProject {
  const used = new Set(slice.flatMap((m) => Object.values(m.slots ?? {})));
  used.add("painted-bark");
  const materials = catalog.materials.filter((m) => used.has(m.id)) as unknown as StudioMaterial[];
  return {
    schemaVersion: 2,
    kind: "material",
    entry: materials[0].entry,
    parameters: {},
    modules,
    models: slice,
    materials,
    seed: 7,
    lighting: projectLighting(packet.manifest),
    profile: "production",
    resolution: 2048,
    texelDensity: 160,
    mappings: catalog.mappings as AuthoringProject["mappings"],
    mapping: "world",
    domain: [1, 1],
    edges: [],
    gravity: [0, 0, -1],
    base: { kind: "swatch" },
    target: { enabled: false, scope: "mesh" },
    inspection: "final",
    assets,
  };
}
