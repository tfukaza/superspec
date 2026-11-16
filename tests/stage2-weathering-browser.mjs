/**
 * Stage 2 gate for weathering layers, in the Material Studio's CPU bake (`?preview=bake`): the washroom built from
 * items (tests/studio/washroom-items) as it is, against a generated copy whose walls use the split Kowloon concrete
 * (an anchored intrinsic base under top grime, ground stain, moss, worn edges and grime along the base and the sill)
 * and whose duct and pipe use the shipped steel under a joint-rust layer, at default layer parameters.
 *
 * 1. The two pictures of the default scene, compared over the whole view and over each wall (they cannot be the same
 *    bytes: the shipped recipe measures its pattern from each face's bounds, the split one from the pattern anchor).
 * 2. Widening the tall wall 2.7 to 3.2 m: how much of the wall that existed before changes, in each project.
 * 3. In the split project, the ground band reaches the new stretch of wall, and the sill grime follows a raised sill.
 *
 * Pictures and a JSON report go to generated/verification/stage2/. STUDIO_UI_ROOT names the built Studio (default
 * generated/stage2/rust-editor, built against the Stage 2 Wasm by generated/stage2/vite.stage2.config.mjs);
 * RESOLUTION (default 512) the texture size; QUERY (default `preview=bake&ao=off`) the Studio's query.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { resolve, extname, sep } from "node:path";
import { PNG } from "pngjs";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom-items/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/stage2/rust-editor");
const out = resolve("generated/verification/stage2");
const resolution = Number(process.env.RESOLUTION ?? 512);
// `gpuprobe` exposes the scene's camera, through which the walls' regions are found on screen.
// Shadows are off: a widened wall refits the shadow map to the scene's bounds, which moves
// shadow edges over the region compared.
const query = process.env.QUERY ?? "preview=bake&ao=off&shadows=off&gpuprobe";
const examples = resolve("crates/engine/src/materialspec/program/examples");

/** The washroom-items project with the split materials on the walls, the duct and the pipe. */
function split(project) {
  const copy = structuredClone(project);
  for (const name of ["kowloon-concrete-anchored", "kowloon-top-grime", "kowloon-ground", "kowloon-ground-stain", "kowloon-moss",
    "kowloon-concrete-edges", "kowloon-feature-grime", "kowloon-joint-rust"]) {
    copy.modules[`${name}.material.ts`] = readFileSync(resolve(examples, `${name}.material.ts`), "utf8");
  }
  const layer = (name, parameters = {}, follow) => ({ entry: `${name}.material.ts`, parameters, seed: 7, ...(follow ? { follow: { follow } } : {}) });
  const walls = copy.materials.find(material => material.id === "concrete-walls");
  walls.entry = "kowloon-concrete-anchored.material.ts";
  delete walls.parameters.featureGrime;
  walls.layers = [
    layer("kowloon-top-grime"), layer("kowloon-ground-stain"), layer("kowloon-moss"), layer("kowloon-concrete-edges"),
    // The washroom-items walls' grime along the base and under the sill (`featureGrime` 0.8).
    layer("kowloon-feature-grime", { amount: 0.68, near: 0, far: 0.3, color: [0.035, 0.032, 0.026], roughness: 0.95 }, "base"),
    layer("kowloon-feature-grime", {}, "sill"),
  ];
  copy.mappings = { ...(copy.mappings ?? {}), "concrete-walls": "anchored" };
  for (const id of ["galvanized", "rusted-pipe"]) {
    const steel = copy.materials.find(material => material.id === id);
    steel.entry = "kowloon-rusted-steel.material.ts";
    const oxide = Object.fromEntries(Object.entries(steel.parameters).filter(([name]) => ["rustRepeat", "rustRoughness", "oxideDark", "oxideLight"].includes(name)));
    steel.layers = [layer("kowloon-joint-rust", { amount: 0.9, ...oxide })];
  }
  return copy;
}

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

