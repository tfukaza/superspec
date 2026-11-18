/**
 * The washroom built from items with the Kowloon building side across the lane (`buildProject(..., { facade: true })`):
 * a composed ItemSpec item whose walls carry a row of windows per floor, whose air conditioners and condensers are the
 * Kowloon fixtures as native generators chosen per bay by its seed, and whose duct riser is a sub-item spanning the
 * floors. Opens the project in the built Material Studio, waits for the bake and pictures it from the room, square on
 * and at an angle. Pictures go to OUT (default generated/verification/item-facade) as <view>.png. FACADE_PARAMETERS
 * (JSON) overrides the side's controls, for example {"floors":6,"seed":3}.
 */
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom-items/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve(process.env.OUT ?? "generated/verification/item-facade");
await mkdir(out, { recursive: true });
// MODEL=building pictures the whole building generated from its footprint (the reference shop house by default;
// BUILDING_PARAMETERS overrides its controls) instead of the one side.
const choice = process.env.MODEL ?? "side";
// LANE=1 adds the paved lane with its grass patch and trees (write it first:
//   cargo test --release -p superspec-engine --test treespec_batches -- --ignored write_lane_plants).
const lane = process.env.LANE === "1" ? JSON.parse(await readFile(resolve("generated/verification/treespec/lane-plants.json"), "utf8")) : null;
const { project } = buildProject(Number(process.env.RESOLUTION ?? 1024), Number(process.env.TEXELS ?? 256), { facade: choice === "side" ? true : choice, lane });
const facade = project.models.find(model => model.id === (choice === "side" ? "facade" : "building"));
// BUILDING_PARAMETERS is JSON, or a .json file of it (tests/studio/washroom-items/reference-shop-house.json is the
// reference picture's plan); keys starting with "/" are comments.
const overrides = process.env.BUILDING_PARAMETERS?.endsWith(".json") ? await readFile(resolve(process.env.BUILDING_PARAMETERS), "utf8") : process.env.BUILDING_PARAMETERS;
Object.assign(facade.parameters, Object.fromEntries(Object.entries(JSON.parse(overrides ?? "{}")).filter(([key]) => !key.startsWith("/"))));
// Above 512 texels a project needs the production profile.
if (project.resolution > 512) project.profile = "production";
Object.assign(facade.parameters, JSON.parse(process.env.FACADE_PARAMETERS ?? "{}"));
const text = JSON.stringify(project);

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

