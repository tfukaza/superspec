/**
 * Weathering layers composed on the GPU in the scene view: the washroom built from items with the split materials
 * (tests/studio/washroom-items/split.mjs: the anchored concrete under six layers on the walls, the steel under joint
 * rust on the duct and the pipe), opened in the Material Studio's default GPU-baked preview.
 *
 * 1. Opening: every receiver of a layered slot shows maps composed from its layers' planes (`sceneBake.layers`), with
 *    the time to the baked scene split into compile, draw and CPU-field work; the per-pixel look reports that it shows
 *    the base alone (`data-layers`).
 * 2. The GPU preview against the CPU bake (`?preview=bake`) of the same project: the share of pixels off by more than
 *    16/255, over the view and over each wall. Measured, not gated (the CPU bake is the reference; this is a preview).
 * 3. Widening the tall wall reads the base's intrinsic lattice tiles already drawn: no tile is drawn again.
 * 4. A layer parameter edit (the top layer, then the lowest) draws only that layer's planes, the planes above it that
 *    read them and the composed maps; the base's planes and tiles are reused.
 * 5. Reopening the scene after another one (warm): textures the pass graph kept are reused.
 * 6. A handle drag shows the per-pixel look (the base alone, reported) and bakes on release.
 *
 * STUDIO_UI_ROOT names the built Studio (default generated/rust-editor); RESOLUTION (default 512) the texture size;
 * QUERY (default `ao=off&shadows=off&gpuprobe`) the GPU page's query. Pictures and a JSON report go to
 * generated/verification/stage2-gpu-layers/. On the real GPU: BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist".
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { PNG } from "pngjs";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom-items/build.mjs";
import { split } from "./studio/washroom-items/split.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve("generated/verification/stage2-gpu-layers");
const resolution = Number(process.env.RESOLUTION ?? 512);
const query = process.env.QUERY ?? "ao=off&shadows=off&gpuprobe";

// BASELINE_ROOT optionally names a Studio built before this change: its GPU preview of the washroom without layers is
// compared with this one's (they must be the same picture), and its preview of the split one (the base alone) with the CPU bake.
const baselineRoot = process.env.BASELINE_ROOT ? resolve(process.env.BASELINE_ROOT) : null;

const serve = async directory => {
  const server = createServer(async (request, response) => {
    try {
      const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
      const path = resolve(directory, name === "/" ? "index.html" : `.${name}`);
      if (!path.startsWith(directory + sep)) { response.writeHead(403).end(); return; }
      response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".png": "image/png" }[extname(path)] ?? "application/octet-stream");
      response.end(await readFile(path));
    } catch { response.writeHead(404).end(); }
  });
  await new Promise(done => server.listen(0, "127.0.0.1", done));
  return server;
};
const server = await serve(root);
const baselineServer = baselineRoot ? await serve(baselineRoot) : null;

/** Pixels of a convex screen quad (all of the picture without one) that differ by more than `tolerance` in any channel. */
function differing(a, b, quad, clientWidth, tolerance = 16) {
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
  let total = 0, changed = 0, worst = 0, sum = 0;
  for (let y = 0; y < a.height; y++) for (let x = 0; x < a.width; x++) {
    if (!inside(x / scale, y / scale)) continue;
    total++;
    const at = (y * a.width + x) * 4;
    let difference = 0;
    for (let c = 0; c < 3; c++) difference = Math.max(difference, Math.abs(a.data[at + c] - b.data[at + c]));
    worst = Math.max(worst, difference); sum += difference;
    if (difference > tolerance) changed++;
  }
  return { pixels: total, changed, share: +(changed / Math.max(1, total)).toFixed(4), worst, mean: +(sum / Math.max(1, total)).toFixed(2) };
}

/** The node ids a bake report's detail lines name as drawn (stack bakes list them in brackets), by mesh. */
const drawnNodes = status => Object.fromEntries(status.details.map(line => [line.slice(0, line.indexOf(":")), (line.match(/\[(.*)\]$/)?.[1] ?? "").split(" ").filter(Boolean)]));
const summary = status => ({ ms: status.ms, compileMs: status.compileMs, drawMs: status.drawMs, fieldMs: status.fieldMs, drawn: status.drawn, reused: status.reused,
  nodesDrawn: status.nodesDrawn, nodesReused: status.nodesReused, mapsMiB: +(status.bytes / 1048576).toFixed(1), graph: status.graph });

