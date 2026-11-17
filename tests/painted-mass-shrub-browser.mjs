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
 *
 * Trees: write one with TREE_TREE=<absolute path to poplar.tree.json or birch.tree.json> (it stands in the broadleaf's
 * stead), then picture it with PLANT=poplar (or birch) DIST=13 TILE=1.0 SIZE=0.6 DENSITY=5 COARSE=0.3.
 *
 * Cost: TIME=1 prints the median frame time of the full picture and of its parts; COPIES=25 draws a grid of that many
 * copies of the surface and fuzz while timing; VIEWPORT=WxH sizes the page.
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
// PLANT picks the plant by id (default sprig-shrub; a tree written through TREE_TREE goes by its own id).
const plantId = process.env.PLANT ?? "sprig-shrub";
const plants = loadTrees(document);
const shrub = plants.plants.find(plant => plant.id === plantId);
if (!shrub) throw new Error(`${document} has no ${plantId}; write it as the header says.`);
const one = { ...plants, plants: [shrub], batches: plants.batches.filter(batch => batch.foliage?.patch === plantId)
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
  const [width, height] = (process.env.VIEWPORT ?? "1000x1000").split("x").map(Number);
  const page = await browser.newPage({ viewport: { width, height } });
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?gpuprobe&ao=off`);
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 600000 });
  await page.locator("#open-project").setInputFiles({ name: "shrub.studio.json", mimeType: "application/json", buffer: Buffer.from(text) });
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready" && JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked", {}, { timeout: 900000 });
  const info = await page.evaluate(({ tile, brush, size, density, reach, strength, offset, coarse, densityView, densityBase, hide, highlight }) => {
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
    let geometry, surface, f;
    if (densityBase) {
      // BASE=density: the leaf density field's surface at a level (leaf-density.ts) in place of the clumps.
      const started = performance.now();
      f = window.__densityField(densityBase.sites, densityBase.radius, densityBase.cell);
      geometry = window.__densitySurface(f, densityBase.level);
      surface = { triangles: geometry.index.count / 3, ms: performance.now() - started, full: f.full };
    } else {
      // The clumps' mesh in world space.
      geometry = new T.BufferGeometry();
      geometry.setAttribute("position", hull.geometry.getAttribute("position").clone());
      geometry.setAttribute("normal", hull.geometry.getAttribute("normal").clone());
      if (hull.geometry.index) geometry.setIndex(hull.geometry.index.clone());
      const placed = new T.Matrix4().fromArray(hull.instanceMatrix.array, 0).premultiply(hull.matrixWorld);
      geometry.applyMatrix4(placed);
    }
    geometry.computeBoundingBox();
    const centre = geometry.boundingBox.getCenter(new T.Vector3());
    const sun = new T.Vector3(...JSON.parse(document.querySelector("#viewport").dataset.lighting).lights[0].direction);
    // The painted parts go by name (painted/surface, painted/fuzz, painted/cards) for the views and timing below.
    const painted = new T.Mesh(window.__smoothGeometry(geometry), window.__leafNormalMaterial(sun, tile, strength, 0.35, true, centre, brush, offset));
    painted.name = "painted/surface";
    s.models.add(painted);
    const started = performance.now();
    const fuzz = window.__contourFuzz(geometry, window.__leafCards().cards, { size, density, reach, sun, normals: window.__leafCards().normals, coarse,
      leafMap: { texture: window.__leafNormalTexture(brush), tile, origin: centre, strength, offset } });
    const built = performance.now() - started;
    fuzz.name = "painted/fuzz";
    s.models.add(fuzz);
    if (densityBase?.cards) {
      // The looser foliage below the core: cards through the field (density-cards.ts), shaded as the core is.
      const started = performance.now();
      const cards = window.__densityCards(f, window.__leafCards().cards, { level: densityBase.level, size: densityBase.cardSize,
        keep: densityBase.keep, spacing: densityBase.spacing, floor: densityBase.cardFloor, sun,
        leafMap: { texture: window.__leafNormalTexture(brush), tile, origin: centre, strength, offset } });
      cards.name = "painted/cards";
      s.models.add(cards);
      surface.cards = cards.userData.densityCards.cards;
      surface.cardsMs = performance.now() - started;
    }
    if (densityBase?.strips) {
      // Every leafy twig as one strip along it (twig-strips.ts).
      const strips = window.__twigStrips(f, densityBase.twigs, { level: densityBase.level, width: densityBase.stripWidth, lobe: densityBase.stripLobe,
        droop: densityBase.stripDroop, sun, leafMap: { texture: window.__leafNormalTexture(brush), tile, origin: centre, strength, offset } });
      strips.name = "painted/strips";
      s.models.add(strips);
      surface.strips = strips.userData.twigStrips;
    }
    // HIGHLIGHT: how strongly sun-facing leaves take the highlight colour (leaf-normals.ts msLeafPaint), 0 none.
    for (const m of s.models.children) if (m.name.startsWith("painted/") && m.material.uniforms?.highlight) m.material.uniforms.highlight.value = highlight;
    // HIDE: painted parts left out of the pictures, by name (surface, fuzz, cards, strips), comma separated.
    for (const m of s.models.children) if (hide.includes(m.name.replace("painted/", "")) && m.name.startsWith("painted/")) m.visible = false;
    let field;
    if (densityView) {
      // VIEW=density: the leaf density field (leaf-density.ts) as dots in place of the painted surface and fuzz.
      const f = window.__densityField(densityView.sites, densityView.radius);
      for (const m of s.models.children) if (m.name.startsWith("painted/")) m.visible = false;
      s.models.add(window.__densityPoints(f, { floor: densityView.floor, bands: densityView.bands, size: densityView.size }));
      const counts = [0, 0, 0, 0];
      for (const v of f.values) if (v / f.full >= 0.05) counts[Math.min(3, Math.floor(v / f.full * 4))]++;
      field = { dims: f.dims, max: f.max, full: f.full, quarters: counts };
    }
    s.draw();
    return { centre: centre.toArray(), anchors: fuzz.userData.contourFuzz.anchors, built, field, surface };
  }, { tile: Number(process.env.TILE ?? 0.6), brush: Number(process.env.BRUSH ?? 16), size: Number(process.env.SIZE ?? 0.3), density: Number(process.env.DENSITY ?? 14), reach: Number(process.env.REACH ?? 0.08),
    // COARSE: meters of the cells the fuzz's mesh is coarsened by (contour-fuzz.ts); 0 keeps the surface's edges.
    coarse: Number(process.env.COARSE ?? 0),
    hide: (process.env.HIDE ?? "").split(",").filter(Boolean),
    highlight: Number(process.env.HIGHLIGHT ?? 1),
    // VIEW=density: the plant's leaf density field instead of the painted surface (FIELD_RADIUS: kernel meters,
    // FIELD_FLOOR: share of the densest below which cells are left out, FIELD_BANDS: colour steps, 0 smooth).
    densityView: process.env.VIEW === "density" ? { sites: shrub.sites, radius: Number(process.env.FIELD_RADIUS ?? 0.5),
      floor: Number(process.env.FIELD_FLOOR ?? 0.05), bands: Number(process.env.FIELD_BANDS ?? 0), size: Number(process.env.FIELD_DOT ?? 6) } : null,
    // BASE=density: the painted surface is the density field's surface at FIELD_LEVEL of full (FIELD_CELL: grid meters).
    densityBase: process.env.BASE === "density" ? { sites: shrub.sites, radius: Number(process.env.FIELD_RADIUS ?? 0.5),
      cell: Number(process.env.FIELD_CELL ?? 0.2), level: Number(process.env.FIELD_LEVEL ?? 0.6),
      // The looser foliage below the core as cards (CARDS=0 leaves it out): CARD_SIZE meters at the core, CARD_KEEP of
      // the candidates kept there, CARD_SPACING meters between candidates.
      cards: process.env.CARDS !== "0", cardSize: Number(process.env.CARD_SIZE ?? 0.55), keep: Number(process.env.CARD_KEEP ?? 0.55),
      spacing: Number(process.env.CARD_SPACING ?? 0.25), cardFloor: Number(process.env.CARD_FLOOR ?? 0.06),
      // STRIPS=1: every leafy twig as a strip along it (STRIP_WIDTH: half-width meters at its widest, STRIP_LOBE: meters
      // between a side's leaves, STRIP_DROOP: the tip's drop as a share of the strip's length).
      strips: process.env.STRIPS === "1", twigs: shrub.twigs, stripWidth: Number(process.env.STRIP_WIDTH ?? 0.16),
      stripLobe: Number(process.env.STRIP_LOBE ?? 0.11), stripDroop: Number(process.env.STRIP_DROOP ?? 0.3) } : null,
    // The lit side spread over the upper bands rather than clipped into the top one (STRENGTH tips the leaves' normals,
    // OFFSET shifts every band's threshold).
    strength: Number(process.env.STRENGTH ?? 1.5), offset: Number(process.env.OFFSET ?? -0.05) });
  if (info.error) throw new Error(info.error);
  console.log(`clumps centre ${info.centre.map(v => v.toFixed(2))}, ${info.anchors} fuzz anchors, built in ${info.built.toFixed(1)} ms`);
  if (info.surface) console.log(`density surface: ${info.surface.triangles} triangles, field full ${info.surface.full.toFixed(1)}, built in ${info.surface.ms.toFixed(1)} ms`
    + (info.surface.cards !== undefined ? `; ${info.surface.cards} density cards in ${info.surface.cardsMs.toFixed(1)} ms` : "")
    + (info.surface.strips ? `; ${info.surface.strips.strips} twig strips, ${info.surface.strips.triangles} triangles` : ""));
  if (info.field) console.log(`density field ${info.field.dims.join("x")}, densest ${info.field.max.toFixed(1)}, full ${info.field.full.toFixed(1)}, cells by quarter of full ${info.field.quarters.join(" / ")}`);
  const [cx, cy, cz] = info.centre, r = Number(process.env.DIST ?? 3.6);
  if (process.env.TIME) {
    // Frame cost from the first view, per part: every draw synced by reading one pixel back, median of 40.
    const view = (() => { const a = -0.6; return { position: [cx + Math.cos(a) * r, cy + Math.sin(a) * r, cz + r * 0.2], target: [cx, cy, cz - 0.15] }; })();
    const timing = await page.evaluate(view => {
      const s = window.__scene, canvas = document.querySelector("#viewport canvas");
      const gl = canvas.getContext("webgl2"), pixel = new Uint8Array(4);
      s.camera.position.fromArray(view.position); s.controls.target.fromArray(view.target); s.controls.update();
      // COPIES: a square grid of this many copies of the painted parts (the original among them), 6 m apart, as a stand
      // of trees would draw.
      const parts = Object.fromEntries(["surface", "fuzz", "cards", "strips"].map(k => [k, s.models.children.filter(m => m.name === `painted/${k}`)]));
      const side = Math.round(Math.sqrt(view.copies));
      for (let k = 1; k < side * side; k++) {
        const x = (k % side) * 6, y = Math.floor(k / side) * 6;
        for (const list of Object.values(parts)) if (list.length) {
          const copy = list[0].clone(); copy.position.set(x, y, 0); copy.frustumCulled = false; s.models.add(copy); list.push(copy);
        }
      }
      const surface = parts.surface[0], surfaces = parts.surface, fuzzes = parts.fuzz, cardses = parts.cards;
      if (side > 1) { s.camera.position.fromArray([view.position[0] - 10, view.position[1] - 10, view.position[2] + 10]); s.controls.target.fromArray([view.target[0] + side * 3, view.target[1] + side * 3, view.target[2]]); s.controls.update(); }
      const show = (list, on) => list.forEach(m => { m.visible = on; });
      const time = () => {
        const runs = [];
        for (let i = 0; i < 45; i++) {
          const start = performance.now();
          s.draw(); gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
          if (i >= 5) runs.push(performance.now() - start);
        }
        return runs.sort((a, b) => a - b)[runs.length >> 1];
      };
      const result = { canvas: [canvas.width, canvas.height], copies: side * side, triangles: surface.geometry.index ? surface.geometry.index.count / 3 : surface.geometry.getAttribute("position").count / 3 };
      result.full = time();
      result.info = { calls: s.renderer.info.render.calls, triangles: s.renderer.info.render.triangles };
      if (cardses.length) { show(cardses, false); result.noCards = time(); show(cardses, true); }
      show(fuzzes, false); result.noFuzz = time();
      show(surfaces, false); show(cardses, false); result.barkOnly = time();
      show(fuzzes, true); result.fuzzOnly = time();
      if (cardses.length) { show(fuzzes, false); show(cardses, true); result.cardsOnly = time(); }
      for (const list of Object.values(parts)) { for (const m of list.slice(1)) s.models.remove(m); show(list.slice(0, 1), true); }
      return result;
    }, { ...view, copies: Number(process.env.COPIES ?? 1) });
    console.log(JSON.stringify(timing));
  }
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
