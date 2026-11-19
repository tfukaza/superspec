import type { ProjectLayouts } from "../renderer/atlas.ts";
import type { LightingInput } from "../renderer/lighting.ts";

export type ParameterValue = number | boolean | string | number[];
export interface SourceEdge { start: number[]; end: number[] }
/**
 * One model of a scene project; its meshes are named `<id>/<mesh>` after composition. An `item` is an ItemSpec
 * definition (the project module `entry`) with values for its controls, a placement (four affine columns: three basis
 * vectors and the translation, meters, Z up) and, per slot of the item, the project material its meshes show.
 */
export interface StudioModel {
  id: string; kind: "glb" | "aml" | "item"; name?: string; source?: string;
  entry?: string; parameters?: Record<string, ParameterValue>; transform?: number[][]; slots?: Record<string, string>;
  /** `item`: texels per meter for the meshes of a slot of this item, where it is not its material's or the project's. */
  slotDensities?: Record<string, number>;
}
/** A placed item as the engine reports it (`metadata.items[model id]`), in world meters. */
export interface ItemReport {
  entry: string; definition: string; geometryKey: string;
  /** Counts the times any part was generated; `rebuilt` names the parts generated the last time. */
  generation: number; rebuilt: string[];
  parameters: (Parameter & { value: ParameterValue })[];
  derived: Record<string, number>;
  parts: { id: string; key: string; meshes: string[]; rebuilt: boolean; anchor?: { origin: number[]; u: number[]; v: number[]; normal: number[] } }[];
  features: { name: string; part: string; source?: string; face: string; points: number[][]; closed: boolean; normal: number[]; width: number }[];
  attachments: Attachment[];
  decals: { id: string; part: string; face: string; image: string; mesh: string; size: number[]; offset: number; corners: number[][] }[];
  handles: ItemHandle[];
  meshSlots: Record<string, string>;
}
/** A declared link from a dimension to a control: moving `position` by d meters along `axis` changes `parameter` by d * scale. */
export interface ItemHandle { id: string; parameter: string; position: number[]; axis: number[]; scale: number; value: number; min?: number; max?: number }
/** The images the decals of a project's item models name, read from their definitions; unreadable definitions name none. */
export function decalImages(project: Pick<AuthoringProject, "modules" | "models">): Set<string> {
  const names = new Set<string>();
  for (const model of project.models ?? []) {
    if (model.kind !== "item") continue;
    try {
      const definition = JSON.parse(model.source ?? project.modules[model.entry ?? ""] ?? "{}") as { decals?: { image?: unknown }[] };
      for (const decal of Array.isArray(definition.decals) ? definition.decals : []) if (typeof decal?.image === "string") names.add(decal.image);
    } catch { /* the engine reports the definition's own error */ }
  }
  return names;
}
/** One material applied to a set of composed meshes; entries share the project's modules. */
export interface StudioMaterial {
  id: string; entry: string; parameters: Record<string, ParameterValue>; seed: number; meshes: string[];
  /**
   * Weathering layers over the material, the lowest first (`MaterialAssignment::layers`): a `defineLayer` module with its
   * own parameters and seed, and the receiver's features it follows (its own name to the receiver's).
   */
  layers?: StudioLayer[];
}
/** A weathering layer of a material slot (`authoring::Layer`). A name follows one receiver feature, or the nearest of a list of them. */
export interface StudioLayer { entry: string; parameters: Record<string, ParameterValue>; seed: number; follow?: Record<string, string | string[]> }
/** Portable studio inputs; original scene assets remain separate from compiled render output. */
export interface AuthoringProject {
  /** Version 2 adds `models` and `materials`; version 1 projects open unchanged. */
  schemaVersion: 1 | 2;
  kind: "material" | "item";
  entry: string;
  modules: Record<string, string>;
  parameters: Record<string, ParameterValue>;
  seed: number;
  /** Texture edge per receiver: 16-512, or up to 2048 with the production profile. */
  resolution: number;
  /** Bake scheduling. Output bytes are the same; production only admits larger textures. */
  profile?: "preview" | "production";
  /** Texels per meter; receivers that reach it in a smaller texture than `resolution` get that texture. */
  texelDensity?: number;
  /**
   * Texels per meter where it is not `texelDensity` (`Project.texel_densities`): by material id, for every receiver of
   * that material, or by mesh id, for that receiver alone. An item model states its slots' with `slotDensities`. A
   * sign's lettering or a notice is so baked finer than the walls; `resolution` still bounds each receiver's texture.
   */
  texelDensities?: Record<string, number>;
  inspection: string;
  mapping: "uv" | "object" | "world";
  domain: [number, number];
  edges: SourceEdge[];
  gravity: number[];
  assets: Record<string, Uint8Array>;
  base: { kind: "swatch" | "aml" | "glb"; source?: string; name?: string };
  target: { mesh?: string; binding?: string; scope: "mesh" | "binding"; enabled: boolean };
  /** Imported models composed into one scene; when present they replace `base`. */
  models?: StudioModel[];
  /** Materials applied in order to disjoint meshes; when present they replace `target`. */
  materials?: StudioMaterial[];
  /**
   * How the scene is lit and rendered (`renderer/lighting.ts`); without it the scene has the shipped look. The viewer
   * and the GLB export read it. The engine's project ignores the field: no build or bake depends on it.
   */
  lighting?: LightingInput;
  /**
   * Instanced batches (`superspec_model::batches::of_project`): each draws a mesh of the composed scene, its template,
   * once per placement, or states its geometry inline. The engine's project ignores the field; the delivered scene
   * (the preview packet and exports) holds the batches in their templates' place.
   */
  batches?: ProjectBatch[];
  /**
   * The mapping of a material where it is not the project's, by material id (`Project.mappings`). `anchored` measures
   * the surface from each receiver's pattern anchor: an item part's, or the one `anchors` states for the mesh.
   */
  mappings?: Record<string, "uv" | "object" | "world" | "anchored">;
  /** The pattern anchor of a mesh, by mesh id, where it is not its item part's (`Project.anchors`). */
  anchors?: Record<string, { origin: number[]; u: number[]; v: number[] }>;
  /**
   * Planes bound to the external inputs of the materials baked on each mesh, by mesh id and input name
   * (`Project.planes`): an asset's name, or a value of another material. The occluder service's planes (`shadow`,
   * `occlusion`, `sunExposure`) are bound by the authoring worker for the build they are baked for, never stored here.
   */
  planes?: Record<string, Record<string, string | { material: string; value: string }>>;
}
/** The external inputs the occluder service supplies (`authoring::occlusion::PLANES`), in the order it makes them. */
export const OCCLUSION_PLANES = ["shadow", "occlusion", "sunExposure"] as const;
/**
 * What the occluder service did for a request: for each plane name, the milliseconds its call took in the worker
 * (composing the scene, laying out the receivers and keying every plane included), then every plane with its key, its
 * size, the rays cast for it and whether it was baked now (`false`: the worker or the page held a plane of that key).
 */
