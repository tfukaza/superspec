/** Shade the AML compiler's city receivers through the same ItemSpec feature, layout and
 * MaterialSpec bake services as the standalone studio. Geometry, picking and collision
 * stay with the supplied WorldSpec packet. */
import {
  type AuthoringProject,
  type AuthoringResult,
  type ItemReport,
  type StudioModel,
} from "../browser/runtime/authoring.ts";
import { imageIdentity } from "../browser/renderer/textures.ts";
import { projectLighting } from "../browser/renderer/lighting.ts";
import { libraryAsset } from "../browser/runtime/library.ts";
import type { ScenePacket } from "../browser/model/packet.ts";
import type { SceneShading } from "../browser/renderer/shade.ts";
import type { ProjectLayouts } from "../browser/renderer/atlas.ts";
import type { FieldSource } from "../browser/renderer/scene-bake.ts";
import type { RustScene } from "../browser/renderer/scene.ts";
import type { PlacementMove } from "../browser/renderer/placement-preview.ts";
import { authoringCompiler } from "./compiler-host.ts";
import { modelsOf, cityProject } from "./city-project.ts";
import { adjacentBuildingContext, allowsSeparateContactSolve, buildingContactGeometry, buildingMaterialFrame, movedBuildingFrame, needsWorldMaterialSolve, splitBuildingSlice, type MaterialSliceIdentity } from "./city-material-frame.ts";
const textures = import.meta.glob<string>(
  "../crates/engine/src/materialspec/program/examples/textures/concrete-*.png",
  { eager: true, query: "?url", import: "default" },
);
const images = new Map(
  Object.entries(textures).map(([path, url]) => ["textures/" + path.split("/").pop(), url]),
);
const recipeImages = new Map<string, Promise<Uint8Array>>();
function image(name: string): Promise<Uint8Array> {
  const url = images.get(name);
  if (!url) return libraryAsset(name);
  let pending = recipeImages.get(url);
  if (!pending) {
    pending = fetch(url).then(async response => {
      if (!response.ok) throw Error("Could not load " + name);
      return new Uint8Array(await response.arrayBuffer());
    }).catch(error => { recipeImages.delete(url); throw error; });
    recipeImages.set(url, pending);
  }
  return pending;
}

async function materialProjectKey(project: AuthoringProject, assetKey: string, contactKey = "") {
  const { assets: _shipped, ...inputs } = project;
  return assetKey + contactKey + await imageIdentity(new TextEncoder().encode(JSON.stringify(inputs)));
}

interface FootprintSource { owner: string; project: AuthoringProject; assetKey: string; index: number }

export class CityAppearance {
  private readonly compiler = authoringCompiler();
  private readonly fields = authoringCompiler({ keepWorker: true });
  private sequence = 0;
  private displayedSequence = -1;
  private fieldRevision = -1;
  private displayedMaterials?: { packet: ScenePacket; slices: MaterialSliceIdentity[]; footprint?: FootprintSource };
  private earlyLayout?: { index: number; key: Promise<string | undefined>; result: Promise<AuthoringResult | undefined> };
  private readonly reports = new Map<number, { key: string; result: AuthoringResult; bytes: number }>();
  private sharedSolves = new Map<number, string>();
  private reportBytes = 0;
  /** During a footprint edit the last admitted standalone building supplies the recipe.
   * Start its layout solve alongside WorldSpec. The actual packet must establish the
   * same complete project key (including contacts) before it can accept this result. */
  prepareFootprint(moves: PlacementMove[]) {
    const source = this.displayedMaterials?.footprint, move = moves[0];
    if (!source || moves.length !== 1 || !move.footprint || move.owners.length !== 1
      || move.owners[0] !== source.owner || move.translation.some(delta => delta !== 0)) return;
    const model = source.project.models![0];
    if (JSON.stringify(model.parameters?.footprint) !== JSON.stringify(move.footprint.from)) return;
    const footprint = move.footprint.to.map(point => [...point]);
    const project = { ...source.project, models: [{ ...model, parameters: { ...model.parameters, footprint } }] };
    this.earlyLayout = { index: source.index, key: materialProjectKey(project, source.assetKey).catch(() => undefined),
      result: this.compiler.shadeProject(project, this.sequence + 1, true, false, true).catch(() => undefined) };
  }

