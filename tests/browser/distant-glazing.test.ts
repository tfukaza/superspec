import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import * as THREE from "three";
const hooks = registerHooks({ resolve(specifier, context, nextResolve) {
  return nextResolve(specifier.startsWith(".") && !/\.[a-z]+$/i.test(specifier) ? specifier + ".ts" : specifier, context);
} });
const { DistantGlazingDraws } = await import("../../adapters/three/src/distantGlazing.ts");
const { enableSinglePassGlazing } = await import("../../adapters/three/src/glazing.ts");
hooks.deregister();


function setup() {
  const root = new THREE.Group(), material = new THREE.MeshStandardMaterial({ transparent: true, side: THREE.DoubleSide });
  material.userData.worldSpecSurface = { recipe: "architectural-glazing" };
  material.emissive.setRGB(0.3, 0.2, 0.1); material.emissiveMap = new THREE.Texture();
  material.normalMap = new THREE.Texture(); material.map = new THREE.Texture();
  const geometry = new THREE.PlaneGeometry(1, 1);
  geometry.setAttribute("_ms_aspect", new THREE.Float32BufferAttribute([1, 2, 3, 4], 1));
  geometry.setAttribute("color", new THREE.Uint8BufferAttribute([20, 30, 40, 60, 70, 80, 90, 100, 110, 120, 130, 140], 3, true));
  const a = new THREE.Mesh(geometry, material), b = new THREE.Mesh(geometry, material);
  a.position.set(-2, 0, -60); b.position.set(2, 0, -60); a.castShadow = b.castShadow = true;
  root.add(a, b);
  const camera = new THREE.PerspectiveCamera(90, 1, 0.1, 500);
  return { root, material, geometry, a, b, camera };
}

test("distant panes share an instance draw while retaining authored geometry, materials, glow and picking", () => {
  const { root, material, geometry, a, b, camera } = setup();
  const draws = new DistantGlazingDraws(root);
  draws.frame(camera, 1080, 1080);
  const batch = root.children.find(x => x instanceof THREE.InstancedMesh) as THREE.InstancedMesh;
  assert.equal(batch.material, material); assert.equal(batch.castShadow, false);
  assert.ok(batch.visible && !a.visible && !b.visible);
  assert.equal(draws.stats.savedDraws, 1); assert.equal(draws.stats.farPlacements, 2);
  for (const name of ["position", "normal", "uv", "color", "_ms_aspect"]) {
    assert.deepEqual(batch.geometry.getAttribute(name).array, geometry.getAttribute(name).array);
    assert.equal(batch.geometry.getAttribute(name).normalized, geometry.getAttribute(name).normalized);
  }
  assert.deepEqual(batch.getMatrixAt(0, new THREE.Matrix4()).elements, a.matrixWorld.elements);
  const ray = new THREE.Raycaster(new THREE.Vector3(-2, 0, 0), new THREE.Vector3(0, 0, -1));
  assert.equal(ray.intersectObject(root)[0].object, a, "picking still identifies the original hidden source");
  draws.restore();
  assert.ok(a.visible && b.visible && a.castShadow && b.castShadow && !batch.visible);
  draws.frame(camera, 1080, 1080);
  assert.equal(draws.stats.savedDraws, 1, "shadow restoration can reapply an unchanged view");
  let disposed = false; batch.addEventListener("dispose", () => { disposed = true; });
  draws.dispose(); assert.ok(disposed); assert.deepEqual(root.children, [a, b]);
});

test("near, zoomed and large panes remain exact without increasing submissions", () => {
  const { root, a, b, camera } = setup();
  const draws = new DistantGlazingDraws(root, { maxPanePixels: 20 });
  draws.frame(camera, 1080, 1080); assert.equal(draws.stats.savedDraws, 1);
  camera.zoom = 4; camera.updateProjectionMatrix();
  draws.frame(camera, 1080, 1080); assert.equal(draws.stats.savedDraws, 0); assert.ok(a.visible && b.visible);
  camera.zoom = 1; camera.updateProjectionMatrix();
  draws.frame(camera, 4320, 4320); assert.equal(draws.stats.savedDraws, 0);
  camera.position.z = -45; draws.frame(camera, 1080, 1080);
  assert.ok(a.visible && b.visible); assert.equal(draws.stats.savedDraws, 0);
  draws.dispose();
  const large = setup(); large.a.scale.set(20, 20, 20); large.b.scale.set(20, 20, 20);
  const largeDraws = new DistantGlazingDraws(large.root); largeDraws.frame(large.camera, 1080, 1080);
  assert.ok(large.a.visible && large.b.visible); assert.equal(largeDraws.stats.savedDraws, 0); largeDraws.dispose();
});

