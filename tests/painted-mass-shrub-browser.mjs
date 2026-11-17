/**
 * Prototype candidate: the sprig shrub painted as a mass (docs/plans/tree-item-roadmap.md, "Exploring: painted masses
 * with a leaf normal map"). Its 12 clump surfaces (TreeSpec's mass model, clumps only) are the base mesh, shaded through
 * the stroke-painted leaf normal map (browser/renderer/leaf-normals.ts, 16-leaf brush, camera-facing) with leafy contour
 * fuzz (browser/renderer/contour-fuzz.ts) reading the same map; its bark is kept and its leaf cards hidden.
 *
 * Two candidates, by base mesh: `sprig-shrub-clumps.tree.json` (its 12 clumps) and `sprig-shrub-hull.tree.json` (its one
 * canopy hull, the limbs' shrink-wrap, drawn whole at the near level). Either is the shrub's near `hull/lod0`, so this
 * script draws whichever was written; LABEL names the pictures (default painted-mass).
 *
 * Write the plant first (e.g. from crates/engine/src/treespec/fixtures/sprig-shrub-clumps.tree.json):
 *   SHRUB_TREE=crates/engine/src/treespec/fixtures/sprig-shrub-clumps.tree.json \
 *   TREESPEC_OUTPUT=generated/verification/painted-mass \
 *   cargo test --release -p superspec-engine --test treespec_batches -- --ignored write_washroom_trees
 * then run this (the real GPU wants BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist"). Pictures from four sides
 * and above go to OUT (default generated/verification/painted-mass). TILE, BRUSH, SIZE, DENSITY, REACH, DIST tune it.
 * Ambient occlusion is off: its pass does not draw the prototype's shaders.
 */
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject, loadTrees } from "./studio/washroom/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve(process.env.OUT ?? "generated/verification/painted-mass");
const document = resolve(process.env.TREES_DOCUMENT ?? `${out}/washroom-trees.json`);
await mkdir(out, { recursive: true });

// The washroom project holding the shrub alone (and the lawn it stands on), its canopy in the painted shrub recipe.
const plants = loadTrees(document);
const shrub = plants.plants.find(plant => plant.id === "sprig-shrub");
if (!shrub) throw new Error(`${document} has no sprig-shrub; write it as the header says.`);
const one = { ...plants, plants: [shrub], batches: plants.batches.filter(batch => batch.foliage?.patch === "sprig-shrub")
  .map(batch => batch.foliage.material === "painted-canopy" ? { ...batch, foliage: { ...batch.foliage, material: "painted-shrub" } } : batch) };
const { project } = buildProject(256, undefined, { trees: one });
project.modules["painted-shrub.material.ts"] = readFileSync(resolve("tests/studio/washroom/materials/painted-shrub.material.ts"), "utf8");
project.materials.push({ id: "painted-shrub", entry: "painted-shrub.material.ts", parameters: {}, meshes: [], seed: 7 });
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
  const page = await browser.newPage({ viewport: { width: 1000, height: 1000 } });
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?gpuprobe&ao=off`);
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 600000 });
  await page.locator("#open-project").setInputFiles({ name: "shrub.studio.json", mimeType: "application/json", buffer: Buffer.from(text) });
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready" && JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked", {}, { timeout: 900000 });
  const info = await page.evaluate(({ tile, brush, size, density, reach, strength, offset }) => {
    const s = window.__scene, T = window.__three;
    s.draw();
    let hull;
    for (const m of s.models.children) {
      if ((m.name.startsWith("washroom") && !m.name.includes("node/12/")) || m.name === "bolts") m.visible = false;
      const family = m.userData.foliage?.family;
      if (!family) continue;
      if (m.name.endsWith("hull/lod0")) hull = m;
      if (family !== "bark") { m.visible = false; m.userData.source = { visible: false }; }
    }
    if (!hull) return { error: "the shrub has no near hull (its clumps)" };
    // The clumps' mesh in world space.
    const geometry = new T.BufferGeometry();
    geometry.setAttribute("position", hull.geometry.getAttribute("position").clone());
    geometry.setAttribute("normal", hull.geometry.getAttribute("normal").clone());
    if (hull.geometry.index) geometry.setIndex(hull.geometry.index.clone());
    const placed = new T.Matrix4().fromArray(hull.instanceMatrix.array, 0).premultiply(hull.matrixWorld);
    geometry.applyMatrix4(placed);
    geometry.computeBoundingBox();
    const centre = geometry.boundingBox.getCenter(new T.Vector3());
    const sun = new T.Vector3(...JSON.parse(document.querySelector("#viewport").dataset.lighting).lights[0].direction);
    s.models.add(new T.Mesh(window.__smoothGeometry(geometry), window.__leafNormalMaterial(sun, tile, strength, 0.35, true, centre, brush, offset)));
    const fuzz = window.__contourFuzz(geometry, window.__leafCards().cards, { size, density, reach, sun, normals: window.__leafCards().normals,
      leafMap: { texture: window.__leafNormalTexture(brush), tile, origin: centre, strength, offset } });
    s.models.add(fuzz);
    s.draw();
    return { centre: centre.toArray(), anchors: fuzz.userData.contourFuzz.anchors };
  }, { tile: Number(process.env.TILE ?? 0.6), brush: Number(process.env.BRUSH ?? 16), size: Number(process.env.SIZE ?? 0.3), density: Number(process.env.DENSITY ?? 14), reach: Number(process.env.REACH ?? 0.08),
    // The lit side spread over the upper bands rather than clipped into the top one (STRENGTH tips the leaves' normals,
    // OFFSET shifts every band's threshold).
    strength: Number(process.env.STRENGTH ?? 1.5), offset: Number(process.env.OFFSET ?? -0.05) });
  if (info.error) throw new Error(info.error);
  console.log(`clumps centre ${info.centre.map(v => v.toFixed(2))}, ${info.anchors} fuzz anchors`);
  const [cx, cy, cz] = info.centre, r = Number(process.env.DIST ?? 3.6);
  for (const name of ["0", "90", "180", "270", "top"]) {
    const view = name === "top"
      ? { position: [cx + 0.6, cy - 0.7, cz + r * 1.3], target: [cx, cy, cz - 0.2] }
      : (() => { const a = Number(name) * Math.PI / 180 - 0.6; return { position: [cx + Math.cos(a) * r, cy + Math.sin(a) * r, cz + r * 0.2], target: [cx, cy, cz - 0.15] }; })();
    await page.evaluate(view => { const s = window.__scene; s.camera.position.fromArray(view.position); s.controls.target.fromArray(view.target); s.controls.update(); s.draw(); }, view);
    await waitForFrames(page, 4);
    const file = resolve(out, `${process.env.LABEL ?? "painted-mass"}-${name}.png`);
    await writeFile(file, await page.locator("#viewport canvas").screenshot());
    console.log(`Wrote ${file}`);
  }
} finally {
  await browser.close();
  server.close();
}