  // Authored replacement receivers are filtered after lookup on every revision.
  private async shade(project: AuthoringProject, key: string, revision: number, slice: number, current: () => boolean) {
    if (!current() || revision !== this.sequence) throw new DOMException("Superseded", "AbortError");
    const previous = this.reports.get(slice);
    const early = this.earlyLayout;
    if (early?.index === slice) this.earlyLayout = undefined;
    if (previous?.key === key) return previous.result;
    const prepared = early?.index === slice && await early.key === key ? await early.result : undefined;
    if (!current() || revision !== this.sequence) throw new DOMException("Superseded", "AbortError");
    const result = prepared?.layouts && prepared.metadata && !prepared.error ? prepared
      : await this.compiler.shadeProject(project, revision, true, false, true);
    const size = result.retainedBytes ?? result.responseBytes;
    const budget = 128 * 1024 * 1024;
    if (current() && revision === this.sequence && result.layouts && result.metadata && !result.error
      && size && this.reportBytes - (previous?.bytes ?? 0) + size <= budget) {
      this.reports.set(slice, { key, result, bytes: size });
      this.reportBytes += size - (previous?.bytes ?? 0);
    }
    return result;
  }

  async apply(scene: RustScene, packet: ScenePacket, current: () => boolean, fit = false, replaced?: ReadonlySet<string>) {
    const revision = ++this.sequence;
    const { models, owners } = modelsOf(packet);
    const city = packet.manifest.architecture?.metadata?.some((section) => section.kind === "city");
    let acceptedMaterials: typeof this.displayedMaterials;
    const apply = async (decorate?: Parameters<RustScene["apply"]>[3]) => {
      const applied = await scene.apply(packet, current, fit, decorate);
      if (applied) {
        this.displayedSequence = revision;
        this.fieldRevision = revision;
        this.displayedMaterials = acceptedMaterials;
        scene.setBackdrop(city ? "#d2ccc4" : "#d5d7ce");
        scene.setLighting(projectLighting(packet.manifest), packet.manifest.lighting, city);
      }
      return applied;
    };
    if (!models.length) {
      if (!city || !packet.manifest.batches?.some((batch) => batch.foliage?.material === "painted-bark")) {
        this.reports.clear();
        this.sharedSolves.clear();
        this.reportBytes = 0;
        return apply();
      }
      // The bark program shades foliage batches directly; there are no item receivers.
      models.push({ id: "plants", kind: "aml", source: '<architecture version="1" id="plants"/>' });
    }
    const assets: Record<string, Uint8Array> = {};
    for (const name of ["lettering.png", "signboard.png", "grime-streaks.png"]) {
      const id = "superspec-material:" + name;
      assets[id] = await libraryAsset(id);
    }
    const assetKey = JSON.stringify(await Promise.all(Object.entries(assets).map(async ([name, bytes]) => [name, await imageIdentity(bytes as Uint8Array<ArrayBuffer>)])));
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
    // The studio admits sixteen model sources per layout. Each slice uses globally unique model IDs. A shop house
    // takes about a gigabyte to build and describe, so a slice holds at most one: several in one project ran the
    // worker's Wasm memory out on a playable level.
    const slices: StudioModel[][] = [];
    const hasBuilding = models.some(model => model.entry === "kowloon-building.item.json");
    for (const model of models) {
      const last = slices.at(-1);
      const building = model.entry === "kowloon-building.item.json";
      // Reserve one model for a geometry-only contact companion when a building can move.
      if (!last || last.length >= (hasBuilding ? 15 : 16) || (building && last.some((m) => m.entry === "kowloon-building.item.json"))) slices.push([model]);
      else last.push(model);
    }
    if (!current() || revision !== this.sequence) return false;
    // Once a group of recipes asks for a scene plane or a feature with scene reach,
    // keep it in the complete solver through edits. This caches only a conservative fallback.
    const shared = this.sharedSolves;
    this.sharedSolves = new Map();
    const groups = slices.map((slice, i) => {
      const recipeGroup = JSON.stringify(slice.map(model => model.entry));
      if (shared.get(i) === recipeGroup) this.sharedSolves.set(i, recipeGroup);
      const parts = this.sharedSolves.has(i) ? [slice] : splitBuildingSlice(slice, owners, packet.manifest);
      return parts.map((models, j) => ({
        models, index: 3 * i + (parts.length === 1 ? 0 : j + 1),
      }));
    });
    const keep = new Set(groups.flat().map(part => part.index));
    for (const [index, report] of this.reports) {
      if (!keep.has(index)) {
        this.reportBytes -= report.bytes;
        this.reports.delete(index);
      }
    }
    const prepare = async (slice: StudioModel[], index: number, world: boolean) => {
      const frame = world ? undefined : buildingMaterialFrame(slice, owners, packet.manifest);
      const project = cityProject(frame ? [frame.model] : [...slice], packet, assets);
      const context = frame ?? (hasBuilding && !world ? adjacentBuildingContext(slice, owners, packet.manifest) : undefined);
      const contacts = context && buildingContactGeometry(context, packet);
      let contactKey = "";
      if (contacts) {
        const name = "receiver-contacts.glb";
        project.assets = { ...assets, [name]: contacts };
        project.models!.push({ id: "contact-context", kind: "glb", name });
        contactKey = await imageIdentity(contacts as Uint8Array<ArrayBuffer>);
      }
      const key = await materialProjectKey(project, assetKey, contactKey);
      const identity: MaterialSliceIdentity = { index, key, contactsCovered: !!context, owner: frame?.owner, origin: frame?.origin };
      return { project, frame, index, identity, optional: !!contacts || !!frame };
    };
    type Prepared = Awaited<ReturnType<typeof prepare>>;
    const plans = new Map<number, Prepared>();
    const previous = this.displayedMaterials;
    if (!fit && !replaced?.size && previous) {
      // Fingerprints of the displayed solve are tiny. They remain valid when its large
      // layout reports have left the bounded cache but their baked maps are still on screen.
      for (const [i, group] of groups.entries()) {
        for (const { models, index } of group) {
          if (!current()) return false;
          plans.set(index, await prepare(models, index, this.sharedSolves.has(i)));
        }
      }
      const identities = [...plans.values()].map(plan => plan.identity);
      const moved = movedBuildingFrame(previous.slices, identities);
      if (moved && current() && await scene.applyMaterialTranslation(packet, previous.packet, moved.owner, moved.from, moved.to, current)) {
        this.displayedMaterials = { ...previous, packet, slices: identities };
        this.displayedSequence = revision;
        return true;
      }
    }
    const describe = async (plan: Prepared) => {
      const { project, index, identity } = plan;
      const result = await this.shade(project, identity.key, revision, index, current);
      if (!result.layouts || !result.metadata)
        throw Error(
          result.error ??
            result.diagnostics?.[0]?.message ??
            String(result.metadata?.layoutError ?? "Missing city material layouts"),
        );
      return { ...plan, layouts: result.layouts, metadata: result.metadata };
    };
    const reports: Awaited<ReturnType<typeof describe>>[] = [];
    for (const [i, group] of groups.entries()) {
      const parts: typeof reports = [];
      const complete = this.sharedSolves.has(i);
      let optional = group.length > 1, useWorld = false;
      try {
        for (const part of group) {
          if (!current()) return false;
          const plan = plans.get(part.index) ?? await prepare(part.models, part.index, complete);
          optional ||= plan.optional;
          parts.push(await describe(plan));
        }
      } catch (error) {
        if (!current() || revision !== this.sequence) return false;
        if (complete || !optional) throw error;
        // A contact-only companion or translated atlas can exceed an importer allowance.
        // Retry the ordinary complete project and retain its source-linked diagnostic on failure.
        useWorld = true;
      }
      if (useWorld || needsWorldMaterialSolve(parts)) {
        // The authoritative program's external inputs need the other side, even at a distance.
        if (!current() || revision !== this.sequence) return false;
        if (!useWorld) this.sharedSolves.set(i, JSON.stringify(slices[i].map(model => model.entry)));
        for (const { index } of parts) {
          this.reportBytes -= this.reports.get(index)?.bytes ?? 0;
          this.reports.delete(index);
        }
        if (!current()) return false;
        reports.push(await describe(await prepare(slices[i], 3 * i, true)));
      } else reports.push(...parts);
    }
    if (!current()) return false;
    if (!replaced?.size && reports.some(r => r.frame)
      && reports.every(r => r.identity.contactsCovered && allowsSeparateContactSolve(r.metadata.materials, r.metadata.items))) {
      acceptedMaterials = { packet, slices: reports.map(r => r.identity) };
      const only = reports[0];
      if (reports.length === 1 && only.frame && only.project.models?.length === 1)
        acceptedMaterials.footprint = { owner: only.frame.owner, project: only.project, assetKey, index: only.index };
    }
    for (const { project, frame, layouts: l, metadata } of reports) {
      const materials = project.materials!;
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
      const anchors = new Map<string, NonNullable<ItemReport["parts"][number]["anchor"]>>();
      for (const item of Object.values((metadata.items ?? {}) as Record<string, ItemReport>)) {
        for (const part of item.parts)
          if (part.anchor)
            for (const mesh of part.meshes) anchors.set(mesh, part.anchor);
      }
      for (const [id, spec] of Object.entries(meshes)) {
        const mesh = mapped(id);
        if (replaced?.has(mesh)) continue;
        const anchor = anchors.get(id);
        shading.meshes[mesh] = { ...spec, ...(anchor ? { anchor } : {}), ...(frame ? { materialOrigin: frame.origin } : {}) };
        sources.set(mesh, { project, mesh: id });
      }
      layout.record = l.record;
      layout.gutter = l.gutter;
      for (const receiver of l.receivers)
        if (!replaced?.has(mapped(receiver.id))) layout.receivers.push({
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
      // A source-only edit can advance the document while this appearance continues to own its field jobs.
      const live = () => (this.displayedSequence === this.sequence && this.fieldRevision === revision)
        || (this.sequence === revision && current());
      const found: NonNullable<Awaited<ReturnType<FieldSource>>> = new Map();
      const groups = new Map<AuthoringProject, typeof requests>();
      for (const request of requests) {
        const src = sources.get(request.mesh)!;
        const group = groups.get(src.project) ?? [];
        group.push({ ...request, mesh: src.mesh });
        groups.set(src.project, group);
      }
      for (const [project, group] of groups) {
        if (!live()) return null;
        const result = await this.fields.bakeProjectFields(
          project,
          group.map(({ mesh, names }) => ({ mesh, names })),
          revision,
          group.some((r) => r.planes.some((p) => p.length)),
        );
        if (!live()) return null;
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
  retain(scene: RustScene, packet: ScenePacket): boolean {
    if (this.displayedSequence !== this.sequence || !scene.applyRetained(packet)) return false;
    if (this.displayedMaterials) this.displayedMaterials.packet = packet;
    return true;
  }
  /** The drawn revision keeps its maps and field jobs while a newer draft cancels
   * unfinished receiver descriptions. The compiler retains its chart and recipe caches. */
  cancelPendingLayouts(): void {
    this.earlyLayout = undefined;
    this.compiler.cancelPending();
  }
  close() {
    this.earlyLayout = undefined;
    this.sequence++;
    this.fieldRevision = -1;
    this.displayedMaterials = undefined;
    this.reports.clear();
    this.sharedSolves.clear();
    this.reportBytes = 0;
    this.compiler.close();
    this.fields.close();
  }
}