export interface OcclusionReport {
  ms: Record<string, number>;
  planes: { mesh: string; name: string; key: string; width: number; height: number; rays: number; occluders: number; baked: boolean }[];
}
/** One instanced batch of a project; `value` and `tint` are given for every instance or for none. */
export interface ProjectBatch {
  id: string;
  objectId?: string;
  /** A mesh of the composed scene, or geometry stated here. */
  mesh: string | { positions: number[]; normals: number[]; uvs?: number[]; colors?: number[]; indices: number[];
    scalars?: Record<string, number[]>; vectors?: Record<string, number[]> };
  /** Named per-instance scalars, and vectors as three numbers per instance. */
  scalars?: Record<string, number[]>;
  vectors?: Record<string, number[]>;
  collision?: "none" | "instances";
  visible?: boolean;
  instances: { translation: number[]; rotation?: number[]; scale?: number[]; value?: number[]; tint?: number[] }[];
}
export interface Parameter {
  name: string;
  type: "scalar" | "number" | "integer" | "vec2" | "vec3" | "bool" | "boolean" | "color" | "choice";
  default: ParameterValue;
  min?: number;
  max?: number;
  step?: number;
  unit?: string;
  label?: string;
  role?: string;
  choices?: string[];
}
export interface Receiver {
  id: string;
  objectId: string;
  positions: number[];
  normals: number[];
  uvs: number[];
  indices: number[];
  objectToWorld?: number[];
}
export interface AuthoringRequest {
  requestId: number;
  kind: "material" | "item" | "import-glb";
  entry: string;
  source: string;
  modules: Record<string, string>;
  parameters: Record<string, ParameterValue>;
  seed: number;
  assets: Record<string, Uint8Array>;
  inspection?: string;
  resolution?: number;
  receivers?: Receiver[];
  mapping?: "uv" | "object" | "world";
  domain?: [number, number];
  edges?: SourceEdge[];
  gravity?: number[];
}
export interface AuthoringExportRequest {
  action: "export-project";
  requestId: number;
  project: Omit<AuthoringProject, "assets">;
  /** Texture storage in the GLB: KTX2 (`KHR_texture_basisu`, the default) or PNG. */
  textures: ExportTextures;
  assets: Record<string, Uint8Array>;
}
export interface AuthoringPreviewRequest {
  action: "preview-project";
  requestId: number;
  project: Omit<AuthoringProject, "assets">;
  assets: Record<string, Uint8Array>;
}
/** Describes a project for per-pixel shading: composed models and each material's shader, no bake. */
export interface AuthoringShadeRequest {
  action: "shade-project";
  requestId: number;
  project: Omit<AuthoringProject, "assets">;
  assets: Record<string, Uint8Array>;
  /** Also lay out every assigned receiver's atlas as the CPU bake would (`AuthoringResult.layouts`), for a GPU bake in UV space. */
  layouts?: boolean;
}
/**
 * Evaluates CPU-only fields for meshes of a scene project, each over its receiver's atlas, for a GPU bake of the scene
 * that binds them as textures. Nothing is built.
 */
