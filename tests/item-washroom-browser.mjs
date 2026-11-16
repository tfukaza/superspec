/**
 * The washroom built from items (tests/studio/washroom-items) in the built Material Studio: opens the project through
 * the production Wasm workers and runs the item MVP's acceptance checks against it.
 *
 * 1. Widening the tall wall keeps the window, tile and decals where their rules put them: the picture of the part of
 *    the wall that existed before is compared before and after.
 * 2. Shortening the pipe re-spaces its couplings and keeps the wall's meshes and maps.
 * 3. Grime follows the wall's base and the window's sill after a resize (the material reads those named features).
 * 4. A decal that lands in an opening is a source-linked diagnostic and the last valid scene stays.
 * 5. A handle drag shows per pixel without a bake; the bake runs on release, and only for what changed.
 *
 * The page is opened with `?ao=off&gpuprobe&shadows=off` unless QUERY says otherwise: fitted shadow maps move with the
 * scene's bounds, which would count as differences in check 1. RESOLUTION (default 256) sets the texture size; SCREENSHOTS names a directory for PNGs of each step;
 * WEATHERING=0 uses the shipped recipes as they are (check 3 is then skipped).
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { PNG } from "pngjs";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom-items/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const resolution = Number(process.env.RESOLUTION ?? 256);
const weathering = process.env.WEATHERING !== "0";
const shots = process.env.SCREENSHOTS ? resolve(process.env.SCREENSHOTS) : null;
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

let browser;
try {
  const { project } = buildProject(resolution, undefined, { weathering, anchored: process.env.ANCHORED === "1", walls: process.env.WALLS });
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  const viewport = name => page.evaluate(name => document.querySelector("#viewport").dataset[name], name);
  const json = async name => JSON.parse(await viewport(name) ?? "null");
  const state = () => page.locator("#status").getAttribute("data-state");
  const settled = async () => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 900000 });
    return state();
  };
  const ready = async () => {
    const findings = await settled() === "ready" ? "" : (await page.locator("#diagnostics button").allTextContents()).join("\n");
    assert.equal(await state(), "ready", `${await page.locator("#status").textContent()}\n${findings}`);
  };
  /** Waits until every mesh shows GPU-baked maps of the scene on screen, and returns the bake's report. */
  const baked = async () => {
    await page.waitForFunction(() => document.querySelector("#viewport").dataset.preview === "gpu-bake"
      && JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "{}").state === "baked", {}, { timeout: 900000 });
    await waitForFrames(page, 3);
    return json("sceneBake");
  };
  const revision = async () => Number(await viewport("inputRevision") ?? 0);
  const shot = async name => {
    // Pictures are taken without the handles.
    const handles = on => page.evaluate(on => {
      const box = document.querySelector("#item-handles"), was = box.checked;
      if (was !== on) { box.checked = on; box.dispatchEvent(new Event("change")); }
      return was;
    }, on);
    const shown = await handles(false);
    await waitForFrames(page, 3);
    const bytes = await page.locator("#viewport canvas").screenshot();
    await handles(shown);
    if (shots) { await mkdir(shots, { recursive: true }); await writeFile(resolve(shots, `${name}.png`), bytes); }
    return PNG.sync.read(bytes);
  };
  /** Sets one control of an item through its number field and waits for the settled, baked scene. */
  const setControl = async (item, parameter, value) => {
    const from = await revision(), began = Date.now();
    const field = page.getByRole("spinbutton", { name: `${item} ${parameter}`, exact: true });
    await field.fill(String(value)); await field.press("Tab");
    await page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, from, { timeout: 900000 });
    await ready();
    const shown = Date.now() - began;
    const status = await baked();
    return { shown, baked: Date.now() - began, edit: await json("itemEdit"), status, items: await json("items") };
  };
  /** The meshes a bake report says were drawn: the names before each line's first colon. */
  const drawn = status => status.details.map(line => line.slice(0, line.indexOf(":")));
  /** Where a world point is drawn, in canvas pixels of a screenshot. */
  const project2d = points => page.evaluate(points => {
    const scene = window.__scene, canvas = document.querySelector("#viewport canvas");
    const three = window.__three;
    return points.map(point => {
      const at = new three.Vector3(...point).project(scene.camera);
      return [(at.x + 1) / 2 * canvas.clientWidth, (1 - at.y) / 2 * canvas.clientHeight];
    });
  }, points);
  /** Pixels of a convex screen quad whose color differs by more than `tolerance` (of 255, any channel), and the quad's size. */
  const differing = (a, b, quad, tolerance = 6) => {
    assert.deepEqual([a.width, a.height], [b.width, b.height]);
    const scale = a.width / (quadScale ?? a.width);
    const insideQuad = (x, y) => {
      let sign = 0;
      for (let i = 0; i < quad.length; i++) {
        const [x0, y0] = quad[i], [x1, y1] = quad[(i + 1) % quad.length];
        const cross = (x1 - x0) * (y - y0) - (y1 - y0) * (x - x0);
        if (cross !== 0) { if (sign && Math.sign(cross) !== sign) return false; sign = Math.sign(cross); }
      }
      return true;
    };
    let total = 0, changed = 0, worst = 0;
    for (let y = 0; y < a.height; y++) for (let x = 0; x < a.width; x++) {
      if (!insideQuad(x / scale, y / scale)) continue;
      total++;
      const at = (y * a.width + x) * 4;
      const difference = Math.max(Math.abs(a.data[at] - b.data[at]), Math.abs(a.data[at + 1] - b.data[at + 1]), Math.abs(a.data[at + 2] - b.data[at + 2]));
      worst = Math.max(worst, difference);
      if (difference > tolerance) changed++;
    }
    return { pixels: total, changed, share: +(changed / Math.max(1, total)).toFixed(4), worst };
  };
  let quadScale;

  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?${process.env.QUERY ?? "ao=off&gpuprobe&shadows=off"}`);
  await ready();
  const opened = Date.now();
  await page.locator("#open-project").setInputFiles({ name: "washroom-items.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
  await page.waitForFunction(() => document.querySelector("#viewport").dataset.items !== undefined, {}, { timeout: 900000 });
  await ready();
  const first = await baked();
  quadScale = await page.evaluate(() => document.querySelector("#viewport canvas").clientWidth);
  console.log(`Opened in ${Date.now() - opened} ms: ${first.meshes} receivers, ${first.baked} baked, bake ${first.ms} ms (compile ${first.compileMs}, draw ${first.drawMs}), unsupported ${JSON.stringify(first.unsupported)}`);
  assert.deepEqual(first.unsupported, {}, "every receiver of the item washroom shows GPU-baked maps");
  const start = await json("items");
  assert.deepEqual(Object.keys(start).sort(), ["bench", "drain", "duct", "pipe", "slab", "wall-low", "wall-tall"]);
  for (const item of Object.values(start)) assert.equal(item.generation, 1);
  // Every part's face group is a receiver bound through its item's slot.
  const bindings = await json("bindings");
  assert.equal(bindings["wall-tall/part/wall/wall/front"], "concrete-walls");
  assert.equal(bindings["wall-tall/part/wall/wall/glazing"], "glass");
  assert.equal(bindings["wall-tall/part/tiles/tiles/side"], "tile");
  assert.equal(bindings["pipe/part/riser/riser/body"], "rusted-pipe");
  assert.equal(bindings["pipe/part/riser/riser/bolt"], "flat-steel");
  assert.equal(bindings["duct/part/duct/duct/bracket"], "flat-steel");
  assert.equal(bindings["bench/part/plank-1/plank-1/top"], "bench-wood");
  // Decals are drawn as render-only meshes; the overlays state the handles.
  assert.deepEqual((await json("decals")).sort(), ["pipe/decal/label", "wall-tall/decal/notice", "wall-tall/decal/number"]);
  assert.deepEqual((await json("itemOverlays")).handles.map(handle => `${handle.item}/${handle.id}`).sort(), ["pipe/height", "wall-tall/height", "wall-tall/width"]);
  // The Studio lists each item's controls with their ranges, its parts and what it names.
  assert.equal(await page.locator('#item-list details[data-item="wall-tall"] [data-parameter="width"] input[type=range]').getAttribute("max"), "6");
  assert.equal(await page.locator('#item-list details[data-item="wall-tall"] ul.item-parts li').count(), 3);
  assert.match(await page.locator('#item-list details[data-item="pipe"] ul.item-named').textContent(), /joint ×5.*Decal label.*Handle height drags height/s);
  await page.locator("#item-overlays").check();
  const overlays = await json("itemOverlays");
  assert.ok(overlays.features > 20 && overlays.anchors >= 7 && overlays.decals === 3, JSON.stringify(overlays));
  if (shots) { await waitForFrames(page, 3); await mkdir(shots, { recursive: true }); await writeFile(resolve(shots, "overlays.png"), await page.locator("#viewport canvas").screenshot()); }
  await page.locator("#item-overlays").uncheck();
  const before = await shot("items-default");

  // Check 1: widening the wall. Only the panel is generated again; the tile panels, and every other item, are kept.
  const wide = await setControl("wall-tall", "width", 3.2);
  assert.deepEqual(wide.items["wall-tall"].rebuilt, ["wall"]);
  assert.equal(wide.items["wall-tall"].parts.tiles, start["wall-tall"].parts.tiles);
  assert.notEqual(wide.items["wall-tall"].parts.wall, start["wall-tall"].parts.wall);
  for (const id of Object.keys(start)) if (id !== "wall-tall") assert.deepEqual(wide.items[id], start[id], `${id} is untouched by a wall edit`);
  const wideDrawn = drawn(wide.status);
  console.log(`Wall width 2.7 to 3.2 m: shown ${wide.shown} ms, baked ${wide.baked} ms; engine edit ${wide.edit.ms} ms; bakes drawn ${wide.status.drawn}, reused ${wide.status.reused} (nodes ${wide.status.nodesDrawn} drawn, ${wide.status.nodesReused} reused); drawn: ${wideDrawn.join(", ")}`);
  const after = await shot("items-wall-wide");
  // The part of the wall that existed before, clear of its old end: world x 0.05 to 2.0 on the plane y = 0 (in front of
  // it stand the tile, the window and the decals), up to the underside of the duct.
  const region = await project2d([[0.05, -0.013, 0.05], [2.0, -0.013, 0.05], [2.0, -0.013, 1.9], [0.05, -0.013, 1.9]]);
  const kept = differing(before, after, region);
  console.log(`Check 1: of ${kept.pixels} pixels of the wall that existed before, ${kept.changed} differ by more than 6/255 (${(kept.share * 100).toFixed(2)} %, largest difference ${kept.worst}/255).`);
  // The tile panel with its notice, and the stencilled number: parts and decals the edit did not touch.
  const tile = differing(before, after, await project2d([[0.83, -0.014, 0.78], [1.97, -0.014, 0.78], [1.97, -0.014, 1.62], [0.83, -0.014, 1.62]]));
  const number = differing(before, after, await project2d([[0.23, -0.003, 0.57], [0.51, -0.003, 0.57], [0.51, -0.003, 0.7], [0.23, -0.003, 0.7]]), 24);
  console.log(`Check 1: tile panel and notice: ${tile.changed} of ${tile.pixels} pixels differ (largest difference ${tile.worst}/255); stencilled number: ${number.changed} of ${number.pixels} differ by more than 24/255.`);
  // (With shadows on, the shadow map is fitted to the scene's new bounds and its edges move by a texel.)
  if (/shadows=off/.test(process.env.QUERY ?? "shadows=off")) assert.ok(tile.share < 0.002, "the tile panel, its grout and its notice are the same picture after the wall is widened");
  // The window and the decals did not move: their meshes are the same in both scenes.
  const meshBounds = async id => (await json("meshes")).find(mesh => mesh.id === id)?.bounds;
  const windowBounds = await meshBounds("wall-tall/part/wall/wall/glazing");
  assert.ok(Math.abs(windowBounds[0][0] - 0.14) < 1e-5 && Math.abs(windowBounds[1][0] - 0.66) < 1e-5, JSON.stringify(windowBounds));

  // Check 3: the grime follows the base along the new stretch of wall, and the sill when the window moves.
  if (weathering) {
    const reach = async (points, a, b) => differing(a, b, await project2d(points), 10);
    // The new stretch of wall, beyond the old end at x = 2.3: its lowest 0.2 m is darker than the band above it.
    const brightness = (image, quad) => {
      let sum = 0, count = 0;
      const xs = quad.map(point => point[0]), ys = quad.map(point => point[1]);
      const scale = image.width / quadScale;
      for (let y = Math.floor(Math.min(...ys) * scale); y < Math.max(...ys) * scale; y++) for (let x = Math.floor(Math.min(...xs) * scale); x < Math.max(...xs) * scale; x++) {
        const at = (y * image.width + x) * 4;
        sum += image.data[at] + image.data[at + 1] + image.data[at + 2]; count++;
      }
      return +(sum / count / 3).toFixed(1);
    };
    const low = await project2d([[2.4, -0.001, 0.02], [2.75, -0.001, 0.02], [2.75, -0.001, 0.14], [2.4, -0.001, 0.14]]);
    const high = await project2d([[2.4, -0.001, 0.6], [2.75, -0.001, 0.6], [2.75, -0.001, 0.72], [2.4, -0.001, 0.72]]);
    const base = { low: brightness(after, low), high: brightness(after, high) };
    console.log(`Check 3: on the new stretch of wall the band at its base has mean brightness ${base.low}, the band 0.6 m up ${base.high}.`);
    assert.ok(base.low < base.high - 8, "the base grime reaches the new stretch of wall");
    const sillUp = await setControl("wall-tall", "windowSill", 1.15);
    const moved = await shot("items-sill-raised");
    // Under the old sill (z 0.95) the grime band is gone; under the new one (z 1.15) it appears.
    const underOld = await reach([[0.16, -0.001, 0.8], [0.6, -0.001, 0.8], [0.6, -0.001, 0.92], [0.16, -0.001, 0.92]], after, moved);
    console.log(`Check 3: raising the sill 0.95 to 1.15 m changed ${(underOld.share * 100).toFixed(1)} % of the pixels in the band under the old sill (largest difference ${underOld.worst}/255); parts generated: ${sillUp.items["wall-tall"].rebuilt.join(", ")}.`);
    assert.ok(underOld.share > 0.3, "the sill's grime moved with the sill");
    await setControl("wall-tall", "windowSill", 0.95);
  }

  // Check 2: shortening the pipe. Its couplings keep an even spacing; the wall is not generated or baked again.
  const beforePipe = await json("items");
  const short = await setControl("pipe", "height", 2.0);
  assert.deepEqual(short.items.pipe.rebuilt, ["riser"]);
  assert.equal(short.items.pipe.derived.spacing, (2.0 - 0.1) / 4);
  for (const id of Object.keys(start)) if (id !== "pipe") assert.deepEqual(short.items[id], beforePipe[id], `${id} is untouched by a pipe edit`);
  const shortDrawn = drawn(short.status);
  console.log(`Pipe height 2.3 to 2.0 m: shown ${short.shown} ms, baked ${short.baked} ms; engine edit ${short.edit.ms} ms; bakes drawn ${short.status.drawn}, reused ${short.status.reused}; drawn: ${shortDrawn.join(", ")}`);
  assert.ok(shortDrawn.length > 0 && shortDrawn.every(mesh => mesh.startsWith("pipe/")), `only the pipe's receivers are baked again: ${shortDrawn.join(", ")}`);
  assert.ok(wideDrawn.length > 0 && wideDrawn.every(mesh => mesh.startsWith("wall-tall/part/wall/") || !mesh.startsWith("wall-tall/")), `of the wall item only the panel's receivers are baked again: ${wideDrawn.join(", ")}`);
  await shot("items-pipe-short");
  const label = await meshBounds("pipe/decal/label");
  // The label sits at 1.5 spacings: 0.7125 m up, 0.1 m tall.
  assert.ok(Math.abs(label[0][2] - 0.6625) < 1e-4 && Math.abs(label[1][2] - 0.7625) < 1e-4, JSON.stringify(label));

  // Check 5: dragging the wall's width handle. While it moves the scene is shown per pixel and nothing is baked.
  await page.locator("#item-handles").check();
  await waitForFrames(page, 3);
  const handle = (await json("itemOverlays")).handles.find(handle => handle.item === "wall-tall" && handle.id === "width");
  const box = await page.locator("#viewport canvas").boundingBox();
  const direction = [handle.meter[0] - handle.screen[0], handle.meter[1] - handle.screen[1]];
  const from = [box.x + handle.screen[0], box.y + handle.screen[1]];
  const dragStart = await revision();
  await page.mouse.move(from[0], from[1]);
  await page.mouse.down();
  const steps = [];
  for (let step = 1; step <= 6; step++) {
    // Toward a wall 0.3 m narrower, in six moves.
    const began = Date.now(), revisionBefore = await revision();
    await page.mouse.move(from[0] - direction[0] * 0.05 * step, from[1] - direction[1] * 0.05 * step);
    await page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, revisionBefore, { timeout: 900000 });
    await ready();
    const edit = await json("itemEdit");
    assert.equal(edit.dragging, true);
    assert.equal(await viewport("preview"), "shader", "a drag shows the per-pixel look");
    assert.equal(await viewport("sceneBake"), undefined, "nothing is baked while a handle is dragged");
    steps.push({ ms: Date.now() - began, engine: edit.ms, width: edit.value });
  }
  await shot("items-dragging");
  const released = Date.now();
  await page.mouse.up();
  const dropped = await baked();
  await ready();
  const droppedDrawn = drawn(dropped);
  const final = await json("itemEdit");
  console.log(`Check 5: drag steps (ms to the per-pixel picture; engine and viewer ms; width): ${JSON.stringify(steps)}; on release baked in ${Date.now() - released} ms: ${dropped.drawn} drawn, ${dropped.reused} reused; drawn: ${droppedDrawn.join(", ")}`);
  assert.equal(final.dragging, false);
  assert.ok(Math.abs(final.value - (3.2 - 0.3)) < 0.06, `the handle moved the width by the distance dragged: ${final.value}`);
  assert.ok(await revision() > dragStart);
  assert.ok(droppedDrawn.every(mesh => !mesh.startsWith("wall-tall/part/tiles") && !mesh.startsWith("pipe/") && !mesh.startsWith("duct/") && !mesh.startsWith("bench/") && !mesh.startsWith("drain/")),
    `the release bakes only what the drag changed: ${droppedDrawn.join(", ")}`);
  assert.ok(dropped.reused > dropped.drawn, "most bakes are reused on release");

  // Check 4: lowering the sill until the window reaches the stencilled number. The error names the decal in the
  // definition and the last valid scene stays.
  const lastValid = await revision();
  const field = page.getByRole("spinbutton", { name: "wall-tall windowSill", exact: true });
  await field.fill("0.7"); await field.press("Tab");
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "error", {}, { timeout: 900000 });
  const finding = await page.locator("#diagnostics button").first();
  const text = await finding.textContent();
  console.log(`Check 4: ${text}`);
  assert.match(text, /^items\/washroom-wall\.item\.json:\d+:\d+ \/decals\/1 Decal number lands in opening window/);
  assert.match(await page.locator("#status").textContent(), /Last valid preview retained/);
  assert.equal(await revision(), lastValid, "the viewport still shows the last valid scene");
  assert.equal((await json("decals")).length, 3);
  await shot("items-decal-refused");
  // The finding selects the decal in the definition.
  await finding.dispatchEvent("click");
  const selection = await page.evaluate(() => { const code = document.querySelector("#code"); return code.value.slice(code.selectionStart, code.selectionEnd); });
  assert.match(selection, /^\{\s*"id": "number"/);
  await field.fill("0.95"); await field.press("Tab");
  await page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, lastValid, { timeout: 900000 });
  await ready(); await baked();

  assert.equal(errors.length, 0, errors.join("\n"));
  console.log(`Item washroom passed at ${resolution} px (${first.meshes} receivers, ${Object.keys(start).length} items).`);
} finally {
  await browser?.close();
  server.close();
}
