import { MAX_COMPANION_BYTES, MAX_COMPANIONS, MAX_FILE_BYTES, MAX_PROJECT_FILE_BYTES, megabytes } from "../../browser/model/limits.ts";
import "./style.css";
import { RustScene } from "../../browser/renderer/scene.ts";
import { decodePacket, type SceneMesh, type ScenePacket } from "../../browser/model/packet.ts";
import { AuthoringCompiler, type AuthoringDiagnostic, type AuthoringMetadata, type AuthoringRequest, type StudioMaterial } from "../../browser/runtime/authoring.ts";
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
  private readonly recipeTools = new RecipeTools();
  private readonly typeChecker = new TypeChecker();
  private recipe?: RecipePanel;
  private readonly aml = new AmlLoader();
  private readonly scene: RustScene;
  private readonly parameters: ParameterControls;
  private readonly channels: Channels;
  private readonly code = element<HTMLTextAreaElement>("code");
  private readonly status = element<HTMLOutputElement>("status");
  /** Advances only when the edited document is replaced, not for recompiles of the same source. */
  private documentRevision = 0;
  /** The scene material whose entry, parameters and source the editor shows. */
  private activeMaterial?: string;

  constructor(private readonly kind: StudioProject["kind"]) {
    this.project = initialProject(kind); this.module = this.project.entry;
    this.scene = new RustScene(element("viewport"), mesh => this.selection(mesh));
    this.parameters = new ParameterControls(element("parameters"), (name, value) => {
      this.project.parameters[name] = value; this.edit();
    });
    this.channels = new Channels(element("channels"));
    if (kind === "material") this.recipe = new RecipePanel(element("recipe-body"), this.recipeTools, {
      revision: () => this.documentRevision,
      entry: () => this.project.entry, module: () => this.module, modules: () => this.project.modules,
      source: () => this.code.value, apply: (patches, source) => this.applyPatches(patches, source),
      insert: text => this.insert(text), override: name => this.project.parameters[name],
    });
    this.bindControls(); this.showProject(); this.edit();
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
    element<HTMLInputElement>("exposure").addEventListener("input", event => this.scene.setExposure((event.target as HTMLInputElement).valueAsNumber));
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
      choose.textContent = `${material.id} · ${material.entry} · ${material.meshes.length} mesh${material.meshes.length === 1 ? "" : "es"}`;
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

  /** A material override is scoped to explicit selected receiver identities and can be restored atomically. */
  private bindMaterialControls(): void {
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
    clearTimeout(this.timer); this.authoring.cancelPending(); this.aml.cancel();
    this.status.dataset.state = "compiling"; this.status.textContent = message;
    element<HTMLButtonElement>("export-glb").disabled = true;
    return ++this.revision;
  }

  /** Debounces typing without allowing an older worker or texture load to replace the newest draft. */
  private edit(immediate = false): void {
    const revision = this.begin();
    void this.recipe?.refresh();
    if (!this.environmentValid) {
      this.failure(new Error("Correct the runoff edges and gravity draft before compiling or exporting."));
      return;
    }
    this.syncActive();
    const candidate = copyProject(this.project);
    this.timer = setTimeout(() => void this.compileProgressive(candidate, revision, this.base, this.baseMetadata).catch(error => {
      if (revision === this.revision) this.failure(error);
    }), immediate ? 0 : 180);
  }

  /**
   * Large textures first show a quick low-resolution draft, then refine to the requested size. The draft changes only
   * resolution and scheduling: same source, seed and parameters, never accepted for save or export. A newer edit cancels
   * the refinement, so dragging a control stays cheap and refines once the input settles.
   */
  private async compileProgressive(candidate: StudioProject, revision: number, base?: ScenePacket, baseMetadata?: AuthoringMetadata, opened = false): Promise<void> {
    if (this.kind === "material" && candidate.resolution >= 128) {
      const draft = { ...candidate, resolution: 64 };
      if (!await this.compile(draft, revision, base, baseMetadata, false, opened, true)) return;
    }
    await this.compile(candidate, revision, base, baseMetadata, opened, opened);
  }

  /** Executes the shared authoring bridge and commits a complete scene only after renderer acceptance. */
  private async compile(candidate: StudioProject, revision: number, base?: ScenePacket, baseMetadata?: AuthoringMetadata, opened = false, fit = false, draft = false): Promise<boolean> {
    if (revision !== this.revision) return false;
    // Scene projects build whole through build_project: every model and every material at once.
    const scene = this.sceneMode(candidate);
    if (scene) base = undefined;
    const ids = scene ? new Set<string>() : this.targets(candidate, base);
    const request: AuthoringRequest = { requestId: revision, kind: this.kind, entry: candidate.entry,
      source: candidate.modules[candidate.entry], modules: candidate.modules, parameters: candidate.parameters,
      seed: candidate.seed, resolution: candidate.resolution, inspection: candidate.inspection === "final" ? undefined : candidate.inspection,
      assets: candidate.assets, mapping: candidate.mapping, domain: candidate.domain, edges: candidate.edges, gravity: candidate.gravity };
    if (this.kind === "material" && base && candidate.target.enabled) request.receivers = receivers(base, ids, baseMetadata?.receiverTransforms);
    const result = scene ? await this.authoring.previewProject(candidate, revision) : await this.authoring.compile(request);
    if (revision !== this.revision) return false;
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
    if (draft) {
      this.draftVisible = true;
      // A draft only updates the viewport; readiness, saving, export and inspection wait for the full-size build.
      element("viewport").dataset.draftRevision = String(revision);
      this.status.textContent = "Refining texture… Draft shown.";
      return true;
    }
    if (opened) {
      this.project = candidate; this.base = base; this.baseMetadata = baseMetadata;
      this.module = candidate.entry;
      if (fit && !scene) this.selected = candidate.target.mesh;
      if (scene && !candidate.materials?.some(material => material.id === this.activeMaterial)) this.activeMaterial = candidate.materials?.[0]?.id;
      const active = this.active();
      if (active) { this.project.entry = active.entry; this.project.parameters = active.parameters; this.module = active.entry; }
      this.showProject();
    }
    if (scene) {
      // Which material each mesh shows, by material identity, for inspection and tests.
      const bindings: Record<string, string> = {};
      for (const material of candidate.materials ?? []) for (const mesh of material.meshes) bindings[mesh] = material.id;
      element("viewport").dataset.bindings = JSON.stringify(bindings);
    } else delete element("viewport").dataset.bindings;
    this.accepted = copyProject(candidate);
    this.draftVisible = false;
    this.visible = packet; this.metadata = result.metadata;
    this.diagnostics([]); this.parameters.show(result.metadata?.parameters ?? [], candidate.parameters);
    this.showInspection(result.metadata); this.channels.show(artifact); this.showMeshes(packet); void this.recipe?.refresh();
    element("scene-title").textContent = packet.manifest.name || candidate.entry;
    element("model-details").textContent = `${packet.manifest.meshes.filter(mesh => mesh.visible).length} meshes · ${packet.manifest.vertexCount.toLocaleString()} vertices · meters, Z up`;
    element("viewport").dataset.inputRevision = String(revision);
    this.guides(); this.ready(`Revision ${revision} ready${base && !candidate.target.enabled ? " · Original bindings; select a surface and apply." : ""}`);
    return true;
  }

  /** Exports only a captured ready revision; a newer edit cancels delivery without changing the viewport. */
  private async exportGlb(): Promise<void> {
    if (this.status.dataset.state !== "ready" || !this.accepted || !this.environmentValid) return;
    const candidate = copyProject(this.accepted);
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
    if (revision !== this.revision) return;
    await this.compileOpenedInputs(candidate, revision, true);
  }

  /** Rebuilds an imported base from staged companions so preview, save and export see the same input snapshot. */
  private async compileOpenedInputs(candidate: StudioProject, revision: number, fit: boolean): Promise<void> {
    let base: ScenePacket | undefined, metadata: AuthoringMetadata | undefined;
    if (this.kind === "material" && candidate.base.kind === "aml") base = await this.aml.load(candidate.base.source!, candidate.assets, revision);
    if (this.kind === "material" && candidate.base.kind === "glb") ({ packet: base, metadata } = await this.importGlb(candidate, revision));
    if (revision === this.revision) await this.compile(candidate, revision, base, metadata, true, fit);
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
  private selection(mesh?: SceneMesh): void {
    if (!mesh) return;
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
      const assigned = this.project.materials?.find(material => material.meshes.includes(mesh.id));
      return assigned ? `material ${assigned.id}` : "Original material";
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
      this.guides(); report();
    }).catch(report);
  }

  /** Publishes readiness only after the matching viewport transaction completes. */
  private ready(message: string): void {
    this.status.dataset.state = "ready"; this.status.textContent = message;
    element<HTMLButtonElement>("export-glb").disabled = false;
  }

  /** Releases workers, staged inputs, URLs and renderer resources on page departure. */
  private dispose(): void { clearTimeout(this.timer); this.authoring.cancel(); this.recipeTools.dispose(); this.typeChecker.dispose(); this.aml.cancel(); this.channels.clear(); this.scene.dispose(); }
}

const kind = document.body.dataset.studio === "item" ? "item" : "material";
mount(kind);
new Studio(kind);