test("mixed near and far instances keep independent exact near buffers and full shadow sources", () => {
  const { root, geometry, material, camera } = setup();
  const mixed = new THREE.InstancedMesh(geometry, material, 2);
  mixed.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -8));
  mixed.setMatrixAt(1, new THREE.Matrix4().makeTranslation(0, 0, -70));
  mixed.castShadow = true; root.add(mixed); const original = mixed.instanceMatrix.array.slice();
  const draws = new DistantGlazingDraws(root); draws.frame(camera, 1080, 1080);
  const near = root.children.find(x => x !== mixed && x instanceof THREE.InstancedMesh && !x.name.startsWith("Distant glazing")) as THREE.InstancedMesh;
  assert.ok(near.visible && !mixed.visible); assert.equal(near.count, 1); assert.equal(near.instanceMatrix.array[14], -8);
  assert.equal(near.castShadow, false); assert.equal(near.material, material);
  assert.deepEqual(mixed.instanceMatrix.array, original); assert.equal(mixed.count, 2);
  assert.equal(draws.stats.savedDraws, 1); assert.equal(draws.stats.farPlacements, 3);
  draws.restore(); assert.ok(mixed.visible && mixed.castShadow && !near.visible);
  draws.frame(camera, 1080, 1080); assert.deepEqual(mixed.instanceMatrix.array, original);
  draws.dispose(); assert.equal(near.parent, null);
});

test("quantized interleaved pane attributes are shared byte-for-byte and edits invalidate the view proxy", () => {
  const { root, geometry, a, b, camera } = setup();
  const packed = new Int16Array([
    -32768, 32767, 0, 981, 0, 0, 32767, 123,
    32767, 32767, 0, 981, 0, 0, 32767, 123,
    -32768, -32768, 0, 981, 0, 0, 32767, 123,
    32767, -32768, 0, 981, 0, 0, 32767, 123,
  ]);
  const data = new THREE.InterleavedBuffer(packed, 8);
  geometry.setAttribute("position", new THREE.InterleavedBufferAttribute(data, 3, 0, true));
  geometry.setAttribute("normal", new THREE.InterleavedBufferAttribute(data, 3, 4, true));
  const snapshot = packed.slice();
  const draws = new DistantGlazingDraws(root);
  draws.frame(camera, 1080, 1080);
  const proxy = root.children.find(x => x instanceof THREE.InstancedMesh) as THREE.InstancedMesh;
  assert.ok(proxy.visible && !a.visible && !b.visible);
  assert.equal(proxy.geometry, geometry);
  assert.equal(proxy.geometry.getAttribute("position"), geometry.getAttribute("position"));
  assert.deepEqual(packed, snapshot, "signed normalized endpoint bits and interleaved padding survive unchanged");
  data.needsUpdate = true;
  draws.frame(camera, 1080, 1080);
  assert.equal(draws.stats.savedDraws, 0); assert.ok(a.visible && b.visible && !proxy.visible);
  draws.dispose();
});

test("far instances sort back to front and changed root transforms restore sources", () => {
  const { root, a, b, camera } = setup(); b.position.z = -80;
  const draws = new DistantGlazingDraws(root); draws.frame(camera, 1080, 1080);
  const proxy = root.children.find(x => x instanceof THREE.InstancedMesh) as THREE.InstancedMesh;
  const matrix = new THREE.Matrix4();
  proxy.getMatrixAt(0, matrix); assert.equal(matrix.elements[14], -80);
  proxy.getMatrixAt(1, matrix); assert.equal(matrix.elements[14], -60);
  root.position.x = 10; draws.frame(camera, 1080, 1080);
  assert.ok(a.visible && b.visible && !proxy.visible); assert.equal(draws.stats.savedDraws, 0);
  draws.dispose();
});