/** Pixels of a convex screen quad (all of the picture without one) that differ, with the mean and largest difference. */
function differing(a, b, quad, clientWidth, tolerance) {
  assert.deepEqual([a.width, a.height], [b.width, b.height]);
  const scale = a.width / clientWidth;
  const inside = (x, y) => {
    if (!quad) return true;
    let sign = 0;
    for (let i = 0; i < quad.length; i++) {
      const [x0, y0] = quad[i], [x1, y1] = quad[(i + 1) % quad.length];
      const cross = (x1 - x0) * (y - y0) - (y1 - y0) * (x - x0);
      if (cross !== 0) { if (sign && Math.sign(cross) !== sign) return false; sign = Math.sign(cross); }
    }
    return true;
  };
  let total = 0, changed = 0, large = 0, worst = 0, sum = 0;
  const mean = [[0, 0, 0], [0, 0, 0]];
  for (let y = 0; y < a.height; y++) for (let x = 0; x < a.width; x++) {
    if (!inside(x / scale, y / scale)) continue;
    total++;
    const at = (y * a.width + x) * 4;
    let difference = 0;
    for (let c = 0; c < 3; c++) {
      difference = Math.max(difference, Math.abs(a.data[at + c] - b.data[at + c]));
      mean[0][c] += a.data[at + c]; mean[1][c] += b.data[at + c];
    }
    worst = Math.max(worst, difference); sum += difference;
    if (difference > tolerance) changed++;
    if (difference > 32) large++;
  }
  const average = values => values.map(value => +(value / Math.max(1, total)).toFixed(1));
  return {
    pixels: total, changed, share: +(changed / Math.max(1, total)).toFixed(4), over32: +(large / Math.max(1, total)).toFixed(4),
    worst, mean: +(sum / Math.max(1, total)).toFixed(2), meanColor: [average(mean[0]), average(mean[1])],
  };
}

/** Mean brightness of a screen rectangle's bounding box. */
function brightness(image, quad, clientWidth) {
  let sum = 0, count = 0;
  const xs = quad.map(point => point[0]), ys = quad.map(point => point[1]), scale = image.width / clientWidth;
  for (let y = Math.floor(Math.min(...ys) * scale); y < Math.max(...ys) * scale; y++) for (let x = Math.floor(Math.min(...xs) * scale); x < Math.max(...xs) * scale; x++) {
    const at = (y * image.width + x) * 4;
    sum += image.data[at] + image.data[at + 1] + image.data[at + 2]; count++;
  }
  return +(sum / count / 3).toFixed(1);
}

