/**
 * The washroom's lawn and its painted grass alone (roadmap stage 6), for comparing with the painted trees
 * (tests/painted-mass-shrub-browser.mjs): the washroom project with its lawn (`buildProject(..., { grass })`), the room
 * hidden, pictured from above at an angle, at eye height, and low and close. Ambient occlusion is off, as for the trees.
 * Write the lawn first:
 *   cargo test --release -p superspec-engine --test treespec_batches -- --ignored write_washroom_lawn
 * Pictures go to OUT (default generated/verification/grass-scene) as LABEL-<view>.png (LABEL default grass).
 * PROFILE=lit draws the lit foliage profile instead of the painted one; the real GPU wants
 * BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist".
 */
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject, loadLawn } from "./studio/washroom/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve(process.env.OUT ?? "generated/verification/grass-scene");
await mkdir(out, { recursive: true });
const { project } = buildProject(256, undefined, { grass: loadLawn() });
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
  // GRASS_BLUR=<pixels>: the far grass levels (from GRASS_BLUR_FROM, default 3) blurred into dabs (scene.ts grassBlur).
  // A lawn written with LAWN_LEVEL=k (LAWN names its document) has that level alone, as level 0: GRASS_BLUR_FROM=0.
  const profile = (process.env.PROFILE === "lit" ? "&foliage=lit" : "")
    + (process.env.GRASS_BLUR ? `&grassblur=${process.env.GRASS_BLUR}&grassblurfrom=${process.env.GRASS_BLUR_FROM ?? 3}` : "");
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?gpuprobe&ao=off${profile}`);
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 600000 });
  await page.locator("#open-project").setInputFiles({ name: "grass.studio.json", mimeType: "application/json", buffer: Buffer.from(text) });
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready" && JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked", {}, { timeout: 900000 });
  // A boundary look (LAWN_BOUNDARY=1 lawn) has its field made after the first frames; grass everywhere needs none.
  await waitForFrames(page, 8);
  await page.waitForFunction(() => { const f = JSON.parse(document.querySelector("#viewport").dataset.foliage ?? "null"); return !!f; }, {}, { timeout: 120000 });
  // The lawn: its mesh's bounds frame the views; every other washroom part is hidden.
  const info = await page.evaluate(keep => {
    const s = window.__scene, T = window.__three, box = new T.Box3();
    let lawn = null;
    for (const m of s.models.children) {
      if (m.userData.foliage) continue;
      const name = m.name;
      // The lawn is the washroom model's node 12 (as the painted-mass tree script keeps it).
      const isLawn = name.includes("node/12/");
      if (isLawn) { lawn = m; box.expandByObject(m); } else if (name.startsWith("washroom") || name === "bolts") m.visible = false;
    }
    if (!lawn) {
      // Fall back to the grass's own bounds.
      for (const m of s.models.children) if (m.userData.foliage) box.expandByObject(m);
    }
    // KEEP_SHADOWS=1 keeps the room's shadows on the lawn (the map is made once; hiding the room leaves them).
    if (!keep) { s.renderer.shadowMap.needsUpdate = true; s.invalidate?.(); }
    s.draw();
    return { found: !!lawn, name: lawn?.name ?? null, min: box.min.toArray(), max: box.max.toArray() };
  }, process.env.KEEP_SHADOWS === "1");
  console.log(`lawn ${info.found ? info.name : "(not found: framed by the grass)"}: ${info.min.map(v => v.toFixed(2))} to ${info.max.map(v => v.toFixed(2))}`);
  const [x0, y0, z0] = info.min, [x1, y1, z1] = info.max;
  const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2, cz = (z0 + z1) / 2, span = Math.max(x1 - x0, y1 - y0);
  const views = {
    above: { position: [cx - span * 0.55, cy - span * 0.75, cz + span * 0.7], target: [cx, cy, cz] },
    eye: { position: [cx - span * 0.45, cy - span * 0.85, cz + 1.6], target: [cx, cy, cz + 0.2] },
    low: { position: [cx - span * 0.2, cy - span * 0.35, cz + 0.45], target: [cx + span * 0.05, cy, cz + 0.15] },
  };
  for (const [name, view] of Object.entries(views)) {
    await page.evaluate(view => { const s = window.__scene; s.camera.position.fromArray(view.position); s.controls.target.fromArray(view.target); s.controls.update(); s.draw(); }, view);
    await waitForFrames(page, 6);
    const file = resolve(out, `${process.env.LABEL ?? "grass"}-${name}.png`);
    await writeFile(file, await page.locator("#viewport canvas").screenshot());
    console.log(`Wrote ${file}`);
  }
} finally {
  await browser.close();
  server.close();
}
