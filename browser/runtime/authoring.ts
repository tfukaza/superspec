import type { ProjectLayouts } from "../renderer/atlas.ts";

export type ParameterValue = number | boolean | string | number[];
export interface SourceEdge { start: number[]; end: number[] }
/** One imported model of a scene project; its meshes are named `<id>/<mesh>` after composition. */
export interface StudioModel { id: string; kind: "glb" | "aml"; name?: string; source?: string }
/** One material applied to a set of composed meshes; entries share the project's modules. */
export interface StudioMaterial { id: string; entry: string; parameters: Record<string, ParameterValue>; seed: number; meshes: string[] }
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
  /** Baked fields by mesh, for a `bake-project-fields` request; a mesh whose bake failed carries the reason instead. */
  projectFields?: ({ mesh: string } & (BakedFields | { error: string }))[];
  download?: ArrayBuffer;
  metadata?: AuthoringMetadata;
  diagnostics?: AuthoringDiagnostic[];
  error?: string;
}
/** Export progress: the bake is done once textures are listed, then each encoded texture counts. */
export interface ExportProgress { encoded: number; textures: number }
/** An interim worker message; the request stays pending. */
export interface AuthoringProgress { requestId: number; progress: ExportProgress }

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
    // Shaders read images on the page; the engine only needs the models.
    const models = Object.fromEntries(Object.entries(assets).filter(([name]) => /\.(glb|ktx2)$/i.test(name)
      || inputs.models?.some(model => model.name === name)));
    return this.dispatch({ action: "shade-project", requestId, project: inputs, assets: models, layouts });
  }

  /** Bakes CPU-only fields for meshes of a scene project (see `AuthoringProjectFieldsRequest`). */
  bakeProjectFields(project: AuthoringProject, meshes: { mesh: string; names: string[] }[], requestId: number): Promise<AuthoringResult> {
    const { assets, ...inputs } = project;
    return this.dispatch({ action: "bake-project-fields", requestId, project: inputs, meshes, assets });
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
      created.onmessage = (event: MessageEvent<AuthoringResult | AuthoringProgress>) => {
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
        this.pending?.reject(new Error(event.message || "Authoring worker failed."));
        this.pending = undefined;
        this.close();
      };
    }
    // The worker cannot hear a message while it computes, but it can ask whether this URL still exists (see `cancel`).
    this.token = URL.createObjectURL(new Blob(["1"]));
    this.running = true;
    worker.postMessage({ ...request, cancelToken: this.token });
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
