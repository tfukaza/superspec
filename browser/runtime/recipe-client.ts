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

export abstract class RecipeClient {
  protected abstract call<T>(action: RecipeAction, request?: unknown): Promise<T>;
  abstract dispose(): void;
  check(project: RecipeModules): Promise<RecipeResult<RecipeCalls["check"]>> { return this.call("check", project); }
  inspect(project: RecipeModules): Promise<RecipeResult<RecipeCalls["inspect"]>> { return this.call("inspect", project); }
  edit(project: RecipeModules, op: EditOp): Promise<RecipeResult<RecipeCalls["edit"]>> { return this.call("edit", { ...project, op }); }
  catalog(): Promise<Catalog> { return this.call("catalog"); }
  sdk(): Promise<RecipeCalls["sdk"]> { return this.call("sdk"); }

}