export interface AuthoringProjectFieldsRequest {
  action: "bake-project-fields";
  requestId: number;
  project: Omit<AuthoringProject, "assets">;
  meshes: { mesh: string; names: string[] }[];
  assets: Record<string, Uint8Array>;
  /**
   * The occluder service's planes these meshes' materials read are baked first (lit by the project's `lighting`) and
   * bound for the field bake. Each mesh's reply then carries the keys of its planes.
   */
  occlusion?: boolean;
}
/**
 * Evaluates fields of a material request's program over its receiver atlas on the CPU, for a GPU preview that binds
 * them as textures. Nothing is built, and the accepted model is untouched.
 */
export interface AuthoringFieldsRequest {
  action: "bake-fields";
  requestId: number;
  request: Omit<AuthoringRequest, "requestId">;
  names: string[];
  assets: Record<string, Uint8Array>;
}
/** Texels of baked fields: one array per field, four floats per texel, rows from the atlas's upper edge. */
export interface BakedFields { width: number; height: number; fields: Float32Array[] }
export type AuthoringMessage = AuthoringRequest | AuthoringExportRequest | AuthoringPreviewRequest | AuthoringShadeRequest | AuthoringFieldsRequest
  | AuthoringProjectFieldsRequest;
/** KTX2 is several times smaller on disk and in GPU memory; PNG suits tools without KTX2 support (Blender). */
export type ExportTextures = "ktx2" | "png";
export interface AuthoringDiagnostic {
  sourceName?: string;
  line: number;
  column: number;
  offset: number;
  end?: number;
  message: string;
  code?: string;
}
export interface Attachment {
  name: string;
  position: number[];
  direction?: number[];
}
export interface AuthoringMetadata {
  parameters?: Parameter[];
  fields?: { name: string; type: string }[];
  dependencies?: string[];
  attachments?: Attachment[];
  collision?: string;
  materials?: unknown;
  receiverTransforms?: Record<string, number[]>;
  meshSlots?: Record<string, string>;
  [key: string]: unknown;
}
export interface AuthoringResult {
  requestId: number;
  packet?: ArrayBuffer;
  fields?: BakedFields;
  /** Receiver atlases, when a shade request asked for them. */
  layouts?: ProjectLayouts;
  /** With the layouts: how long the worker's steps took, in milliseconds, and the sizes of the texts it passed. */
  timings?: Record<string, number>;
  /** Baked fields by mesh, for a `bake-project-fields` request; a mesh whose bake failed carries the reason instead. */
  projectFields?: ({ mesh: string; planes?: Record<string, string> } & (BakedFields | { error: string }))[];
  /** What the occluder service baked or found held for the request, when it ran. */
  occlusion?: OcclusionReport;
  download?: ArrayBuffer;
  metadata?: AuthoringMetadata;
  diagnostics?: AuthoringDiagnostic[];
  error?: string;
}
/** Export progress: the bake is done once textures are listed, then each encoded texture counts. */
export interface ExportProgress { encoded: number; textures: number }
/** An interim worker message; the request stays pending. */
export interface AuthoringProgress { requestId: number; progress: ExportProgress }
/**
 * Messages about the occluder service's planes between a worker and the page (see `PlaneStore`): planes a worker baked,
 * for the page to keep; a worker asking for planes the page holds; and the page's answer.
 */
