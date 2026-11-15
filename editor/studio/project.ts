import { MAX_COMPANION_BYTES, MAX_COMPANIONS, MAX_FILE_BYTES, megabytes } from "../../browser/model/limits.ts";
import type { AuthoringProject, SourceEdge } from "../../browser/runtime/authoring.ts";

export type StudioProject = AuthoringProject;
type SerializedProject = Omit<StudioProject, "assets"> & { assets: Record<string, string> };
const maximumBytes = MAX_COMPANION_BYTES;

/** Converts bounded binary companions without spreading a large array onto the call stack. */
function encode(bytes: Uint8Array): string {
  let binary = "";
  for (let start = 0; start < bytes.length; start += 8192) binary += String.fromCharCode(...bytes.subarray(start, start + 8192));
  return btoa(binary);
}

/** Saves the complete editing inputs; source modules and original scene bytes remain authored data. */
export function serializeProject(project: StudioProject): string {
  const assets = Object.fromEntries(Object.entries(project.assets).map(([name, bytes]) => [name, encode(bytes)]));
  return JSON.stringify({ ...project, assets }, null, 2) + "\n";
}

/** Admits only the versioned studio bundle and bounded source/companion inputs. Rust validates authoring semantics. */
export function parseProject(text: string, kind: StudioProject["kind"]): StudioProject {
  const value = JSON.parse(text) as SerializedProject;
  if (![1, 2].includes(value.schemaVersion) || value.kind !== kind || typeof value.entry !== "string"
    || !value.modules || typeof value.modules !== "object" || Array.isArray(value.modules)
    || !Object.values(value.modules).every(source => typeof source === "string")
    || !Object.hasOwn(value.modules, value.entry) || !value.parameters || typeof value.parameters !== "object"
    || !Number.isInteger(value.seed) || !Number.isInteger(value.resolution) || value.resolution < 16
    || value.resolution > (value.profile === "production" ? 2048 : 512)
    || (value.profile !== undefined && !["preview", "production"].includes(value.profile))
    || !["uv", "object", "world"].includes(value.mapping) || typeof value.inspection !== "string"
    || !Array.isArray(value.domain) || value.domain.length !== 2 || !value.domain.every(n => Number.isFinite(n) && n > 0)
    || !value.base || !["swatch", "aml", "glb"].includes(value.base.kind)
    || !value.target || !["mesh", "binding"].includes(value.target.scope) || typeof value.target.enabled !== "boolean"
    || !value.assets || typeof value.assets !== "object" || Array.isArray(value.assets)) {
    throw new Error("This is not a supported version 1 or 2 studio project.");
  }
  const assets: Record<string, Uint8Array> = Object.create(null);
  let bytes = 0;
  if (Object.keys(value.assets).length > MAX_COMPANIONS || Object.keys(value.modules).length > 128) throw new Error("Project contains too many files.");
  for (const [name, encoded] of Object.entries(value.assets)) {
    if (typeof encoded !== "string") throw new Error(`Invalid companion ${name}.`);
    const binary = atob(encoded);
    bytes += binary.length;
    if (binary.length > MAX_FILE_BYTES || bytes > maximumBytes) throw new Error(`Project companion ${name} exceeds the ${megabytes(MAX_FILE_BYTES)} file or ${megabytes(maximumBytes)} total limit.`);
    assets[name] = Uint8Array.from(binary, character => character.charCodeAt(0));
  }
  for (const source of Object.values(value.modules)) {
    bytes += new TextEncoder().encode(source).length;
    if (source.length > 2 * 1024 * 1024 || bytes > maximumBytes) throw new Error("Project source byte limit exceeded.");
  }
  if (value.base.kind === "aml" && typeof value.base.source !== "string") throw new Error("Project is missing its AML scene.");
  if (value.base.kind === "glb" && (!value.base.name || !assets[value.base.name])) throw new Error("Project is missing its original GLB.");
  parseScene(value, assets);
  const environment = parseEnvironment(JSON.stringify({ edges: value.edges === undefined ? [] : value.edges, gravity: value.gravity === undefined ? [0, 0, -1] : value.gravity }));
  return { ...value, ...environment, assets };
}

