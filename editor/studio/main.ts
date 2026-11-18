import { MAX_COMPANION_BYTES, MAX_COMPANIONS, MAX_FILE_BYTES, MAX_PROJECT_FILE_BYTES, megabytes } from "../../browser/model/limits.ts";
import "./style.css";
import * as THREE from "three";
import { RustScene } from "../../browser/renderer/scene.ts";
import { contourFuzz } from "../../browser/renderer/contour-fuzz.ts";
import { densityField, densityPoints, densitySurface, sampleDensity, shadeNormals } from "../../browser/renderer/leaf-density.ts";
import { branchCardTexture, densityCards } from "../../browser/renderer/density-cards.ts";
import { twigStrips } from "../../browser/renderer/twig-strips.ts";
import { grassClumps } from "../../browser/renderer/grass-clumps.ts";
import { domeGeometry, leafNormalMaterial, leafNormalTexture, smoothGeometry } from "../../browser/renderer/leaf-normals.ts";
import { leafCardTextures } from "../../browser/renderer/foliage-plants.ts";
import { projectLighting, statesAmbientOcclusion, storedLighting, sunOf, type Lighting } from "../../browser/renderer/lighting.ts";
import { LightingPanel } from "./lighting.ts";
import type { MaterialShading, SceneShading } from "../../browser/renderer/shade.ts";
import { gpuBakeUnsupported, scaledDomain, swatchDomain, type FieldTexels, type GpuBake } from "../../browser/renderer/bake.ts";
import { libraryAsset } from "../../browser/runtime/library.ts";
import { decodePacket, type SceneMesh, type ScenePacket } from "../../browser/model/packet.ts";
import { AuthoringCompiler, OCCLUSION_PLANES, type OcclusionReport, type AuthoringDiagnostic, type AuthoringMetadata, type AuthoringRequest, type ItemReport, type ParameterValue, type StudioMaterial } from "../../browser/runtime/authoring.ts";
import { ItemPanel } from "./items.ts";
import { readAmlSelection } from "../open-aml.ts";
import { AmlLoader } from "./aml.ts";
import { Channels } from "./channels.ts";
import { mount } from "./layout.ts";
import { ParameterControls } from "./parameters.ts";
import { RecipeTools, type Patch } from "../../browser/runtime/recipe.ts";
import { RecipePanel } from "./recipe.ts";
import { TypeChecker } from "../../browser/runtime/typecheck.ts";
import { mergeReceivers, receivers } from "./packets.ts";
import { copyProject, download, mergeAssets, openProject, parseEnvironment, projectFile, type StudioProject } from "./project.ts";
import { initialProject, itemSource, kowloonYard, loadKowloonTextures, needsKowloonTextures } from "./starters.ts";