export interface PlanesBaked { planesBaked: Record<string, Uint8Array> }
export interface PlanesWanted { planesWanted: string[] }
export interface PlanesAnswer { planesAnswer: Record<string, Uint8Array> }

/**
 * The occluder service's planes of the page, by the asset name a project binds them under (`occlusion/<name>/<key>`;
 * the key names everything that decides the plane's values). Every authoring worker of the page (the build's and the
 * GPU preview lane's) reads and adds to it, and it outlives a replaced worker, so a plane is computed once per page
 * whichever worker, build or lighting edit asks for it: a moved sun bakes `shadow` again and finds the others here.
 */
export class PlaneStore {
  /** Encoded planes kept at most; the least recently used go first. */
  static readonly ALLOWANCE = 96 * 1024 * 1024;
  private readonly planes = new Map<string, Uint8Array>();
  private bytes = 0;
  names(): string[] { return [...this.planes.keys()]; }
  keep(name: string, plane: Uint8Array): void {
    const previous = this.planes.get(name);
    if (previous) { this.bytes -= previous.byteLength; this.planes.delete(name); }
    this.planes.set(name, plane); this.bytes += plane.byteLength;
    for (const [oldest, bytes] of this.planes) {
      if (this.bytes <= PlaneStore.ALLOWANCE || oldest === name) break;
      this.planes.delete(oldest); this.bytes -= bytes.byteLength;
    }
  }
  /** Copies of the planes of `names` the page holds (a worker keeps its own); the others are left out. */
  copies(names: string[]): Record<string, Uint8Array> {
    const found: Record<string, Uint8Array> = {};
    for (const name of names) {
      const plane = this.planes.get(name);
      if (!plane) continue;
      this.planes.delete(name); this.planes.set(name, plane);
      found[name] = plane.slice();
    }
    return found;
  }
}
/** The page's planes, shared by every `AuthoringCompiler`. */
export const pagePlanes = new PlaneStore();

/** How long a worker may go on with a cancelled request before it is replaced. */
const CANCEL_GRACE_MS = 1500;

/** Owns a real Rust/Wasm authoring worker and rejects superseded requests before publication. */
export class AuthoringCompiler {
  private worker?: Worker;

  /**
   * `keepWorker` never replaces the worker, however long a cancelled request runs on. The Studio gives the GPU preview
   * lane (shader plans, field bakes) a compiler of its own with it, so the lane's requests and the authoritative CPU
   * build never wait for, or cancel, each other.
   */
  constructor(private readonly options: { keepWorker?: boolean } = {}) {}
  private pending?: { requestId: number; resolve: (result: AuthoringResult) => void; reject: (error: Error) => void;
    progress?: (progress: ExportProgress) => void };