const report = { resolution, query, studio: root };
let browser;
try {
  browser = await launchBrowser();
  const run = async (project, name, layered) => {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    const viewport = key => page.evaluate(key => document.querySelector("#viewport").dataset[key], key);
    const ready = async () => {
      await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 1800000 });
      assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", `${await page.locator("#status").textContent()}\n${(await page.locator("#diagnostics button").allTextContents()).join("\n")}`);
      await waitForFrames(page, 3);
    };
    const shot = async label => {
      await page.evaluate(() => { const box = document.querySelector("#item-handles"); if (box?.checked) { box.checked = false; box.dispatchEvent(new Event("change")); } });
      await waitForFrames(page, 3);
      const bytes = await page.locator("#viewport canvas").screenshot();
      await mkdir(out, { recursive: true });
      await writeFile(resolve(out, `${name}-${label}.png`), bytes);
      return PNG.sync.read(bytes);
    };
    const project2d = points => page.evaluate(points => {
      const scene = window.__scene, canvas = document.querySelector("#viewport canvas"), three = window.__three;
      return points.map(point => {
        const at = new three.Vector3(...point).project(scene.camera);
        return [(at.x + 1) / 2 * canvas.clientWidth, (1 - at.y) / 2 * canvas.clientHeight];
      });
    }, points);
    const setControl = async (item, parameter, value) => {
      const from = Number(await viewport("inputRevision") ?? 0), began = Date.now();
      const field = page.getByRole("spinbutton", { name: `${item} ${parameter}`, exact: true });
      await field.fill(String(value)); await field.press("Tab");
      await page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, from, { timeout: 1800000 });
      await ready();
      return Date.now() - began;
    };
    await page.goto(`http://127.0.0.1:${server.address().port}/material.html?${query}`);
    await ready();
    const opened = Date.now();
    await page.locator("#open-project").setInputFiles({ name: "washroom-items.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
    await page.waitForFunction(() => document.querySelector("#viewport").dataset.items !== undefined, {}, { timeout: 1800000 });
    await ready();
    const openMs = Date.now() - opened;
    const width = await page.evaluate(() => document.querySelector("#viewport canvas").clientWidth);
    const regions = {
      // The tall wall's front, clear of its old end, up to the duct (the tile, window and decals stand in front).
      tallWall: await project2d([[0.05, -0.013, 0.05], [2.0, -0.013, 0.05], [2.0, -0.013, 1.9], [0.05, -0.013, 1.9]]),
      // The low wall's face, which looks along +x from x = 0.
      lowWall: await project2d([[0.013, -2.25, 0.05], [0.013, -0.45, 0.05], [0.013, -0.45, 1.15], [0.013, -2.25, 1.15]]),
      // The tall wall's lowest 0.25 m and its top 0.6 m: the ground band and the top grime.
      tallBase: await project2d([[0.05, -0.013, 0.02], [2.0, -0.013, 0.02], [2.0, -0.013, 0.25], [0.05, -0.013, 0.25]]),
      tallTop: await project2d([[0.05, -0.013, 2.0], [2.6, -0.013, 2.0], [2.6, -0.013, 2.55], [0.05, -0.013, 2.55]]),
    };
    const before = await shot("default");
    const widenMs = await setControl("wall-tall", "width", 3.2);
    const wide = await shot("wide");
    const result = { openMs, widenMs, width, regions, before, wide };
    if (layered) {
      // The new stretch of wall, beyond the old end at x = 2.3 (the wall stands from x = -0.4).
      const low = await project2d([[2.4, -0.001, 0.02], [2.75, -0.001, 0.02], [2.75, -0.001, 0.14], [2.4, -0.001, 0.14]]);
      const high = await project2d([[2.4, -0.001, 0.6], [2.75, -0.001, 0.6], [2.75, -0.001, 0.72], [2.4, -0.001, 0.72]]);
      result.newStretch = { base: brightness(wide, low, width), above: brightness(wide, high, width) };
      result.sillMs = await setControl("wall-tall", "windowSill", 1.15);
      result.sill = await shot("sill-raised");
      // Under the old sill (z 0.95), and under the new one (z 1.15, where the window was).
      result.underOldSill = await project2d([[0.16, -0.001, 0.8], [0.6, -0.001, 0.8], [0.6, -0.001, 0.92], [0.16, -0.001, 0.92]]);
      result.underNewSill = await project2d([[0.16, -0.001, 1.0], [0.6, -0.001, 1.0], [0.6, -0.001, 1.12], [0.16, -0.001, 1.12]]);
      result.sillBands = { underNew: brightness(result.sill, result.underNewSill, width), underOld: brightness(result.sill, result.underOldSill, width) };
    }
    assert.equal(errors.length, 0, errors.join("\n"));
    await page.close();
    return result;
  };
  const { project } = buildProject(resolution, 128);
  const current = await run(project, "current", false);
  const layered = await run(split(project), "split", true);
  const width = current.width;
  // 1. The default scene, the current project against the split one.
  report.sameScene = {
    whole: differing(current.before, layered.before, null, width, 16),
    tallWall: differing(current.before, layered.before, current.regions.tallWall, width, 16),
    lowWall: differing(current.before, layered.before, current.regions.lowWall, width, 16),
    tallBase: differing(current.before, layered.before, current.regions.tallBase, width, 16),
    tallTop: differing(current.before, layered.before, current.regions.tallTop, width, 16),
  };
  // 2. Widening: the wall that existed before.
  report.widen = {
    current: differing(current.before, current.wide, current.regions.tallWall, width, 6),
    split: differing(layered.before, layered.wide, layered.regions.tallWall, width, 6),
  };
  // 3. Grime that follows the base along the new stretch and the raised sill.
  report.newStretch = layered.newStretch;
  report.sill = {
    underOld: differing(layered.wide, layered.sill, layered.underOldSill, width, 10),
    underNew: differing(layered.wide, layered.sill, layered.underNewSill, width, 10),
    // After the raise: the band under the new sill against the band where the old sill's grime was.
    bands: layered.sillBands,
  };
  report.times = { current: { openMs: current.openMs, widenMs: current.widenMs }, split: { openMs: layered.openMs, widenMs: layered.widenMs, sillMs: layered.sillMs } };
  await writeFile(resolve(out, `weathering-report-${resolution}.json`), JSON.stringify(report, null, 2) + "\n");
  console.log(JSON.stringify(report, null, 2));
  assert.ok(report.widen.split.share < 0.002, "the split concrete keeps the wall's pattern where it was when the wall is widened");
  assert.ok(report.newStretch.base < report.newStretch.above - 8, "the ground band reaches the new stretch of wall");
  assert.ok(report.sill.underOld.share > 0.3, "the sill grime moved with the sill");
  assert.ok(report.sill.bands.underNew < report.sill.bands.underOld - 8, "the grime lies under the raised sill");
} finally {
  await browser?.close();
  server.close();
}
