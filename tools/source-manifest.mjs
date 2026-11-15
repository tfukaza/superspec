import { createHash } from "node:crypto";
import { readFile, readdir, stat, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { resolve, relative } from "node:path";

const root = fileURLToPath(new URL("../", import.meta.url));
const roots = [".gitattributes", ".fallowrc.json", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "package.json", "package-lock.json", "tsconfig.json", "vite.rust.config.ts", "LICENSE", "crates", "assets", "schema", "editor", "browser", "tests", "tools", "docs/AML.md", "docs/AUTHORING.md", ".github/workflows"];

/** Identifies the portable source distribution, including all implementation assets and tests. */
export async function sourceManifest() {
  const files = {};
  async function visit(path) {
    const entries = await readdir(path, { withFileTypes: true });
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name, "en"))) {
      const full = resolve(path, entry.name);
      if (entry.isDirectory()) await visit(full);
      else if (entry.isFile()) await record(full);
      else throw new Error(`Unsupported distribution entry ${full}.`);
    }
  }
  async function record(path) {
    files[relative(root, path).replaceAll("\\", "/")] = createHash("sha256").update(await readFile(path)).digest("hex");
  }
  for (const path of roots) {
    if ((await stat(resolve(root, path))).isDirectory()) await visit(resolve(root, path));
    else await record(resolve(root, path));
  }
  const ordered = Object.fromEntries(Object.entries(files).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0));
  return { schemaVersion: 1, baseline: "fb5df8cbf1a3c8b5a8a75a66bf976678cf2106ff", sha256: createHash("sha256").update(JSON.stringify(ordered)).digest("hex"), files: ordered };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const manifest = await sourceManifest();
  if (process.argv[2] === "write") {
    await writeFile(resolve(root, "superspec.manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  } else if (process.argv[2] === "verify") {
    const pinned = JSON.parse(await readFile(resolve(root, "superspec.manifest.json"), "utf8"));
    if (manifest.sha256 !== pinned.sha256) throw new Error("SuperSpec source differs from its distribution manifest. Regenerate and review the consumer pin after intentional changes.");
  } else throw new Error("Usage: node tools/source-manifest.mjs write|verify");
  console.log(`SuperSpec ${manifest.sha256} (${Object.keys(manifest.files).length} files)`);
}