/** Admits a version 2 project's models and material assignments; Rust checks their meshes. */
function parseScene(value: SerializedProject, assets: Record<string, Uint8Array>): void {
  const identity = (id: unknown): id is string => typeof id === "string" && /^[A-Za-z0-9._-]{1,64}$/.test(id);
  const models = value.models ?? [], materials = value.materials ?? [];
  if (!Array.isArray(models) || models.length > 16 || !Array.isArray(materials) || materials.length > 64) throw new Error("Project models or materials exceed their limits.");
  if ((models.length || materials.length) && value.schemaVersion !== 2) throw new Error("Models and materials need a version 2 project.");
  const ids = new Set<string>();
  for (const model of models) {
    if (!model || !identity(model.id) || ids.has(model.id) || !["glb", "aml"].includes(model.kind)) throw new Error(`Invalid project model ${String(model?.id)}.`);
    ids.add(model.id);
    if (model.kind === "aml" && typeof model.source !== "string") throw new Error(`Model ${model.id} is missing its AML scene.`);
    if (model.kind === "glb" && (!model.name || !assets[model.name])) throw new Error(`Model ${model.id} is missing its GLB.`);
  }
  const materialIds = new Set<string>();
  for (const material of materials) {
    if (!material || !identity(material.id) || materialIds.has(material.id) || typeof material.entry !== "string" || !Object.hasOwn(value.modules, material.entry)
      || !material.parameters || typeof material.parameters !== "object" || !Number.isInteger(material.seed)
      || !Array.isArray(material.meshes) || !material.meshes.every(mesh => typeof mesh === "string")) throw new Error(`Invalid project material ${String(material?.id)}.`);
    materialIds.add(material.id);
  }
}

/** Admits finite world-space source segments and a nonzero gravity direction before replacing accepted inputs. */
export function parseEnvironment(text: string): { edges: SourceEdge[]; gravity: number[] } {
  const value = JSON.parse(text);
  const vector = (numbers: unknown): numbers is number[] => Array.isArray(numbers) && numbers.length === 3
    && numbers.every(number => typeof number === "number" && Number.isFinite(number) && Math.abs(number) <= 1e12);
  if (!value || !vector(value.gravity) || Math.hypot(...value.gravity) < 1e-12 || !Array.isArray(value.edges)
    || value.edges.length > 512 || value.edges.some((edge: SourceEdge) => !edge || !vector(edge.start) || !vector(edge.end))) {
    throw new Error("Use up to 512 finite edges with three-coordinate start/end points and a nonzero gravity vector.");
  }
  return { edges: value.edges, gravity: value.gravity };
}

/** Offers an ordinary download and releases its temporary object URL after browser dispatch. */
export function download(name: string, content: string | ArrayBuffer, type = "application/json"): void {
  const link = document.createElement("a");
  const url = URL.createObjectURL(new Blob([content], { type }));
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

/** Copies mutable draft state so an asynchronous open can commit atomically. */
export function copyProject(project: StudioProject): StudioProject {
  return { ...project, modules: { ...project.modules }, parameters: structuredClone(project.parameters),
    assets: { ...project.assets }, base: { ...project.base }, target: { ...project.target }, domain: [...project.domain],
    edges: structuredClone(project.edges), gravity: [...project.gravity],
    ...(project.models ? { models: project.models.map(model => ({ ...model })) } : {}),
    ...(project.materials ? { materials: structuredClone(project.materials) } : {}) };
}

/** Combines inputs without allowing a new scene to silently replace a recipe's existing image. */
export function mergeAssets(existing: Record<string, Uint8Array>, incoming: Record<string, Uint8Array>): Record<string, Uint8Array> {
  const merged = { ...existing };
  for (const [name, bytes] of Object.entries(incoming)) {
    const previous = merged[name];
    if (previous && (previous.length !== bytes.length || previous.some((value, index) => value !== bytes[index]))) {
      throw new Error(`Companion ${name} conflicts with an already loaded input. Rename the companion and its source reference before opening it.`);
    }
    merged[name] = bytes;
  }
  if (Object.keys(merged).length > MAX_COMPANIONS || Object.values(merged).reduce((sum, bytes) => sum + bytes.length, 0) > maximumBytes) {
    throw new Error("Combined project companions exceed file limits.");
  }
  return merged;
}
