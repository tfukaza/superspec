import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
import { buildProject } from "./studio/washroom/build.mjs";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";

// Exercise the same native authoring CLI, GLB loader and renderer a published world uses.
const out = resolve("generated/exported-lod-review"), viewer = resolve(process.env.LOD_VIEWER ?? "generated/native-viewer");
const cli = process.env.STUDIO_CLI ?? resolve("target/release/superspec-studio" + (process.platform === "win32" ? ".exe" : ""));
await mkdir(out, { recursive: true });
const { project } = buildProject(32, 0, { batches: false });
const concrete = project.entry;
project.materials = project.materials.filter(material => material.id === "concrete-walls");
project.modules["distant.ms"] = "material 1; output base_color = vec3(0.29,0.32,0.31); output roughness = 0.85;";
project.materialLods = { [concrete]: [{ screenCoverage: .22, entry: "distant.ms" }] };
project.modules["test-pipe.item.json"] = JSON.stringify({
  format: "itemspec", version: 1, id: "witness-pipe",
  parameters: { segments: { type: "integer", role: "geometry", default: 24, min: 3, max: 64 } },
  generator: { kind: "pipe", length: 2, radius: .2, segments: { parameter: "segments" }, capped: true },
  lods: [{ distance: 8, parameters: { segments: 8 } }, { distance: 22, parameters: { segments: 4 } }],
});
project.models.push({ id: "witness", kind: "item", entry: "test-pipe.item.json" });
const input = resolve(out, "washroom.studio.json");
await writeFile(input, JSON.stringify(project));
async function exportGlb(input, name) {
  const built = spawnSync(cli, ["glb", input, "--textures", "png"], { maxBuffer: 64 * 1024 * 1024, windowsHide: true });
  if (built.error) throw built.error;
  assert.equal(built.status, 0, built.stderr.toString());
  const bytes = built.stdout;
  await writeFile(resolve(out, name), bytes);
  return JSON.parse(bytes.toString("utf8", 20, 20 + bytes.readUInt32LE(12)));
}
const doc = await exportGlb(input, "washroom.glb");
assert.ok(doc.extensionsUsed.includes("MSFT_lod"));
assert.ok(!doc.extensionsRequired?.includes("MSFT_lod"));
const group = doc.nodes.find(node => node.extensions?.MSFT_lod);
assert.deepEqual(group.extras.worldSpecLod.distances, [0, 8, 22]);
const levels = [group, ...group.extensions.MSFT_lod.ids.map(index => doc.nodes[index])];
const triangles = levels.map(level => level.children.reduce((sum, index) => sum + doc.meshes[doc.nodes[index].mesh].primitives
  .reduce((total, p) => total + doc.accessors[p.indices].count / 3, 0), 0));
assert.ok(triangles[0] > triangles[1] && triangles[1] > triangles[2]);
const materialOwners = doc.materials.filter(material => material.extensions?.MSFT_lod);
assert.ok(materialOwners.length, "authored washroom walls carry their material alternatives");
const materialNames = materialOwners.map(m => [m.name, doc.materials[m.extensions.MSFT_lod.ids[0]].name]);
const tree = await exportGlb(resolve("crates/engine/src/treespec/fixtures/birch.tree.json"), "birch.glb");
const patch = tree.nodes.find(node => node.extensions?.MSFT_lod);
assert.deepEqual(patch.extras.worldSpecLod.distances, [0, 22, 45]);
const treeLevels = [patch, ...patch.extensions.MSFT_lod.ids.map(i => tree.nodes[i])];
const server = spawn(cli, ["serve", "--ui", viewer, "--artifacts", out], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
let browser;
try {
  const address = await new Promise((done, fail) => {
    createInterface({ input: server.stderr }).on("line", line => {
      const match = line.match(/listening at (http:\/\/[^/\s]+)/);
      if (match) done(match[1]);
    });
    server.once("error", fail);
    server.once("exit", code => fail(Error("Viewer service exited with code " + code)));
  });
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => {
    if (message.type() === "error") errors.push([message.location().url, message.text()].filter(Boolean).join(" "));
  });
  async function open(name, fallback = false) {
    await page.goto(address + "/native.html?gpuprobe&gltf=/artifacts/" + name + (fallback ? "&lod=off" : ""));
    await page.waitForFunction(() => window.__worldSpecView || document.querySelector("#status")?.dataset.state === "error", undefined, { timeout: 90000 });
    assert.notEqual(await page.locator("#status").getAttribute("data-state"), "error", await page.locator("#status").textContent());
    await page.addStyleTag({ content: "#viewport { position:fixed; inset:0; width:100vw; height:100vh; z-index:99 }" });
    await waitForFrames(page, 2);
  }
  for (const fallback of [false, true]) {
    await open("washroom.glb", fallback);
    const captures = [];
    for (const [label, distance, tier] of [["near", 1.8, 0], ["mid", 15, 1], ["far", 75, 2], ["return", 1.8, 0]]) {
      const result = await page.evaluate(({ distance, names, materials }) => {
        const { camera, render, scene } = window.__worldSpecView;
        camera.fov = 70; camera.updateProjectionMatrix();
        camera.position.set(.6, 1.3, distance); camera.lookAt(0, 1, 0); camera.updateMatrixWorld();
        render();
        const active = [], shown = new Set();
        scene.traverseVisible(object => {
          const name = object.userData.name ?? object.name;
          if (names.includes(name)) active.push(names.indexOf(name));
          if (object.isMesh) for (const m of [].concat(object.material)) shown.add(m.name);
        });
        return { active, materials: materials.map(pair => pair.map(name => shown.has(name))) };
      }, { distance, names: levels.map(node => node.name), materials: materialNames });
      assert.deepEqual(result.active, [fallback ? 0 : tier], label + " " + JSON.stringify(result));
      if (label !== "mid") {
        const expected = fallback || label !== "far" ? [true, false] : [false, true];
        assert.ok(result.materials.some(pair => pair.every((value, i) => value === expected[i])), label + " " + JSON.stringify(result));
      }
      captures.push({ label, distance, ...result });
      if (!fallback && label !== "return") await page.locator("#viewport").screenshot({ path: resolve(out, label + ".png") });
    }
    await writeFile(resolve(out, fallback ? "fallback.json" : "selection.json"), JSON.stringify({ triangles, captures }, null, 2));
  }
  await open("birch.glb");
  const canopy = [];
  for (const [distance, expected] of [[4,0], [34,1], [90,2], [4,0]]) {
    const active = await page.evaluate(({distance, names}) => {
      const { scene, camera, render } = window.__worldSpecView;
      camera.fov = 70; camera.updateProjectionMatrix();
      camera.position.set(0, 3, distance); camera.lookAt(0, 3, 0); camera.updateMatrixWorld();
      render();
      const active = [];
      scene.traverseVisible(object => {
        const index = names.indexOf(object.userData.name ?? object.name);
        if (index >= 0) active.push(index);
      });
      return active;
    }, { distance, names: treeLevels.map(node => node.name) });
    assert.deepEqual(active, [expected]);
    canopy.push({ distance, active });
  }
  await writeFile(resolve(out, "tree-selection.json"), JSON.stringify(canopy, null, 2));
  assert.deepEqual(errors, []);
  console.log("Packaged viewer passed washroom and birch LOD checks.", triangles, out);
} finally {
  await browser?.close();
  server.kill();
}
