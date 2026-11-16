import { MAX_COMPANION_BYTES, MAX_COMPANIONS, MAX_FILE_BYTES, MAX_PROJECT_FILE_BYTES, megabytes } from "../../browser/model/limits.ts";
import type { AuthoringProject, SourceEdge } from "../../browser/runtime/authoring.ts";
import { ASSET_DIRECTORY, PROJECT_ENTRY, isArchive, readArchive, writeArchive } from "./archive.ts";

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

/**
 * Companion bytes above which Save project writes a `.studio.zip` archive instead of JSON: base64
 * JSON of a few hundred megabytes exceeds what browsers can hold in one string, and is slow to parse.
 */
export const ARCHIVE_THRESHOLD = 64 * 1024 * 1024;

/** The saved form of a project: JSON for ordinary projects, an archive for large companions. */
export function projectFile(project: StudioProject, threshold = ARCHIVE_THRESHOLD): { name: string; content: Blob } {
  const bytes = Object.values(project.assets).reduce((sum, asset) => sum + asset.length, 0);
  if (bytes <= threshold) return { name: `${project.kind}.project.json`, content: new Blob([serializeProject(project)], { type: "application/json" }) };
  const names = Object.keys(project.assets);
  const paths = Object.fromEntries(names.map((name, index) => [name, archivePath(name, index)]));
  const document = JSON.stringify({ ...project, assets: paths }, null, 2) + "\n";
  return {
    name: `${project.kind}.studio.zip`,
    content: writeArchive([{ name: PROJECT_ENTRY, bytes: new TextEncoder().encode(document) },
      ...names.map(name => ({ name: paths[name], bytes: project.assets[name] }))]),
  };
}

/** Companion names become entry paths when they are plain relative paths, so archives unpack cleanly. */
function archivePath(name: string, index: number): string {
  const segments = name.split("/");
  const plain = segments.every(segment => /^[A-Za-z0-9._ -]{1,128}$/.test(segment) && segment !== "." && segment !== "..");
  return `${ASSET_DIRECTORY}${plain ? name : `companion-${index}`}`;
}

/** Opens a saved JSON project or `.studio.zip` archive, recognized by its content. */
export async function openProject(file: Blob, kind: StudioProject["kind"]): Promise<StudioProject> {
  if (file.size > MAX_PROJECT_FILE_BYTES) throw new Error(`The project exceeds the ${megabytes(MAX_PROJECT_FILE_BYTES)} project file limit.`);
  const head = new Uint8Array(await file.slice(0, 4).arrayBuffer());
  return isArchive(head) ? parseArchive(new Uint8Array(await file.arrayBuffer()), kind) : parseProject(await file.text(), kind);
}

/** Admits a `.studio.zip` project: `project.json` maps each companion to its archive entry. */
export async function parseArchive(bytes: Uint8Array, kind: StudioProject["kind"]): Promise<StudioProject> {
  let total = 0;
  const entries = await readArchive(bytes, (name, size) => {
    if (name === PROJECT_ENTRY ? size > 64 * 1024 * 1024 : size > MAX_FILE_BYTES) throw new Error(`Project archive entry ${name} exceeds its ${megabytes(MAX_FILE_BYTES)} limit.`);
    total += size;
    if (total > maximumBytes + 64 * 1024 * 1024) throw new Error(`Project archive exceeds the ${megabytes(maximumBytes)} companion limit.`);
  });
  const document = entries.get(PROJECT_ENTRY);
  if (!document) throw new Error(`The project archive has no ${PROJECT_ENTRY}.`);
  const value = JSON.parse(new TextDecoder().decode(document)) as SerializedProject;
  checkDocument(value, kind);
  const assets: Record<string, Uint8Array> = Object.create(null);
  for (const [name, path] of Object.entries(value.assets)) {
    const bytes = typeof path === "string" && path.startsWith(ASSET_DIRECTORY) ? entries.get(path) : undefined;
    if (!bytes) throw new Error(`The project archive is missing companion ${name}.`);
    assets[name] = bytes;
  }
  return admit(value, assets);
}

