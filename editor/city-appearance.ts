/** Shade the AML compiler's city receivers through the same ItemSpec feature, layout and
 * MaterialSpec bake services as the standalone studio. Geometry, picking and collision
 * stay with the supplied WorldSpec packet. */
import {
  AuthoringCompiler,
  type AuthoringProject,
  type ItemReport,
  type StudioMaterial,
  type StudioModel,
} from "../browser/runtime/authoring.ts";
import { projectLighting } from "../browser/renderer/lighting.ts";
import { libraryAsset } from "../browser/runtime/library.ts";
import type { ScenePacket } from "../browser/model/packet.ts";
import type { SceneShading } from "../browser/renderer/shade.ts";
import type { ProjectLayouts } from "../browser/renderer/atlas.ts";
import type { FieldSource } from "../browser/renderer/scene-bake.ts";
import type { RustScene } from "../browser/renderer/scene.ts";
import catalog from "../crates/engine/src/worldspec/themes/kowloon/appearance/street-materials.json";
import reference from "../crates/engine/src/worldspec/themes/kowloon/appearance/reference-building.json";
import families from "../crates/engine/src/worldspec/themes/kowloon/appearance/building-families.json";

const raw = import.meta.glob<string>(
  [
    "../crates/engine/src/worldspec/themes/kowloon/items/*.item.json",
    "../crates/engine/src/worldspec/themes/kowloon/appearance/*.material.ts",
    "../crates/engine/src/materialspec/program/examples/kowloon-*.material.ts",
  ],
  { eager: true, query: "?raw", import: "default" },
);
const modules = Object.fromEntries(
  Object.entries(raw).map(([path, source]) => [path.split("/").pop()!, source]),
);
const textures = import.meta.glob<string>(
  "../crates/engine/src/materialspec/program/examples/textures/concrete-*.png",
  { eager: true, query: "?url", import: "default" },
);
const images = new Map(
  Object.entries(textures).map(([path, url]) => ["textures/" + path.split("/").pop(), url]),
);
async function image(name: string) {
  const url = images.get(name);
  if (!url) return libraryAsset(name);
  const response = await fetch(url);
  if (!response.ok) throw Error("Could not load " + name);
  return new Uint8Array(await response.arrayBuffer());
}
type CityNode = {
  id: string;
  kind: string;
  attributes: {
    position: number[];
    step?: number[];
    heading?: number;
    count?: number;
    length?: number;
    width?: number;
    floors?: number;
    depth?: number;
    seed?: number;
    model?: string;
    banner?: boolean;
  };
};
type Role = keyof typeof catalog.roles;
function roleOf(kind: string): kind is Role {
  return Object.hasOwn(catalog.roles, kind);
}
function modelsOf(packet: ScenePacket) {
  const models: StudioModel[] = [],
    owners = new Map<string, string>();
  for (const section of packet.manifest.architecture?.metadata ?? []) {
    if (section.kind !== "city") continue;
    for (const node of (section as unknown as { children: CityNode[] }).children) {
      if (!roleOf(node.kind)) continue;
      const a = node.attributes,
        count = a.count ?? 1,
        angle = ((a.heading ?? 0) * Math.PI) / 180;
      for (let i = 0; i < count; i++) {
        const id = "p" + models.length;
        owners.set(id, "city/" + node.id + (count > 1 ? "/" + (i + 1) : ""));
        const params: Record<string, unknown> = {};
        if (node.kind === "building") {
          if (a.model === "reference-corner") Object.assign(params, reference);
          else {
            const seed = a.seed ?? a.floors ?? 4, w = a.width ?? 9.8, d = a.depth ?? 7;
            Object.assign(params, families[seed % families.length], {
              seed, footprint: [[0, 0], [w, 0], [w, d], [0, d]],
            });
          }
        } else if (a.width !== undefined) params.width = a.width;
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
export class CityAppearance {
  private readonly compiler = new AuthoringCompiler();
  private readonly fields = new AuthoringCompiler({ keepWorker: true });
  private sequence = 0;

  async apply(scene: RustScene, packet: ScenePacket, current: () => boolean) {
    const revision = ++this.sequence;
    const { models, owners } = modelsOf(packet);
    const city = packet.manifest.architecture?.metadata?.some((section) => section.kind === "city");
    const apply = async (decorate?: Parameters<RustScene["apply"]>[3]) => {
      const applied = await scene.apply(packet, current, false, decorate);
      if (applied) {
        scene.setBackdrop(city ? "#d2ccc4" : "#d5d7ce");
        scene.setLighting(projectLighting(packet.manifest), packet.manifest.lighting);
      }
      return applied;
    };
    if (!models.length) {
      if (!city || !packet.manifest.batches?.some((batch) => batch.foliage?.material === "painted-bark"))
        return apply();
      // The bark program shades foliage batches directly; there are no item receivers.
      models.push({ id: "plants", kind: "aml", source: '<architecture version="1" id="plants"/>' });
    }
    const assets: Record<string, Uint8Array> = {};
    for (const name of ["lettering.png", "signboard.png", "grime-streaks.png"]) {
      const id = "superspec-material:" + name;
      assets[id] = await libraryAsset(id);
    }
    const shading: SceneShading = {
      materials: {},
      meshes: {},
      gravity: [0, 0, -1],
      domain: [1, 1],
    };
    const values: Record<string, Record<string, unknown>> = {},
      layers: Record<string, Record<string, unknown>[]> = {};
    const layout: ProjectLayouts = {
      gutter: 0,
      record: 0,
      receivers: [],
      records: new Float32Array(),
    };
    const buffers: Float32Array[] = [];
    const sources = new Map<string, { project: AuthoringProject; mesh: string }>();
    let offset = 0;
    // The studio admits sixteen model sources per layout. Each slice uses globally unique model IDs.
    for (let start = 0; start < models.length; start += 16) {
      if (!current()) return false;
      const slice = models.slice(start, start + 16),
        used = new Set(slice.flatMap((m) => Object.values(m.slots ?? {})));
      used.add("painted-bark");
      const materials = catalog.materials.filter((m) => used.has(m.id)) as unknown as StudioMaterial[];
      const project: AuthoringProject = {
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
      const result = await this.compiler.shadeProject(project, revision, true);
      if (!current()) return false;
      if (!result.layouts || !result.metadata)
        throw Error(
          result.error ??
            result.diagnostics?.[0]?.message ??
            String(result.metadata?.layoutError ?? "Missing city material layouts"),
        );
      const metadata = result.metadata;
      const meshes = metadata.shading as SceneShading["meshes"];
      const mapped = (id: string) => {
        const slash = id.indexOf("/");
        return owners.get(id.slice(0, slash)) + id.slice(slash);
      };
      for (const [id, material] of Object.entries(metadata.materials as SceneShading["materials"])) {
        const base = materials.find((m) => m.id === id) || materials.find((m) => id.startsWith(m.id + "."));
        if (!base) throw Error("Unknown city material " + id);
        const mapping = project.mappings?.[base.id];
        shading.materials[id] = mapping ? { ...material, mapping } : material;
        values[id] = base.parameters;
        const stack = (material as { layers?: { parameters?: Record<string, unknown> }[] }).layers;
        if (stack) layers[id] = stack.map((l) => l.parameters ?? {});
      }
      for (const item of Object.values((metadata.items ?? {}) as Record<string, ItemReport>)) {
        for (const part of item.parts)
          if (part.anchor)
            for (const mesh of part.meshes) if (meshes[mesh]) meshes[mesh].anchor = part.anchor;
      }
      for (const [id, spec] of Object.entries(meshes)) {
        const mesh = mapped(id);
        shading.meshes[mesh] = spec;
        sources.set(mesh, { project, mesh: id });
      }
      const l = result.layouts;
      layout.record = l.record;
      layout.gutter = l.gutter;
      for (const receiver of l.receivers)
        layout.receivers.push({
          ...receiver,
          id: mapped(receiver.id),
          offset: receiver.offset + offset,
        });
      buffers.push(l.records);
      offset += l.records.length;
    }
    layout.records = buffers.length === 1 ? buffers[0] : new Float32Array(offset);
    if (buffers.length > 1) {
      let at = 0;
      for (const b of buffers) {
        layout.records.set(b, at);
        at += b.length;
      }
    }
    const fields: FieldSource = async (requests) => {
      const found: NonNullable<Awaited<ReturnType<FieldSource>>> = new Map();
      const groups = new Map<AuthoringProject, typeof requests>();
      for (const request of requests) {
        const src = sources.get(request.mesh)!;
        const group = groups.get(src.project) ?? [];
        group.push({ ...request, mesh: src.mesh });
        groups.set(src.project, group);
      }
      for (const [project, group] of groups) {
        if (!current()) return null;
        const result = await this.fields.bakeProjectFields(
          project,
          group.map(({ mesh, names }) => ({ mesh, names })),
          revision,
          group.some((r) => r.planes.some((p) => p.length)),
        );
        if (!current()) return null;
        if (!result.projectFields) throw Error(result.error ?? "City material fields failed");
        for (const field of result.projectFields) {
          const slash = field.mesh.indexOf("/");
          found.set(owners.get(field.mesh.slice(0, slash)) + field.mesh.slice(slash), field);
        }
      }
      return found;
    };
    const decorate = await scene.prepareShading(
      shading,
      values,
      image,
      {
        layouts: layout,
        fields,
        // The asphalt recipe supplies the full road color. The plain road mesh carries a fallback tint.
        vertexTints: (id) => shading.meshes[id]?.material !== "asphalt",
        allowanceMiB: 6144,
      },
      layers,
    );
    return current() && apply(decorate);
  }
  close() {
    this.compiler.close();
    this.fields.close();
  }
}