const report = { resolution, query, studio: root };
let browser;
try {
  browser = await launchBrowser();
  const { project } = buildProject(resolution, 128);
  const layered = split(project);
  const open = async (page, candidate, name) => {
    const began = Date.now();
    await page.locator("#open-project").setInputFiles({ name, mimeType: "application/json", buffer: Buffer.from(JSON.stringify(candidate)) });
    return began;
  };
  const pageFor = async (search, from = server) => {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    await page.goto(`http://127.0.0.1:${from.address().port}/material.html?${search}`);
    const viewport = key => page.evaluate(key => document.querySelector("#viewport").dataset[key], key);
    const ready = async () => {
      await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 1800000 });
      assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", `${await page.locator("#status").textContent()}\n${(await page.locator("#diagnostics button").allTextContents()).join("\n")}`);
    };
    const baked = async () => {
      await page.waitForFunction(() => document.querySelector("#viewport").dataset.preview === "gpu-bake"
        && JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "{}").state === "baked", {}, { timeout: 1800000 });
      await waitForFrames(page, 3);
      return JSON.parse(await viewport("sceneBake"));
    };
    const shot = async label => {
      await page.evaluate(() => { const box = document.querySelector("#item-handles"); if (box?.checked) { box.checked = false; box.dispatchEvent(new Event("change")); } });
      await waitForFrames(page, 3);
      const bytes = await page.locator("#viewport canvas").screenshot();
      await mkdir(out, { recursive: true });
      await writeFile(resolve(out, `${label}.png`), bytes);
      return PNG.sync.read(bytes);
    };
    const project2d = points => page.evaluate(points => {
      const scene = window.__scene, canvas = document.querySelector("#viewport canvas"), three = window.__three;
      return points.map(point => {
        const at = new three.Vector3(...point).project(scene.camera);
        return [(at.x + 1) / 2 * canvas.clientWidth, (1 - at.y) / 2 * canvas.clientHeight];
      });
    }, points);
    await ready();
    return { page, errors, viewport, ready, baked, shot, project2d };
  };

  // 1. Opening, cold: a new page (and so no program compiled, no texture kept).
  const gpu = await pageFor(query);
  const opened = await open(gpu.page, layered, "washroom-split.studio.json");
  await gpu.page.waitForFunction(() => document.querySelector("#viewport").dataset.items !== undefined, {}, { timeout: 1800000 });
  await gpu.ready();
  const shownMs = Date.now() - opened;
  const first = await gpu.baked();
  report.open = { shownMs, bakedMs: Date.now() - opened, firstLook: await gpu.viewport("firstLook"), firstLookMs: Number(await gpu.viewport("firstLookMs")), ...summary(first),
    layers: first.layers, unsupported: first.unsupported, status: await gpu.page.locator("#status").textContent() };
  console.log(`Opened the split washroom: ${JSON.stringify(report.open)}`);
  assert.deepEqual(first.unsupported, {}, "every receiver shows GPU-baked maps");
  assert.ok(first.layers?.meshes > 0, "layered slots are composed on the GPU");
  assert.equal(first.layers.perPixel, "base only");
  assert.equal(JSON.parse(await gpu.viewport("layers")).perPixel, "base only", "the per-pixel look reports that it shows the base alone");
  const opening = drawnNodes(first);
  const stackMeshes = Object.entries(opening).filter(([, nodes]) => nodes.some(node => node.startsWith("stack.")));
  report.open.stackMeshes = stackMeshes.length;
  report.open.tiles = [...new Set(Object.values(opening).flat().filter(node => node.startsWith("tile/")))].length;
  console.log(`Stack bakes: ${stackMeshes.length} receivers; intrinsic tile nodes drawn: ${report.open.tiles}`);
  const width = await gpu.page.evaluate(() => document.querySelector("#viewport canvas").clientWidth);
  const regions = {
    tallWall: await gpu.project2d([[0.05, -0.013, 0.05], [2.0, -0.013, 0.05], [2.0, -0.013, 1.9], [0.05, -0.013, 1.9]]),
    lowWall: await gpu.project2d([[0.013, -2.25, 0.05], [0.013, -0.45, 0.05], [0.013, -0.45, 1.15], [0.013, -2.25, 1.15]]),
    tallBase: await gpu.project2d([[0.05, -0.013, 0.02], [2.0, -0.013, 0.02], [2.0, -0.013, 0.25], [0.05, -0.013, 0.25]]),
    tallTop: await gpu.project2d([[0.05, -0.013, 2.0], [2.6, -0.013, 2.0], [2.6, -0.013, 2.55], [0.05, -0.013, 2.55]]),
  };
  const gpuPicture = await gpu.shot("gpu-default");

  // 2. The CPU bake of the same project, as the reference.
  const cpu = await pageFor(`preview=bake&${query}`);
  const cpuOpened = await open(cpu.page, layered, "washroom-split.studio.json");
  await cpu.page.waitForFunction(() => document.querySelector("#viewport").dataset.items !== undefined, {}, { timeout: 1800000 });
  await cpu.ready();
  report.cpuOpenMs = Date.now() - cpuOpened;
  await waitForFrames(cpu.page, 3);
  const cpuPicture = await cpu.shot("cpu-default");
  report.gpuAgainstCpu = Object.fromEntries([["whole", null], ...Object.entries(regions)].map(([name, quad]) => [name, differing(gpuPicture, cpuPicture, quad, width, 16)]));
  console.log(`GPU against CPU (share of pixels off by more than 16/255): ${JSON.stringify(report.gpuAgainstCpu)}`);
  // The washroom without layers in the CPU bake, for what the GPU preview differs by without any layer (step 5).
  await open(cpu.page, project, "washroom-items.studio.json");
  await cpu.ready(); await waitForFrames(cpu.page, 3);
  const cpuPlain = await cpu.shot("cpu-plain");
  assert.equal(cpu.errors.length, 0, cpu.errors.join("\n"));
  await cpu.page.close();
  if (baselineServer) {
    // Before this change the GPU preview of a layered slot was its base alone.
    const baseline = await pageFor(query, baselineServer);
    await open(baseline.page, layered, "washroom-split.studio.json");
    await baseline.page.waitForFunction(() => document.querySelector("#viewport").dataset.items !== undefined, {}, { timeout: 1800000 });
    await baseline.ready(); await baseline.baked();
    const before = await baseline.shot("baseline-split");
    report.baseOnlyAgainstCpu = Object.fromEntries([["whole", null], ...Object.entries(regions)].map(([name, quad]) => [name, differing(before, cpuPicture, quad, width, 16)]));
    console.log(`Before (base alone) against CPU: ${JSON.stringify(report.baseOnlyAgainstCpu)}`);
    await open(baseline.page, project, "washroom-items.studio.json");
    await baseline.ready(); await baseline.baked();
    report.baselinePlain = await baseline.shot("baseline-plain");
    await baseline.page.close();
  }

  // 3. Widening the tall wall. (First: a project opened again, as the layer edits below are made, brings new image
  // bytes, which the scene's image textures are kept by.)
  const setControl = async (item, parameter, value) => {
    const from = Number(await gpu.viewport("inputRevision") ?? 0), began = Date.now();
    const field = gpu.page.getByRole("spinbutton", { name: `${item} ${parameter}`, exact: true });
    await field.fill(String(value)); await field.press("Tab");
    await gpu.page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, from, { timeout: 1800000 });
    await gpu.ready();
    const shown = Date.now() - began;
    const status = await gpu.baked();
    return { shownMs: shown, bakedMs: Date.now() - began, status };
  };
  const wide = await setControl("wall-tall", "width", 3.2);
  const wideNodes = drawnNodes(wide.status);
  report.widen = { shownMs: wide.shownMs, bakedMs: wide.bakedMs, ...summary(wide.status),
    tilesDrawn: [...new Set(Object.values(wideNodes).flat().filter(node => node.startsWith("tile/")))],
    meshesDrawn: Object.keys(wideNodes), nodes: wideNodes };
  console.log(`Wall width 2.7 to 3.2 m: ${JSON.stringify(report.widen)}`);
  assert.deepEqual(report.widen.tilesDrawn, [], "the base's intrinsic tiles are reused across the resize");
  await gpu.shot("gpu-wide");

  // 4. Layer parameter edits, through the project: the top layer of the walls (grime under the sill), then the lowest (top grime).
  // Each edit is made to the project as the edits before it left it (the tall wall widened).
  const edited = structuredClone(layered);
  edited.models.find(model => model.id === "wall-tall").parameters = { width: 3.2 };
  const editLayer = async (index, parameters, label) => {
    Object.assign(edited.materials.find(material => material.id === "concrete-walls").layers[index].parameters, parameters);
    const candidate = structuredClone(edited);
    const from = Number(await gpu.viewport("inputRevision") ?? 0);
    const began = await open(gpu.page, candidate, `${label}.studio.json`);
    await gpu.page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, from, { timeout: 1800000 });
    await gpu.ready();
    const shown = Date.now() - began;
    // The change settles before it is baked.
    await gpu.page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "{}").state === "baking", {}, { timeout: 5000 }).catch(() => {});
    const status = await gpu.baked();
    const nodes = Object.values(drawnNodes(status)).flat();
    const result = { shownMs: shown, bakedMs: Date.now() - began, ...summary(status), drawnNodes: [...new Set(nodes)].sort() };
    console.log(`Layer edit ${label}: ${JSON.stringify(result)}`);
    return result;
  };
  report.editTop = await editLayer(5, { amount: 0.4 }, "sill-grime-0.4");
  // Only the sill grime's planes and the composed maps: nothing beneath it, and no tile.
  assert.ok(report.editTop.drawnNodes.length > 0, "the edit is baked");
  assert.ok(report.editTop.drawnNodes.every(node => node.startsWith("stack.6/") || ["base", "properties", "emissive"].includes(node)),
    `an edit of the top layer draws only its planes and the composed maps: ${report.editTop.drawnNodes.join(", ")}`);
  report.editLowest = await editLayer(0, { amount: 0.5 }, "top-grime-0.5");
  assert.ok(!report.editLowest.drawnNodes.some(node => node.startsWith("stack.base/") || node.startsWith("tile/")), "an edit of a layer leaves the base alone");
  assert.ok(report.editLowest.drawnNodes.includes("stack.1/base_color") && report.editLowest.drawnNodes.includes("stack.6/base_color"), "the planes above that read it are drawn again");
  assert.ok(!report.editLowest.drawnNodes.some(node => /^stack\.\d\/(roughness|height|normal_offset)$/.test(node)),
    `channels the layer cannot reach are kept: ${report.editLowest.drawnNodes.join(", ")}`);
  // Back to the defaults.
  await editLayer(0, { amount: 1 }, "top-grime-1");
  await editLayer(5, { amount: 1 }, "sill-grime-1");

  // 5. Warm: another scene, then the split one again; what the graph kept is drawn from.
  const warmOther = await open(gpu.page, project, "washroom-items.studio.json");
  await gpu.ready(); await gpu.baked();
  report.otherSceneMs = Date.now() - warmOther;
  // The washroom without layers: the GPU preview against the CPU bake, and against the Studio before this change.
  const gpuPlain = await gpu.shot("gpu-plain");
  report.plainGpuAgainstCpu = Object.fromEntries([["whole", null], ...Object.entries(regions)].map(([name, quad]) => [name, differing(gpuPlain, cpuPlain, quad, width, 16)]));
  console.log(`Without layers, GPU against CPU: ${JSON.stringify(report.plainGpuAgainstCpu)}`);
  if (report.baselinePlain) {
    report.plainAgainstBaseline = differing(gpuPlain, report.baselinePlain, null, width, 0);
    delete report.baselinePlain;
    console.log(`Without layers, against the Studio before this change (any difference): ${JSON.stringify(report.plainAgainstBaseline)}`);
    assert.equal(report.plainAgainstBaseline.changed, 0, "materials without layers look exactly as before");
  }
  const warmBegan = await open(gpu.page, layered, "washroom-split.studio.json");
  await gpu.ready();
  const warm = await gpu.baked();
  report.warmOpen = { bakedMs: Date.now() - warmBegan, ...summary(warm) };
  console.log(`Reopened warm: ${JSON.stringify(report.warmOpen)}`);

  // 6. A drag shows the per-pixel look (the base alone), and bakes on release.
  await gpu.page.locator("#item-handles").check();
  await waitForFrames(gpu.page, 3);
  const handle = JSON.parse(await gpu.viewport("itemOverlays")).handles.find(handle => handle.item === "wall-tall" && handle.id === "width");
  const box = await gpu.page.locator("#viewport canvas").boundingBox();
  const direction = [handle.meter[0] - handle.screen[0], handle.meter[1] - handle.screen[1]];
  const from = [box.x + handle.screen[0], box.y + handle.screen[1]];
  await gpu.page.mouse.move(from[0], from[1]);
  await gpu.page.mouse.down();
  const steps = [];
  for (let step = 1; step <= 3; step++) {
    const began = Date.now(), before = Number(await gpu.viewport("inputRevision") ?? 0);
    await gpu.page.mouse.move(from[0] - direction[0] * 0.05 * step, from[1] - direction[1] * 0.05 * step);
    await gpu.page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, before, { timeout: 1800000 });
    await gpu.ready();
    assert.equal(await gpu.viewport("preview"), "shader", "a drag shows the per-pixel look");
    assert.equal(JSON.parse(await gpu.viewport("layers")).perPixel, "base only", "and says it shows a layered slot's base alone");
    steps.push(Date.now() - began);
  }
  const status = await gpu.page.locator("#status").textContent();
  const released = Date.now();
  await gpu.page.mouse.up();
  const dropped = await gpu.baked();
  report.drag = { stepsMs: steps, status, releaseBakedMs: Date.now() - released, ...summary(dropped) };
  console.log(`Drag: ${JSON.stringify(report.drag)}`);
  assert.match(status, /base only/, "the status says what the per-pixel look shows");

  assert.equal(gpu.errors.length, 0, gpu.errors.join("\n"));
  await mkdir(out, { recursive: true });
  await writeFile(resolve(out, `report-${resolution}.json`), JSON.stringify(report, null, 2) + "\n");
  console.log(`GPU layers passed at ${resolution} px.`);
} finally {
  await browser?.close();
  server.close();
  baselineServer?.close();
}