/** Admits only the versioned studio bundle and bounded source/companion inputs. Rust validates authoring semantics. */
export function parseProject(text: string, kind: StudioProject["kind"]): StudioProject {
  const value = JSON.parse(text) as SerializedProject;
  checkDocument(value, kind);
  const assets: Record<string, Uint8Array> = Object.create(null);
  for (const [name, encoded] of Object.entries(value.assets)) {
    if (typeof encoded !== "string") throw new Error(`Invalid companion ${name}.`);
    const binary = atob(encoded);
    if (binary.length > MAX_FILE_BYTES) throw new Error(`Project companion ${name} exceeds the ${megabytes(MAX_FILE_BYTES)} file limit.`);
    // A plain loop: megabytes of companions decode several times faster than through a mapping callback.
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index);
    assets[name] = bytes;
  }
  return admit(value, assets);
}

/** The project document's fields and shape, before its companions are read. */
function checkDocument(value: SerializedProject, kind: StudioProject["kind"]): void {
  if (![1, 2].includes(value.schemaVersion) || value.kind !== kind || typeof value.entry !== "string"
    || !value.modules || typeof value.modules !== "object" || Array.isArray(value.modules)
    || !Object.values(value.modules).every(source => typeof source === "string")
    || !Object.hasOwn(value.modules, value.entry) || !value.parameters || typeof value.parameters !== "object"
    || !Number.isInteger(value.seed) || !Number.isInteger(value.resolution) || value.resolution < 16
    || (value.profile !== undefined && !["preview", "production"].includes(value.profile))
    || (value.texelDensity !== undefined && !(Number.isFinite(value.texelDensity) && value.texelDensity >= 1 && value.texelDensity <= 100000))
    || !["uv", "object", "world"].includes(value.mapping) || typeof value.inspection !== "string"
    || !Array.isArray(value.domain) || value.domain.length !== 2 || !value.domain.every(n => Number.isFinite(n) && n > 0)
    || !value.base || !["swatch", "aml", "glb"].includes(value.base.kind)
    || !value.target || !["mesh", "binding"].includes(value.target.scope) || typeof value.target.enabled !== "boolean"
    || !value.assets || typeof value.assets !== "object" || Array.isArray(value.assets)) {
    throw new Error("This is not a supported version 1 or 2 studio project.");
  }
  // Said on its own: a project that only asks for too many texels is otherwise a valid one.
  const limit = value.profile === "production" ? 2048 : 512;
  if (value.resolution > limit) {
    throw new Error(`Project resolution ${value.resolution} is above the ${value.profile ?? "preview"} profile's ${limit} texels${value.profile === "production" ? "" : ' (the "production" profile allows 2048)'}.`);
  }
  if (Object.keys(value.assets).length > MAX_COMPANIONS || Object.keys(value.modules).length > 128) throw new Error("Project contains too many files.");
}

/** Checks companion and source totals and the scene references, then returns the project. */
function admit(value: SerializedProject, assets: Record<string, Uint8Array>): StudioProject {
  let bytes = 0;
  for (const [name, asset] of Object.entries(assets)) {
    bytes += asset.length;
    if (asset.length > MAX_FILE_BYTES || bytes > maximumBytes) throw new Error(`Project companion ${name} exceeds the ${megabytes(MAX_FILE_BYTES)} file or ${megabytes(maximumBytes)} total limit.`);
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
    if (!model || !identity(model.id) || ids.has(model.id) || !["glb", "aml", "item"].includes(model.kind)) throw new Error(`Invalid project model ${String(model?.id)}.`);
    ids.add(model.id);
    if (model.kind === "item") {
      // The engine checks the definition, its controls and its slots; this admits the document's shape.
      if (typeof model.source !== "string" && !(typeof model.entry === "string" && Object.hasOwn(value.modules, model.entry))) throw new Error(`Model ${model.id} is missing its item definition.`);
      if ((model.parameters !== undefined && (typeof model.parameters !== "object" || Array.isArray(model.parameters)))
        || (model.slots !== undefined && (typeof model.slots !== "object" || !Object.values(model.slots).every(material => typeof material === "string")))
        || (model.transform !== undefined && !(Array.isArray(model.transform) && model.transform.length === 4
          && model.transform.every(column => Array.isArray(column) && column.length === 3 && column.every(Number.isFinite))))) throw new Error(`Invalid item model ${model.id}.`);
    }
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
export function download(name: string, content: string | ArrayBuffer | Blob, type = "application/json"): void {
  const link = document.createElement("a");
  const url = URL.createObjectURL(content instanceof Blob ? content : new Blob([content], { type }));
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
    ...(project.models ? { models: structuredClone(project.models) } : {}),
    ...(project.materials ? { materials: structuredClone(project.materials) } : {}),
    ...(project.batches ? { batches: structuredClone(project.batches) } : {}) };
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
