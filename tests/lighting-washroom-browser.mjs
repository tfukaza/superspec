/**
 * Lighting, end to end on the washroom (tests/studio/washroom) in the built Material Studio:
 *
 * 1. The Lighting panel edits the project's lighting record: each edit lights the viewport at once without a rebuild,
 *    a value the record refuses is said and changes nothing, and the record is saved with the project and read back.
 * 2. Download GLB carries the record: the sun as a `KHR_lights_punctual` light and the whole record in the extras.
 * 3. A three.js "game" (tests/browser/rust-game-lighting.fixture.ts) loads that GLB with three's own loader, lights it
 *    from the extras through the game runtime module (browser/renderer/game-lighting.ts) and draws the Studio's
 *    picture of the same project, from the same camera.
 *
 * RESOLUTION (default 128) sets the texture size; SCREENSHOTS names a directory for the compared pictures. The Studio
 * page runs with `ao=off&preview=bake` (QUERY replaces it): baked maps are what the GLB holds.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { build } from "vite";
import { PNG } from "pngjs";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom/build.mjs";

const studioRoot = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const gameRoot = resolve("generated/lighting-washroom-browser");
const resolution = Number(process.env.RESOLUTION ?? 128);
const query = process.env.QUERY ?? "ao=off&preview=bake";
const shots = process.env.SCREENSHOTS ? resolve(process.env.SCREENSHOTS) : undefined;
if (shots) await mkdir(shots, { recursive: true });
await build({ configFile: false, root: "tests/browser", base: "./", publicDir: false, logLevel: "warn",
  build: { outDir: gameRoot, emptyOutDir: true, rollupOptions: { input: resolve("tests/browser/rust-game-lighting.html") } } });

let exported;
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    if (name === "/game/export.glb" && exported) { response.setHeader("Content-Type", "model/gltf-binary"); response.end(exported); return; }
    const game = name.startsWith("/game/"), root = game ? gameRoot : studioRoot;
    const path = resolve(root, game ? `.${name.slice(5)}` : name === "/" ? "index.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".png": "image/png" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));
const origin = `http://127.0.0.1:${server.address().port}`;

/** sRGB hex to linear RGB by the sRGB standard's transfer function (IEC 61966-2-1), independent of the viewer's. */
const linear = hex => [0, 2, 4].map(at => {
  const value = parseInt(hex.slice(at, at + 2), 16) / 255;
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
});
const radians = degrees => degrees * Math.PI / 180;
const near = (actual, expected, tolerance, what) => {
  assert.equal(actual.length, expected.length, what);
  actual.forEach((value, index) => assert.ok(Math.abs(value - expected[index]) <= tolerance, `${what}: ${actual} is not ${expected}`));
};
/** Differing pixels, those further apart than 8 levels, the largest channel difference and the mean, of two PNGs of one size. */
const difference = (first, second, maskPicture) => {
  const a = PNG.sync.read(first), b = PNG.sync.read(second), mask = maskPicture && PNG.sync.read(maskPicture);
  assert.deepEqual([a.width, a.height], [b.width, b.height], "the two pictures have one size");
  let differing = 0, far = 0, largest = 0, sum = 0, pixels = 0;
  for (let at = 0; at < a.data.length; at += 4) {
    // With a mask, only where it is fully white: not on a masked mesh, the backdrop, or an edge between them.
    if (mask && mask.data[at] !== 255) continue;
    pixels++;
    let worst = 0;
    for (let channel = 0; channel < 3; channel++) { const d = Math.abs(a.data[at + channel] - b.data[at + channel]); worst = Math.max(worst, d); sum += d; }
    if (worst) differing++;
    if (worst > 8) far++;
    largest = Math.max(largest, worst);
  }
  return { pixels, differing, far, largest, mean: +(sum / pixels / 3).toFixed(4) };
};

