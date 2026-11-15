/** Built studio acceptance uses native exports and the production Wasm workers, without a substitute engine. */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { PNG } from "pngjs";
import { readFile, writeFile, mkdir, copyFile, rename } from "node:fs/promises";
import { resolve, dirname, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";

const root = resolve("generated/rust-editor");
const output = resolve("generated/studio-acceptance");
const folder = resolve(output, "relocated material scene");
await mkdir(resolve(folder, "textures"), { recursive: true });
const aml = `<architecture version="1" id="studio-panels" name="Material acceptance panels">
<material id="stone" color="#aaa89c" texture="textures/source.png"/>
<material id="other" color="#706d64"/>
<box id="panel-a" width="1" depth="0.1" height="1" material="stone"/>
<box id="panel-b" position="2 0 0" width="1" depth="0.1" height="1" material="stone"/>
<box id="panel-c" position="4 0 0" width="1" depth="0.1" height="1" material="other"/>
</architecture>`;
await writeFile(resolve(folder, "scene.aml"), aml);
await copyFile("tests/fixtures/aml/asymmetric-heightmap.png", resolve(folder, "textures/source.png"));
const executable = resolve(process.env.RUST_CLI ?? `target/release/superspec${process.platform === "win32" ? ".exe" : ""}`);
const studio = resolve(process.env.STUDIO_CLI ?? resolve(dirname(executable), `superspec-studio${process.platform === "win32" ? ".exe" : ""}`));
const glb = execFileSync(executable, ["glb", resolve(folder, "scene.aml")], { maxBuffer: 64 * 1024 * 1024 });
await writeFile(resolve(output, "panels.glb"), glb);
const shifted = resolve(output, "shifted.aml");
await writeFile(shifted, aml.replace('position="2 0 0"', 'position="20 0 0"').replace('textures/source.png', 'relocated material scene/textures/source.png'));
await writeFile(resolve(output, "shifted.glb"), execFileSync(executable, ["glb", shifted], { maxBuffer: 64 * 1024 * 1024 }));

/** Reads the exported material image to compare actual native and Wasm recipe results. */
function nativeChannel(project) {
  const packet = execFileSync(studio, ["scene", project], { maxBuffer: 64 * 1024 * 1024 });
  assert.equal(packet.subarray(0, 4).toString(), "LSP3");
  const jsonLength = packet.readUInt32LE(4);
  const manifest = JSON.parse(packet.subarray(8, 8 + jsonLength).toString());
  const material = manifest.materials.find(material => material.texture !== null);
  const asset = manifest.assets.find(asset => asset.id === material.texture);
  const start = 8 + jsonLength + manifest.vertexCount * 44 + manifest.indexCount * 4 + asset.byteStart;
  return createHash("sha256").update(packet.subarray(start, start + asset.byteLength)).digest("hex");
}
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "index.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));
let browser, page;
try {
  browser = await launchBrowser();
  page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const address = `http://127.0.0.1:${server.address().port}`;
  const ready = async () => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 120000 });
    assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", await page.locator("#status").textContent());
  };
  const state = () => page.evaluate(() => ({
    meshes: JSON.parse(document.querySelector("#viewport").dataset.meshes).map(({ id, bounds }) => ({ id, bounds })),
    // OrbitControls round-trips spherical coordinates; nanometer precision distinguishes actual view changes.
    camera: JSON.stringify(JSON.parse(document.querySelector("#viewport").dataset.camera), (_key, value) => typeof value === "number" ? Math.round(value * 1e9) / 1e9 : value),
    selection: document.querySelector("#mesh").value,
  }));
  const channel = () => page.locator("#channels img").first().evaluate(async image => {
    const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", await (await fetch(image.src)).arrayBuffer()));
    return Array.from(hash, byte => byte.toString(16).padStart(2, "0")).join("");
  });
  const save = async name => {
    const downloaded = page.waitForEvent("download");
    await page.locator("#save-project").click();
    const download = await downloaded;
    const path = resolve(output, name); await download.saveAs(path); return path;
  };
  const exportGlb = async (project, name) => {
    const before = await state();
    const pending = page.waitForEvent("download");
    await page.locator("#export-glb").click();
    const download = await pending;
    const path = resolve(output, name); await download.saveAs(path); await ready();
    const bytes = await readFile(path);
    assert.equal(bytes.subarray(0, 4).toString(), "glTF");
    assert.equal(bytes.readUInt32LE(8), bytes.length);
    assert.deepEqual(bytes, execFileSync(studio, ["glb", project], { maxBuffer: 64 * 1024 * 1024 }), "download uses the same full-project native exporter");
    assert.deepEqual(await state(), before, "export preserves camera, selection and displayed geometry");
  };
  const channelHasLight = () => page.locator("#channels img").first().evaluate(async image => {
    await image.decode();
    const canvas = document.createElement("canvas"); canvas.width = image.naturalWidth; canvas.height = image.naturalHeight;
    const context = canvas.getContext("2d"); context.drawImage(image, 0, 0);
    return context.getImageData(0, 0, canvas.width, canvas.height).data.some((value, index) => index % 4 !== 3 && value > 0);
  });

  await page.goto(`${address}/material.html`); await ready();
  const starterColors = await page.locator("#channels img").first().evaluate(async image => {
    await image.decode();
    const canvas = document.createElement("canvas"); canvas.width = image.naturalWidth; canvas.height = image.naturalHeight;
    const context = canvas.getContext("2d"); context.drawImage(image, 0, 0);
    const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data, colors = new Set();
    for (let at = 0; at < pixels.length; at += 4) if (pixels[at + 3]) colors.add(pixels.subarray(at, at + 3).join(","));
    return colors.size;
  });
  assert.ok(starterColors > 16, "the initial chipped material retains visible spatial variation at its physical scale");
  await waitForFrames(page);
  const litFrame = await page.locator("#viewport").screenshot();
  const litState = await state();
  const litRevision = await page.locator("#viewport").getAttribute("data-input-revision");
  const litChannel = await channel();
  await page.locator("#exposure").press("Home"); await waitForFrames(page);
  assert.notDeepEqual(await page.locator("#viewport").screenshot(), litFrame, "exposure changes the rendered lighting");
  assert.deepEqual(await state(), litState); assert.equal(await channel(), litChannel);
  assert.equal(await page.locator("#viewport").getAttribute("data-input-revision"), litRevision, "view lighting does not recompile authored inputs");
  for (let step = 0; step < 15; step++) await page.locator("#exposure").press("ArrowRight");
  await waitForFrames(page);
  const first = await channel();
  await page.getByLabel("chip_size", { exact: true }).fill("0.17");
  await page.getByLabel("chip_size", { exact: true }).press("Tab"); await ready();
  assert.notEqual(await channel(), first, "metadata parameter changes the real generated material");
  await page.locator("#open-folder").setInputFiles(folder); await ready();
  await page.locator("#mesh").selectOption("panel-a");
  const original = await state();
  await page.locator("#scope").selectOption("binding");
  await page.locator("#apply-material").click(); await ready();
  assert.deepEqual(await state(), original, "material binding preserves mesh identity, bounds, camera and selection");
  assert.match(await page.locator('#mesh option[value="panel-a"]').textContent(), /stone.*chipped-concrete\.ms/);
  assert.match(await page.locator('#mesh option[value="panel-c"]').textContent(), /other/);
  const valid = await channel();
  const source = await page.locator("#code").inputValue();
  await page.locator("#code").fill(source + "\noutput roughness = missing_field;\n");
  await page.waitForFunction(() => document.querySelector("#status").dataset.state === "error");
  assert.ok(await page.locator("#export-glb").isDisabled(), "invalid drafts cannot export stale or partial output");
  assert.deepEqual(await state(), original); assert.equal(await channel(), valid);
  assert.ok(await page.locator("#diagnostics button").count(), "compiler error links to source");
  await page.locator("#diagnostics button").first().click();
  assert.ok(await page.locator("#code").evaluate(input => input.selectionEnd > input.selectionStart));
  await page.locator("#code").fill(source); await ready();
  for (const mapping of ["uv", "world", "object"]) {
    await page.locator("#mapping").selectOption(mapping); await ready();
    assert.deepEqual(await state(), original);
  }
  await page.locator("#inspection").selectOption("chips"); await ready();
  assert.notEqual(await channel(), valid, "intermediate field inspection comes from the compiled recipe");
  await page.locator("#inspection").selectOption("normal"); await ready();
  await page.locator("#inspection").selectOption("final"); await ready();
  const project = await save("material.project.json");
  const saved = JSON.parse(await readFile(project, "utf8"));
  assert.equal(saved.schemaVersion, 1); assert.ok(saved.modules["common.ms"]);
  assert.equal(saved.target.binding, "stone", "saved overrides retain the original material binding");
  assert.equal(saved.base.source, aml); assert.ok(saved.assets["textures/source.png"]);
  const savedChannel = await channel();
  assert.equal(nativeChannel(project), savedChannel, "saved material project compiles to the same image in native and Wasm engines");
  await page.locator("#mesh").selectOption("panel-c");
  const selectedOther = JSON.parse(await readFile(await save("selection-only.project.json"), "utf8"));
  assert.equal(selectedOther.target.mesh, "panel-a", "inspection selection cannot change the saved applied receiver");
  await page.locator("#compile").click(); await ready();
  assert.equal((await state()).selection, "panel-c", "recompilation preserves independent inspection selection");
  await exportGlb(project, "material.glb");
  await page.locator("#mesh").selectOption("panel-a");
  const cancelledDownloads = [];
  const recordCancelledDownload = download => cancelledDownloads.push(download);
  page.on("download", recordCancelledDownload);
  await page.evaluate(() => {
    const original = Worker.prototype.postMessage;
    window.exportGate = {};
    Worker.prototype.postMessage = function(message, ...options) {
      if (message.action !== "export-project") return original.call(this, message, ...options);
      Worker.prototype.postMessage = original;
      window.exportGate.release = () => { original.call(this, message, ...options); window.exportGate.finished = true; };
    };
    window.exportGate.restore = () => { Worker.prototype.postMessage = original; };
  });
  try {
    await page.locator("#export-glb").click();
    await page.waitForFunction(() => window.exportGate.release);
    assert.ok(await page.locator("#export-glb").isDisabled());
    await page.locator("#code").fill(source + "\n// Supersedes the pending export.\n"); await ready();
    await page.evaluate(() => window.exportGate.release());
    await page.waitForFunction(() => window.exportGate.finished); await waitForFrames(page);
    assert.equal(cancelledDownloads.length, 0, "a newer edit cancels the pending project download");
  } finally {
    await page.evaluate(() => window.exportGate.restore()); page.off("download", recordCancelledDownload);
  }
  await page.locator("#code").fill(source); await ready();
  await page.locator("#restore-material").click(); await ready();
  assert.match(await page.locator('#mesh option[value="panel-a"]').textContent(), /stone/);
  const assetFolder = resolve(output, "replacement-inputs");
  await mkdir(resolve(assetFolder, "textures"), { recursive: true });
  const replacementImage = resolve(assetFolder, "textures/source.png");
  const redImage = PNG.sync.write({ width: 1, height: 1, data: Buffer.from([255, 25, 10, 255]) });
  await writeFile(replacementImage, redImage);
  await waitForFrames(page);
  const oldBaseFrame = await page.locator("#viewport").screenshot();
  await page.locator("#open-modules-folder").setInputFiles(assetFolder); await ready(); await waitForFrames(page);
  const replacedFrame = await page.locator("#viewport").screenshot();
  assert.notDeepEqual(replacedFrame, oldBaseFrame, "replacing an original companion rebuilds the displayed base scene");
  assert.deepEqual(await state(), original);
  const replacedProject = await save("replacement-image.project.json");
  await exportGlb(replacedProject, "replacement-image.glb");
  await writeFile(replacementImage, "not an image");
  await page.locator("#open-modules-folder").setInputFiles(assetFolder);
  await page.waitForFunction(() => document.querySelector("#status").dataset.state === "error");
  await waitForFrames(page);
  assert.deepEqual(await page.locator("#viewport").screenshot(), replacedFrame, "failed base reconstruction preserves the accepted image");
  assert.deepEqual(await state(), original);
  await writeFile(replacementImage, redImage);
  await page.locator("#open-modules-folder").setInputFiles(assetFolder); await ready();
  await page.locator("#open-project").setInputFiles(project); await ready();
  assert.equal(await channel(), savedChannel);
  assert.deepEqual(await state(), original);

  const rejected = resolve(output, "rejected.project.json");
  await writeFile(rejected, JSON.stringify({ ...saved, target: { ...saved.target, mesh: "removed-surface" } }));
  await page.locator("#open-project").setInputFiles(rejected);
  await page.waitForFunction(() => document.querySelector("#status").dataset.state === "error");
  assert.match(await page.locator("#status").textContent(), /removed-surface/);
  assert.deepEqual(await state(), original, "an explicit stale receiver cannot silently select another surface");
  const bindingOnly = resolve(output, "binding-only.project.json");
  await writeFile(bindingOnly, JSON.stringify({ ...saved, target: { scope: "binding", binding: "stone", enabled: true } }));
  await page.locator("#open-project").setInputFiles(bindingOnly); await ready();
  assert.equal(await channel(), savedChannel);
  assert.equal(nativeChannel(bindingOnly), savedChannel);
  assert.deepEqual(await state(), original);
  await page.locator("#open-project").setInputFiles(project); await ready();
  await writeFile(rejected, JSON.stringify({ ...saved, modules: { ...saved.modules, [saved.entry]: "material 1; output base_color = missing;" } }));
  await page.locator("#open-project").setInputFiles(rejected);
  await page.waitForFunction(() => document.querySelector("#status").dataset.state === "error");
  assert.equal(await page.locator("#code").inputValue(), source);
  assert.equal(await page.locator("#diagnostics button:not(:disabled)").count(), 0, "rejected source findings cannot navigate unrelated retained text");
  assert.deepEqual(await state(), original);
  await page.locator("#open-project").setInputFiles(project); await ready();

  // Hold only browser file reading; the newer draft still compiles through the real worker.
  await page.evaluate(() => {
    const original = File.prototype.text;
    window.heldRead = { release: undefined, finished: false };
    File.prototype.text = async function() {
      File.prototype.text = original;
      await new Promise(resolve => { window.heldRead.release = resolve; });
      const text = await original.call(this); window.heldRead.finished = true; return text;
    };
  });
  await page.locator("#open-project").setInputFiles(project);
  await page.waitForFunction(() => window.heldRead.release);
  await page.locator("#code").fill(source + "\n// Newer draft stays selected.\n"); await ready();
  const newerRevision = await page.locator("#viewport").getAttribute("data-input-revision");
  await page.evaluate(() => window.heldRead.release());
  await page.waitForFunction(() => window.heldRead.finished); await waitForFrames(page);
  assert.match(await page.locator("#code").inputValue(), /Newer draft stays selected/);
  assert.equal(await page.locator("#viewport").getAttribute("data-input-revision"), newerRevision);
  await page.locator("#open-project").setInputFiles(project); await ready();

  const imageRecipe = resolve(output, "image-recipe.project.json");
  await writeFile(imageRecipe, JSON.stringify({ ...saved, parameters: {},
    modules: { ...saved.modules, [saved.entry]: 'material 1; output base_color = sample_color("recipe.png", uv); output roughness = 0.5;' },
    assets: { ...saved.assets, "recipe.png": saved.assets["textures/source.png"] } }));
  await page.locator("#open-project").setInputFiles(imageRecipe); await ready();
  await page.locator("#open-folder").setInputFiles(folder); await ready();
  await page.locator("#apply-material").click(); await ready();
  const preserved = JSON.parse(await readFile(await save("preserved-image.project.json"), "utf8"));
  assert.ok(preserved.assets["recipe.png"], "opening a scene retains existing recipe image inputs");

  const runoff = resolve(output, "runoff.project.json");
  await writeFile(runoff, JSON.stringify({ ...saved, parameters: {}, base: { kind: "swatch" }, target: { scope: "mesh", enabled: false },
    modules: { ...saved.modules, [saved.entry]: "material 1; output base_color = vec3(runoff(0.1, 2.0)); output roughness = 0.7;" },
    edges: [], gravity: [0, 0, -1] }));
  await page.locator("#open-project").setInputFiles(runoff); await ready();
  assert.equal(await channelHasLight(), false, "runoff requires an explicit finite source edge");
  const runoffEnvironment = { edges: [{ start: [0, 0, 1], end: [1, 0, 1] }], gravity: [0, 0, -1] };
  await page.locator("#environment").fill(JSON.stringify(runoffEnvironment)); await ready();
  assert.equal(await channelHasLight(), true, "the authored top edge contributes to the swatch");
  await page.locator("#environment").fill(JSON.stringify({ ...runoffEnvironment, edges: [{ start: [10, 0, 1], end: [11, 0, 1] }] })); await ready();
  assert.equal(await channelHasLight(), false, "an edge outside the receiver cannot create infinite runoff");
  await page.locator("#environment").fill(JSON.stringify({ ...runoffEnvironment, gravity: [0, 0, 0] }));
  await page.waitForFunction(() => document.querySelector("#status").dataset.state === "error");
  assert.equal(await channelHasLight(), false, "invalid physical inputs retain the previous complete material");
  await page.locator("#environment").fill('{"edges":');
  await page.locator("#code").fill((await page.locator("#code").inputValue()) + "\n// Other edits cannot hide invalid physical inputs.\n");
  await page.locator("#compile").click();
  assert.equal(await page.locator("#status").getAttribute("data-state"), "error");
  assert.equal(await page.locator("#environment").inputValue(), '{"edges":');
  assert.ok(await page.locator("#export-glb").isDisabled());
  await page.locator("#environment").fill(JSON.stringify(runoffEnvironment)); await ready();
  const runoffSaved = await save("runoff-saved.project.json");
  const runoffInputs = JSON.parse(await readFile(runoffSaved, "utf8"));
  assert.deepEqual(runoffInputs.edges, runoffEnvironment.edges); assert.deepEqual(runoffInputs.gravity, runoffEnvironment.gravity);
  assert.equal(nativeChannel(runoffSaved), await channel());
  await page.locator("#open-project").setInputFiles(runoffSaved); await ready();
  assert.deepEqual(JSON.parse(await page.locator("#environment").inputValue()), runoffEnvironment);
  await page.locator("#open-project").setInputFiles(project); await ready();
  const companion = resolve(folder, "textures/source.png"), backup = resolve(output, "held.png");
  await rename(companion, backup);
  try {
    await page.locator("#open-folder").setInputFiles(folder);
    await page.waitForFunction(() => document.querySelector("#status").dataset.state === "error");
    assert.match(await page.locator("#status").textContent(), /textures\/source\.png/);
    assert.deepEqual(await state(), original);
  } finally { await rename(backup, companion); }
  await page.locator("#open-folder").setInputFiles(folder); await ready();
  await page.locator("#open-glb").setInputFiles(resolve(output, "panels.glb")); await ready();
  assert.equal(await page.locator("#mesh option").count(), 3);
  await page.locator("#apply-material").click(); await ready();
  const firstImport = await state();
  await page.locator("#open-glb").setInputFiles(resolve(output, "shifted.glb")); await ready();
  assert.notEqual((await state()).camera, firstImport.camera, "opening another GLB frames its new bounds despite a shared model identity");
  const secondImport = await state();
  await page.locator("#apply-material").click(); await ready();
  assert.equal((await state()).camera, secondImport.camera, "material edits preserve the imported model's camera");
  await waitForFrames(page); await page.screenshot({ path: resolve(output, "material.png") });

  await page.goto(`${address}/item.html`); await ready();
  const pipe = await state();
  await page.getByLabel("roughness", { exact: true }).fill("0.8");
  await page.getByLabel("roughness", { exact: true }).press("Tab"); await ready();
  assert.deepEqual(await state(), pipe, "finish changes preserve item geometry and camera");
  const item = await save("pipe.item.project.json");
  assert.equal(nativeChannel(item), await channel(), "saved item finish matches native and Wasm compilation");
  await exportGlb(item, "pipe.glb");
  const itemSaved = JSON.parse(await readFile(item, "utf8"));
  assert.equal(itemSaved.parameters.roughness, 0.8);
  assert.ok(JSON.parse(itemSaved.modules[itemSaved.entry]).slots.surface);
  await page.locator("#starter").selectOption("duct"); await ready();
  assert.notDeepEqual((await state()).meshes, pipe.meshes);
  await page.locator("#open-project").setInputFiles(item); await ready();
  assert.deepEqual((await state()).meshes, pipe.meshes);
  assert.match(await page.locator("#metadata").textContent(), /start|end/);
  await page.locator("#collision").check();
  assert.equal(await page.locator("#viewport").getAttribute("data-collision-visible"), "true");
  await page.locator("#open-glb").setInputFiles(resolve(output, "panels.glb")); await ready();
  assert.equal(await page.locator("#mesh option").count(), 3);
  await page.locator("#assign-material").click(); await ready();
  await waitForFrames(page); await page.screenshot({ path: resolve(output, "item.png") });
  for (const kind of ["material", "item"]) {
    await page.goto(`${address}/${kind}.html`); await ready();
    for (const viewport of [{ width: 1440, height: 1000 }, { width: 1280, height: 900 }]) {
      await page.setViewportSize(viewport); await waitForFrames(page);
      const layout = await page.evaluate(() => ({
        width: innerWidth, documentWidth: document.documentElement.scrollWidth,
        panels: [...document.querySelectorAll(".source-panel,.preview-panel,.inspector")].map(panel => {
          const bounds = panel.getBoundingClientRect(); return { left: bounds.left, right: bounds.right, width: bounds.width, height: bounds.height };
        }),
      }));
      assert.ok(layout.documentWidth <= layout.width, "studio has no horizontal page overflow");
      assert.ok(layout.panels.every(panel => panel.left >= 0 && panel.right <= layout.width && panel.width > 200 && panel.height > 300));
      await page.screenshot({ path: resolve(output, `${kind}-${viewport.width}.png`) });
    }
  }
  assert.deepEqual(errors, []);
  await writeFile(resolve(output, "result.json"), JSON.stringify({ success: true, errors }, null, 2));
  console.log("Material and Item studio real-engine browser acceptance passed.");
} catch (error) {
  if (page) {
    await page.screenshot({ path: resolve(output, "failure.png") }).catch(() => {});
    await writeFile(resolve(output, "failure.json"), JSON.stringify(await page.evaluate(() => ({
      status: document.querySelector("#status")?.textContent,
      diagnostics: document.querySelector("#diagnostics")?.textContent,
      meshes: document.querySelector("#viewport")?.dataset.meshes,
      source: document.querySelector("#code")?.value,
    })).catch(() => ({ closed: true })), null, 2));
  }
  console.error(error);
  throw error;
} finally { await browser?.close(); await new Promise(done => server.close(done)); }
