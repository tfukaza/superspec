/**
 * The anchored mapping in the Material Studio's GPU preview, on the washroom built from items
 * (tests/studio/washroom-items): the walls' material is measured from each part's pattern anchor
 * (`mappings: { "concrete-walls": "anchored" }`), and at the project's texel density a wall face is a chart on the
 * anchor's texel lattice (role 3 of the layout records).
 *
 * 1. The GPU bake against the CPU bake (`?preview=bake`) of the same project: the picture of the tall wall is compared.
 * 2. Widening the wall leaves the part that existed before unchanged, with a wall material that reads no receiver
 *    bounds (WALLS, the tiled courtyard recipe by default).
 * 3. The same with the shipped concrete (which reads its receiver's bounds, so only the comparison with the CPU is
 *    asserted for it).
 *
 * RESOLUTION (default 512: the 3.2 m wall reaches 128 texels per meter only in a 512 px atlas; in a smaller one it is
 * laid out off the lattice, as any receiver) sets the texture size; SCREENSHOTS names a directory for PNGs.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { PNG } from "pngjs";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom-items/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const resolution = Number(process.env.RESOLUTION ?? 512);
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

/** Pixels of a convex screen quad that differ by more than `tolerance` (of 255, any channel), with the mean difference. */
function differing(a, b, quad, clientWidth, tolerance) {
  assert.deepEqual([a.width, a.height], [b.width, b.height]);
  // Screenshot pixels per CSS pixel of the canvas, in which the quad is given.
  const scale = a.width / clientWidth;
  const inside = (x, y) => {
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
    const difference = Math.max(Math.abs(a.data[at] - b.data[at]), Math.abs(a.data[at + 1] - b.data[at + 1]), Math.abs(a.data[at + 2] - b.data[at + 2]));
    worst = Math.max(worst, difference); sum += difference;
    if (difference > tolerance) changed++;
  }
  assert.ok(total > 1000, `the compared region covers the wall (${total} pixels)`);
  return { pixels: total, changed, share: +(changed / Math.max(1, total)).toFixed(4), worst, mean: +(sum / Math.max(1, total)).toFixed(2) };
}