// What the panel is asked for, and the record that must come of it (worked out here, not read back from the viewer).
const edits = [
  ["#lighting-azimuth", "135"], ["#lighting-elevation", "35"], ["#lighting-sun-color", "#ffd2a0"], ["#lighting-sun-intensity", "4"],
  ["#lighting-shadow-softness", "0.03"], ["#lighting-shadow-darkness", "0.5"], ["#lighting-shadow-tint", "#c03020"], ["#lighting-shadow-tint-strength", "0.6"],
  ["#lighting-noise-amount", "0.3"], ["#lighting-noise-scale", "0.6"], ["#lighting-ambient-intensity", "0.4"], ["#lighting-ao-radius", "0.5"],
];
const expectedSun = {
  direction: [Math.cos(radians(35)) * Math.cos(radians(135)), Math.cos(radians(35)) * Math.sin(radians(135)), Math.sin(radians(35))],
  color: linear("ffd2a0"), intensity: 4,
  shadow: { style: "illustration", softness: 0.03, darkness: 0.5, tint: linear("c03020"), tintStrength: 0.6, noise: { amount: 0.3, scale: 0.6 } },
};
const checkRecord = (record, what) => {
  const sun = record.lights[0];
  near(sun.direction, expectedSun.direction, 1e-9, `${what}: sun direction`);
  near(sun.color, expectedSun.color, 1e-6, `${what}: sun colour`);
  near(sun.shadow.tint, expectedSun.shadow.tint, 1e-6, `${what}: shadow tint`);
  assert.deepEqual([sun.intensity, sun.castsShadow, sun.shadow.style, sun.shadow.softness, sun.shadow.darkness, sun.shadow.tintStrength, sun.shadow.noise.amount, sun.shadow.noise.scale],
    [4, true, "illustration", 0.03, 0.5, 0.6, 0.3, 0.6], what);
  assert.deepEqual([record.version, record.toneMapping, record.exposure, record.ambient.intensity, record.ambientOcclusion.radius], [1, "neutral", 1, 0.4, 0.5], what);
  // What the panel did not touch is the shipped look.
  assert.deepEqual([sun.id, sun.shadow.spread, sun.shadow.maxSoftness, sun.shadow.wrap, sun.shadow.noise.seed, record.environment.intensity], ["sun", 0.025, 0.06, 0.3, 7, 0.35], what);
};

