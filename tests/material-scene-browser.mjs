/** Multi-model, multi-material scenes in the built Material Studio, through the production Wasm worker. */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "index.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".png": "image/png" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

const timings = {};
let browser;
try {
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  // Shader compile and link failures surface as console errors from Three.js.
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  const status = () => page.locator("#status");
  const ready = async (message = "") => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 240000 });
    const state = await status().getAttribute("data-state");
    const findings = state === "ready" ? "" : (await page.locator("#diagnostics button").allTextContents()).join("\n");
    assert.equal(state, "ready", `${message} ${await status().textContent()}\n${findings}`);
  };
  /** Waits for a build that starts after `action`, then for it to finish. */
  const rebuilt = async (action, message) => {
    const before = Number(await page.locator("#viewport").getAttribute("data-input-revision") ?? 0);
    await action();
    await page.waitForFunction(start => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > start, before, { timeout: 240000 });
    await ready(message); await waitForFrames(page);
  };
  const bindings = () => page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.bindings ?? "{}"));
  /** The rendered material identity of each visible mesh. */
  const shown = () => page.evaluate(() => Object.fromEntries(window.__studioScene?.() ?? []));
  const meshes = () => page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.meshes).map(({ id }) => id));
  const materialRows = () => page.locator("#material-list li").evaluateAll(rows => rows.map(row => row.dataset.material));
  const activeRow = () => page.locator('#material-list .scene-choose[aria-pressed="true"]').textContent();
  const savedProject = async () => {
    const pending = page.waitForEvent("download");
    await page.locator("#save-project").click();
    return JSON.parse(await readFile(await (await pending).path(), "utf8"));
  };

  await page.goto(`http://127.0.0.1:${server.address().port}/material.html`);
  await ready("initial");

  // 1. The Kowloon yard starter composes two models and previews four materials at once.
  let started = Date.now();
  await rebuilt(() => page.locator("#starter").selectOption("kowloon-yard"), "yard");
  timings.yardFirstFull = Date.now() - started;
  const ids = await meshes();
  for (const id of ["yard/wall", "yard/slab", "steel/crate", "steel/post"]) assert.ok(ids.includes(id), `${id} in ${ids}`);
  assert.deepEqual(await materialRows(), ["concrete-wall", "concrete-slab", "rusted-steel", "painted-steel"]);
  assert.deepEqual(await bindings(), { "yard/wall": "concrete-wall", "yard/slab": "concrete-slab", "steel/crate": "rusted-steel", "steel/post": "painted-steel" });
  assert.match(await activeRow(), /concrete-wall/);
  assert.equal(await page.locator("#module").inputValue(), "kowloon-concrete.material.ts");
  // Steel and concrete retain their respective live operations in the viewer.
  const live = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.liveOperations ?? "{}"));
  for (const mesh of ["steel/crate", "steel/post"]) {
    assert.deepEqual(live[mesh], ["modulateDiffuse", "grazingFalloff", "scaleSpecular", "filterEnvironment", "bandLuminance", "edgeGlint"], `live operations on ${mesh}`);
  }
  for (const mesh of ["yard/wall", "yard/slab"])
    assert.deepEqual(live[mesh], ["grazingFalloff", "scaleSpecular"]);
  await waitForFrames(page);
  assert.equal(errors.length, 0, `shaders compile: ${errors.join("\n")}`);
  assert.ok(await page.getByRole("spinbutton", { name: "Chip amount", exact: true }).isVisible(), "the active concrete material's controls");
  const yardProject = await savedProject();
  assert.equal(yardProject.schemaVersion, 2);
  assert.equal(yardProject.models.length, 2);
  for (const name of ["textures/concrete-plain.png", "textures/concrete-fine-sand.png"]) assert.ok(yardProject.assets[name], `${name} saved with the project`);

  // 2. Switching material shows its own source and controls; editing one leaves the others alone.
  const before = await page.evaluate(() => document.querySelector("#channels")?.innerHTML.length ?? 0);
  await rebuilt(() => page.locator('#material-list li[data-material="rusted-steel"] .scene-choose').click(), "activate steel");
  assert.equal(await page.locator("#module").inputValue(), "kowloon-rusted-steel.material.ts");
  const edgeRust = page.getByRole("spinbutton", { name: "Edge rust", exact: true });
  assert.ok(await edgeRust.isVisible());
  started = Date.now();
  await rebuilt(async () => { await edgeRust.fill("0"); await edgeRust.press("Tab"); }, "edge rust off");
  timings.steelParameterEdit = Date.now() - started;
  const afterEdit = await savedProject();
  const rusted = afterEdit.materials.find(material => material.id === "rusted-steel");
  const painted = afterEdit.materials.find(material => material.id === "painted-steel");
  assert.equal(rusted.parameters.edgeRust, 0, "the edit belongs to the active material");
  assert.equal(painted.parameters.edgeRust, undefined, "the other steel material keeps its own parameters");
  assert.deepEqual(afterEdit.materials.find(material => material.id === "concrete-wall").parameters, {});
  void before;

  // 3. A mesh can move to another material, and a new material starts from the current module.
  await page.locator("#mesh").selectOption("steel/post");
  await rebuilt(() => page.locator("#assign-mesh").click(), "post to rusted steel");
  assert.equal((await bindings())["steel/post"], "rusted-steel");
  assert.match(await page.locator('#material-list li[data-material="painted-steel"]').textContent(), /0 meshes/);
  await rebuilt(() => page.locator("#new-material").click(), "new material");
  assert.match(await activeRow(), /^material · kowloon-rusted-steel\.material\.ts · 0 meshes/);
  await page.locator("#starter").selectOption("painted-metal.material.ts");
  await ready("painted metal entry");
  await page.locator("#mesh").selectOption("steel/post");
  await rebuilt(() => page.locator("#assign-mesh").click(), "post to painted metal");
  assert.equal((await bindings())["steel/post"], "material");
  assert.equal(await page.locator("#module").inputValue(), "painted-metal.material.ts");

  // 4. Several models can be added at once; their meshes keep separate identities.
  const crate = `<architecture version="1" id="extra"><material id="paint" color="#445566"/><box id="wall" position="0 -0.8 0" width="0.5" depth="0.1" height="0.4" material="paint"/></architecture>`;
  await rebuilt(() => page.locator("#add-models").setInputFiles([
    { name: "extra.aml", mimeType: "text/plain", buffer: Buffer.from(crate) },
    { name: "extra two.aml", mimeType: "text/plain", buffer: Buffer.from(crate.replace('position="0 -0.8 0"', 'position="1 -0.8 0"')) },
  ]), "added models");
  const added = await meshes();
  assert.ok(added.includes("extra/wall") && added.includes("extra-two/wall") && added.includes("yard/wall"), `${added}`);
  assert.deepEqual(await page.locator("#model-list li").evaluateAll(rows => rows.map(row => row.dataset.model)), ["yard", "steel", "extra", "extra-two"]);
  await page.locator('#material-list li[data-material="concrete-wall"] .scene-choose').click(); await ready("concrete active");
  await page.locator("#mesh").selectOption("extra/wall");
  await rebuilt(() => page.locator("#assign-mesh").click(), "extra wall to concrete");
  assert.equal((await bindings())["extra/wall"], "concrete-wall");

  // 5. A saved scene reopens with every model, material and assignment.
  const saved = await savedProject();
  await page.reload(); await ready("reloaded");
  await rebuilt(() => page.locator("#open-project").setInputFiles({ name: "scene.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(saved)) }), "reopened");
  assert.deepEqual(await bindings(), Object.fromEntries(saved.materials.flatMap(material => material.meshes.map(mesh => [mesh, material.id]))));
  assert.deepEqual(await materialRows(), saved.materials.map(material => material.id));

  // 6. The same project exports through build_project.
  const pending = page.waitForEvent("download");
  await page.locator("#export-glb").click();
  const glb = await readFile(await (await pending).path());
  assert.equal(glb.subarray(0, 4).toString("latin1"), "glTF");
  assert.ok(glb.length > 100_000, `GLB carries baked maps (${glb.length} bytes)`);
  await ready("exported");

  // 7. Strict TypeScript runs on demand in its own worker, over every recipe module.
  const typeStatus = () => page.locator("#type-status");
  started = Date.now();
  await page.locator("#check-types").click();
  await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#type-status")?.dataset.state), {}, { timeout: 120000 });
  timings.firstTypeCheck = Date.now() - started;
  assert.equal(await typeStatus().getAttribute("data-state"), "ready", `${await typeStatus().textContent()}\n${(await page.locator("#type-diagnostics button").allTextContents()).join("\n")}`);
  assert.match(await typeStatus().textContent(), /no problems/);
  await page.locator('#material-list li[data-material="concrete-wall"] .scene-choose').click(); await ready("concrete active for types");
  const concreteSource = await page.locator("#code").inputValue();
  await page.locator("#code").fill(concreteSource + "\nconst broken: Scalar = vec3(1, 1, 1);\n");
  await page.locator("#check-types").click();
  await page.waitForFunction(() => document.querySelector("#type-status")?.dataset.state === "error", {}, { timeout: 120000 });
  const typeFindings = await page.locator("#type-diagnostics button").allTextContents();
  assert.ok(typeFindings.some(text => /kowloon-concrete\.material\.ts:\d+:\d+ TS2322/.test(text)), typeFindings.join("\n"));
  await page.locator("#code").fill(concreteSource); await ready("restored concrete");

  // 8. Removing a model drops its meshes from every material.
  await rebuilt(() => page.getByRole("button", { name: "Remove model extra-two" }).click(), "removed model");
  assert.ok(!(await meshes()).some(id => id.startsWith("extra-two/")));
  assert.equal(errors.filter(error => !/Source could not be compiled|Material evaluation|MS_/.test(error)).length, 0, errors.join("\n"));
  void shown;
  console.log(`Material scene browser acceptance passed. Timings (ms, 128 px scene, includes the 64 px draft): ${JSON.stringify(timings)}`);
} finally {
  await browser?.close();
  server.close();
}
