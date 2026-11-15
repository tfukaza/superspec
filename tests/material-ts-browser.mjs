/** TypeScript recipes in the built Material Studio, through the production Wasm workers. */
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
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

const SDK = "@superspec/material/v1";
const shared = `import { defineMaterial, layer, vec3, mix, fbm, seed, surface, smoothstep } from "${SDK}";

// keep this comment through every edit
const noise1 = fbm(surface, seed("a"), 3);
const mask = smoothstep(0.4, 0.6, noise1);

export default defineMaterial({
  base: { baseColor: vec3(0.4, 0.4, 0.4), roughness: mix(0.9, 0.5, mask) },
  layers: [
    layer({ id: "stain", mask, baseColor: vec3(0.1, 0.1, 0.1) }),
    layer({ id: "dust", mask: noise1, roughness: 0.8 }),
  ],
  channels: { height: mix(0, 0.01, mask) },
});
`;

/** Measured, not asserted: wall-clock milliseconds on this machine, including the debounce. */
const timings = {};
let browser;
let reviewPage;
try {
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });  reviewPage = page;
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const address = `http://127.0.0.1:${server.address().port}`;
  const status = () => page.locator("#status");
  const ready = async (message = "") => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 120000 });
    const state = await status().getAttribute("data-state");
    const findings = state === "ready" ? "" : (await page.locator("#diagnostics button").allTextContents()).join("\n");
    assert.equal(state, "ready", `${message} ${await status().textContent()}\n${findings}`);
  };
  const revision = async () => Number(await page.locator("#viewport").getAttribute("data-input-revision"));
  const code = () => page.locator("#code").inputValue();
  const setCode = async text => { await page.locator("#code").fill(text); };
  const panel = () => page.locator("#recipe-body");
  const settled = async () => { await ready(); await waitForFrames(page); };
  const view = () => page.evaluate(() => ({
    meshes: JSON.parse(document.querySelector("#viewport").dataset.meshes).map(({ id }) => id),
    camera: document.querySelector("#viewport").dataset.camera,
  }));

  /** Holds one real worker request; releasing it still runs the production Wasm implementation. */
  const holdRequest = async (action, resolution) => page.evaluate(({ action, resolution }) => {
    const original = Worker.prototype.postMessage;
    window.recipeGate = { held: false, delivered: false };
    Worker.prototype.postMessage = function(message, ...options) {
      if ((action ? message.action !== action : message.resolution !== resolution) || window.recipeGate.held)
        return original.call(this, message, ...options);
      window.recipeGate.held = true;
      Worker.prototype.postMessage = original;
      const received = event => {
        if (event.data.requestId !== message.requestId) return;
        this.removeEventListener("message", received); window.recipeGate.delivered = true;
      };
      this.addEventListener("message", received);
      window.recipeGate.release = () => original.call(this, message, ...options);
      window.recipeGate.restore = () => { Worker.prototype.postMessage = original; this.removeEventListener("message", received); };
    };
  }, { action, resolution });
  const releaseRequest = async () => {
    await page.evaluate(() => window.recipeGate.release());
    await page.waitForFunction(() => window.recipeGate.delivered);
    await waitForFrames(page);
  };
  const savedProject = async () => {
    const pending = page.waitForEvent("download");
    await page.locator("#save-project").click();
    const download = await pending;
    return JSON.parse(await readFile(await download.path(), "utf8"));
  };

  await page.goto(`${address}/material.html`);
  await ready("initial");
  assert.equal(await panel().locator(".recipe-hint").count(), 1, "the default .ms module shows the recipe hint, not an inspector");

  // 1. A TypeScript starter compiles through the shared Rust pipeline and exposes groups, layers and fields.
  await page.locator("#starter").selectOption("tiled-courtyard.material.ts");
  await ready("tiled courtyard");
  await panel().locator('.recipe-layer[data-layer="grout"]').waitFor();
  const layerIds = await panel().locator(".recipe-layer[data-layer]").evaluateAll(nodes => nodes.map(node => node.dataset.layer));
  assert.deepEqual(layerIds, ["grout", "chips", "substrate", "stain", "runoff"]);
  const groups = await panel().locator(".recipe-layer[data-group]").evaluateAll(nodes => nodes.map(node => node.dataset.group));
  assert.deepEqual(groups, ["tiles", "ground", "streaks"]);
  const fieldOptions = await page.locator("#inspection option").evaluateAll(options => options.map(option => option.value));
  for (const field of ["tiles.joint", "tiles.face", "tiles.chip", "tiles.substrate", "ground.stain", "layers.grout.coverage"]) {
    assert.ok(fieldOptions.includes(field), `${field} is inspectable`);
  }
  assert.match(await page.locator("#metadata").textContent(), /Live: modulateDiffuse/);
  const courtyard = await code();
  const before = await view();
  assert.ok(before.meshes.length > 0);

  // 2. A numeric edit through the panel changes one literal and keeps the rest of the source.
  const startRevision = await revision();
  const roughness = panel().locator('.recipe-layer[data-layer="substrate"] input[aria-label="roughness"]');
  assert.equal(await roughness.inputValue(), "0.9");
  const editStarted = Date.now();
  await roughness.fill("0.5"); await roughness.press("Tab");
  // A 256 px material shows a 64 px draft first; the draft is never ready, saveable or exportable.
  const draft = await (await page.waitForFunction(start => {
    const viewport = document.querySelector("#viewport");
    if (Number(viewport.dataset.draftRevision ?? 0) <= start) return null;
    return { state: document.querySelector("#status").dataset.state, exportDisabled: document.querySelector("#export-glb").disabled, accepted: viewport.dataset.inputRevision };
  }, startRevision, { timeout: 60000 })).jsonValue();
  timings.draftVisible = Date.now() - editStarted;
  assert.equal(draft.state, "compiling", "the draft is not a ready revision");
  assert.equal(draft.exportDisabled, true, "a draft cannot be exported");
  assert.equal(Number(draft.accepted), startRevision, "the accepted revision is unchanged until the full-size build finishes");
  await page.waitForFunction(start => Number(document.querySelector("#viewport").dataset.inputRevision) > start, startRevision, { timeout: 60000 });
  timings.panelEditToPreview = Date.now() - editStarted;
  await settled();
  const edited = await code();
  assert.equal(edited, courtyard.replace('id: "substrate", mask: tiles.substrate, baseColor: vec3(0.4, 0.38, 0.34), roughness: 0.9', 'id: "substrate", mask: tiles.substrate, baseColor: vec3(0.4, 0.38, 0.34), roughness: 0.5'));
  assert.deepEqual(await view(), before, "model geometry, selection and camera survive a material edit");

  // 3. Reordering moves one layer and keeps comments.
  await panel().locator('.recipe-layer[data-layer="chips"] button[aria-label="Move chips earlier"]').click();
  await page.waitForFunction(() => document.querySelector("#recipe-body .recipe-layer[data-layer]")?.dataset.layer === "chips", {}, { timeout: 60000 });
  await settled();
  assert.ok((await code()).indexOf('id: "chips"') < (await code()).indexOf('id: "grout"'));
  assert.ok((await code()).includes("// Tile and Grout expresses clean square"));

  // 4. Dragging a parameter reuses the compiled recipe and never touches the source.
  const text = await code();
  const tile = page.locator('#parameter-stagger-0');
  const dragRevision = await revision();
  const dragStarted = Date.now();
  await tile.fill("0.25"); await tile.press("Tab");
  await page.waitForFunction(start => Number(document.querySelector("#viewport").dataset.inputRevision) > start, dragRevision, { timeout: 60000 });
  timings.parameterOverrideToPreview = Date.now() - dragStarted;
  await settled();
  assert.equal(await code(), text, "parameter overrides leave the recipe text alone");
  const staggerDefault = panel().locator('[data-recipe-parameter="stagger"]');
  await staggerDefault.getByRole("button", { name: "Use current as default" }).click();
  await page.waitForFunction(() => document.querySelector('[aria-label="stagger default"]')?.value === "0.25");
  await settled();
  const committedDefault = await code();
  await tile.fill("0.75"); await tile.press("Tab"); await settled();
  await staggerDefault.getByRole("button", { name: "Use current as default" }).click();
  await page.waitForFunction(() => document.querySelector('[aria-label="stagger default"]')?.value === "0.75");
  await settled();
  assert.equal(await code(), committedDefault.replace("default: 0.25", "default: 0.75"), "commit uses the newest override with unchanged recipe structure");

  // Reopening another project with identical source still supersedes its predecessor's pending edits.
  const sameSourceProject = await savedProject();
  await tile.fill("0.3"); await tile.press("Tab"); await settled();
  await holdRequest("edit");
  await staggerDefault.getByRole("button", { name: "Use current as default" }).click();
  await page.waitForFunction(() => window.recipeGate.held);
  sameSourceProject.parameters.stagger = 0.65;
  await page.locator("#open-project").setInputFiles({ name: "another.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(sameSourceProject)) });
  await ready("same source in another project");
  await releaseRequest();
  assert.equal(await code(), sameSourceProject.modules[sameSourceProject.entry], "a prior project's reply cannot edit the reopened identical source");
  assert.equal((await savedProject()).parameters.stagger, 0.65);

  // A parameter check from the previous source cannot prune a newer recipe's valid preset.
  await holdRequest("check");
  await setCode(shared);
  await page.waitForFunction(() => window.recipeGate.held);
  const newParameter = shared.replace("defineMaterial, layer,", "defineMaterial, param, layer,")
    .replace("const noise1", 'const stagger = param.scalar({ default: 0.5, range: [0, 1], unit: "m", label: "Tile stagger" });\nconst noise1');
  await setCode(newParameter); await ready("new source retains stagger");
  await releaseRequest();
  assert.equal((await savedProject()).parameters.stagger, 0.65, "an obsolete parameter check never deletes the current override");
  const labelled = page.getByRole("spinbutton", { name: "Tile stagger", exact: true });
  assert.equal(await labelled.getAttribute("id"), "parameter-stagger-0", "authored labels retain stable parameter identities");
  assert.equal(await page.locator('label[for="parameter-stagger-0"]').textContent(), "Tile stagger");
  assert.equal(await page.locator("#parameters .parameter small").textContent(), "m", "the declared unit remains visible");
  await labelled.fill("0.4"); await labelled.press("Tab"); await settled();
  assert.deepEqual((await savedProject()).parameters, { stagger: 0.4 }, "labelled controls still write the declared parameter name");

  // Old numeric spans must not edit a different valid literal after a same-length source reorder.
  const swapped = shared.replace('roughness: 0.8', 'roughness: 0.8, metalness: 0.2');
  await setCode(swapped); await ready("span source");
  await panel().locator('.recipe-layer[data-layer="dust"] input[aria-label="metalness"]').waitFor();
  await holdRequest("inspect");
  const reordered = swapped.replace('roughness: 0.8, metalness: 0.2', 'metalness: 0.2, roughness: 0.8');
  const staleDisabled = await page.evaluate(text => {
    const old = document.querySelector('.recipe-layer[data-layer="dust"] input[aria-label="roughness"]');
    const editor = document.querySelector("#code"); editor.value = text; editor.dispatchEvent(new Event("input"));
    const disabled = old.matches(":disabled");
    old.value = "0.6"; old.dispatchEvent(new Event("change"));
    return disabled;
  }, reordered);
  assert.equal(staleDisabled, true, "old spans are disabled synchronously when source changes");
  await releaseRequest(); await ready("reordered source");
  assert.equal(await code(), reordered, "the old roughness control cannot change metalness");
  const freshRoughness = panel().locator('.recipe-layer[data-layer="dust"] input[aria-label="roughness"]');
  await freshRoughness.fill("0.6"); await freshRoughness.press("Tab");
  await page.waitForFunction(() => document.querySelector("#code").value.includes("metalness: 0.2, roughness: 0.6"));
  await settled();

  // A still-valid edit reply also becomes obsolete when an imported source module changes.
  await holdRequest("edit");
  await freshRoughness.fill("0.4"); await freshRoughness.press("Tab");
  await page.waitForFunction(() => window.recipeGate.held);
  await page.locator("#module").selectOption("common.material.ts");
  const changedHelper = `${await code()}\n// Updated helper while a structured edit was pending.\n`;
  await setCode(changedHelper);
  await page.locator("#module").selectOption("tiled-courtyard.material.ts");
  await releaseRequest(); await ready("changed imported module");
  assert.equal(await code(), reordered.replace("roughness: 0.8", "roughness: 0.6"), "an old edit cannot publish against a different module map");

  // 5. Every use of a shared field reconnects on its own.
  await setCode(shared);
  await ready("shared recipe");
  const field = panel().locator('.recipe-field[data-field="mask"]');
  await field.waitFor();
  assert.match(await field.locator("small").textContent(), /3 uses/);
  const sharedRevision = await revision();
  await field.locator("select").first().selectOption("noise1");
  await page.waitForFunction(start => Number(document.querySelector("#viewport").dataset.inputRevision) > start, sharedRevision, { timeout: 60000 });
  await settled();
  const reconnected = await code();
  assert.equal(reconnected, shared.replace("mix(0.9, 0.5, mask)", "mix(0.9, 0.5, noise1)"), "only the first use changed");
  assert.ok(reconnected.includes("// keep this comment through every edit"));
  assert.match(await panel().locator('.recipe-field[data-field="mask"] small').textContent(), /2 uses/);

  // 6. An invalid edit is rejected with a diagnostic and changes nothing.
  const guarded = await code();
  const guardedRevision = await revision();
  await panel().locator('input[aria-label="New layer id"]').fill("bad");
  await panel().locator('input[aria-label="New layer mask"]').fill("nope");
  await panel().locator(".recipe-add button").click();
  await panel().locator(".recipe-message").filter({ hasText: "Edit rejected" }).waitFor();
  assert.equal(await code(), guarded);
  assert.equal(await revision(), guardedRevision);
  assert.equal(await status().getAttribute("data-state"), "ready");

  // 7. A source error keeps the last valid preview, lists diagnostics, and recovers.
  const goodView = await view();
  await setCode(shared.replace("mix(0.9, 0.5, mask)", "mix(0.9, 0.5)"));
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "error", {}, { timeout: 60000 });
  assert.match(await status().textContent(), /Last valid preview retained/);
  assert.match(await page.locator("#diagnostics button").first().textContent(), /main\.material\.ts|chipped|\.material\.ts:\d+:\d+/);
  assert.deepEqual(await view(), goodView, "the last valid preview is still displayed");
  await setCode(shared);
  await ready("recovered");

  // A superseded low-resolution preview must not survive a later invalid draft.
  await waitForFrames(page);
  const acceptedFrame = await page.locator("#viewport canvas").screenshot();
  const acceptedView = await view();
  const acceptedRevision = await revision();
  await holdRequest(undefined, 256);
  await setCode(shared.replace("vec3(0.4, 0.4, 0.4)", "vec3(0.9, 0.1, 0.1)"));
  await page.waitForFunction(() => window.recipeGate.held);
  await waitForFrames(page);
  assert.notDeepEqual(await page.locator("#viewport canvas").screenshot(), acceptedFrame, "the held refinement has a visibly different real draft");
  await setCode(shared.replace("mix(0.9, 0.5, mask)", "mix(0.9, 0.5)"));
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "error");
  await waitForFrames(page);
  assert.deepEqual(await page.locator("#viewport canvas").screenshot(), acceptedFrame, "failure restores the actual accepted material pixels");
  assert.deepEqual(await view(), acceptedView);
  assert.equal(await revision(), acceptedRevision);
  assert.equal(await page.locator("#export-glb").isDisabled(), true);
  await page.evaluate(() => window.recipeGate.restore());
  await setCode(shared); await ready("draft rollback recovery");

  // 8. Rapid edits never leave the viewport without its meshes (no flicker through an empty scene).
  await page.evaluate(() => {
    window.__empty = 0; window.__frames = 0;
    const tick = () => {
      window.__frames++;
      if (JSON.parse(document.querySelector("#viewport").dataset.meshes ?? "[]").length === 0) window.__empty++;
      window.__watch = requestAnimationFrame(tick);
    };
    tick();
  });
  for (const value of ["0.2", "0.3", "0.4", "0.5", "0.6"]) {
    const input = panel().locator('.recipe-layer[data-layer="dust"] input[aria-label="roughness"]');
    await input.fill(value); await input.press("Tab");
  }
  // Panel edits round-trip through the worker before the textarea changes and compilation starts,
  // so the status can still read "ready" from the previous edit until the newest patch lands.
  await page.waitForFunction(() => document.querySelector("#code").value.includes("roughness: 0.6"), {}, { timeout: 60000 });
  await settled();
  const flicker = await page.evaluate(() => { cancelAnimationFrame(window.__watch); return { empty: window.__empty, frames: window.__frames }; });
  assert.ok(flicker.frames > 5, "frames were observed");
  assert.equal(flicker.empty, 0, "no frame showed an empty scene during rapid edits");
  assert.ok((await code()).includes("roughness: 0.6"), "the newest edit won");

  // 9. The operation catalog is searchable and inserts examples at the caret.
  await panel().locator('input[aria-label="Search SDK operations"]').fill("tile");
  await panel().locator(".recipe-operation", { hasText: "tileAndGrout" }).first().click();
  assert.ok((await code()).includes("tileAndGrout({ size: vec2(0.3, 0.15), rowOffset: 0.5 })"));
  await page.locator("#code").fill(shared); await ready("restored");

  assert.deepEqual(errors, [], "no page errors");
  console.log("TypeScript recipe studio acceptance passed.");
  console.log(`Timings (ms, 256 px texture, includes the 180 ms edit debounce): ${JSON.stringify(timings)}`);
} catch (error) {
  if (reviewPage) console.error(await reviewPage.evaluate(() => ({ status: document.querySelector("#status")?.textContent,
    recipeMessage: document.querySelector(".recipe-message")?.textContent, source: document.querySelector("#code")?.value,
    parameters: [...document.querySelectorAll(".recipe-parameter")].map(row => ({ name: row.dataset.recipeParameter, value: row.querySelector("input")?.value })) })));
  throw error;
} finally {
  await browser?.close();
  server.close();
}
