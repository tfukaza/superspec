import assert from "node:assert/strict";
import { mkdir, mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { sourceManifest } from "./source-manifest.mjs";

test("adapter and vendored codec changes affect source identity independently of installed tooling", async t => {
  const fixture = await mkdtemp(join(tmpdir(), "superspec-source-"));
  t.after(() => rm(fixture, { recursive: true, force: true }));
  async function put(name, text) {
    await mkdir(dirname(join(fixture, name)), { recursive: true });
    await writeFile(join(fixture, name), text);
  }
  for (const entry of await readdir(new URL("../", import.meta.url), { withFileTypes: true })) {
    if (entry.isFile()) await put(entry.name, "");
  }
  for (const path of ["crates", "assets", "schema", "editor", "browser", "tests", "tools", ".github/workflows"])
    await mkdir(join(fixture, path), { recursive: true });
  const adapter = "adapters/three/src/bindScene.ts", codec = "third_party/basisu_c_sys/codec.cpp";
  for (const name of [adapter, codec, "docs/AUTHORING.md", "docs/AML.md"]) await put(name, "v1");
  const original = await sourceManifest(fixture);
  await put(adapter, "v2");
  const updated = await sourceManifest(fixture);
  assert.notEqual(updated.sha256, original.sha256);
  await put(codec, "v2");
  const expected = await sourceManifest(fixture);
  assert.notEqual(expected.sha256, updated.sha256);
  for (const name of ["tools/headless/node_modules/p/index.js", "adapters/three/dist/index.js",
    "tools/target/debug/lib", "crates/src/__pycache__/tool.pyc", "tools/generated/assets",
    "editor/tsconfig.tsbuildinfo", "crates/codec/build.log"]) await put(name, "local");
  assert.deepEqual(await sourceManifest(fixture), expected);
});