let browser;
try {
  const { project } = buildProject(resolution);
  browser = await launchBrowser();
  const context = await browser.newContext({ viewport: { width: 1440, height: 1100 }, acceptDownloads: true });
  const errors = [];
  const open = async () => {
    const page = await context.newPage();
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    await page.goto(`${origin}/material.html?${query}`);
    return page;
  };
  const ready = async page => {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 900000 });
    assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", await page.locator("#status").textContent());
  };
  const load = async (page, file) => {
    await ready(page);
    const before = Number(await page.locator("#viewport").getAttribute("data-input-revision") ?? 0);
    await page.locator("#open-project").setInputFiles({ name: "washroom.studio.json", mimeType: "application/json", buffer: Buffer.from(file) });
    await page.waitForFunction(start => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > start || document.querySelector("#status")?.dataset.state === "error", before, { timeout: 900000 });
    await ready(page); await waitForFrames(page, 4);
    // Only the scene is compared: the panel's overlays are not part of the render.
    await page.addStyleTag({ content: ".preview-toolbar,.preview-caption{visibility:hidden}" });
  };
  const lighting = page => page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.lighting));
  const revision = page => page.locator("#viewport").getAttribute("data-input-revision");
  const picture = async page => { await waitForFrames(page, 3); return page.locator("#viewport canvas").screenshot(); };
  const download = async (page, selector) => {
    const [file] = await Promise.all([page.waitForEvent("download", { timeout: 900000 }), page.locator(selector).click()]);
    return readFile(await file.path());
  };

  // 1. The panel.
  let page = await open();
  await load(page, JSON.stringify(project));
  const shipped = await lighting(page);
  assert.equal(shipped.lights[0].shadow.softness, 0.012, "a project without a record has the shipped look");
  assert.equal(await page.locator("#lighting-azimuth").inputValue(), String(+(180 + Math.atan(5 / 3) * 180 / Math.PI).toFixed(2)), "the panel shows the shipped sun, which stands at (-3, -5, 8)");
  const shippedPicture = await picture(page);
  const builtRevision = await revision(page), builtStatus = await page.locator("#status").textContent();
  for (const [selector, value] of edits) await page.locator(selector).fill(value);
  await page.locator("#lighting-tone-mapping").selectOption("neutral");
  checkRecord(await lighting(page), "the viewport after the panel's edits");
  assert.equal(await revision(page), builtRevision, "lighting edits rebuild nothing");
  assert.equal(await page.locator("#status").textContent(), builtStatus);
  const editedPicture = await picture(page);
  const moved = difference(shippedPicture, editedPicture);
  assert.ok(moved.far / moved.pixels > 0.05, `the edits change the picture (${JSON.stringify(moved)})`);

  // A value outside the record's limits is refused by the record's own validation, by field, and nothing changes.
  await page.locator("#lighting-shadow-softness").fill("5");
  assert.match(await page.locator("#lighting-status").textContent(), /lights\[0\]\.shadow\.softness must be between 0 and 2/);
  assert.equal(await page.locator("#lighting-shadow-softness").getAttribute("aria-invalid"), "true");
  assert.equal((await lighting(page)).lights[0].shadow.softness, 0.03);
  await page.locator("#lighting-shadow-softness").fill("0.03");
  assert.equal(await page.locator("#lighting-status").textContent(), "");
  assert.equal(await page.locator("#lighting-shadow-softness").getAttribute("aria-invalid"), null);

  // Shadows off and on again, and the plain style, reach the renderer without a rebuild either.
  const rendering = () => page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.rendering));
  await page.locator("#lighting-shadows").uncheck();
  assert.deepEqual([(await rendering()).shadows, (await lighting(page)).lights[0].castsShadow], [false, false]);
  await page.locator("#lighting-shadows").check();
  await page.locator("#lighting-shadow-style").selectOption("plain");
  assert.deepEqual([(await rendering()).shadows, (await rendering()).shadowStyle], [true, false]);
  await page.locator("#lighting-shadow-style").selectOption("illustration");
  assert.equal((await rendering()).shadowStyle, true);
  // Without the sun its controls rest; turning it back on brings the same sun.
  await page.locator("#lighting-sun").uncheck();
  assert.deepEqual((await lighting(page)).lights, []);
  assert.equal(await page.locator("#lighting-azimuth").isDisabled(), true);
  await page.locator("#lighting-sun").check();
  checkRecord(await lighting(page), "the sun turned off and on again");
  assert.deepEqual(difference(editedPicture, await picture(page)).differing, 0, "the same record draws the same picture again");
  assert.equal(await revision(page), builtRevision);

  // Saved with the project, as a record that leaves ambient occlusion unsaid (the author did not choose it).
  const saved = await download(page, "#save-project");
  const savedProject = JSON.parse(saved.toString("utf8"));
  checkRecord({ ...savedProject.lighting, ambientOcclusion: { ...savedProject.lighting.ambientOcclusion } }, "the saved project");
  assert.equal("enabled" in savedProject.lighting.ambientOcclusion, false);

  // 2. The export.
  await page.locator("#export-ktx2").uncheck();
  exported = await download(page, "#export-glb");
  const gltf = JSON.parse(exported.subarray(20, 20 + exported.readUInt32LE(12)).toString("utf8"));
  assert.deepEqual(gltf.extensionsUsed, ["KHR_lights_punctual"]);
  const [light] = gltf.extensions.KHR_lights_punctual.lights;
  assert.deepEqual([light.type, light.intensity, light.name], ["directional", 4, "sun"]);
  near(light.color, expectedSun.color, 1e-6, "the punctual light's colour");
  const carried = gltf.extras.superspec.lighting;
  checkRecord(carried, "the GLB's extras");
  assert.equal(carried.ambientOcclusion.enabled, true, "the exported record is complete");
  const studioCamera = await page.evaluate(() => JSON.parse(document.querySelector("#viewport").dataset.camera));
  const canvas = await page.locator("#viewport canvas").boundingBox();
  const studioPicture = await picture(page);
  assert.equal(difference(editedPicture, studioPicture).differing, 0);
  // Materials with live operations are shaded further in the Studio's own shader, which three's loader does not run:
  // their meshes are where the two pictures may differ for a reason that is not lighting.
  const live = await page.evaluate(() => Object.keys(JSON.parse(document.querySelector("#viewport").dataset.liveOperations ?? "{}")));
  console.log(`Meshes with live material operations in the Studio (not drawn by a plain glTF loader): ${live.length ? live.join(", ") : "none"}`);
  await page.close();

  // Opened again, the project has the record: the viewport and the panel show it.
  page = await open();
  await load(page, saved);
  checkRecord(await lighting(page), "the reopened project");
  assert.deepEqual([await page.locator("#lighting-azimuth").inputValue(), await page.locator("#lighting-elevation").inputValue(), await page.locator("#lighting-sun-color").inputValue(),
    await page.locator("#lighting-tone-mapping").inputValue(), await page.locator("#lighting-shadow-darkness").inputValue()], ["135", "35", "#ffd2a0", "neutral", "0.5"]);
  const reopened = difference(studioPicture, await picture(page));
  assert.equal(reopened.differing, 0, `the reopened project draws the saved picture (${JSON.stringify(reopened)})`);
  // Reset gives the shipped look back, and the project then stores no record at all.
  await page.locator("#lighting-reset").click();
  assert.deepEqual(await lighting(page), shipped);
  assert.equal(difference(shippedPicture, await picture(page)).differing, 0, "reset draws the shipped picture");
  assert.equal("lighting" in JSON.parse((await download(page, "#save-project")).toString("utf8")), false);
  await page.close();

  // 3. The game.
  const game = await context.newPage();
  game.on("pageerror", error => errors.push(error.message));
  game.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  await game.goto(`${origin}/game/rust-game-lighting.html`);
  await game.waitForFunction(() => window.gameLighting);
  const overrides = [...new URLSearchParams(query)].filter(([name]) => /^(ao|shadow)/.test(name)).map(pair => pair.join("=")).join("&");
  const view = { width: canvas.width, height: canvas.height, position: studioCamera.position, target: studioCamera.target, fov: studioCamera.fov, overrides };
  const shown = await game.evaluate(view => window.gameLighting.show("/game/export.glb", view), view);
  checkRecord(shown.stated, "the record the game read from the GLB");
  // The file's own light, as three.js reads it, shines the recorded way in the Y-up world: (x, y, z) at (x, z, -y), reversed.
  assert.equal(shown.punctual.length, 1);
  const [x, y, z] = expectedSun.direction;
  near(shown.punctual[0].travel, [-x, -z, y], 1e-6, "the punctual light's direction");
  assert.deepEqual([shown.lights, shown.shadowed, shown.styledShadows], [2, shown.meshes, true], "the game holds the record's two lights, not the file's third, and every mesh takes the shadow");
  const gamePicture = await game.locator("canvas").screenshot();
  const compared = difference(studioPicture, gamePicture);
  console.log(`Studio against game, same record and camera (${query}): ${JSON.stringify(compared)}`);
  // Where no live material operation is involved the picture is the lighting's alone: the surfaces of the other
  // meshes, with the light, the styled shadows cast on them (by every mesh) and the tone mapping.
  await game.evaluate(ids => window.gameLighting.mask(ids), live);
  const maskPicture = await game.locator("canvas").screenshot();
  const lit = difference(studioPicture, gamePicture, maskPicture);
  console.log(`The same, on meshes without live material operations: ${JSON.stringify(lit)}`);
  if (shots) await writeFile(resolve(shots, "lighting-roundtrip-mask.png"), maskPicture);
  assert.ok(lit.pixels > 0.1 * compared.pixels, "the masked comparison covers a real share of the picture");
  // Not bit for bit: the file's root node turns the scene to Y-up in floating point, so an antialiased edge may take
  // one coverage sample more or less, and a highlight may round one level the other way.
  assert.ok(lit.far / lit.pixels < Number(process.env.LIT_FAR_LIMIT ?? 0.002) && lit.mean < Number(process.env.LIT_MEAN_LIMIT ?? 0.1), `the lighting is the same picture (${JSON.stringify(lit)})`);
  if (shots) {
    await writeFile(resolve(shots, "lighting-roundtrip-studio.png"), studioPicture);
    await writeFile(resolve(shots, "lighting-roundtrip-game.png"), gamePicture);
    await writeFile(resolve(shots, "lighting-roundtrip-shipped.png"), shippedPicture);
    // The inputs and the output, for comparing with a native export of the same project.
    await writeFile(resolve(shots, "lighting-roundtrip.studio.json"), saved);
    await writeFile(resolve(shots, "lighting-roundtrip.glb"), exported);
  }
  assert.ok(compared.mean < Number(process.env.MEAN_LIMIT ?? 0.5) && compared.far / compared.pixels < Number(process.env.FAR_LIMIT ?? 0.01),
    `the game draws the Studio's picture (${JSON.stringify(compared)})`);
  // With the file's record alone (no overrides) the game also draws ambient occlusion, where this renderer does by default.
  const full = await game.evaluate(view => window.gameLighting.show("/game/export.glb", { ...view, overrides: "" }), view);
  console.log(`Game with the record alone: ambient occlusion ${full.ambientOcclusion ? "drawn" : "not drawn"}.`);
  if (shots) await writeFile(resolve(shots, "lighting-roundtrip-game-record-alone.png"), await game.locator("canvas").screenshot());
  assert.equal(errors.length, 0, errors.join("\n"));
  console.log(`Lighting washroom test passed at ${resolution} px.`);
} finally {
  await browser?.close();
  server.close();
}
