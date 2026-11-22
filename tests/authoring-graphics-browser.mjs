/** Run with a built native viewer and STUDIO_CLI pointing to superspec-studio. */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";

const viewer = resolve(process.env.STUDIO_UI_ROOT ?? "generated/native-viewer");
const cli = resolve(process.env.STUDIO_CLI ?? `target/release/superspec-studio${process.platform === "win32" ? ".exe" : ""}`);
const tree = resolve("crates/engine/src/treespec/fixtures/birch.tree.json");
const studio = spawn(cli, ["serve", "--ui", viewer, "--port", "0"], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
const stop = once(studio, "exit").catch(() => {});
let browser;
try {
  const address = await new Promise((ready, fail) => {
    createInterface({ input: studio.stderr }).on("line", line => {
      const match = line.match(/listening at (http:\/\/[^/\s]+)/);
      if (match) ready(match[1]);
    });
    studio.once("error", fail);
    studio.once("exit", code => fail(Error(`Native Studio exited with status ${code}.`)));
  });
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1.25 });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const graphics = page.getByLabel("Graphics", { exact: true });
  const status = page.locator("#status");
  const ready = async () => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), undefined, { timeout: 120000 });
    assert.equal(await status.getAttribute("data-state"), "ready", await status.textContent());
  };
  async function assertGraphics(name, preset, scale, shadowSize) {
    await page.waitForFunction(({ preset, scale }) => {
      const error = document.querySelector(".graphics-profile-error:not([hidden])");
      if (error?.textContent) throw Error(error.textContent);
      const view = window.__scene;
      const canvas = view?.renderer.domElement;
      const ratio = Math.min(devicePixelRatio, 2) * scale;
      return !!canvas && document.querySelector('[aria-label="Graphics"]')?.value === preset
        && canvas.width === Math.floor(canvas.clientWidth * ratio) && canvas.height === Math.floor(canvas.clientHeight * ratio);
    }, { preset, scale });
    await waitForFrames(page, 2);
    const state = await page.evaluate(() => {
      const view = window.__scene;
      const shadow = view.lights.sun.shadow;
      return { ratio: view.renderer.getPixelRatio(), cap: view.renderer.capabilities.maxTextureSize,
        shadowSize: shadow.mapSize.width, shadowAllocation: shadow.map?.width,
        tier: [...(view.foliage.patches ?? [])]
          .flatMap(patch => [...patch.visibility.chosen.values()]).filter(level => level >= 0) };
    });
    assert.equal(state.ratio, 1.25 * scale, `${name} actual pixel ratio`);
    assert.equal(state.shadowSize, Math.min(shadowSize, state.cap), `${name} actual shadow map size`);
    if (state.shadowAllocation !== undefined)
      assert.equal(state.shadowAllocation, state.shadowSize, `${name} allocated shadow map matches current settings`);
    return state;
  }
  const snapshot = () => page.evaluate(() => {
    const view = window.__scene, host = document.querySelector("#viewport");
    return { revision: host.dataset.revision, inputRevision: host.dataset.inputRevision,
      pose: [...view.camera.position.toArray(), ...view.controls.target.toArray()].map(coordinate => Math.round(coordinate * 1e9)),
      models: view.models.children.map(model => [model.name, model.uuid, model.geometry.uuid]),
      sameDocument: window.authoringGraphicsDocument === document };
  });
  const statusState = expected => page.waitForFunction(state => document.querySelector("#status")?.dataset.state === state, expected, { timeout: 120000 });
  const cases = [
    ["WorldSpec", "index.html", "#aml", text => text.replace("</architecture>", "<unrecognized/></architecture>")],
    ["MaterialSpec", "material.html", "#code", text => text + "\noutput roughness = missing_field;\n"],
    ["ItemSpec", "item.html", "#code", text => text + "\n% invalid item JSON"],
    ["TreeSpec", "native.html"],
  ];
  for (const [name, entry, field, edit] of cases) {
    await page.goto(`${address}/${entry}?gpuprobe`);
    if (name === "TreeSpec") {
      await graphics.waitFor({ state: "visible" });
      await page.locator("#file").setInputFiles(tree);
    }
    await ready();
    await assertGraphics(name, "medium", 1, 2048);
    const source = field ? page.locator(field) : undefined;
    const original = await source?.inputValue();
    const draft = edit?.(original);
    if (source) {
      assert.notEqual(draft, original);
      await source.fill(draft);
      await statusState("error");
    }
    await page.evaluate(isTree => {
      const view = window.__scene;
      window.authoringGraphicsDocument = document;
      if (isTree) {
        // The shipped birch's first transition is 22 m and its second is 45 m.
        const [tile] = view.foliage.patches[0].tiles.values();
        tile.getCenter(view.controls.target);
        view.camera.position.copy(view.controls.target);
        view.camera.position.y = tile.min.y - 26;
      } else view.camera.position.x += 0.25;
      view.controls.update();
      view.draw();
    }, name === "TreeSpec");
    const medium = await assertGraphics(name, "medium", 1, 2048);
    const before = await snapshot();
    assert.ok(before.models.length, `${name} has visible-scene resources before switching`);
    await graphics.selectOption("very-high");
    const higher = await assertGraphics(name, "very-high", 1.25, 4096);
    await waitForFrames(page, 3);
    assert.deepEqual(await snapshot(), before, `${name} switches graphics in place with the accepted scene and camera intact`);
    if (name === "TreeSpec") {
      assert.ok(medium.tier.length && higher.tier.length, "the standalone birch has visible foliage tiers");
      assert.ok(Math.min(...higher.tier) < Math.min(...medium.tier), `higher geometry quality retains a finer birch tier (${medium.tier} → ${higher.tier})`);
    }
    if (source) {
      assert.equal(await source.inputValue(), draft, `${name} retains its unsaved draft`);
      assert.equal(await status.getAttribute("data-state"), "error", `${name} preserves compile status`);
      await source.fill(original);
      await statusState("ready");
    }
    console.log(`${name} switched Medium → Very High without navigation or scene replacement.`);
  }

  const before = await snapshot();
  const options = page.locator("details").filter({ has: page.getByText("Graphics options", { exact: true }) });
  await options.locator("summary").click();
  await options.getByLabel("Render scale (%)", { exact: true }).fill("80");
  await options.getByLabel("Shadows", { exact: true }).selectOption("medium");
  await options.getByRole("button", { name: "Apply", exact: true }).click();
  await assertGraphics("TreeSpec custom", "very-high", 0.8, 2048);
  assert.deepEqual(await snapshot(), before, "custom settings also keep the standalone tree and camera");
  const saved = JSON.parse(new URL(page.url()).searchParams.get("profile"));
  assert.equal(saved.version, 1);
  assert.equal(saved.preset, "very-high");
  assert.equal(saved.overrides.renderScale, 0.8);
  assert.equal(saved.overrides.shadows, "medium");
  assert.deepEqual(errors, []);
} finally {
  await browser?.close();
  if (studio.pid) { studio.kill(); await stop; }
}