test("bounded allocation, hidden ancestors, edited geometry and custom hooks retain original draws", () => {
  const unsupported = setup();
  const noMulti = new DistantGlazingDraws(unsupported.root, { maxInstances: 1 });
  noMulti.frame(unsupported.camera); assert.deepEqual(unsupported.root.children, [unsupported.a, unsupported.b]); noMulti.dispose();
  const state = setup(), parent = new THREE.Group(); state.root.remove(state.a); parent.add(state.a); state.root.add(parent);
  const draws = new DistantGlazingDraws(state.root);
  parent.visible = false; draws.frame(state.camera); assert.equal(draws.stats.savedDraws, 0);
  parent.visible = true; draws.frame(state.camera); assert.equal(draws.stats.savedDraws, 1);
  state.geometry.setAttribute("position", state.geometry.getAttribute("position").clone());
  draws.frame(state.camera); assert.equal(draws.stats.savedDraws, 0); assert.ok(state.a.visible && state.b.visible); draws.dispose();
  const custom = setup(); custom.material.onBeforeCompile = () => {};
  const untrusted = new DistantGlazingDraws(custom.root); untrusted.frame(custom.camera);
  assert.deepEqual(custom.root.children, [custom.a, custom.b]); untrusted.dispose();
});

test("the existing single-pass classifier does not mistake batched local geometry for final placements", () => {
  const { material, geometry, camera } = setup(); enableSinglePassGlazing(material, { distantPixels: 64 });
  const batch = new THREE.BatchedMesh(2, 4, 6, material), id = batch.addGeometry(geometry); batch.addInstance(id); batch.addInstance(id);
  material.forceSinglePass = true;
  material.onBeforeRender({} as THREE.WebGLRenderer, new THREE.Scene(), camera, batch.geometry, batch, null);
  assert.equal(material.forceSinglePass, false); batch.dispose();
});

test("glass beyond the range is left out per placement, while lit glass, opaque frames and hidden meshes are untouched", async () => {
  const { GlazingRange } = await import("../../adapters/three/src/glazingRange.ts");
  const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(90, 1, 0.1, 500);
  const glass = () => {
    const material = new THREE.MeshStandardMaterial({ transparent: true, side: THREE.DoubleSide });
    material.userData.worldSpecSurface = { recipe: "architectural-glazing" };
    return material;
  };
  const pane = new THREE.PlaneGeometry(1, 1);
  const rows = new THREE.InstancedMesh(pane, glass(), 3);
  for (const [i, z] of [-5, -20, -60].entries()) rows.setMatrixAt(i, new THREE.Matrix4().makeTranslation(0, 0, z));
  rows.castShadow = true;
  const at = <T extends THREE.Mesh>(mesh: T, x: number, z: number) => { mesh.position.set(x, 0, z); root.add(mesh); return mesh; };
  const far = at(new THREE.Mesh(pane, glass()), 3, -40), near = at(new THREE.Mesh(pane, glass()), 3, -4);
  const lit = at(new THREE.Mesh(pane, glass()), -3, -40);
  lit.material.emissive.set(0xffcc88); lit.material.emissiveIntensity = 2;
  const frame = at(new THREE.Mesh(pane, new THREE.MeshStandardMaterial()), 6, -40);
  const culled = at(new THREE.Mesh(pane, glass()), -6, -4);
  root.add(rows);
  const range = new GlazingRange(root, 10);
  const authored = root.children.slice();
  const drawn = () => root.children.filter(child => child.visible);

  culled.visible = false; // View culling runs first.
  range.frame(camera);
  const subset = root.children.find(child => !authored.includes(child)) as THREE.InstancedMesh;
  assert.deepEqual(drawn(), [near, lit, frame, subset]);
  assert.deepEqual([subset.count, subset.instanceMatrix.array[14], subset.material, subset.castShadow], [1, -5, rows.material, false]);
  assert.equal(rows.count, 3, "the authored batch keeps every placement for shadows and picking");

  // Walking forward brings the second row into range and leaves the first inside it.
  camera.position.z = -11;
  camera.updateMatrixWorld();
  subset.computeBoundingSphere();
  range.frame(camera);
  assert.deepEqual([subset.count, subset.instanceMatrix.array[14], subset.instanceMatrix.array[30]], [2, -5, -20]);
  assert.equal(subset.boundingSphere, null, "the renderer measures the new set before culling it");
  assert.equal(far.visible, false);

  range.restore();
  culled.visible = true;
  assert.deepEqual(drawn(), authored, "shadow passes and the next view start from the authored scene");
  range.distance = 100;
  range.frame(camera);
  assert.deepEqual(drawn(), authored, "every pane is inside a long range");
  range.distance = 0;
  range.frame(camera);
  assert.deepEqual(drawn(), [lit, frame], "a zero range keeps only what is not plain glass");
  range.distance = Infinity;
  range.frame(camera);
  assert.deepEqual(drawn(), authored);
  range.dispose();
  assert.deepEqual(root.children, authored);
});