const browser = await launchBrowser();
try {
  const page = await browser.newPage({ viewport: { width: 1400, height: 1000 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?gpuprobe&ao=off`);
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 600000 });
  await page.locator("#open-project").setInputFiles({ name: "washroom-facade.studio.json", mimeType: "application/json", buffer: Buffer.from(text) });
  try {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state)
      && (document.querySelector("#status")?.dataset.state === "error" || JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked"), {}, { timeout: Number(process.env.BAKE_TIMEOUT ?? 900000) });
  } finally {
    const state = await page.evaluate(() => ({ status: document.querySelector("#status")?.textContent, state: document.querySelector("#status")?.dataset.state,
      diagnostics: [...document.querySelectorAll("#diagnostics button")].map(button => button.textContent) }));
    if (state.state !== "ready") { console.log(`Not ready: ${JSON.stringify(state)}\n${errors.slice(0, 8).join("\n")}`); process.exitCode = 1; }
  }
  if (!process.exitCode) {
    await waitForFrames(page, 6);
    // The sun, turned round to the other side of the scene and softened (SUN=<azimuth>,<elevation> degrees from +x toward +y
    // sets its direction).
    // LIGHT: "keep" leaves the scene's lighting alone; otherwise the sun is turned round and `softSun` (below) is merged into
    // the sun, then LIGHT_SUN, a JSON object merged the same way (nested objects merge: {"shadow":{"softness":0.05}}).
    // The record is the scene's global lighting (docs/plans/lighting-settings.md); nothing here is a material setting.
    const softSun = {
      intensity: 2,
      shadow: {
        // A wider, softer edge that widens with the distance to what casts it.
        softness: 0.05, spread: 0.09, maxSoftness: 0.3,
        // A cool shade, lighter than the default, with a mottled interior (patches of darker and bluer shade 0.6 m across).
        darkness: 0.2, tintStrength: 0.5, tint: [0.05, 0.2, 0.8], tintAlt: [0.08, 0.14, 0.7], wrap: 0.4,
        noise: { scale: 0.6, edge: 0.1, amount: 0.5, seed: 7 },
      },
    };
    const lit = await page.evaluate(({ sun, soft, extra, keep }) => {
      const s = window.__scene, record = JSON.parse(document.querySelector("#viewport").dataset.lighting), light = record.lights[0];
      const before = JSON.stringify(light);
      if (keep) return { before };
      const merge = (target, source) => { for (const [key, value] of Object.entries(source)) target[key] = value && typeof value === "object" && !Array.isArray(value) ? merge(target[key] ?? {}, value) : value; return target; };
      if (sun) { const [az, el] = sun.split(",").map(v => Number(v) * Math.PI / 180); light.direction = [Math.cos(el) * Math.cos(az), Math.cos(el) * Math.sin(az), Math.sin(el)]; }
      else light.direction = [-light.direction[0], -light.direction[1], light.direction[2]];
      merge(light, soft); merge(light, extra);
      s.setLighting(record); s.renderer.shadowMap.needsUpdate = true; s.invalidate?.();
      return { before, after: JSON.stringify(light) };
    }, { sun: process.env.SUN ?? "", soft: softSun, extra: JSON.parse(process.env.LIGHT_SUN ?? "{}"), keep: process.env.LIGHT === "keep" });
    console.log(`sun before ${lit.before}`);
    if (lit.after) console.log(`sun after  ${lit.after}`);
    await waitForFrames(page, 6);
    const info = await page.evaluate(id => {
      const s = window.__scene, T = window.__three, box = new T.Box3();
      let meshes = 0;
      for (const m of s.models.children) if (m.name.startsWith(`${id}/`)) { box.expandByObject(m); meshes++; }
      return { meshes, min: box.min.toArray(), max: box.max.toArray() };
    }, facade.id);
    console.log(`facade: ${info.meshes} meshes, ${info.min.map(v => v.toFixed(2))} to ${info.max.map(v => v.toFixed(2))}`);
    const [x0, y0, z0] = info.min, [x1, y1, z1] = info.max;
    const cy = (y0 + y1) / 2, cz = (z0 + z1) / 2, front = x1;
    const views = {
      // From the room, over the low wall, up at the side across the lane.
      room: { position: [2.4, -0.9, 1.5], target: [front, cy + 0.5, 3.2] },
      // Square on, from the lane.
      front: { position: [front + 30, cy, cz], target: [front, cy, cz] },
      // At an angle from down the lane, to see the fixtures and riser stand off the wall.
      angle: { position: [front + 6, y0 - 7, 4.5], target: [front, cy, cz * 0.8] },
      // Close on the riser's corner and a few bays.
      close: { position: [front + 4, y1 + 1.5, 6], target: [front, y1 - 2, 5.5] },
      // From the street corner (+x, +y), raised, as the reference picture of the shop house is taken.
      corner: { position: [front + 13, y1 + 11, 10.5], target: [(x0 + front) / 2, (y0 + y1) / 2, 5.5] },
      // The same without the lane's plants, to compare the building alone.
      "corner-bare": { position: [front + 13, y1 + 11, 10.5], target: [(x0 + front) / 2, (y0 + y1) / 2, 5.5], bare: true },
    };
    for (const [name, view] of Object.entries(views)) {
      await page.evaluate(view => { const s = window.__scene; for (const m of s.models.children) if (m.userData.foliage) m.visible = !view.bare; s.camera.position.fromArray(view.position); s.controls.target.fromArray(view.target); s.controls.update(); s.draw(); }, view);
      await waitForFrames(page, 6);
      await writeFile(resolve(out, `${name}.png`), await page.locator("#viewport canvas").screenshot({ timeout: 300000 }));
    }
    console.log(`Wrote ${Object.keys(views).length} pictures to ${out}${errors.length ? `; page errors:\n${errors.slice(0, 8).join("\n")}` : ""}`);
  }
} finally {
  await browser.close();
  server.close();
}
