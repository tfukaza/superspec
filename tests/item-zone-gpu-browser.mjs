/**
 * Zones evaluated in the GPU bake against the CPU bake: opens the zoned wall the native engine wrote
 * (`ZONE_PARITY_OUTPUT=<dir> cargo test -p superspec-engine --test item_zones native_zone_scene_packet_for_parity`; a
 * base material that shows u, v and whether a texel is in a zone, and a layer over the zones it follows) in the built
 * Material Studio twice: with the default GPU-baked preview, whose shaders evaluate `zoneCoord`/`zoneExtent` from the
 * receiver's packed zones, and with `?preview=bake`, the CPU bake. The pictures must agree within TOLERANCE/255 on
 * all but SHARE of the pixels (zone edges fall on other texels in single precision).
 * Usage: node tests/item-zone-gpu-browser.mjs <dir>. STUDIO_UI_ROOT names the built Studio (default generated/rust-editor).
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { PNG } from "pngjs";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const directory = resolve(process.argv[2] ?? "generated/item-zone-parity");
const project = JSON.parse(await readFile(resolve(directory, "project.json"), "utf8"));
project.resolution = 256;
// A studio file carries its assets (this project has none).
project.assets ??= {};
const tolerance = Number(process.env.TOLERANCE ?? 16), allowed = Number(process.env.SHARE ?? 0.02);

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
  const picture = async (search, gpu) => {
    const page = await browser.newPage({ viewport: { width: 900, height: 700 } });
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    await page.goto(`http://127.0.0.1:${server.address().port}/material.html?${search}`);
    await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 600000 });
    await page.locator("#open-project").setInputFiles({ name: "zones.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
    await page.waitForFunction(() => document.querySelector("#viewport").dataset.items !== undefined, {}, { timeout: 600000 });
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 600000 });
    assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", await page.locator("#status").textContent());
    let bake = null;
    if (gpu) {
      await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "{}").state === "baked", {}, { timeout: Number(process.env.BAKE_TIMEOUT ?? 600000) })
        .catch(async error => {
          const said = await page.evaluate(() => ({ preview: document.querySelector("#viewport").dataset.preview, bake: document.querySelector("#viewport").dataset.sceneBake,
            fallback: document.querySelector("#viewport").dataset.sceneBakeFallback, status: document.querySelector("#status")?.textContent }));
          throw new Error(`${error.message}\n${JSON.stringify(said).slice(0, 3000)}\n${errors.join("\n").slice(0, 2000)}`);
        });
      bake = JSON.parse(await page.evaluate(() => document.querySelector("#viewport").dataset.sceneBake));
    }
    // Square on to the wall's front (it faces -y), framing it.
    await page.evaluate(() => {
      const s = window.__scene, T = window.__three, box = new T.Box3();
      for (const m of s.models.children) box.expandByObject(m);
      const center = box.getCenter(new T.Vector3());
      s.camera.position.set(center.x, box.min.y - 9, center.z); s.controls.target.copy(center); s.controls.update(); s.draw();
    });
    await waitForFrames(page, 6);
    const bytes = await page.locator("#viewport canvas").screenshot();
    assert.equal(errors.length, 0, errors.join("\n"));
    await page.close();
    return { png: PNG.sync.read(bytes), bytes, bake };
  };
  const gpu = await picture("ao=off&shadows=off&gpuprobe", true);
  assert.deepEqual(gpu.bake.unsupported, {}, "every receiver is GPU-baked");
  assert.equal(gpu.bake.fieldMs ?? 0, 0, "no CPU field is baked for the zones");
  const cpu = await picture("preview=bake&ao=off&shadows=off&gpuprobe", false);
  await writeFile(resolve(directory, "gpu.png"), gpu.bytes);
  await writeFile(resolve(directory, "cpu.png"), cpu.bytes);
  let changed = 0, worst = 0;
  for (let at = 0; at < gpu.png.data.length; at += 4) {
    let difference = 0;
    for (let c = 0; c < 3; c++) difference = Math.max(difference, Math.abs(gpu.png.data[at + c] - cpu.png.data[at + c]));
    worst = Math.max(worst, difference);
    if (difference > tolerance) changed++;
  }
  const share = changed / (gpu.png.data.length / 4);
  console.log(`GPU zones against the CPU bake: ${(share * 100).toFixed(2)}% of pixels off by more than ${tolerance}/255 (worst ${worst}); bake ${JSON.stringify({ ms: gpu.bake.ms, fieldMs: gpu.bake.fieldMs })}`);
  assert.ok(share <= allowed, `${share} of the pixels differ`);
} finally {
  await browser.close();
  server.close();
}
