// Run against a native Studio viewer with STUDIO_URL=http://127.0.0.1:PORT and the path to
// an existing single, rectangular city-building AML. The displayed GPU resources must survive rollback.
import assert from "node:assert/strict";
import { resolve } from "node:path";
import { launchBrowser } from "../tools/browser.mjs";

const file = process.argv[2];
if (!file || !process.env.STUDIO_URL) throw Error("Supply STUDIO_URL and a single-building AML path.");
const browser = await launchBrowser();
try {
  const page = await browser.newPage();
  // Hold the admitted shader form fixed: this test compares exact resource identities before and after rollback.
  await page.goto(new URL("/index.html?gpuprobe&noise=inline&relief=quad", process.env.STUDIO_URL).href);
  async function ready() {
    await page.waitForFunction(() => ["ready","error"].includes(document.querySelector("#status").dataset.state),
      undefined, { timeout: 180000 });
    assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", await page.locator("#diagnostics").textContent());
  }
  await ready();
  const before = await page.locator("#viewport").getAttribute("data-revision");
  await page.locator("#open-aml").setInputFiles(resolve(file));
  await page.waitForFunction(previous => document.querySelector("#viewport").dataset.revision !== previous, before, { timeout: 180000 });
  await ready();
  await page.waitForFunction(() => JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked",
    undefined, { timeout: 180000 });

  const result = await page.evaluate(async () => {
    const scene = window.__scene, text = document.querySelector("#aml"), view = document.querySelector("#viewport");
    const status = document.querySelector("#status"), base = text.value, revision = Number(view.dataset.revision);
    const edit = draft => { text.value = draft; text.dispatchEvent(new Event("input", { bubbles: true })); };
    const frame = () => new Promise(done => requestAnimationFrame(done));
    const until = async test => {
      const start = performance.now();
      while (!test()) { if (performance.now() - start > 30000) throw Error("Timed out: " + status.textContent); await frame(); }
    };
    const state = () => scene.models.children.map(mesh => [mesh.uuid, mesh.geometry.uuid, mesh.material.uuid,
      mesh.matrixAutoUpdate, mesh.layers.mask, ...mesh.position.toArray(), ...mesh.matrix.elements]);
    const accepted = JSON.stringify(state()), building = base.match(/<building\s+[^>]*\/>/)?.[0];
    if (!building || /\b(model|count|footprint)=/.test(building)) throw Error("Use a single rectangular procedural lot.");
    const moved = building.replace(/position="([^"]+)"/, (_, origin) => {
      const p = origin.split(/\s+/).map(Number); p[0] += 1.25;
      return 'position="' + p.join(" ") + '"';
    });
    const trap = building.replace(/\s+(width|depth)="[^"]*"/g, "").replace("/>", ' footprint="0 0; 10 0; 8.5 7; 0 7"/>');
    const arrayBuffer = Response.prototype.arrayBuffer;
    let release, waiting = false, expectedIndices;
    let installedShadow = false, restoredShadow = false;
    const originalDraw = scene.draw;
    scene.draw = function(...args) {
      if (this.renderer.shadowMap.needsUpdate) {
        if (this.regeneratedFootprint) installedShadow = true;
        else if (installedShadow) restoredShadow = true;
      }
      return originalDraw.apply(this, args);
    };
    try {
      Response.prototype.arrayBuffer = async function () {
        const bytes = await arrayBuffer.call(this);
        if (!waiting && this.url.endsWith("/aml-update")) {
          waiting = true;
          await new Promise(done => { release = done; });
        } else if (waiting && this.url.endsWith("/aml-update")) {
          const length = new DataView(bytes).getUint32(4, true);
          const header = JSON.parse(new TextDecoder().decode(new Uint8Array(bytes, 8, length)));
          const owner = "city/" + building.match(/\bid="([^"]+)"/)[1];
          expectedIndices = header.manifest.meshes.filter(item => item.visible && item.objectId === owner)
            .reduce((sum, item) => sum + item.indexCount, 0);
        }
        return bytes;
      };
      edit(base.replace(building, moved));
      await until(() => waiting && Number(view.dataset.placementFrame) === revision + 1);
      edit(base.replace(building, trap));
      await until(() => Number(view.dataset.placementFrame) === revision + 2);
      const pieces = scene.placementTransforms.pieces.children, firstHalf = pieces[0];
      const resident = new Set(scene.models.children.map(mesh => mesh.material));
      const texturedHalves = pieces.filter(mesh => mesh.visible && mesh.geometry.hasAttribute("uv") && resident.has(mesh.material)).length;
      release();
      await frame();
      const newerShown = Number(view.dataset.placementFrame) === revision + 2 &&
        view.dataset.revision === String(revision) && view.dataset.footprintDragLook === "resident-materials" &&
        scene.placementTransforms.pieces.children[0] === firstHalf;
      await until(() => Number(view.dataset.footprintGeometryFrame) === revision + 2);
      const stage = scene.regeneratedFootprint;
      const retained = [...stage.userData.retained];
      const retainedIndices = retained.reduce((sum, mesh) => sum + mesh.userData.source.indexCount, 0);
      const original = JSON.parse(accepted);
      const chartsIntact = retained.every(mesh => {
        const saved = original[scene.models.children.indexOf(mesh)];
        return saved[1] === mesh.geometry.uuid && saved[2] === mesh.material.uuid && saved[4] === mesh.layers.mask
          && mesh.geometry !== mesh.userData.sourceGeometry && mesh.matrix.equals(new window.__three.Matrix4().fromArray(saved.slice(8)));
      });
      const regeneratedIndices = stage.geometry.index.count + retainedIndices, exactShape = expectedIndices === regeneratedIndices &&
        stage.parent === scene.scene && !scene.placementTransforms.pieces.visible && view.dataset.revision === String(revision);
      const coverage = JSON.parse(view.dataset.footprintMaterialCoverage);
      const casters = !scene.shadows || (stage.castShadow && stage.children.every(part => part.castShadow === !part.userData.decal));
      edit(base.replace(building, moved));
      await until(() => Number(view.dataset.placementFrame) === revision + 3);
      const stageReplaced = !view.dataset.footprintGeometryFrame && !stage.parent && !scene.regeneratedFootprint;
      edit("<");
      await until(() => status.dataset.state === "error");
      const restored = JSON.stringify(state()) === accepted && !view.dataset.placementFrame &&
        !scene.placementTransforms.hulls.children.length && !scene.placementTransforms.pieces.children.length && view.dataset.revision === String(revision);
      const after = state(), before = JSON.parse(accepted);
      const changedDraw = after.findIndex((value, index) => JSON.stringify(value) !== JSON.stringify(before[index]));
      const rollback = restored ? undefined : { changedDraw, before: before[changedDraw], after: after[changedDraw],
        hulls: scene.placementTransforms.hulls.children.length, pieces: scene.placementTransforms.pieces.children.length,
        placement: view.dataset.placementFrame, accepted: revision, revision: view.dataset.revision };
      edit(base);
      await until(() => status.dataset.state === "ready" && Number(view.dataset.revision) > revision);
      return { meshes: scene.models.children.length, newerShown, texturedHalves, coverage, exactShape, regeneratedIndices,
        retained: retained.length, retainedIndices, chartsIntact, drawGroups: stage.material.length, stageReplaced,
        refreshedShadows: !scene.shadows || (casters && installedShadow && restoredShadow), restored, rollback, recovered: status.dataset.state };
    } finally { release?.(); scene.draw = originalDraw; Response.prototype.arrayBuffer = arrayBuffer; }
  });
  assert.ok(result.newerShown, "stale native response replaced the newer provisional lot");
  assert.ok(result.texturedHalves > 0);
  assert.equal(result.coverage.triangles * 3, result.regeneratedIndices);
  assert.equal(result.coverage.retainedAtlas * 3, result.retainedIndices);
  assert.ok(result.retained > 0, "the taper should preserve at least one stationary atlas");
  assert.ok(result.chartsIntact);
  assert.ok(result.refreshedShadows, "the regenerated footprint and rollback must refresh the sun shadow");
  assert.equal(result.coverage.shaded + result.coverage.flat + result.coverage.fallback, result.coverage.triangles);
  assert.ok(result.coverage.shaded > .8 * result.coverage.triangles, JSON.stringify(result.coverage));
  assert.ok(result.exactShape, "staged draw does not contain the entire regenerated building");
  assert.ok(result.stageReplaced, "obsolete regenerated geometry survived a new placement");
  assert.ok(result.restored, "invalid source did not restore the entire displayed scene: " + JSON.stringify(result.rollback));
  assert.equal(result.recovered, "ready");
  console.log(JSON.stringify(result));
} finally { await browser.close(); }
