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
    + (process.env.GRASS_BLUR ? `&grassblur=${process.env.GRASS_BLUR}&grassblurfrom=${process.env.GRASS_BLUR_FROM ?? 3}`
      + (process.env.GRASS_BLUR_PATCH ? `&grassblurpatch=${process.env.GRASS_BLUR_PATCH}` : "") : "")
    // CLUMP_BLUR=<pixels>: the clumps' colours blurred among themselves (CLUMP_BLUR_MIX how much, default 1).
    + (process.env.CLUMP_BLUR ? `&clumpblur=${process.env.CLUMP_BLUR}&clumpblurmix=${process.env.CLUMP_BLUR_MIX ?? 1}` : "");
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?gpuprobe&ao=off${profile}`);
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 600000 });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  await page.locator("#open-project").setInputFiles({ name: "grass.studio.json", mimeType: "application/json", buffer: Buffer.from(text) });
  try {
    await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready" && JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked", {}, { timeout: Number(process.env.BAKE_TIMEOUT ?? 900000) });
  } catch (error) {
    // Say why: the page's status, its bake state and its errors.
    const state = await page.evaluate(() => ({ status: document.querySelector("#status")?.textContent, state: document.querySelector("#status")?.dataset.state, bake: document.querySelector("#viewport")?.dataset.sceneBake,
      diagnostics: [...document.querySelectorAll("#diagnostics button")].map(button => button.textContent) }));
    console.log(`Not baked: ${JSON.stringify(state)}\n${errors.slice(0, 8).join("\n")}`);
    throw error;
  }
  // A boundary look (LAWN_BOUNDARY=1 lawn) has its field made after the first frames; grass everywhere needs none.
  await waitForFrames(page, 8);
  await page.waitForFunction(() => { const f = JSON.parse(document.querySelector("#viewport").dataset.foliage ?? "null"); return !!f; }, {}, { timeout: 120000 });
  // The lawn: its mesh's bounds frame the views; every other washroom part is hidden.
  const info = await page.evaluate(({ keep, pillar, ball, sun, noGrass, clumps, relief, reliefOptions, tufts }) => {
    let lightingInfo = null;
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
    // NO_GRASS=1: the lawn's grass hidden, to judge the painted ground alone.
    if (noGrass) for (const m of s.models.children) if (m.userData.foliage) { m.visible = false; m.userData.source = { visible: false }; }
    // CLUMPS=<json options>: soft clump mounds over the lawn (grass-clumps.ts); CLUMPS=1 at their defaults.
    let clumpInfo = null;
    if (clumps && lawn) {
      // The grass's shadow-map uniforms, so the sun's cast shadow falls on the clumps too.
      let shadow;
      s.models.traverse(o => { const u = o.material?.userData?.foliageUniforms; if (!shadow && u?.msFoliageShadow) shadow = { msFoliageShadow: u.msFoliageShadow, msFoliageShadowMatrix: u.msFoliageShadowMatrix, msFoliageShadowParams: u.msFoliageShadowParams }; });
      const mesh = window.__grassClumps(lawn, { ...(clumps === "1" ? {} : JSON.parse(clumps)), shadow });
      s.models.add(mesh);
      clumpInfo = mesh.userData.grassClumps;
    }
    // PILLAR=1: a concrete pillar standing on the lawn (0.35 m square, 1.8 m tall), to see the shadows it casts on
    // the ground and grass.
    if (pillar && lawn) {
      const at = new T.Vector3(box.min.x + (box.max.x - box.min.x) * 0.28, box.min.y + (box.max.y - box.min.y) * 0.62, box.max.z + 5);
      const hit = new T.Raycaster(at, new T.Vector3(0, 0, -1)).intersectObject(lawn)[0];
      const base = hit ? hit.point.z : 0;
      const column = new T.Mesh(new T.BoxGeometry(0.35, 0.35, 1.8), new T.MeshStandardMaterial({ color: 0x9a9893, roughness: 0.9 }));
      column.position.set(at.x, at.y, base + 0.9 - 0.05);
      column.castShadow = column.receiveShadow = true;
      column.name = "test/pillar";
      s.models.add(column);
      // A ball for scale: a size 5 soccer ball, 22 cm across (BALL=basketball: 24 cm), resting on the lawn a meter in
      // front of the pillar.
      const ballAt = new T.Vector3(at.x + 0.6, at.y - 0.8, box.max.z + 5);
      const ballHit = new T.Raycaster(ballAt, new T.Vector3(0, 0, -1)).intersectObject(lawn)[0];
      const radius = ball === "basketball" ? 0.12 : 0.11;
      const sphere = new T.Mesh(new T.SphereGeometry(radius, 48, 24),
        new T.MeshStandardMaterial({ color: ball === "basketball" ? 0xc8642a : 0xf2f0ea, roughness: 0.6 }));
      sphere.position.set(ballAt.x, ballAt.y, (ballHit ? ballHit.point.z : base) + radius);
      sphere.castShadow = sphere.receiveShadow = true;
      sphere.name = "test/ball";
      s.models.add(sphere);
      // Lighting again, so the pillar casts and the shadow map is fitted to the scene with it. SUN=<azimuth>,<elevation>
      // (degrees, azimuth from +x toward +y) turns the sun.
      const record = JSON.parse(document.querySelector("#viewport").dataset.lighting);
      if (sun) {
        const [az, el] = sun.split(",").map(v => Number(v) * Math.PI / 180);
        record.lights[0].direction = [Math.cos(el) * Math.cos(az), Math.cos(el) * Math.sin(az), Math.sin(el)];
      }
      s.setLighting(record);
      lightingInfo = { direction: record.lights[0].direction, shadows: record.shadows ?? record.lights[0].shadow ?? null, casting: column.castShadow, map: s.renderer.shadowMap.enabled };
    }
    // RELIEF=normal|displace|parallax (RELIEF_OPTIONS=<json>): the lawn drawn as grass relief (grass-relief.ts) in its
    // place; the lawn itself is hidden after the ground fields were made from it.
    let reliefInfo = null;
    if (relief && lawn) {
      let shadow;
      s.models.traverse(o => { const u = o.material?.userData?.foliageUniforms; if (!shadow && u?.msFoliageShadow) shadow = { msFoliageShadow: u.msFoliageShadow, msFoliageShadowMatrix: u.msFoliageShadowMatrix, msFoliageShadowParams: u.msFoliageShadowParams }; });
      // RELIEF=1: the normal map, the chosen method.
      const mesh = window.__grassRelief(lawn, relief === "1" ? "normal" : relief, { ...(reliefOptions ? JSON.parse(reliefOptions) : {}), shadow });
      s.models.add(mesh);
      lawn.visible = false;
      reliefInfo = mesh.userData.grassRelief;
      // TUFTS=1|<json>: pre-blurred grass tuft billboards over the relief (grass-tufts.ts).
      if (tufts) {
        const tuftMesh = window.__grassTufts(lawn, mesh, tufts === "1" ? {} : JSON.parse(tufts));
        s.models.add(tuftMesh);
        reliefInfo = { ...reliefInfo, tufts: tuftMesh.userData.grassTufts };
      }
    }
    // KEEP_SHADOWS=1 keeps the room's shadows on the lawn (the map is made once; hiding the room leaves them).
    if (!keep) { s.renderer.shadowMap.needsUpdate = true; s.invalidate?.(); }
    s.draw();
    return { found: !!lawn, name: lawn?.name ?? null, min: box.min.toArray(), max: box.max.toArray(), lighting: lightingInfo, clumps: clumpInfo, relief: reliefInfo };
  }, { keep: process.env.KEEP_SHADOWS === "1", pillar: process.env.PILLAR === "1", ball: process.env.BALL ?? "soccer", sun: process.env.SUN ?? "", noGrass: process.env.NO_GRASS === "1", clumps: process.env.CLUMPS ?? "", relief: process.env.RELIEF ?? "", reliefOptions: process.env.RELIEF_OPTIONS ?? "", tufts: process.env.TUFTS ?? "" });
  if (info.lighting) console.log(`lighting ${JSON.stringify(info.lighting)}`);
  if (info.relief) console.log(`relief ${JSON.stringify(info.relief)}`);
  if (info.clumps) console.log(`clumps ${JSON.stringify(info.clumps)}`);
  console.log(`lawn ${info.found ? info.name : "(not found: framed by the grass)"}: ${info.min.map(v => v.toFixed(2))} to ${info.max.map(v => v.toFixed(2))}`);
  const [x0, y0, z0] = info.min, [x1, y1, z1] = info.max;
  const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2, cz = (z0 + z1) / 2, span = Math.max(x1 - x0, y1 - y0);
  const views = {
    above: { position: [cx - span * 0.55, cy - span * 0.75, cz + span * 0.7], target: [cx, cy, cz] },
    eye: { position: [cx - span * 0.45, cy - span * 0.85, cz + 1.6], target: [cx, cy, cz + 0.2] },
    low: { position: [cx - span * 0.2, cy - span * 0.35, cz + 0.45], target: [cx + span * 0.05, cy, cz + 0.15] },
    // From the far side: the slopes the other views see from behind.
    back: { position: [cx + span * 0.75, cy + span * 0.55, cz + span * 0.55], target: [cx, cy, cz] },
  };
  // VIEW_SCALE: every view this many times as far from the lawn's middle (a hill lawn wants about 1.8).
  const scale = Number(process.env.VIEW_SCALE ?? 1);
  for (const view of Object.values(views)) view.position = view.position.map((v, a) => view.target[a] + (v - view.target[a]) * scale);
  for (const [name, view] of Object.entries(views)) {
    await page.evaluate(view => { const s = window.__scene; s.camera.position.fromArray(view.position); s.controls.target.fromArray(view.target); s.controls.update(); s.draw(); }, view);
    await waitForFrames(page, 6);
    const file = resolve(out, `${process.env.LABEL ?? "grass"}-${name}.png`);
    await writeFile(file, await page.locator("#viewport canvas").screenshot());
    console.log(`Wrote ${file}`);
    // TIME=1: the median of 30 frames of this view, each finished by a one-pixel read back.
    if (process.env.TIME === "1") {
      const ms = await page.evaluate(() => {
        const s = window.__scene, gl = s.renderer.getContext(), pixel = new Uint8Array(4), times = [];
        for (let i = 0; i < 33; i++) { const t = performance.now(); s.draw(); gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel); if (i >= 3) times.push(performance.now() - t); }
        times.sort((a, b) => a - b);
        return times[times.length >> 1];
      });
      console.log(`  ${name}: ${ms.toFixed(2)} ms a frame`);
    }
  }
  if (errors.length) console.log(`Page errors:\n${errors.slice(0, 8).join("\n")}`);
} finally {
  await browser.close();
  server.close();
}
