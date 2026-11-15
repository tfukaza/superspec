import type { AuthoringDiagnostic } from "./authoring.ts";

/** A half-open range of UTF-16 code units, the same offsets editors and diagnostics use. */
export interface Span { start: number; end: number }
export interface Entry { name: string; span: Span; text: string }
export interface Patch { start: number; end: number; text: string }

export interface RecipeParameter {
  name: string; type: "scalar" | "vec2" | "vec3" | "bool"; declaration: Span; default: Span; defaultText: string;
  range?: [number, number]; rangeSpan?: Span; unit?: string; label?: string;
}
export interface RecipeField { name: string; declaration: Span; value: Span; valueText: string; references: Span[] }
export interface RecipeGroup { name: string; group: string; declaration: Span; arguments: Entry[]; outputs: string[]; references: Span[] }
export interface RecipeLayer { id: string; item: Span; declaredAs?: string; mask?: Entry; channels: Entry[] }
export interface RecipeInspection {
  module: string; sdk: string; parameters: RecipeParameter[]; fields: RecipeField[]; helpers: { name: string; declaration: Span }[];
  groups: RecipeGroup[]; base: Entry[]; layers: RecipeLayer[]; channels: Entry[]; live: Entry[];
}

/** One structured edit; Rust turns it into minimal text patches and re-checks the result. */
export type EditOp =
  | { op: "setLiteral"; offset: number; value: number }
  | { op: "reconnect"; start: number; end: number; expression: string }
  | { op: "setParameterDefault"; name: string; value: number | boolean | number[] }
  | { op: "setParameterRange"; name: string; range: [number, number] | null }
  | { op: "addLayer"; source: string; index?: number }
  | { op: "removeLayer"; id: string }
  | { op: "moveLayer"; id: string; to: number };

export interface CatalogOperation { id: string; summary: string; example: string; family?: string }
export interface Catalog {
  catalogVersion: number; module: string;
  operations: CatalogOperation[]; operators: CatalogOperation[]; groups: { id: string; title: string; summary: string; example: string; unavailable?: string }[];
  rendererOperations: CatalogOperation[]; inputs: { tsName: string; summary: string }[];
}
export interface RecipeModules { entry: string; modules: Record<string, string>; module?: string }

export type RecipeAction = "check" | "inspect" | "edit" | "catalog" | "sdk";
export interface RecipeRequestMessage { requestId: number; action: RecipeAction; request?: unknown }
type Failure = { success: false; diagnostics: AuthoringDiagnostic[] };
export type RecipeResult<T> = ({ success: true } & T) | Failure;

export interface RecipeCalls {
  check: { metadata: unknown };
  inspect: { inspection: RecipeInspection };
  edit: { patches: Patch[]; source: string };
  catalog: never;
  sdk: { module: string; files: Record<string, string> };
}

/** Calls the shared Rust recipe tools in a dedicated worker, independent of running bakes. */
export class RecipeTools {
  private worker?: Worker;
  private next = 1;
  private readonly pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();

  private start(): Worker {
    if (this.worker) return this.worker;
    const worker = new Worker(new URL("./recipe-worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (event: MessageEvent<{ requestId: number; result?: unknown; error?: string }>) => {
      const waiting = this.pending.get(event.data.requestId);
      if (!waiting) return;
      this.pending.delete(event.data.requestId);
      if (event.data.error !== undefined) waiting.reject(new Error(event.data.error));
      else waiting.resolve(event.data.result);
    };
    worker.onerror = event => {
      const error = new Error(event.message || "Recipe worker failed.");
      for (const waiting of this.pending.values()) waiting.reject(error);
      this.pending.clear(); worker.terminate(); this.worker = undefined;
    };
    return this.worker = worker;
  }

  private call<T>(action: RecipeAction, request?: unknown): Promise<T> {
    const requestId = this.next++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(requestId, { resolve: resolve as (value: unknown) => void, reject });
      this.start().postMessage({ requestId, action, request } satisfies RecipeRequestMessage);
    });
  }

  check(project: RecipeModules): Promise<RecipeResult<RecipeCalls["check"]>> { return this.call("check", project); }
  inspect(project: RecipeModules): Promise<RecipeResult<RecipeCalls["inspect"]>> { return this.call("inspect", project); }
  edit(project: RecipeModules, op: EditOp): Promise<RecipeResult<RecipeCalls["edit"]>> { return this.call("edit", { ...project, op }); }
  catalog(): Promise<Catalog> { return this.call("catalog"); }
  sdk(): Promise<RecipeCalls["sdk"]> { return this.call("sdk"); }

  dispose(): void {
    for (const waiting of this.pending.values()) waiting.reject(new DOMException("Recipe tools closed.", "AbortError"));
    this.pending.clear(); this.worker?.terminate(); this.worker = undefined;
  }
}
