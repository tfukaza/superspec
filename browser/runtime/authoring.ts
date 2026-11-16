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
export type AuthoringMessage = AuthoringRequest | AuthoringExportRequest | AuthoringPreviewRequest;
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
  download?: ArrayBuffer;
  metadata?: AuthoringMetadata;
  diagnostics?: AuthoringDiagnostic[];
  error?: string;
}
/** Export progress: the bake is done once textures are listed, then each encoded texture counts. */
export interface ExportProgress { encoded: number; textures: number }
/** An interim worker message; the request stays pending. */
export interface AuthoringProgress { requestId: number; progress: ExportProgress }

/** Owns a real Rust/Wasm authoring worker and rejects superseded requests before publication. */
export class AuthoringCompiler {
  private worker?: Worker;
  private pending?: { requestId: number; resolve: (result: AuthoringResult) => void; reject: (error: Error) => void;
    progress?: (progress: ExportProgress) => void };

  /** An idle worker can retain compiler state; replacing busy work releases its Wasm allocations. */
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

  /** Only the active worker and matching consumer revision can complete a request. */
  private dispatch(request: AuthoringMessage, progress?: (progress: ExportProgress) => void): Promise<AuthoringResult> {
    if (this.pending) this.cancel();
    return new Promise((resolve, reject) => {
      if (!this.worker) {
        const worker = new Worker(new URL("./authoring-worker.ts", import.meta.url), { type: "module" });
        this.worker = worker;
        worker.onmessage = (event: MessageEvent<AuthoringResult | AuthoringProgress>) => {
          if (this.worker !== worker || this.pending?.requestId !== event.data.requestId) return;
          if ("progress" in event.data) {
            this.pending.progress?.(event.data.progress);
            return;
          }
          const pending = this.pending;
          this.pending = undefined;
          pending.resolve(event.data);
        };
        worker.onerror = event => {
          if (this.worker !== worker) return;
          this.pending?.reject(new Error(event.message || "Authoring worker failed."));
          this.pending = undefined;
          worker.terminate();
          this.worker = undefined;
        };
      }
      this.pending = { requestId: request.requestId, resolve, reject, progress };
      this.worker.postMessage(request);
    });
  }

  /** Cancels the consumer's request and releases the worker, including after navigation. */
  cancel(): void {
    this.pending?.reject(new DOMException("Authoring request superseded.", "AbortError"));
    this.pending = undefined;
    this.worker?.terminate();
    this.worker = undefined;
  }

  /** Supersedes active work while allowing an idle compiler to retain compatible programs. */
  cancelPending(): void { if (this.pending) this.cancel(); }
}