  /** Builds one request. The worker keeps its session (compiled programs, decoded images, retained bakes) between requests. */
  compile(request: AuthoringRequest): Promise<AuthoringResult> {
    return this.dispatch(request);
  }

  /** Exports captured project inputs through the shared native/Wasm project compiler. */
  exportProject(project: AuthoringProject, requestId: number, textures: ExportTextures = "ktx2",
    progress?: (progress: ExportProgress) => void): Promise<AuthoringResult> {
    const { assets, ...inputs } = project;
    return this.dispatch({ action: "export-project", requestId, project: inputs, assets, textures }, progress);
  }

  /** Builds a whole project, including composed models and several materials, through `build_project`. */
  previewProject(project: AuthoringProject, requestId: number): Promise<AuthoringResult> {
    const { assets, ...inputs } = project;
    return this.dispatch({ action: "preview-project", requestId, project: inputs, assets });
  }

  /** Composes a project's models and compiles its materials to preview shaders; nothing is baked. */
  shadeProject(project: AuthoringProject, requestId: number, layouts = false): Promise<AuthoringResult> {
    const { assets, ...inputs } = project;
    // Shaders read images on the page; the engine only needs the models, the images their decals show and the shipped
    // catalog images a project supplies for its items' own slots (a sign's lettering atlas).
    const decals = decalImages(inputs);
    const models = Object.fromEntries(Object.entries(assets).filter(([name]) => /\.(glb|ktx2)$/i.test(name)
      || inputs.models?.some(model => model.name === name) || decals.has(name) || name.startsWith("superspec-material:")));
    return this.dispatch({ action: "shade-project", requestId, project: inputs, assets: models, layouts });
  }

  /** Bakes CPU-only fields for meshes of a scene project (see `AuthoringProjectFieldsRequest`). */
  bakeProjectFields(project: AuthoringProject, meshes: { mesh: string; names: string[] }[], requestId: number, occlusion = false): Promise<AuthoringResult> {
    const { assets, ...inputs } = project;
    return this.dispatch({ action: "bake-project-fields", requestId, project: inputs, meshes, assets, ...(occlusion ? { occlusion } : {}) });
  }

  /** Bakes fields of `request`'s program on the CPU (see `AuthoringFieldsRequest`). */
  bakeFields(request: Omit<AuthoringRequest, "requestId">, names: string[], requestId: number): Promise<AuthoringResult> {
    const { assets, ...inputs } = request;
    return this.dispatch({ action: "bake-fields", requestId, request: { ...inputs, assets: {} }, names, assets });
  }

  /** Only the active worker and matching consumer revision can complete a request. */
  private dispatch(request: AuthoringMessage, progress?: (progress: ExportProgress) => void): Promise<AuthoringResult> {
    if (this.pending) this.cancel();
    return new Promise((resolve, reject) => {
      this.pending = { requestId: request.requestId, resolve, reject, progress };
      // A worker still running a cancelled request gets the next one when it answers; only the newest waits, so a burst
      // of edits costs the request in flight and one more, not one each (and one copy of the assets, not one each).
      if (this.running) { this.waiting = request; return; }
      this.start(request);
    });
  }