/** A model or material identity: letters, digits, '.', '_' or '-', at most 64 characters. */
function identity(name: string, taken: Iterable<string>): string {
  const base = name.replace(/\.(glb|aml)$/i, "").replace(/[^A-Za-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 48) || "model";
  const used = new Set(taken);
  let candidate = base;
  for (let n = 2; used.has(candidate); n++) candidate = `${base}-${n}`;
  return candidate;
}

/** A one-box model whose only job is to give the engine a mesh to attach a swatch material to when it compiles a shader. */
/** Pause after a source or seed edit before its GPU preview asks the engine for a shader (the CPU build waits 180 ms). */
const GPU_SOURCE_DEBOUNCE_MS = 40;
const gpuSwatchModel = `<architecture version="1" id="s" name="s"><material id="m" color="#888888"/><box id="b" width="1" depth="1" height="1" material="m"/></architecture>`;

/** Mean, spread and distinct values of a GPU preview's texels: evidence for tests that a bake is non-empty and varies. */
function gpuStats(texels: { baseColor: Uint8Array; normal: Uint8Array; height: Float32Array; properties: Uint8Array }): Record<string, number> {
  const channel = (data: Uint8Array | Float32Array, offset: number, step: number) => {
    let sum = 0, low = Infinity, high = -Infinity;
    const distinct = new Set<number>();
    for (let index = offset; index < data.length; index += step) {
      const value = data[index]; sum += value; low = Math.min(low, value); high = Math.max(high, value); distinct.add(value);
    }
    return { mean: sum / (data.length / step), low, high, distinct: distinct.size };
  };
  const color = channel(texels.baseColor, 0, 4), normal = channel(texels.normal, 0, 4), height = channel(texels.height, 0, 1);
  const roughness = channel(texels.properties, 1, 4), metalness = channel(texels.properties, 2, 4);
  const green = channel(texels.baseColor, 1, 4), blue = channel(texels.baseColor, 2, 4);
  return { colorMeanG: green.mean, colorMeanB: blue.mean, roughnessMean: roughness.mean, metalnessMean: metalness.mean, colorMean: color.mean, colorLow: color.low, colorHigh: color.high, colorDistinct: color.distinct,
    normalXLow: normal.low, normalXHigh: normal.high, normalXDistinct: normal.distinct,
    heightLow: height.low, heightHigh: height.high, heightDistinct: height.distinct };
}

/** Resolves a required static studio control with its intended DOM type. */
function element<T extends HTMLElement>(id: string): T { return document.getElementById(id) as T; }

/** Coordinates draft inputs and atomic accepted previews; compilation and rendering retain independent ownership. */
class Studio {
  private project: StudioProject;
  private accepted?: StudioProject;
  private selected?: string;
  private module: string;
  private revision = 0;
  private environmentValid = true;
  private timer?: ReturnType<typeof setTimeout>;
  private base?: ScenePacket;
  private baseMetadata?: AuthoringMetadata;
  private visible?: ScenePacket;
  private draftVisible = false;
  private metadata?: AuthoringMetadata;
  private readonly authoring = new AuthoringCompiler();
  /** The GPU preview lane's own worker (shader plans, field bakes): it never competes with, or cancels, the CPU build's. */
  private readonly lane = new AuthoringCompiler({ keepWorker: true });
  private readonly recipeTools = new RecipeTools();
  private readonly typeChecker = new TypeChecker();
  private recipe?: RecipePanel;
  private readonly aml = new AmlLoader();
  private readonly scene: RustScene;
  private readonly parameters: ParameterControls;
  private readonly channels: Channels;
  /** Controls for the project's lighting record; edits go to the viewport at once and to the project (`relight`). */
  private readonly lightingPanel: LightingPanel;
  private readonly code = element<HTMLTextAreaElement>("code");
  private readonly status = element<HTMLOutputElement>("status");
  /** Advances only when the edited document is replaced, not for recompiles of the same source. */
  private documentRevision = 0;
  /** The scene material whose entry, parameters and source the editor shows. */
  private activeMaterial?: string;
  /**
   * Scene projects preview by evaluating their materials in the viewport's shaders, so an edit
   * shows at once; textures are baked when a GLB is exported. `?preview=bake` previews bakes.
   */
  private readonly live = new URLSearchParams(location.search).get("preview") !== "bake";
  /**
   * The shaded scene is the first look and the look while a value is being changed; once an edit settles, each mesh is
   * baked on the GPU over its receiver atlas and shows ordinary textured maps (a preview of the bake, never saved or
   * exported). `?preview=shader` keeps the per-pixel look alone.
   */
  private readonly sceneBake = this.live && new URLSearchParams(location.search).get("preview") !== "shader";
  /** The project the scene's GPU bake asks CPU fields for: the one last previewed, parameters included. */
  private bakeProject?: StudioProject;
  /** The scene's items as the engine last reported them (`metadata.items`), by model identity. */
  private items: Record<string, ItemReport> = {};
  private itemPanel?: ItemPanel;
  /** The material each mesh shows, by mesh identity: the assignments' own meshes and the ones item slots bind. */
  private meshBindings: Record<string, string> = {};
  /**
   * An item control is being dragged (a slider or a handle in the viewport): the scene is shown per pixel alone, with
   * no bake, until the value is let go; the settled value is then baked (the roadmap's three evaluation paths).
   */
  private dragging = false;
  /** When the item edit being built began, and what it changed, for `data-item-edit`. */
  private itemEditStarted?: { at: number; model: string; parameter: string; value: ParameterValue; dragging: boolean };
  /** The project the viewport's shaders were last built for, with what the engine returned. */
  private shaded?: { key: string; metadata: AuthoringMetadata; packet: ScenePacket };
  /** The swatch shader the GPU preview last asked the engine for, by source, entry, seed and domain (not parameters). */
  private readonly gpuShadings = new Map<string, { shading: MaterialShading; unsupported: string | null; hybrid?: MaterialShading }>();
  /** The CPU-baked fields of the hybrid preview: what they were baked for (key) and which shader and size they fit (layout). */
  private gpuFields?: { key: string; layout: string; fields: Record<string, FieldTexels> };
  private editStarted = 0;
  private gpuTimer?: ReturnType<typeof setTimeout>;
  private gpuFrame = 0;
  /** Texture size of the CPU build the viewport shows (64 for a draft), which fixes the atlas layout a GPU preview must match. */
  private shownResolution?: number;
  /** The newest revision a GPU preview was tried for, and the newest whose CPU build reached the viewport. */
  private gpuAttempted = 0;
  private cpuRevision = 0;

  constructor(private readonly kind: StudioProject["kind"]) {
    this.project = initialProject(kind); this.module = this.project.entry;
    this.scene = new RustScene(element("viewport"), (mesh, instance) => this.selection(mesh, instance));
    // Diagnostics for tests that compare a GPU preview's maps with the CPU build's (`?gpuprobe`); nothing reads it otherwise.
    // Also the contour fuzz prototype and the leaf-card atlas, for previews that build test shapes in the scene.
    if (new URLSearchParams(location.search).has("gpuprobe")) Object.assign(window, { __scene: this.scene, __three: THREE, __contourFuzz: contourFuzz, __leafCards: leafCardTextures, __leafNormalMaterial: leafNormalMaterial, __domeGeometry: domeGeometry, __smoothGeometry: smoothGeometry, __leafNormalTexture: leafNormalTexture, __densityField: densityField, __densityPoints: densityPoints, __densitySurface: densitySurface, __sampleDensity: sampleDensity, __densityCards: densityCards, __twigStrips: twigStrips, __branchCardTexture: branchCardTexture, __shadeNormals: shadeNormals, __grassClumps: grassClumps });
    this.parameters = new ParameterControls(element("parameters"), (name, value) => {
      // A shaded scene takes a parameter as a uniform: nothing to wait for.
      this.project.parameters[name] = value; this.edit(this.liveScene(this.project));
    });
    this.channels = new Channels(element("channels"));
    this.lightingPanel = new LightingPanel(element("lighting-controls"), (lighting, statesOcclusion) => this.relight(lighting, statesOcclusion));
    if (kind === "material") this.itemPanel = new ItemPanel(element("item-list"), (item, parameter, value, dragging) => this.itemEdit(item, parameter, value, dragging));
    if (kind === "material") this.recipe = new RecipePanel(element("recipe-body"), this.recipeTools, {
      revision: () => this.documentRevision,
      entry: () => this.project.entry, module: () => this.module, modules: () => this.project.modules,
      source: () => this.code.value, apply: (patches, source) => this.applyPatches(patches, source),
      insert: text => this.insert(text), override: name => this.project.parameters[name],
    });
    this.showLighting(); this.bindControls(); this.showProject(); this.edit();
    window.addEventListener("pagehide", () => this.dispose(), { once: true });
  }

  /** Registers user actions once, leaving later parameter metadata to its own control owner. */
  private bindControls(): void {
    this.code.addEventListener("input", () => { this.project.modules[this.module] = this.code.value; this.edit(); });
    this.code.addEventListener("keydown", event => {
      if (event.key !== "Tab") return;
      event.preventDefault(); this.code.setRangeText("  ", this.code.selectionStart, this.code.selectionEnd, "end");
      this.code.dispatchEvent(new Event("input"));
    });
    element<HTMLSelectElement>("module").addEventListener("change", event => this.showModule((event.target as HTMLSelectElement).value));
    element("set-entry").addEventListener("click", () => {
      this.project.entry = this.module; this.project.parameters = {}; this.showModule(this.module); this.edit(true);
    });
    element("compile").addEventListener("click", () => this.edit(true));
    if (this.kind === "material") element("check-types").addEventListener("click", () => void this.checkTypes());
    element("save-project").addEventListener("click", () => { const file = projectFile(this.project); download(file.name, file.content); });
    element("save-source").addEventListener("click", () => download(this.module.split("/").at(-1)!, this.project.modules[this.module], "text/plain"));
    element("export-glb").addEventListener("click", () => void this.exportGlb());
    element<HTMLTextAreaElement>("environment").addEventListener("input", event => {
      try {
        const environment = parseEnvironment((event.target as HTMLTextAreaElement).value);
        this.environmentValid = true;
        Object.assign(this.project, environment); this.edit();
      } catch (error) { this.environmentValid = false; this.begin(); this.failure(error); }
    });
    this.files("open-project", files => this.openProject(files[0]));
    this.files("open-modules", files => this.openModules(files));
    this.files("open-modules-folder", files => this.openModules(files));
    this.files("open-glb", files => this.openGlb(files[0]));
    element<HTMLSelectElement>("starter").addEventListener("change", event => {
      const starter = (event.target as HTMLSelectElement).value;
      if (this.kind === "material" && needsKowloonTextures(starter)) { void this.openKowloonStarter(starter); return; }
      this.documentRevision++;
      if (this.kind === "material") this.project.entry = starter;
      else this.project.modules[this.project.entry] = itemSource(starter as "pipe" | "duct");
      this.project.parameters = {}; this.module = this.project.entry; this.showProject(); this.edit(true);
    });
    for (const id of ["seed", "resolution", "inspection"]) element(id).addEventListener("change", () => {
      this.project.seed = element<HTMLInputElement>("seed").valueAsNumber;
      this.project.resolution = Number(element<HTMLSelectElement>("resolution").value);
      this.project.inspection = element<HTMLSelectElement>("inspection").value; this.edit();
    });
    element<HTMLSelectElement>("mesh").addEventListener("change", event => this.scene.select((event.target as HTMLSelectElement).value));
    if (this.kind === "material") this.bindMaterialControls();
    else this.bindItemControls();
  }

  /** All file pickers permit corrected same-file reopening and route failures through retained-preview diagnostics. */
  private files(id: string, action: (files: File[]) => Promise<void>): void {
    const input = element<HTMLInputElement>(id);
    input.addEventListener("change", () => {
      const files = [...(input.files ?? [])]; input.value = "";
      if (!files.length) return;
      const pending = action(files);
      const revision = this.revision;
      void pending.catch(error => { if (revision === this.revision) this.failure(error); });
    });
  }

  /** Loads the concrete study's companion images before a Kowloon starter compiles. */
  private async openKowloonStarter(starter: string): Promise<void> {
    const revision = this.begin("Loading the Kowloon study images…");
    try {
      const textures = await loadKowloonTextures();
      if (revision !== this.revision) return;
      this.documentRevision++;
      const candidate = copyProject(this.project);
      candidate.assets = mergeAssets(candidate.assets, textures);
      if (starter === "kowloon-yard") {
        Object.assign(candidate, { schemaVersion: 2, mapping: "world", resolution: 128,
          models: kowloonYard.models.map(model => ({ ...model })), materials: structuredClone(kowloonYard.materials) });
        const first = candidate.materials![0];
        candidate.entry = first.entry; candidate.parameters = first.parameters;
        this.activeMaterial = first.id;
      } else {
        candidate.entry = starter; candidate.parameters = {};
        const active = this.active(candidate);
        if (active) { active.entry = starter; active.parameters = candidate.parameters; }
      }
      this.module = candidate.entry;
      await this.compileProgressive(candidate, revision, undefined, undefined, true);
    } catch (error) { if (revision === this.revision) this.failure(error); }
  }

  /** True when the project composes imported models with per-mesh materials. */
  private sceneMode(project = this.project): boolean { return this.kind === "material" && (project.models?.length ?? 0) > 0; }

  /** The scene material the editor shows, or undefined outside scene mode. */
  private active(project = this.project): StudioMaterial | undefined {
    if (!project.materials?.length) return undefined;
    return project.materials.find(material => material.id === this.activeMaterial) ?? project.materials[0];
  }

  /** The editor's entry and parameters belong to the active material. */
  private syncActive(): void {
    const active = this.active();
    if (!this.sceneMode() || !active) return;
    active.entry = this.project.entry; active.parameters = this.project.parameters; active.seed = this.project.seed;
  }

  /** Shows another material's entry, parameters and source in the editor. */
  private activate(id: string): void {
    this.syncActive();
    const material = this.project.materials?.find(item => item.id === id);
    if (!material) return;
    this.activeMaterial = id; this.documentRevision++;
    this.project.entry = material.entry; this.project.parameters = material.parameters; this.project.seed = material.seed;
    element<HTMLInputElement>("seed").value = String(material.seed);
    this.showModule(material.entry); this.showScene(); this.edit(true);
  }

  /** Adds GLB and AML files as models (companion images join the project's assets). */
  private async addModels(files: File[]): Promise<void> {
    const revision = this.begin("Adding models…");
    this.syncActive();
    const candidate = copyProject(this.project);
    const models = candidate.models ?? [];
    const companions: Record<string, Uint8Array> = {};
    if (files.length > MAX_COMPANIONS || files.reduce((sum, file) => sum + file.size, 0) > MAX_COMPANION_BYTES) throw new Error(`Model selection exceeds ${MAX_COMPANIONS} files or ${megabytes(MAX_COMPANION_BYTES)} in total.`);
    for (const file of files) {
      if (file.size > MAX_FILE_BYTES) throw new Error(`${file.name} exceeds the ${megabytes(MAX_FILE_BYTES)} file limit.`);
      const id = identity(file.name, models.map(model => model.id));
      if (/\.glb$/i.test(file.name)) {
        companions[file.name] = new Uint8Array(await file.arrayBuffer());
        models.push({ id, kind: "glb", name: file.name });
      } else if (/\.aml$/i.test(file.name)) {
        models.push({ id, kind: "aml", source: await file.text() });
      } else {
        companions[file.webkitRelativePath || file.name] = new Uint8Array(await file.arrayBuffer());
      }
      if (revision !== this.revision) return;
    }
    if (models.length > 16) throw new Error("A scene composes at most 16 models.");
    candidate.assets = mergeAssets(candidate.assets, companions);
    candidate.models = models; candidate.schemaVersion = 2;
    if (!candidate.materials?.length) {
      // The material being edited becomes the scene's first material, waiting to be applied.
      candidate.materials = [{ id: identity(candidate.entry.replace(/\.(ms|material\.ts)$/, ""), []), entry: candidate.entry,
        parameters: candidate.parameters, seed: candidate.seed, meshes: [] }];
      this.activeMaterial = candidate.materials[0].id;
    }
    await this.compile(candidate, revision, undefined, undefined, true, true);
  }

  /** Lists models and materials; the active material's row is pressed. */
  private showScene(): void {
    if (this.kind !== "material") return;
    const scene = this.sceneMode();
    element("scene-section").dataset.active = String(scene);
    for (const id of ["apply-material", "restore-material", "scope"]) element<HTMLButtonElement>(id).disabled = scene;
    const models = element("model-list"); models.replaceChildren();
    for (const model of this.project.models ?? []) {
      const row = document.createElement("li"); row.dataset.model = model.id;
      const label = document.createElement("span"); label.textContent = `${model.id} · ${model.kind.toUpperCase()}`;
      const remove = document.createElement("button"); remove.className = "quiet compact"; remove.textContent = "Remove";
      remove.setAttribute("aria-label", `Remove model ${model.id}`);
      remove.addEventListener("click", () => this.removeModel(model.id));
      row.append(label, remove); models.append(row);
    }
    const materials = element("material-list"); materials.replaceChildren();
    const active = this.active();
    for (const material of this.project.materials ?? []) {
      const row = document.createElement("li"); row.dataset.material = material.id;
      const choose = document.createElement("button"); choose.className = "quiet compact scene-choose";
      choose.setAttribute("aria-pressed", String(material === active));
      // The meshes the material lists itself, and those the scene's items bind to it through their slots.
      const count = new Set([...material.meshes, ...Object.keys(this.meshBindings).filter(mesh => this.meshBindings[mesh] === material.id)]).size;
      choose.textContent = `${material.id} · ${material.entry} · ${count} mesh${count === 1 ? "" : "es"}`;
      choose.addEventListener("click", () => this.activate(material.id));
      const remove = document.createElement("button"); remove.className = "quiet compact"; remove.textContent = "Remove";
      remove.setAttribute("aria-label", `Remove material ${material.id}`);
      remove.addEventListener("click", () => this.removeMaterial(material.id));
      row.append(choose, remove); materials.append(row);
    }
    for (const id of ["new-material", "assign-mesh", "unassign-mesh"]) element<HTMLButtonElement>(id).disabled = !scene;
  }

  private removeModel(id: string): void {
    this.syncActive();
    this.project.models = (this.project.models ?? []).filter(model => model.id !== id);
    for (const material of this.project.materials ?? []) material.meshes = material.meshes.filter(mesh => !mesh.startsWith(`${id}/`));
    if (!this.project.models.length) { delete this.project.models; delete this.project.materials; this.project.schemaVersion = 1; }
    this.showScene(); this.edit(true);
  }

  private removeMaterial(id: string): void {
    this.syncActive();
    this.project.materials = (this.project.materials ?? []).filter(material => material.id !== id);
    if (this.activeMaterial === id) {
      const next = this.project.materials[0];
      this.activeMaterial = next?.id;
      if (next) { this.project.entry = next.entry; this.project.parameters = next.parameters; this.showModule(next.entry); }
    }
    this.showScene(); this.edit(true);
  }

  /**
   * Sets one control of an item model and previews it. While the control is dragged the preview is the per-pixel look
   * alone; the value it is let go on is previewed again with baked maps, of which only the changed parts' are drawn.
   */
  private itemEdit(id: string, parameter: string, value: ParameterValue, dragging: boolean): void {
    const model = this.project.models?.find(candidate => candidate.id === id && candidate.kind === "item");
    if (!model) return;
    model.parameters = { ...model.parameters, [parameter]: value };
    this.itemEditStarted = { at: performance.now(), model: id, parameter, value, dragging };
    this.edit(true, dragging);
  }

  /** Draws what the scene's items report: their drag handles and, when asked, features, anchors and decal outlines. */
  private overlays(): void {
    if (this.kind !== "material") return;
    this.scene.showItemOverlays(Object.entries(this.items).map(([id, item]) => ({ id, features: item.features,
      anchors: item.parts.flatMap(part => part.anchor ? [part.anchor] : []), decals: item.decals, handles: item.handles })),
      { features: element<HTMLInputElement>("item-overlays").checked, handles: element<HTMLInputElement>("item-handles").checked },
      (item, handle, value, done) => this.itemEdit(item, handle.parameter, value, !done));
  }

  /** Takes what a build reports about the scene's items and bindings, and shows it. */
  private showItems(metadata: AuthoringMetadata | undefined, candidate: StudioProject): void {
    this.items = (metadata?.items ?? {}) as Record<string, ItemReport>;
    const bindings: Record<string, string> = {};
    for (const material of candidate.materials ?? []) for (const mesh of material.meshes) bindings[mesh] = material.id;
    // With items, the engine says which meshes their slots bind.
    this.meshBindings = { ...bindings, ...(metadata?.bindings as Record<string, string> | undefined) };
    this.itemPanel?.show(this.items);
    this.overlays();
    const viewport = element("viewport");
    viewport.dataset.bindings = JSON.stringify(this.meshBindings);
    if (Object.keys(this.items).length) viewport.dataset.items = JSON.stringify(Object.fromEntries(Object.entries(this.items).map(([id, item]) => [id,
      { generation: item.generation, rebuilt: item.rebuilt, parts: Object.fromEntries(item.parts.map(part => [part.id, part.key])),
        parameters: Object.fromEntries(item.parameters.map(parameter => [parameter.name, parameter.value])), derived: item.derived,
        features: item.features.length, decals: item.decals.map(decal => decal.mesh), handles: item.handles.map(handle => handle.id) }])));
    else delete viewport.dataset.items;
    const edit = this.itemEditStarted;
    if (edit) viewport.dataset.itemEdit = JSON.stringify({ model: edit.model, parameter: edit.parameter, value: edit.value, dragging: edit.dragging, ms: Math.round(performance.now() - edit.at) });
    this.itemEditStarted = undefined;
  }

  /** A material override is scoped to explicit selected receiver identities and can be restored atomically. */
  private bindMaterialControls(): void {
    for (const id of ["item-handles", "item-overlays"]) element(id).addEventListener("change", () => this.overlays());
    this.files("add-models", files => this.addModels(files));
    element("new-material").addEventListener("click", () => {
      this.syncActive();
      const materials = this.project.materials ??= [];
      const material: StudioMaterial = { id: identity("material", materials.map(item => item.id)), entry: this.project.entry,
        parameters: {}, seed: this.project.seed, meshes: [] };
      materials.push(material); this.activate(material.id);
    });
    element("assign-mesh").addEventListener("click", () => {
      const active = this.active();
      if (!active || !this.selected) return;
      this.syncActive();
      for (const material of this.project.materials ?? []) material.meshes = material.meshes.filter(mesh => mesh !== this.selected);
      active.meshes.push(this.selected); this.showScene(); this.edit(true);
    });
    element("unassign-mesh").addEventListener("click", () => {
      if (!this.selected) return;
      this.syncActive();
      for (const material of this.project.materials ?? []) material.meshes = material.meshes.filter(mesh => mesh !== this.selected);
      this.showScene(); this.edit(true);
    });
    this.files("open-aml", files => this.openAml(files));
    this.files("open-folder", files => this.openAml(files));
    element("swatch-edge").addEventListener("click", () => {
      this.project.edges = [{ start: [0, 0, this.project.domain[1]], end: [this.project.domain[0], 0, this.project.domain[1]] }];
      this.project.gravity = [0, 0, -1]; this.showEnvironment(); this.edit();
    });
    element("swatch").addEventListener("click", () => {
      this.base = undefined; this.baseMetadata = undefined; this.project.base = { kind: "swatch" };
      this.project.target = { scope: "mesh", enabled: false }; this.edit(true);
    });
    element("apply-material").addEventListener("click", () => {
      this.project.target.mesh = this.selected;
      this.project.target.binding = this.base?.manifest.meshes.find(mesh => mesh.id === this.selected)?.material ?? undefined;
      this.project.target.enabled = true; this.edit(true);
    });
    element("restore-material").addEventListener("click", () => {
      const valid = this.status.dataset.state === "ready";
      const revision = this.begin("Restoring original material…"); this.project.target.enabled = false;
      if (this.base) void this.scene.apply(this.base, () => revision === this.revision).then(applied => {
        if (applied && this.base) {
          this.visible = this.base; this.showMeshes(this.base); element("viewport").dataset.inputRevision = String(revision);
          if (valid) { this.accepted = copyProject(this.project); this.ready("Original material restored."); }
          else this.failure(new Error("Original bindings restored. Correct the current draft before compiling or exporting."));
        }
      }).catch(error => { if (revision === this.revision) this.failure(error); });
      else this.edit(true);
    });
    for (const id of ["scope", "mapping", "domain-u", "domain-v"]) element(id).addEventListener("change", () => {
      this.project.target.scope = element<HTMLSelectElement>("scope").value as "mesh" | "binding";
      this.project.mapping = element<HTMLSelectElement>("mapping").value as StudioProject["mapping"];
      this.project.domain = [element<HTMLInputElement>("domain-u").valueAsNumber, element<HTMLInputElement>("domain-v").valueAsNumber];
      this.edit();
    });
  }

  /** Item assignment edits the portable definition, so material choices survive reload and later placement. */
  private bindItemControls(): void {
    element("assign-material").addEventListener("click", () => {
      try {
        const source = JSON.parse(this.project.modules[this.project.entry]);
        const slot = element<HTMLSelectElement>("material-slot").value;
        const entry = element<HTMLSelectElement>("material-source").value;
        if (!slot || !entry) throw new Error("Select a material slot and source module.");
        source.slots ??= {}; source.slots[slot] = { entry, parameters: source.slots[slot]?.parameters ?? {}, seed: this.project.seed };
        this.project.modules[this.project.entry] = JSON.stringify(source, null, 2) + "\n";
        this.showModule(this.project.entry); this.edit(true);
      } catch (error) { this.failure(error); }
    });
    this.files("open-material", async files => {
      const revision = this.begin("Opening material project…");
      const material = await openProject(files[0], "material");
      if (revision !== this.revision) return;
      const candidate = copyProject(this.project);
      for (const [name, source] of Object.entries(material.modules)) {
        if (Object.hasOwn(candidate.modules, name) && candidate.modules[name] !== source) {
          throw new Error(`Module ${name} conflicts with the item project. Rename the material module and its imports before assigning it.`);
        }
        candidate.modules[name] = source;
      }
      candidate.assets = mergeAssets(candidate.assets, material.assets);
      const source = JSON.parse(candidate.modules[candidate.entry]);
      const slot = element<HTMLSelectElement>("material-slot").value || "surface";
      source.slots ??= {}; source.slots[slot] = { entry: material.entry, parameters: material.parameters, seed: material.seed };
      candidate.modules[candidate.entry] = JSON.stringify(source, null, 2) + "\n";
      await this.compile(candidate, revision, undefined, undefined, true);
    });
    for (const id of ["collision", "attachments"]) element(id).addEventListener("change", () => this.guides());
  }

  /** Starts a new input revision before asynchronous reads or Wasm work can publish. */
  private begin(message = "Compiling… Last valid preview remains visible."): number {
    clearTimeout(this.timer); clearTimeout(this.gpuTimer); cancelAnimationFrame(this.gpuFrame);
    this.authoring.cancelPending(); this.lane.cancelPending(); this.aml.cancel();
    this.status.dataset.state = "compiling"; this.status.textContent = message;
    element<HTMLButtonElement>("export-glb").disabled = true;
    return ++this.revision;
  }

  /** Debounces typing without allowing an older worker or texture load to replace the newest draft. */
  private edit(immediate = false, dragging = false): void {
    // Only an item control that is still moving previews without a bake; every other edit is a settled one.
    this.dragging = dragging;
    const revision = this.begin();
    void this.recipe?.refresh();
    if (!this.environmentValid) {
      this.failure(new Error("Correct the runoff edges and gravity draft before compiling or exporting."));
      return;
    }
    this.syncActive();
    const candidate = copyProject(this.project);
    this.editStarted = performance.now();
    if (!immediate && this.kind === "material" && candidate.resolution >= 128 && !this.liveScene(candidate)) {
      // A parameter edit (shader already known) needs no engine call or recompile: bake on the next frame, so a burst of
      // input events costs one bake. A source or seed edit asks the engine for a shader (milliseconds) after a short pause,
      // well before the CPU debounce.
      if (this.cachedShader(candidate)) this.gpuFrame = requestAnimationFrame(() => void this.gpuPreview(candidate, revision));
      else this.gpuTimer = setTimeout(() => void this.gpuPreview(candidate, revision), GPU_SOURCE_DEBOUNCE_MS);
    }
    this.timer = setTimeout(() => void this.compileProgressive(candidate, revision, this.base, this.baseMetadata).catch(error => {
      if (revision === this.revision) this.failure(error);
    }), immediate ? 0 : 180);
  }

  /**
   * Large textures first show a quick low-resolution draft, then refine to the requested size. The draft changes only
   * resolution and scheduling: same source, seed and parameters, never accepted for save or export. A newer edit cancels
   * the refinement, so dragging a control stays cheap and refines once the input settles.
   */
  private async compileProgressive(candidate: StudioProject, revision: number, base?: ScenePacket, baseMetadata?: AuthoringMetadata, opened = false, fit = opened): Promise<void> {
    if (this.kind === "material" && candidate.resolution >= 128 && !this.liveScene(candidate)) {
      // A GPU preview of a swatch comes first when the browser can bake it; any failure leaves the CPU draft to run alone.
      // It runs on the lane's own worker, in parallel with the CPU draft, which never waits for or cancels it.
      void this.gpuPreview(candidate, revision);
      if (revision !== this.revision) return;
      const draft = { ...candidate, resolution: 64 };
      if (!await this.compile(draft, revision, base, baseMetadata, false, fit, true)) return;
    }
    await this.compile(candidate, revision, base, baseMetadata, opened, fit);
  }

  /** What the engine's swatch shader depends on: source, entry, seed (a named seed's value) and domain, but never parameters. */
  private shaderKey(candidate: StudioProject): string {
    return JSON.stringify([candidate.entry, candidate.modules, candidate.seed, candidate.mapping, candidate.domain, candidate.gravity,
      Object.entries(candidate.assets).map(([name, bytes]) => [name, this.assetId(bytes)])]);
  }

  /** Names an asset's contents for the session: supplied bytes are replaced, never edited in place, so the array is the identity. */
  private assetId(bytes: Uint8Array): number {
    let id = this.assetIds.get(bytes);
    if (id === undefined) this.assetIds.set(bytes, id = ++this.lastAssetId);
    return id;
  }
  private readonly assetIds = new WeakMap<Uint8Array, number>();
  private lastAssetId = 0;

  /** Whether the last shader (or the reason it cannot preview) still describes `candidate`, so only uniforms change. */
  private cachedShader(candidate: StudioProject): boolean { return this.gpuShadings.has(this.shaderKey(candidate)); }

  /** Labels (or clears the label of) textures that are a GPU preview, so they are never taken for a build. */
  private previewLabel(shown: boolean, stage?: string, hybrid?: { fields: string[]; channels: string[]; fresh: boolean }): void {
    const label = document.getElementById("preview-label");
    if (!label) return;
    label.hidden = !shown;
    // Hybrid previews say which channels rest on fields the CPU baked; the others are computed on the GPU alone.
    const lane = hybrid ? ` · hybrid: ${hybrid.channels.join(", ").replace(/_/g, " ")} use CPU-baked ${hybrid.fields.join(", ")}${hybrid.fresh ? "" : " (refreshing)"}; other channels GPU only` : "";
    label.textContent = shown ? `GPU preview${stage === "low" ? " · quick" : ""} · approximate, not the saved bake${lane}` : "";
  }
  /**
   * Shows a swatch edit as GPU-baked textures on the mesh already displayed, ahead of the CPU draft. It follows the
   * draft rules and is weaker: never ready, saved or exportable; the CPU draft and full build replace it; its output
   * is labeled `gpu-preview` and is an approximation of the bake, never a comparison target. Parameter edits only
   * update uniforms and rebake; a new source, entry or seed asks the engine for a new shader. Programs the shader
   * cannot express, or a browser that cannot bake, leave the CPU path alone, with the reason in
   * `data-gpu-preview-fallback`.
   */
  private async gpuPreview(candidate: StudioProject, revision: number): Promise<void> {
    const viewport = element("viewport");
    // `?gpupreview=off` keeps the CPU path alone (for comparisons and debugging).
    if (new URLSearchParams(location.search).get("gpupreview") === "off") return;
    const fallback = (reason: string): void => { viewport.dataset.gpuPreviewFallback = reason; };
    // One attempt per revision: an edit that can skip the debounce tries first, the build flow only if it did not.
    if (this.gpuAttempted === revision) return;
    this.gpuAttempted = revision;
    delete viewport.dataset.gpuPreviewFallback;
    const meshes = this.visible?.manifest.meshes.filter(mesh => mesh.visible) ?? [];
    if (this.sceneMode(candidate) || candidate.inspection !== "final" || candidate.base.kind !== "swatch" || candidate.target.enabled
      || candidate.texelDensity !== undefined || meshes.length !== 1 || !this.shownResolution) return fallback("not a swatch preview");
    const unavailable = this.scene.gpuPreviewUnavailable();
    if (unavailable) return fallback(unavailable);
    // The displayed mesh fixes the layout: the atlas of the CPU build it came from (64 px for a draft).
    const resolution = this.shownResolution;
    const layout = swatchDomain(candidate.domain, resolution, candidate.mapping, candidate.gravity);
    if (!layout) return fallback("swatch layout is not modeled");
    const started = performance.now();
    try {
      const key = this.shaderKey(candidate);
      const image = async (name: string) => candidate.assets[name] ?? libraryAsset(name);
      let shadeMs = 0;
      let known = this.gpuShadings.get(key);
      if (known) { this.gpuShadings.delete(key); this.gpuShadings.set(key, known); }
      else {
        const shadeStarted = performance.now();
        const project = { ...copyProject(candidate), schemaVersion: 2 as const, models: [{ id: "s", kind: "aml" as const, source: gpuSwatchModel }],
          materials: [{ id: "gpu", entry: candidate.entry, parameters: candidate.parameters, seed: candidate.seed, meshes: ["s/b"] }] };
        const result = await this.lane.shadeProject(project, revision);
        if (revision !== this.revision) return;
        type Planned = MaterialShading & { requiredInputs?: string[]; plan?: { shader: MaterialShading["shader"]; seedValues: number[] }; planError?: string };
        const material = (result.metadata?.materials as Record<string, Planned> | undefined)?.gpu;
        if (result.error || result.diagnostics?.length || !material) return fallback("no shader for this program");
        // The swatch domain supplies its rim edges as receiver features, so programs that query them can preview.
        let unsupported = gpuBakeUnsupported(material, true);
        let hybrid: MaterialShading | undefined;
        if (unsupported && material.plan) {
          // The CPU-only parts (runoff) become fields the CPU bakes; the rest stays in the shader (lane H).
          const planned: MaterialShading = { ...material, shader: material.plan.shader, seedValues: material.plan.seedValues };
          const reason = gpuBakeUnsupported(planned, true);
          if (reason) unsupported = reason; else { hybrid = planned; unsupported = null; }
        } else if (unsupported && material.planError) unsupported = `${unsupported}; not partitionable: ${material.planError}`;
        known = { shading: material, unsupported, hybrid };
        this.gpuShadings.set(key, known);
        while (this.gpuShadings.size > 8) this.gpuShadings.delete(this.gpuShadings.keys().next().value!);
        shadeMs = performance.now() - shadeStarted;
      }
      if (known.unsupported) return fallback(known.unsupported);
      const use = known.hybrid ?? known.shading;
      // The contents of the images the program samples: the same shader with another image must bind again.
      const images = JSON.stringify(use.shader.images.map(entry => [entry.name, candidate.assets[entry.name] ? this.assetId(candidate.assets[entry.name]) : "library"]));
      const shading = this.scene.gpuPreviewReady(use, images) ? undefined : use;
      const probe = new URLSearchParams(location.search).has("gpuprobe");
      // Hybrid: the CPU bakes the fields only it can evaluate, once per layout; a change to what they read bakes them again
      // while the previous ones keep showing (stale while revalidating). The CPU build of this edit waits for this, so the
      // field bake (a subset of that build) never competes with it; a newer edit cancels it.
      const cpuFields = use.shader.cpuFields ?? [];
      const hybrid = cpuFields.length > 0;
      // The preview is drawn at the requested size. While the displayed mesh is a draft's, its layout is kept and its grid
      // refined by a whole factor (the same atlas with more texels, gutter included), so the textures land on that mesh
      // exactly. A hybrid preview stays on the displayed grid, the one the CPU bakes its fields on.
      const finer = hybrid ? 1 : Math.floor(candidate.resolution / resolution);
      const domain = finer > 1 ? scaledDomain(layout, 1 / finer) : layout;
      let fields: Record<string, FieldTexels> | undefined;
      let fieldsFresh = true, fieldMs = 0;
      const fieldKey = hybrid ? JSON.stringify([key, resolution, candidate.edges, candidate.gravity, cpuFields.map(field => [field.name,
        field.parameters.map(name => candidate.parameters[name] ?? use.parameters.find(p => p.name === name)?.default)])]) : "";
      if (hybrid) {
        const cached = this.gpuFields;
        if (cached?.key === fieldKey) fields = cached.fields;
        else {
          fieldsFresh = false;
          // The previous fields, if they describe the same layout, keep the preview going at once.
          if (cached?.layout === `${key}:${resolution}`) fields = cached.fields;
        }
      }
      const show = (shown: GpuBake, stage: "low" | "full", imagesMs: number, fresh: boolean): void => {
        // Like a draft: the viewport changes, readiness, saving and export wait for the CPU build.
        this.draftVisible = true;
        viewport.dataset.gpuPreviewRevision = String(revision);
        const cpuOutputs = use.shader.cpuOutputs ?? [];
        viewport.dataset.gpuPreview = JSON.stringify({ profile: shown.profile, key: shown.key, width: shown.width, height: shown.height, stage,
          shader: shading ? "bound" : "reused", units: shown.units,
          // Which channels rest on CPU-supplied fields; the rest are GPU only.
          lane: hybrid ? "hybrid" : "gpu",
          ...(hybrid ? { cpuFields: cpuFields.map(field => field.name), cpuChannels: cpuOutputs,
            gpuChannels: use.shader.outputs.filter(name => !cpuOutputs.includes(name)), fieldsFresh: fresh, fieldMs } : {}),
          timings: { ...shown.timings, images: imagesMs, shade: shadeMs, sinceStart: performance.now() - started, sinceEdit: performance.now() - this.editStarted } });
        if (probe) {
          viewport.dataset.gpuPreviewStats = JSON.stringify(gpuStats(this.scene.readBakedPreview(shown)));
          if (shown.live) viewport.dataset.gpuPreviewLive = JSON.stringify(this.scene.readBakedLive(shown));
        }
        this.previewLabel(true, stage, hybrid ?{ fields: cpuFields.map(f => f.name), channels: cpuOutputs, fresh } : undefined);
        this.status.textContent = `Refining texture… GPU preview shown (${stage === "low" ? "quick, " : ""}approximate).`;
      };
      const current = () => revision === this.revision && this.cpuRevision !== revision;
      // Without fields yet (nothing to show), a hybrid preview waits for them.
      if (!hybrid || fields) {
        await this.scene.bakePreview(meshes[0].id, domain, candidate.parameters, shading, image, probe, current,
          (shown, stage, imagesMs) => show(shown, stage, imagesMs, fieldsFresh), fields, images);
      }
      // A revision that is no longer current asks the lane for nothing: its request would cancel the current one's.
      if (hybrid && !fieldsFresh && current()) {
        const request: Omit<AuthoringRequest, "requestId"> = { kind: this.kind, entry: candidate.entry, source: candidate.modules[candidate.entry],
          modules: candidate.modules, parameters: candidate.parameters, seed: candidate.seed, resolution, assets: candidate.assets,
          mapping: candidate.mapping, domain: candidate.domain, edges: candidate.edges, gravity: candidate.gravity };
        const fieldStarted = performance.now();
        const result = await this.lane.bakeFields(request, cpuFields.map(field => field.name), revision);
        if (!current()) return;
        if (result.error || result.diagnostics?.length || !result.fields) return fallback(result.error ?? result.diagnostics?.[0]?.message ?? "the CPU field bake failed");
        const { width, height, fields: texels } = result.fields;
        if (width !== domain.width || height !== domain.height || texels.length !== cpuFields.length) return fallback("the CPU field grid does not match the preview's");
        const baked: Record<string, FieldTexels> = {};
        cpuFields.forEach((field, index) => {
          baked[field.uniform] = { id: `${fieldKey}#${field.name}`, width, height, rgba: texels[index] };
        });
        this.gpuFields = { key: fieldKey, layout: `${key}:${resolution}`, fields: baked };
        fieldMs = performance.now() - fieldStarted;
        await this.scene.bakePreview(meshes[0].id, domain, candidate.parameters, shading && !fields ? shading : undefined, image, probe, current,
          (shown, stage, imagesMs) => show(shown, stage, imagesMs, true), baked, images);
      }
    } catch (error) {
      if (revision !== this.revision) return;
      if (error instanceof DOMException && error.name === "AbortError") return;
      fallback(String(error instanceof Error ? error.message : error));
    }
  }
  /** Executes the shared authoring bridge and commits a complete scene only after renderer acceptance. */
  private async compile(candidate: StudioProject, revision: number, base?: ScenePacket, baseMetadata?: AuthoringMetadata, opened = false, fit = false, draft = false): Promise<boolean> {
    if (revision !== this.revision) return false;
    // Scene projects build whole through build_project: every model and every material at once.
    const scene = this.sceneMode(candidate);
    if (scene && this.liveScene(candidate)) return this.compileLive(candidate, revision, opened, fit);
    if (scene) base = undefined;
    const ids = scene ? new Set<string>() : this.targets(candidate, base);
    const request: AuthoringRequest = { requestId: revision, kind: this.kind, entry: candidate.entry,
      source: candidate.modules[candidate.entry], modules: candidate.modules, parameters: candidate.parameters,
      seed: candidate.seed, resolution: candidate.resolution, inspection: candidate.inspection === "final" ? undefined : candidate.inspection,
      assets: candidate.assets, mapping: candidate.mapping, domain: candidate.domain, edges: candidate.edges, gravity: candidate.gravity };
    if (this.kind === "material" && base && candidate.target.enabled) request.receivers = receivers(base, ids, baseMetadata?.receiverTransforms);
    const built = performance.now();
    const result = scene ? await this.authoring.previewProject(candidate, revision) : await this.authoring.compile(request);
    if (revision !== this.revision) return false;
    // A CPU-baked scene whose materials read the occluder service's planes had them baked for this build.
    if (result.occlusion) this.noteOcclusion(result.occlusion, performance.now() - built);
    if (scene && !result.error && !result.diagnostics?.length) this.readsShadow = result.occlusion?.planes.some(plane => plane.name === "shadow") ?? false;
    // What a scene build says about its items and their bindings, before the metadata is narrowed to the active material.
    const itemMetadata = result.metadata;
    if (scene && result.metadata) {
      // The editor's controls describe the active material; the scene carries every material.
      const materials = (result.metadata.materials ?? {}) as Record<string, AuthoringMetadata>;
      const activeId = candidate.materials?.find(material => material.id === this.activeMaterial)?.id ?? candidate.materials?.[0]?.id;
      result.metadata = { ...(activeId ? materials[activeId] : {}), materials, meshes: result.metadata.meshes };
    }
    if (result.error || !result.packet || result.diagnostics?.length) {
      // Overrides for parameters the new source no longer declares would block every build: drop them and retry once.
      // When they are the only problem, the build stays "compiling" instead of flashing a failure it repairs itself.
      const stale = (finding: AuthoringDiagnostic) => /Unknown public parameter/.test(finding.message);
      if (result.diagnostics?.some(stale)) {
        const retry = this.dropStaleParameters(candidate, revision).then(dropped => {
          if (dropped && revision === this.revision) this.edit(true);
          return dropped;
        });
        // An opened project is not current yet, so its failure is always reported.
        if (!opened && !result.error && result.diagnostics.every(stale) && await retry) return false;
      }
      if (revision !== this.revision) return false;
      this.diagnostics(result.diagnostics ?? [], candidate); void this.recipe?.refresh();
      throw new Error(result.error || "Source could not be compiled.");
    }
    const artifact = decodePacket(result.packet);
    if (!artifact.manifest.success) throw new Error(artifact.manifest.diagnostics.map(finding => finding.message).join("\n"));
    let packet = artifact;
    if (this.kind === "material" && base) packet = candidate.target.enabled ? mergeReceivers(base, artifact, ids) : base;
    if (!await this.scene.apply(packet, () => revision === this.revision, fit)) return false;
    this.shownResolution = candidate.resolution;
    this.cpuRevision = revision;
    delete element("viewport").dataset.gpuPreview; this.previewLabel(false);
    if (draft) {
      this.draftVisible = true;
      // A draft only updates the viewport; readiness, saving, export and inspection wait for the full-size build.
      element("viewport").dataset.draftRevision = String(revision);
      this.status.textContent = "Refining texture… Draft shown.";
      return true;
    }
    if (opened) {
      this.project = candidate; this.base = base; this.baseMetadata = baseMetadata;
      this.showLighting();
      this.module = candidate.entry;
      if (fit && !scene) this.selected = candidate.target.mesh;
      if (scene && !candidate.materials?.some(material => material.id === this.activeMaterial)) this.activeMaterial = candidate.materials?.[0]?.id;
      const active = this.active();
      if (active) { this.project.entry = active.entry; this.project.parameters = active.parameters; this.module = active.entry; }
      this.showProject();
    }
    this.accepted = copyProject(candidate);
    this.draftVisible = false;
    this.visible = packet; this.metadata = result.metadata;
    // Which material each mesh shows, by material identity, for inspection and tests; and the scene's items.
    if (scene) this.showItems(itemMetadata, candidate);
    else { this.items = {}; this.meshBindings = {}; this.itemPanel?.show({}); this.overlays(); delete element("viewport").dataset.bindings; delete element("viewport").dataset.items; }
    this.diagnostics([]); this.parameters.show(result.metadata?.parameters ?? [], candidate.parameters);
    this.showInspection(result.metadata); this.channels.show(artifact); this.showMeshes(packet); void this.recipe?.refresh();
    element("scene-title").textContent = packet.manifest.name || candidate.entry;
    element("model-details").textContent = `${packet.manifest.meshes.filter(mesh => mesh.visible).length} meshes · ${packet.manifest.vertexCount.toLocaleString()} vertices · meters, Z up`;
    element("viewport").dataset.inputRevision = String(revision);
    this.guides(); this.ready(`Revision ${revision} ready${base && !candidate.target.enabled ? " · Original bindings; select a surface and apply." : ""}`);
    return true;
  }

  /**
   * Lights the viewport by the project's lighting record (the shipped look without one) and shows it in the Lighting
   * panel. The record was admitted when the project was opened.
   */
  private showLighting(): void {
    const stated = this.project.lighting, lighting = projectLighting(this.project);
    this.scene.setLighting(lighting, stated);
    this.shadowSun = this.sunKey();
    this.lightingPanel.show(lighting, statesAmbientOcclusion(stated));
    this.noteLighting();
  }

  /** Says in the Lighting panel what stands between the record and the picture: page parameters, and a renderer's own default. */
  private noteLighting(): void {
    // Page parameters stay on top of the record for the session; the panel edits the record itself.
    const overrides = [...new URLSearchParams(location.search).keys()].filter(name => /^(shadow|ao)/.test(name));
    const notes = overrides.length ? [`This page's parameters (${overrides.join(", ")}) override the record in the viewport; they are not saved.`] : [];
    const occlusion = this.scene.ambientOcclusionDefault;
    if (occlusion) notes.push(`Ambient occlusion is ${occlusion}.`);
    this.lightingPanel.viewer(notes.join(" "), occlusion !== null);
  }

  /**
   * Takes an edit of the Lighting panel: the viewport is lit by it at once (no material is rebuilt and nothing is
   * compiled by the engine) and the project stores it, so Save project and Download GLB carry it. The shipped look is
   * stored as no record at all.
   */
  private relight(lighting: Lighting, statesOcclusion: boolean): void {
    const stored = storedLighting(lighting, statesOcclusion);
    if (stored) this.project.lighting = stored; else delete this.project.lighting;
    this.scene.setLighting(lighting, stored);
    this.noteLighting();
    // A sun that moved (or stopped casting shadows) changes the occluder service's `shadow` plane and nothing else:
    // the materials that read it are baked again once the control rests, the others are left alone.
    const sun = this.sunKey();
    if (sun === this.shadowSun) return;
    this.shadowSun = sun;
    clearTimeout(this.shadowTimer);
    this.shadowTimer = setTimeout(() => {
      // A CPU-baked scene builds again when a material read `shadow` in its last build: the worker bakes the new
      // `shadow` planes and finds the others by key, and only the maps that read `shadow` are baked again.
      if (this.sceneMode(this.project) && !this.liveScene(this.project)) { if (this.readsShadow) this.edit(true); }
      else this.scene.planesChanged(["shadow"]);
    }, 200);
  }
  /** Whether the last CPU build of the scene bound `shadow` planes (see `noteOcclusion`). */
  private readsShadow = false;
  /** The sun the scene's `shadow` planes were last asked for (`sunKey`). */
  private shadowSun?: string;
  private shadowTimer?: ReturnType<typeof setTimeout>;

  /** Whether a project previews through shaders: a scene project showing its final channels. */
  private liveScene(candidate: StudioProject): boolean {
    return this.live && this.kind === "material" && this.sceneMode(candidate) && candidate.inspection === "final";
  }

  /**
   * Previews a scene project without baking: the engine composes the models and compiles each
   * material to a shader, and the viewport evaluates them per pixel. When only parameters
   * changed since the last preview, their uniforms are updated and nothing is rebuilt.
   */
  private async compileLive(candidate: StudioProject, revision: number, opened: boolean, fit: boolean): Promise<boolean> {
    const values = Object.fromEntries((candidate.materials ?? []).map(material => [material.id, material.parameters]));
    // The values of each layered slot's layers: like a material's own, they reach the GPU bake through uniforms.
    const layerValues = Object.fromEntries((candidate.materials ?? []).filter(material => material.layers?.length)
      .map(material => [material.id, material.layers!.map(layer => layer.parameters as Record<string, unknown>)]));
    // While an item control is dragged the scene is shown per pixel alone; its settled value is baked.
    const baking = this.sceneBake && !this.dragging;
    const key = JSON.stringify([candidate.modules, candidate.models, candidate.batches, candidate.mapping, candidate.mappings, candidate.anchors, candidate.gravity, candidate.domain,
      (candidate.materials ?? []).map(({ id, entry, seed, meshes, layers }) => [id, entry, seed, meshes, layers?.map(layer => [layer.entry, layer.seed, layer.follow])]),
      Object.entries(candidate.assets).map(([name, bytes]) => [name, bytes.length]),
      // Baked maps follow the receivers' atlases, which these size.
      baking ? [candidate.resolution, candidate.texelDensity, candidate.profile] : null]);
    let packet: ScenePacket, metadata: AuthoringMetadata;
    // Before the uniforms change: a bake they start asks for CPU fields of this project.
    const previous = this.bakeProject;
    this.bakeProject = candidate;
    if (this.shaded?.key === key && Object.entries(values).every(([id, parameters]) => this.scene.setShadingParameters(id, parameters, layerValues[id]))) {
      ({ packet, metadata } = this.shaded);
    } else {
      this.bakeProject = previous;
      const result = await this.authoring.shadeProject(candidate, revision, baking);
      if (revision !== this.revision) return false;
      if (result.error || !result.packet || !result.metadata || result.diagnostics?.length) {
        this.diagnostics(result.diagnostics ?? [], candidate); void this.recipe?.refresh();
        throw new Error(result.error || "Source could not be compiled.");
      }
      packet = decodePacket(result.packet); metadata = result.metadata;
      const shading = { materials: metadata.materials, meshes: metadata.shading, gravity: metadata.gravity, domain: metadata.domain } as unknown as SceneShading;
      this.mapShading(shading, metadata, candidate);
      // With the receivers' atlas layouts, GPU-baked maps follow the first look; without them the scene stays per pixel.
      const bake = baking && result.layouts ? { layouts: result.layouts, fields: (requests: { mesh: string; names: string[]; planes: string[][] }[]) => this.sceneFields(requests) } : undefined;
      if (baking && !bake) element("viewport").dataset.sceneBakeFallback = String(metadata.layoutError ?? "the engine returned no receiver layouts");
      else delete element("viewport").dataset.sceneBakeFallback;
      const decorate = await this.scene.prepareShading(shading, values, async name => candidate.assets[name] ?? libraryAsset(name), bake, layerValues);
      if (revision !== this.revision) return false;
      this.bakeProject = candidate;
      if (!await this.scene.apply(packet, () => revision === this.revision, fit, decorate)) { this.bakeProject = previous; return false; }
      this.shownResolution = undefined;
      this.shaded = { key, metadata, packet };
    }
    const materials = (metadata.materials ?? {}) as Record<string, AuthoringMetadata>;
    if (opened) {
      this.project = candidate; this.base = undefined; this.baseMetadata = undefined;
      this.showLighting();
      this.module = candidate.entry;
      if (!candidate.materials?.some(material => material.id === this.activeMaterial)) this.activeMaterial = candidate.materials?.[0]?.id;
      const active = this.active();
      if (active) { this.project.entry = active.entry; this.project.parameters = active.parameters; this.module = active.entry; }
      this.showProject();
    }
    const activeId = candidate.materials?.find(material => material.id === this.activeMaterial)?.id ?? candidate.materials?.[0]?.id;
    const shown: AuthoringMetadata = { ...(activeId ? materials[activeId] : {}), materials, meshes: metadata.meshes };
    element("viewport").dataset.preview = baking && !element("viewport").dataset.sceneBakeFallback ? "gpu-bake" : "shader";
    this.accepted = copyProject(candidate);
    this.draftVisible = false;
    this.visible = packet; this.metadata = shown;
    this.showItems(metadata, candidate);
    this.diagnostics([]); this.parameters.show(shown.parameters ?? [], candidate.parameters);
    this.showInspection(shown); this.channels.show(packet); this.showMeshes(packet); void this.recipe?.refresh();
    element("scene-title").textContent = packet.manifest.name || candidate.entry;
    element("model-details").textContent = `${packet.manifest.meshes.filter(mesh => mesh.visible).length} meshes · ${packet.manifest.vertexCount.toLocaleString()} vertices · meters, Z up`;
    element("viewport").dataset.inputRevision = String(revision);
    // Weathering layers are composed in the GPU bake; the per-pixel look (first look, drags, `?preview=shader`) shows a layered slot's base alone.
    const gpuBaked = element("viewport").dataset.preview === "gpu-bake";
    const layered = Object.values(materials).some(material => (material as { layers?: unknown[] }).layers?.length);
    this.guides(); this.ready(`Revision ${revision} ready · ${gpuBaked ? "GPU-baked preview" : "shader preview"}${layered ? ` · weathering layers ${gpuBaked ? "composed in the bake (per pixel: base only)" : "not shown per pixel (base only)"}` : ""}`);
    return true;
  }

  /**
   * Gives a shaded scene what the engine's description leaves to the project: each material's own mapping
   * (`mappings`) and, for a material mapped `anchored`, each receiver's pattern anchor: the one the project states for
   * the mesh (`anchors`), else its item part's (`Session::resolve_items` binds them by the same rule).
   */
  private mapShading(shading: SceneShading, metadata: AuthoringMetadata, candidate: StudioProject): void {
    const mappings = candidate.mappings ?? {};
    for (const [id, mapping] of Object.entries(mappings)) if (shading.materials[id]) shading.materials[id] = { ...shading.materials[id], mapping };
    if (!Object.values(mappings).includes("anchored")) return;
    const anchors = new Map<string, { origin: number[]; u: number[]; v: number[] }>();
    for (const item of Object.values((metadata.items ?? {}) as Record<string, ItemReport>)) {
      for (const part of item.parts) if (part.anchor) for (const mesh of part.meshes) anchors.set(mesh, part.anchor);
    }
    for (const [mesh, anchor] of Object.entries(candidate.anchors ?? {})) anchors.set(mesh, anchor);
    for (const [id, mesh] of Object.entries(shading.meshes)) if (anchors.has(id)) mesh.anchor = anchors.get(id);
  }

  /** What the occluder service's `shadow` plane reads of the lighting record: the direction of a sun that casts shadows. */
  private sunKey(): string {
    const sun = sunOf(projectLighting(this.project));
    return JSON.stringify(sun?.castsShadow ? sun.direction : null);
  }

  /**
   * CPU-baked fields for the scene's GPU bake: the parts of hybrid programs only the CPU evaluates (runoff, and what
   * reads the occluder service's planes), per mesh over its receiver's atlas, on the preview lane's worker. Null when a
   * newer edit superseded the request.
   *
   * Each field is kept by everything it is baked from, so a scene rebuilt for an edit to another material (or a value
   * set back) asks the CPU for nothing, and a moved sun asks only for the fields that read `shadow`. The worker bakes
   * the planes those fields read first and keeps them by content: it casts rays only for planes whose key is new.
   */
  private async sceneFields(requests: { mesh: string; names: string[]; planes: string[][] }[]): Promise<Map<string, { width: number; height: number; fields: Float32Array[]; keys?: string[] } | { error: string }> | null> {
    const baseline = this.bakeProject;
    if (!baseline) return null;
    // The lighting record is not part of a build: the planes are lit by the one in effect now.
    const project: StudioProject = { ...baseline };
    if (this.project.lighting) project.lighting = this.project.lighting; else delete project.lighting;
    // Parameters count whole: finer than that is the engine's to know.
    const scene = JSON.stringify([project.modules, project.models, project.mapping, project.mappings, project.anchors, project.domain, project.gravity, project.edges,
      project.resolution, project.texelDensity, project.profile, Object.entries(project.assets).map(([name, bytes]) => [name, this.assetId(bytes)])]);
    const sun = this.sunKey();
    const key = (mesh: string, name: string, planes: string[]): string => {
      const bound = this.meshBindings[mesh];
      const material = project.materials?.find(candidate => candidate.id === bound || candidate.meshes.includes(mesh));
      // (A layer's field, `stack.<k>/<name>`, reads the layer's values: the slot's layers are part of what names it.)
      return JSON.stringify([scene, mesh, name, material?.entry, material?.seed, material?.parameters, planes.length ? [planes, planes.includes("shadow") ? sun : null] : null,
        ...(name.startsWith("stack.") && material?.layers ? [material.layers] : [])]);
    };
    type Kept = { width: number; height: number; field: Float32Array; planes?: string };
    const kept = new Map<string, (Kept | undefined)[]>();
    // Meshes whose bake failed, with the engine's reason: the baker shows it for them.
    const failed = new Map<string, string>();
    const missing: { mesh: string; names: string[] }[] = [];
    let occlusion = false;
    for (const request of requests) {
      const fields = request.names.map((name, index) => {
        const at = key(request.mesh, name, request.planes[index] ?? []), field = this.sceneFieldCache.get(at);
        if (field) { this.sceneFieldCache.delete(at); this.sceneFieldCache.set(at, field); }
        return field;
      });
      kept.set(request.mesh, fields);
      const names = request.names.filter((_, index) => !fields[index]);
      if (!names.length) continue;
      missing.push({ mesh: request.mesh, names });
      occlusion ||= request.names.some((_, index) => !fields[index] && (request.planes[index]?.length ?? 0) > 0);
    }
    if (missing.length) {
      let result;
      const started = performance.now();
      try { result = await this.lane.bakeProjectFields(project, missing, this.revision, occlusion); }
      catch (error) { if (error instanceof DOMException && error.name === "AbortError") return null; throw error; }
      if (result.error || !result.projectFields) throw new Error(result.error ?? result.diagnostics?.[0]?.message ?? "the CPU field bake failed");
      if (result.occlusion) this.noteOcclusion(result.occlusion, performance.now() - started);
      for (const baked of result.projectFields) {
        const request = requests.find(candidate => candidate.mesh === baked.mesh), asked = missing.find(candidate => candidate.mesh === baked.mesh);
        if ("error" in baked) failed.set(baked.mesh, baked.error);
        if (!("fields" in baked) || !request || !asked) continue;
        asked.names.forEach((name, index) => {
          const at = request.names.indexOf(name), planes = request.planes[at] ?? [];
          // The content of the planes the field was baked from, where it read any.
          const read = planes.length && baked.planes ? planes.map(plane => `${plane}=${baked.planes![plane] ?? ""}`).join(",") : undefined;
          const field: Kept = { width: baked.width, height: baked.height, field: baked.fields[index], ...(read === undefined ? {} : { planes: read }) };
          kept.get(request.mesh)![at] = field;
          this.sceneFieldCache.set(key(request.mesh, name, planes), field);
        });
      }
      // Least recently used first; a field is four floats per texel of its receiver's atlas.
      while (this.sceneFieldCache.size > 256) this.sceneFieldCache.delete(this.sceneFieldCache.keys().next().value!);
    }
    const found = new Map<string, { width: number; height: number; fields: Float32Array[]; keys?: string[] } | { error: string }>();
    for (const request of requests) {
      const error = failed.get(request.mesh);
      if (error !== undefined) { found.set(request.mesh, { error }); continue; }
      const fields = kept.get(request.mesh)!;
      // A mesh whose bake failed is left out: the scene's baker says so for that mesh.
      if (!fields.length || fields.some(field => !field || field.width !== fields[0]!.width || field.height !== fields[0]!.height)) continue;
      found.set(request.mesh, { width: fields[0]!.width, height: fields[0]!.height, fields: fields.map(field => field!.field),
        ...(fields.some(field => field!.planes !== undefined) ? { keys: fields.map(field => field!.planes ?? "") } : {}) });
    }
    return found;
  }
  private readonly sceneFieldCache = new Map<string, { width: number; height: number; field: Float32Array; planes?: string }>();

  /** States what the occluder service did for the scene's bakes (`data-occlusion`), for inspection and tests: the last request and the totals. */
  private noteOcclusion(report: OcclusionReport, ms: number): void {
    const count = (baked: boolean) => Object.fromEntries(OCCLUSION_PLANES.map(name => [name, report.planes.filter(plane => plane.name === name && plane.baked === baked).length]).filter(([, planes]) => planes));
    const rays = Object.fromEntries(OCCLUSION_PLANES.map(name => [name, report.planes.filter(plane => plane.name === name).reduce((sum, plane) => sum + plane.rays, 0)]).filter(([, cast]) => cast));
    this.occlusionRuns++;
    element("viewport").dataset.occlusion = JSON.stringify({ run: this.occlusionRuns, ms: report.ms, requestMs: Math.round(ms), baked: count(true), held: count(false), rays,
      texels: Object.fromEntries([...new Map(report.planes.map(plane => [plane.mesh, `${plane.width}x${plane.height}`]))]),
      meshes: [...new Set(report.planes.filter(plane => plane.baked).map(plane => plane.mesh))] });
  }
  private occlusionRuns = 0;

  /** Exports only a captured ready revision; a newer edit cancels delivery without changing the viewport. */
  private async exportGlb(): Promise<void> {
    if (this.status.dataset.state !== "ready" || !this.accepted || !this.environmentValid) return;
    const candidate = copyProject(this.accepted);
    // Lighting is not part of a build, so the accepted snapshot may predate the record now shown: the export carries the current one.
    if (this.project.lighting) candidate.lighting = structuredClone(this.project.lighting); else delete candidate.lighting;
    const textures = element<HTMLInputElement>("export-ktx2").checked ? "ktx2" : "png";
    const revision = this.begin(textures === "ktx2" ? "Exporting GLB: baking…" : "Exporting GLB…");
    const started = performance.now();
    try {
      const result = await this.authoring.exportProject(candidate, revision, textures, progress => {
        if (revision !== this.revision) return;
        this.status.textContent = `Exporting GLB: encoding KTX2 textures ${progress.encoded} of ${progress.textures}…`;      });
      if (revision !== this.revision) return;
      if (result.error || !result.download || result.diagnostics?.length) {
        this.diagnostics(result.diagnostics ?? [], candidate);
        throw new Error(result.error || "Project could not be exported.");
      }
      // The export's planes: those the preview already baked are found by key in the page's store.
      if (result.occlusion) this.noteOcclusion(result.occlusion, performance.now() - started);
      download(`${this.kind}.glb`, result.download, "model/gltf-binary");
      this.ready(`GLB downloaded from the current project (${textures === "ktx2" ? "KTX2" : "PNG"} textures, ${(result.download.byteLength / 1048576).toFixed(1)} MB, ${((performance.now() - started) / 1000).toFixed(1)} s).`);
    } catch (error) { if (revision === this.revision) this.failure(error); }
  }

  /** Chooses the requested visible mesh or every mesh using its original material identity. */
  private targets(project: StudioProject, base?: ScenePacket): Set<string> {
    if (!base) return new Set();
    const visible = base.manifest.meshes.filter(mesh => mesh.visible);
    let chosen: SceneMesh | undefined;
    if (project.target.mesh !== undefined) {
      chosen = visible.find(mesh => mesh.id === project.target.mesh);
      if (!chosen) throw new Error(`Selected mesh ${project.target.mesh} is missing from the original model.`);
    } else if (project.target.binding !== undefined) {
      chosen = visible.find(mesh => mesh.material === project.target.binding);
      if (!chosen) throw new Error(`Selected material binding ${project.target.binding} is missing from the original model.`);
    } else chosen = visible[0];
    if (!chosen) throw new Error("The model contains no visible material receivers.");
    project.target.mesh = chosen.id; project.target.binding = chosen.material ?? undefined;
    return new Set(base.manifest.meshes.filter(mesh => mesh.visible && (project.target.scope === "binding" ? mesh.material === chosen.material : mesh.id === chosen.id)).map(mesh => mesh.id));
  }

  /** Loads an AML scene and companions with the same relative-path admission as the Level Viewer. */
  private async openAml(files: File[]): Promise<void> {
    const revision = this.begin("Opening AML and companions…");
    const selected = await readAmlSelection(files, () => revision === this.revision);
    if (revision !== this.revision) return;
    const candidate = copyProject(this.project);
    candidate.assets = mergeAssets(candidate.assets, selected.assets); candidate.base = { kind: "aml", source: selected.source };
    candidate.target = { scope: "mesh", enabled: false };
    const packet = await this.aml.load(selected.source, selected.assets, revision);
    if (revision === this.revision) await this.compile(candidate, revision, packet, undefined, true, true);
  }

  /** Keeps original GLB bytes in the project and lets Rust import or generate its model. */
  private async openGlb(file: File): Promise<void> {
    const revision = this.begin("Opening GLB…");
    if (file.size > MAX_FILE_BYTES) throw new Error(`${file.name} exceeds the ${megabytes(MAX_FILE_BYTES)} file limit.`);
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (revision !== this.revision) return;
    const candidate = copyProject(this.project); candidate.assets = mergeAssets(candidate.assets, { [file.name]: bytes });
    if (this.kind === "item") {
      candidate.modules[candidate.entry] = itemSource("glb", file.name); candidate.parameters = {};
      await this.compile(candidate, revision, undefined, undefined, true, true);
    } else {
      candidate.base = { kind: "glb", name: file.name }; candidate.target = { scope: "mesh", enabled: false };
      const imported = await this.importGlb(candidate, revision);
      if (revision === this.revision) await this.compile(candidate, revision, imported.packet, imported.metadata, true, true);
    }
  }

  /** Admits GLB through ItemSpec's native/Wasm importer, preserving the returned source transform metadata. */
  private async importGlb(project: StudioProject, revision: number): Promise<{ packet: ScenePacket; metadata?: AuthoringMetadata }> {
    const result = await this.authoring.compile({ requestId: revision, kind: "import-glb", entry: project.base.name!, source: "",
      modules: {}, parameters: {}, seed: 0, assets: project.assets });
    if (result.error || !result.packet) throw new Error(result.error || result.diagnostics?.map(finding => finding.message).join("\n") || "GLB import returned no model.");
    return { packet: decodePacket(result.packet), metadata: result.metadata };
  }

  /** Stages a complete saved project, including its base source, before replacing any editor state. */
  private async openProject(file: File): Promise<void> {
    const revision = this.begin("Opening project…");
    this.documentRevision++;
    if (file.size > MAX_PROJECT_FILE_BYTES) throw new Error(`${file.name} exceeds the ${megabytes(MAX_PROJECT_FILE_BYTES)} project file limit.`);
    const candidate = await openProject(file, this.kind);
    // The project's optional `lighting` record is admitted here, so an invalid one is refused before anything is replaced.
    projectLighting(candidate);
    if (revision !== this.revision) return;
    await this.compileOpenedInputs(candidate, revision, true);
  }

  /** Rebuilds an imported base from staged companions so preview, save and export see the same input snapshot. */
  private async compileOpenedInputs(candidate: StudioProject, revision: number, fit: boolean): Promise<void> {
    let base: ScenePacket | undefined, metadata: AuthoringMetadata | undefined;
    if (this.kind === "material" && candidate.base.kind === "aml") base = await this.aml.load(candidate.base.source!, candidate.assets, revision);
    if (this.kind === "material" && candidate.base.kind === "glb") ({ packet: base, metadata } = await this.importGlb(candidate, revision));
    // An opened project shows its quick draft first, like an edit does.
    if (revision === this.revision) await this.compileProgressive(candidate, revision, base, metadata, true, fit);
  }

  /** Stages source modules and texture companions; failed compilation leaves the accepted project untouched. */
  private async openModules(files: File[]): Promise<void> {
    const revision = this.begin("Opening source modules…");
    this.documentRevision++;
    const candidate = copyProject(this.project);
    if (files.length > MAX_COMPANIONS || files.reduce((sum, file) => sum + file.size, 0) > MAX_COMPANION_BYTES) throw new Error(`Source selection exceeds ${MAX_COMPANIONS} files or ${megabytes(MAX_COMPANION_BYTES)} in total.`);
    const folder = files[0].webkitRelativePath?.split("/")[0];
    for (const file of files) {
      if (revision !== this.revision) return;
      if (file.size > MAX_FILE_BYTES) throw new Error(`${file.name} exceeds the ${megabytes(MAX_FILE_BYTES)} file limit.`);
      const path = folder ? file.webkitRelativePath.slice(folder.length + 1) : file.name;
      if (/\.ms$|\.material\.ts$|\.item\.json$/i.test(path)) candidate.modules[path] = await file.text();
      else candidate.assets[path] = new Uint8Array(await file.arrayBuffer());
    }
    const item = files.find(file => file.name.endsWith(".item.json"));
    if (this.kind === "item" && item) candidate.entry = folder ? item.webkitRelativePath.slice(folder.length + 1) : item.name;
    if (revision === this.revision) await this.compileOpenedInputs(candidate, revision, false);
  }

  /** Reflects accepted project fields without normalizing or evaluating authored source. */
  private showProject(): void {
    const modules = element<HTMLSelectElement>("module"); modules.replaceChildren();
    for (const name of Object.keys(this.project.modules)) modules.add(new Option(name, name));
    this.showModule(Object.hasOwn(this.project.modules, this.module) ? this.module : this.project.entry);
    element<HTMLInputElement>("seed").value = String(this.project.seed);
    const resolution = element<HTMLSelectElement>("resolution");
    if (![...resolution.options].some(option => Number(option.value) === this.project.resolution)) resolution.add(new Option(String(this.project.resolution)));
    resolution.value = String(this.project.resolution);
    this.showEnvironment();
    this.showScene();
    if (this.kind === "material") {
      element<HTMLSelectElement>("mapping").value = this.project.mapping;
      element<HTMLSelectElement>("scope").value = this.project.target.scope;
      element<HTMLInputElement>("domain-u").value = String(this.project.domain[0]);
      element<HTMLInputElement>("domain-v").value = String(this.project.domain[1]);
    } else {
      const materials = element<HTMLSelectElement>("material-source"); materials.replaceChildren();
      for (const name of Object.keys(this.project.modules).filter(name => /\.ms$|\.material\.ts$/.test(name))) materials.add(new Option(name, name));
    }
  }

  /** Displays accepted physical inputs without replacing an in-progress JSON draft during compilation. */
  private showEnvironment(): void {
    this.environmentValid = true;
    element<HTMLTextAreaElement>("environment").value = JSON.stringify({ edges: this.project.edges, gravity: this.project.gravity }, null, 2);
  }

  /** Switches editable module while retaining all other source modules in the project. */
  private showModule(name: string): void {
    this.module = name; element<HTMLSelectElement>("module").value = name;
    this.code.value = this.project.modules[name] ?? "";
    element("source-location").textContent = `Entry: ${this.project.entry} · ${this.code.value.split("\n").length} lines`;
    element<HTMLButtonElement>("set-entry").disabled = this.kind === "item" ? !name.endsWith(".item.json") : !/\.ms$|\.material\.ts$/.test(name);
    void this.recipe?.refresh();
  }

  /** Removes parameter overrides that the current source does not declare; true when any were removed. */
  private async dropStaleParameters(candidate: StudioProject, revision: number): Promise<boolean> {
    const project = this.project;
    const checked = await this.recipeTools.check({ entry: candidate.entry, modules: candidate.modules });
    if (!checked.success || revision !== this.revision || project !== this.project) return false;
    const declared = new Set(((checked.metadata as { parameters?: { name: string }[] }).parameters ?? []).map(parameter => parameter.name));
    const stale = Object.keys(this.project.parameters).filter(name => !declared.has(name));
    for (const name of stale) delete this.project.parameters[name];
    return stale.length > 0;
  }

  /** Applies recipe patches in place so selection and scroll stay put, then compiles like any other edit. */
  private applyPatches(patches: Patch[], source: string): void {
    for (const patch of [...patches].sort((a, b) => b.start - a.start)) this.code.setRangeText(patch.text, patch.start, patch.end, "preserve");
    if (this.code.value !== source) this.code.value = source;
    this.code.dispatchEvent(new Event("input"));
    void this.recipe?.refresh();
  }

  /** Inserts catalog examples at the caret. */
  private insert(text: string): void {
    this.code.focus();
    this.code.setRangeText(text, this.code.selectionStart, this.code.selectionEnd, "end");
    this.code.dispatchEvent(new Event("input"));
  }

  /** Keeps UI and ray-picked selection bound to stable mesh and original material identities. */
  private selection(mesh?: SceneMesh, instance?: number): void {
    if (!mesh) return;
    // A picked instance of an instanced batch: the batch's record stands for the mesh, bound as its template is.
    const template = (mesh as SceneMesh & { template?: string }).template;
    if (instance !== undefined || template) {
      this.selected = mesh.id;
      const assigned = this.project.materials?.find(material => material.meshes.includes(template ?? mesh.id));
      element("selection").textContent = `${mesh.id}${instance === undefined ? "" : ` · instance ${instance}`} · ${assigned ? `material ${assigned.id}` : "Original material"}`;
      element("selection").title = `Instanced batch${template ? ` of ${template}` : ""}`;
      return;
    }
    this.selected = mesh.id;
    const originalMesh = this.base?.manifest.meshes.find(item => item.id === mesh.id);
    element<HTMLSelectElement>("mesh").value = mesh.id;
    element("selection").textContent = `${mesh.id} · ${this.bindingLabel(mesh)}`;
    element("selection").title = `Original: ${originalMesh?.material ?? "Unassigned"}\nDisplayed: ${mesh.material ?? "Unassigned"}`;
    if (this.kind === "item") element<HTMLSelectElement>("material-slot").value = this.metadata?.meshSlots?.[mesh.id] ?? mesh.material ?? "surface";
  }

  /** Shows authored binding/slot names while leaving content identities available for detailed inspection. */
  private bindingLabel(mesh: SceneMesh): string {
    if (this.sceneMode()) {
      const assigned = this.meshBindings[mesh.id] ?? this.project.materials?.find(material => material.meshes.includes(mesh.id))?.id;
      // A mesh of an item also says which of the item's slots it is.
      const slot = Object.values(this.items).map(item => item.meshSlots[mesh.id]).find(Boolean);
      return `${assigned ? `material ${assigned}` : "Original material"}${slot ? ` · slot ${slot}` : ""}`;
    }
    const originalMesh = this.base?.manifest.meshes.find(original => original.id === mesh.id);
    const material = originalMesh ? originalMesh.material : mesh.material;
    const metadata = this.base ? this.baseMetadata : this.metadata;
    let label = metadata?.meshSlots?.[mesh.id] ?? material ?? "Unassigned";
    if (label.startsWith("studio/material/")) label = this.project.entry;
    if (label.startsWith("studio/import/material/")) label = "Imported material";
    if (originalMesh && mesh.material !== originalMesh.material) label += ` → ${this.project.entry}`;
    return label;
  }

  /** Rebuilds selectors from accepted mesh records, preserving the current selected identity when possible. */
  private showMeshes(packet: ScenePacket): void {
    const meshes = element<HTMLSelectElement>("mesh"); meshes.replaceChildren();
    for (const mesh of packet.manifest.meshes.filter(mesh => mesh.visible)) {
      const option = new Option(`${mesh.id} · ${this.bindingLabel(mesh)}`, mesh.id);
      option.title = mesh.material ?? "Unassigned"; meshes.add(option);
    }
    const selected = packet.manifest.meshes.find(mesh => mesh.visible && mesh.id === (this.selected ?? this.project.target.mesh))
      ?? packet.manifest.meshes.find(mesh => mesh.visible);
    if (this.kind === "item") {
      const slots = element<HTMLSelectElement>("material-slot"); slots.replaceChildren();
      for (const name of new Set(packet.manifest.meshes.filter(mesh => mesh.visible).map(mesh => this.metadata?.meshSlots?.[mesh.id] ?? mesh.material ?? "surface"))) slots.add(new Option(name, name));
    }
    if (selected) this.scene.select(selected.id);
  }

  /** Exposes compiler-declared intermediate fields beside standard PBR channels. */
  private showInspection(metadata?: AuthoringMetadata): void {
    const select = element<HTMLSelectElement>("inspection");
    for (const option of [...select.options]) if (option.dataset.field) option.remove();
    for (const field of metadata?.fields ?? []) {
      const option = new Option(`Field · ${field.name}`, field.name); option.dataset.field = "true"; select.add(option);
    }
    select.value = this.project.inspection;
    const live = (metadata?.live ?? []) as { id: string; requires: string[] }[];
    element("metadata").textContent = [metadata?.dependencies?.length ? `Modules: ${metadata.dependencies.join(", ")}` : "",
      ...live.map(operation => `Live: ${operation.id} · renderer supplies ${operation.requires.join(", ")}`),
      metadata?.collision ? `Collision: ${metadata.collision}` : "",
      ...(metadata?.attachments ?? []).map(point => `${point.name}: ${point.position.map(n => n.toFixed(3)).join(", ")} m`)].filter(Boolean).join("\n");
  }

  /** Strict TypeScript over every `.material.ts` module, on demand; the preview never waits for it. */
  private async checkTypes(): Promise<void> {
    const status = element("type-status");
    const modules = Object.fromEntries(Object.entries(this.project.modules).filter(([name]) => name.endsWith(".material.ts")));
    if (!Object.keys(modules).length) { status.textContent = "No .material.ts modules to type-check."; status.dataset.state = "idle"; return; }
    status.textContent = "Type-checking with strict TypeScript…"; status.dataset.state = "checking";
    const sources = { ...this.project.modules };
    const sdk = (await this.recipeTools.sdk()).files["index.d.ts"];
    const result = await this.typeChecker.check(sdk, modules);
    if (result.error) { status.textContent = `Type check failed: ${result.error}`; status.dataset.state = "error"; return; }
    const stale = Object.keys(modules).some(name => sources[name] !== this.project.modules[name]);
    this.diagnostics(result.findings, { ...this.project, modules: sources }, "type-diagnostics");
    status.dataset.state = result.findings.length ? "error" : "ready";
    status.textContent = (result.findings.length ? `Strict TypeScript: ${result.findings.length} problem${result.findings.length === 1 ? "" : "s"}.` : "Strict TypeScript: no problems.")
      + (stale ? " Sources changed since; check again." : "");
  }

  /** Source offsets from the compiler select the corresponding UTF-16 text range in its module. */
  private diagnostics(findings: AuthoringDiagnostic[], candidate = this.project, containerId = "diagnostics"): void {
    const container = element(containerId); container.replaceChildren();
    for (const finding of findings) {
      const button = document.createElement("button");
      const name = finding.sourceName ?? candidate.entry;
      const source = candidate.modules[name];
      button.textContent = `${name}:${finding.line}:${finding.column} ${finding.code?.startsWith("TS") ? finding.code + " " : ""}${finding.message}`;
      button.disabled = source === undefined || source !== this.project.modules[name];
      if (button.disabled) button.title = "This finding is from a rejected input; the current source remains unchanged.";
      button.addEventListener("click", () => {
        if (source === undefined || source !== this.project.modules[name]) return;
        this.showModule(name); this.code.focus();
        const start = source.slice(0, finding.offset).replace(/\r\n?/g, "\n").length;
        const end = source.slice(0, finding.end ?? finding.offset + 1).replace(/\r\n?/g, "\n").length;
        this.code.setSelectionRange(start, end);
      });
      container.append(button);
    }
  }

  /** Item overlays remain inspection-only and never become part of exported render or collision data. */
  private guides(): void {
    if (this.kind !== "item") return;
    this.scene.showItemGuides(element<HTMLInputElement>("attachments").checked ? this.metadata?.attachments ?? [] : [], element<HTMLInputElement>("collision").checked);
  }

  /** Reports failure while retaining the complete last valid scene and editor inputs. */
  private failure(error: unknown): void {
    if (error instanceof DOMException && error.name === "AbortError") return;
    element<HTMLButtonElement>("export-glb").disabled = true;
    const revision = this.revision;
    // The revision is settled: a GPU preview of it arriving later must not replace the error or the restored preview.
    this.cpuRevision = revision;
    const message = String(error instanceof Error ? error.message : error);
    const report = (restorationError?: unknown) => {
      if (revision !== this.revision) return;
      this.status.dataset.state = "error";
      this.status.textContent = restorationError ? `${message} Preview restoration failed: ${String(restorationError)}`
        : `${message}${this.visible ? " Last valid preview retained." : ""}`;
    };
    if (!this.draftVisible || !this.visible) { report(); return; }
    this.status.textContent = "Restoring last valid preview…";
    void this.scene.apply(this.visible, () => revision === this.revision).then(applied => {
      if (!applied) return;
      this.draftVisible = false;
      this.shownResolution = this.accepted?.resolution;
      delete element("viewport").dataset.gpuPreview; this.previewLabel(false);
      this.guides(); report();
    }).catch(report);
  }

  /** Publishes readiness only after the matching viewport transaction completes. */
  private ready(message: string): void {
    this.status.dataset.state = "ready"; this.status.textContent = message;
    element<HTMLButtonElement>("export-glb").disabled = false;
  }

  /** Releases workers, staged inputs, URLs and renderer resources on page departure. */
  private dispose(): void { clearTimeout(this.timer); this.authoring.close(); this.lane.close(); this.recipeTools.dispose(); this.typeChecker.dispose(); this.aml.cancel(); this.channels.clear(); this.scene.dispose(); }
}

const kind = document.body.dataset.studio === "item" ? "item" : "material";
mount(kind);
new Studio(kind);