let browser;
try {
  browser = await launchBrowser();
  /** Opens the project in a page of its own and returns pictures of it before and after the wall is widened. */
  const run = async (project, query, name) => {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    const viewport = key => page.evaluate(key => document.querySelector("#viewport").dataset[key], key);
    const ready = async () => {
      await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 900000 });
      assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", `${await page.locator("#status").textContent()}\n${(await page.locator("#diagnostics button").allTextContents()).join("\n")}`);
    };
    const gpu = !/preview=bake/.test(query);
    const shown = async () => {
      await ready();
      if (gpu) await page.waitForFunction(() => document.querySelector("#viewport").dataset.preview === "gpu-bake"
        && JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "{}").state === "baked", {}, { timeout: 900000 });
      await waitForFrames(page, 3);
    };
    const shot = async label => {
      await page.evaluate(() => { const box = document.querySelector("#item-handles"); if (box.checked) { box.checked = false; box.dispatchEvent(new Event("change")); } });
      await waitForFrames(page, 3);
      const bytes = await page.locator("#viewport canvas").screenshot();
      if (shots) { await mkdir(shots, { recursive: true }); await writeFile(resolve(shots, `${name}-${label}.png`), bytes); }
      return PNG.sync.read(bytes);
    };
    await page.goto(`http://127.0.0.1:${server.address().port}/material.html?${query}`);
    await ready();
    await page.locator("#open-project").setInputFiles({ name: "washroom-items.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
    await page.waitForFunction(() => document.querySelector("#viewport").dataset.items !== undefined, {}, { timeout: 900000 });
    await shown();
    const scale = await page.evaluate(() => document.querySelector("#viewport canvas").clientWidth);
    const project2d = points => page.evaluate(points => {
      const scene = window.__scene, canvas = document.querySelector("#viewport canvas"), three = window.__three;
      return points.map(point => {
        const at = new three.Vector3(...point).project(scene.camera);
        return [(at.x + 1) / 2 * canvas.clientWidth, (1 - at.y) / 2 * canvas.clientHeight];
      });
    }, points);
    const atlases = gpu ? JSON.parse(await viewport("sceneBake")).atlases : undefined;
    const before = await shot("default");
    // The part of the wall that existed before, clear of its old end, as the item test takes it; and the bare stretch
    // of it left of the tile panel and above the window, where nothing stands in front.
    const region = await project2d([[0.05, -0.013, 0.05], [2.0, -0.013, 0.05], [2.0, -0.013, 1.9], [0.05, -0.013, 1.9]]);
    const from = Number(await viewport("inputRevision") ?? 0);
    const field = page.getByRole("spinbutton", { name: "wall-tall width", exact: true });
    await field.fill("3.2"); await field.press("Tab");
    await page.waitForFunction(from => Number(document.querySelector("#viewport").dataset.inputRevision ?? 0) > from, from, { timeout: 900000 });
    await shown();
    const after = await shot("wide");
    const wideRegion = await project2d([[0.05, -0.013, 0.05], [3.15, -0.013, 0.05], [3.15, -0.013, 1.9], [0.05, -0.013, 1.9]]);
    assert.equal(errors.length, 0, errors.join("\n"));
    await page.close();
    return { before, after, region, wideRegion, scale, atlases };
  };
  const query = process.env.QUERY ?? "ao=off&gpuprobe&shadows=off";
  const report = [];
  for (const [label, walls] of [["tile", process.env.WALLS ?? "tiled-courtyard.material.ts"], ["concrete", undefined]]) {
    const { project } = buildProject(resolution, undefined, { weathering: false, anchored: true, walls });
    const gpu = await run(project, query, `anchored-${label}-gpu`);
    const cpu = await run(project, `${query}&preview=bake`, `anchored-${label}-cpu`);
    const wall = gpu.atlases["wall-tall/part/wall/wall/front"];
    // 1. GPU bake against CPU bake, on the wall: by a tolerance that passes filtering differences and no moved pattern.
    const close = differing(gpu.before, cpu.before, gpu.region, gpu.scale, 16);
    const closeWide = differing(gpu.after, cpu.after, gpu.wideRegion, gpu.scale, 16);
    // 2. The widened wall against the wall before, in each bake.
    const keptGpu = differing(gpu.before, gpu.after, gpu.region, gpu.scale, 6);
    const keptCpu = differing(cpu.before, cpu.after, cpu.region, cpu.scale, 6);
    console.log(`${label} walls, anchored, ${resolution} px (wall front atlas ${wall}):`);
    console.log(`  GPU bake against CPU bake on the wall: ${close.changed} of ${close.pixels} pixels differ by more than 16/255 (${(close.share * 100).toFixed(2)} %, mean difference ${close.mean}/255, largest ${close.worst}/255); widened: ${closeWide.changed} of ${closeWide.pixels} (${(closeWide.share * 100).toFixed(2)} %, mean ${closeWide.mean}/255)`);
    console.log(`  widening 2.7 to 3.2 m, the wall that existed before: GPU bake ${keptGpu.changed} of ${keptGpu.pixels} pixels differ by more than 6/255 (largest ${keptGpu.worst}/255); CPU bake ${keptCpu.changed} of ${keptCpu.pixels} (largest ${keptCpu.worst}/255)`);
    report.push({ label, close, closeWide, keptGpu, keptCpu });
    assert.ok(close.share < 0.03 && closeWide.share < 0.03, `${label}: the GPU bake of an anchored wall is the CPU bake's picture`);
    if (label === "tile" && resolution >= 512) {
      assert.ok(keptGpu.share < 0.002, "a bounds-free anchored material keeps its pattern through a resize in the GPU bake");
      assert.ok(keptCpu.share < 0.002, "and in the CPU bake");
    }
  }
  console.log(`Anchored mapping in the GPU preview passed at ${resolution} px.`);
} finally {
  await browser?.close();
  server.close();
}