  private start(request: AuthoringMessage): void {
    let worker = this.worker;
    if (!worker) {
      const created = worker = this.worker = new Worker(new URL("./authoring-worker.ts", import.meta.url), { type: "module" });
      created.onmessage = (event: MessageEvent<AuthoringResult | AuthoringProgress | PlanesBaked | PlanesWanted>) => {
        // Planes are the page's whoever asked for them, and a worker waiting for planes is answered even if replaced.
        if ("planesBaked" in event.data) {
          for (const [name, plane] of Object.entries(event.data.planesBaked)) pagePlanes.keep(name, plane);
          return;
        }
        if ("planesWanted" in event.data) {
          created.postMessage({ planesAnswer: pagePlanes.copies(event.data.planesWanted) } satisfies PlanesAnswer);
          return;
        }
        if (this.worker !== created) return;
        // The worker answers every request it was sent, cancelled ones too; each answer frees it for the newest one waiting.
        if (!("progress" in event.data)) {
          this.settled();
          const next = this.waiting;
          this.waiting = undefined;
          if (next) this.start(next);
        }
        if (this.pending?.requestId !== event.data.requestId) return;
        if ("progress" in event.data) {
          this.pending.progress?.(event.data.progress);
          return;
        }
        const pending = this.pending;
        this.pending = undefined;
        pending.resolve(event.data);
      };
      created.onerror = event => {
        if (this.worker !== created) return;
        this.failed(new Error(event.message || "Authoring worker failed."));
      };
      // A message the page cannot read (it never answers its request) fails the request like a crash.
      created.onmessageerror = () => {
        if (this.worker !== created) return;
        this.failed(new Error("The authoring worker sent a message the page could not read."));
      };
    }
    // The worker cannot hear a message while it computes, but it can ask whether this URL still exists (see `cancel`).
    this.token = URL.createObjectURL(new Blob(["1"]));
    this.running = true;
    // The planes the page holds, by name: the worker casts no ray for them and asks for the bytes it lacks.
    worker.postMessage({ ...request, cancelToken: this.token, heldPlanes: pagePlanes.names() });
  }

  /**
   * The worker failed outside any request's answer: the request fails with the reason (the Studio shows it and keeps its
   * last valid scene) and a new worker serves the next request.
   */
  private failed(error: Error): void {
    // A newer request waiting for the worker was never sent to it: a new worker runs it.
    const next = this.waiting;
    if (!next) {
      this.pending?.reject(error);
      this.pending = undefined;
    }
    this.close();
    if (next) this.start(next);
  }

  /** The worker has a request it has not answered, and the newest request made meanwhile. */
  private running = false;
  private waiting?: AuthoringMessage;
  /** The URL the worker reads to learn that its request is still wanted; revoking it cancels the request. */
  private token?: string;
  private watchdog?: ReturnType<typeof setTimeout>;

  /** The worker answered the request it was running. */
  private settled(): void {
    this.running = false;
    if (this.token) URL.revokeObjectURL(this.token);
    this.token = undefined;
    clearTimeout(this.watchdog);
    this.watchdog = undefined;
  }

  /**
   * Cancels the consumer's request. The worker is kept: it holds the session's compiled programs, decoded images and
   * retained bakes, and a new worker would start the Wasm module again and build cold. A request it is running is asked
   * to stop (the engine polls for that while it builds and bakes, and stops within a few tens of milliseconds); its
   * reply is ignored and frees the worker for the next request. A worker that does not stop in time is replaced,
   * unless the compiler was made with `keepWorker` (the GPU preview lane's short requests are simply left to finish).
   */
  cancel(): void {
    this.pending?.reject(new DOMException("Authoring request superseded.", "AbortError"));
    this.pending = undefined;
    // A cancelled request that has not started never does.
    this.waiting = undefined;
    if (!this.running) return;
    if (this.token) { URL.revokeObjectURL(this.token); this.token = undefined; }
    if (this.options.keepWorker || this.watchdog) return;
    this.watchdog = setTimeout(() => {
      this.watchdog = undefined;
      if (!this.running) return;
      // Still computing a request nobody wants: a new worker serves the newest one waiting.
      const next = this.waiting;
      this.close();
      if (next) this.start(next);
    }, CANCEL_GRACE_MS);
  }

  /** Releases the worker whatever it is doing (on leaving the page). */
  close(): void {
    this.worker?.terminate();
    this.worker = undefined;
    this.settled();
    this.waiting = undefined;
  }
  /** Supersedes active work while allowing an idle compiler to retain compatible programs. */
  cancelPending(): void { if (this.pending) this.cancel(); }
}
