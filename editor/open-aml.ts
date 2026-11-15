import { MAX_COMPANION_BYTES, MAX_COMPANIONS, MAX_FILE_BYTES, megabytes } from "../browser/model/limits.ts";
export interface AmlSelection {
  source: string;
  assets: Record<string, Uint8Array>;
}

/** Reads one AML entry and its relative companions without changing the editing session. */
export async function readAmlSelection(files: readonly File[], isCurrent: () => boolean): Promise<AmlSelection> {
  if (!files.length) throw new Error("Select an AML file or its containing folder.");
  const paths = files.map(file => file.webkitRelativePath || file.name);
  const entries = paths.filter(path => /\.(aml|xml)$/i.test(path));
  const scenes = entries.filter(path => /(^|\/)scene\.aml$/i.test(path));
  const candidates = scenes.length ? scenes : entries;
  if (candidates.length !== 1) throw new Error("Select a folder with one scene.aml, or exactly one AML file.");
  const entry = candidates[0];
  const directory = entry.slice(0, entry.lastIndexOf("/") + 1);
  const assets: Record<string, Uint8Array> = Object.create(null);
  const names = new Set<string>();
  let total = 0;
  for (let i = 0; i < files.length; i++) {
    const path = paths[i];
    if (!path.startsWith(directory)) throw new Error(`Companion ${path} is outside the AML folder.`);
    const name = path.slice(directory.length);
    if (!name || name.includes("\\") || name.includes(":") || name.split("/").some(part => !part || part === "." || part === "..")) {
      throw new Error(`Invalid companion path: ${path}.`);
    }
    if (names.has(name)) throw new Error(`Duplicate companion path: ${name}.`);
    names.add(name);
    if (files[i].size > MAX_FILE_BYTES) throw new Error(`${name} exceeds the ${megabytes(MAX_FILE_BYTES)} file limit.`);
    total += files[i].size;
  }
  if (files.length > MAX_COMPANIONS + 1 || total > MAX_COMPANION_BYTES) throw new Error(`AML selection exceeds ${MAX_COMPANIONS} companions or ${megabytes(MAX_COMPANION_BYTES)} in total.`);
  let source = "";
  for (let i = 0; i < files.length; i++) {
    if (!isCurrent()) throw new DOMException("AML selection superseded.", "AbortError");
    if (paths[i] === entry) source = await files[i].text();
    else assets[paths[i].slice(directory.length)] = new Uint8Array(await files[i].arrayBuffer());
    if (!isCurrent()) throw new DOMException("AML selection superseded.", "AbortError");
  }
  return { source, assets };
}
