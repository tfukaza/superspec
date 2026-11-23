import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { PacketLods } from "../../browser/renderer/packet-lods.ts";
import { validateGeometryLods, type GeometryLodGroup } from "../../browser/model/geometry-lods.ts";
import { selectAuthoredLod } from "#render-engine";

const definition: GeometryLodGroup = { id: "pipe", levels: [
  { distance: 0, meshes: ["full"] }, { distance: 20, meshes: ["coarse"] }, { distance: 50, meshes: ["far"] },
] };
function fixture() {
  const scene = new THREE.Scene(), root = new THREE.Group(); scene.add(root);
  const material = new THREE.MeshStandardMaterial();
  const meshes = [24, 8, 3].map((segments, i) => {
    const mesh = new THREE.Mesh(new THREE.CylinderGeometry(.5, .5, 2, segments), material);
    mesh.name = ["full", "coarse", "far"][i]; mesh.castShadow = true;
    mesh.userData.source = { id: mesh.name, visible: true }; root.add(mesh); return mesh;
  });
  const camera = new THREE.PerspectiveCamera(70, 1, .1, 1000);
  const lods = new PacketLods(scene, root, [definition]);
  const frame = (distance: number, height = 1080, editing = false) => {
    camera.position.set(0, 0, distance); camera.lookAt(0, 0, 0); camera.updateMatrixWorld();
    lods.frame(camera, height, editing);
    return meshes.map(mesh => mesh.visible);
  };
  return { scene, root, meshes, camera, lods, frame };
}

test("packet world views select one authored level with hysteresis and viewport scaling", () => {
  const f = fixture();
  assert.deepEqual(f.frame(5), [true, false, false]);
  assert.deepEqual(f.frame(26), [false, true, false]);
  assert.deepEqual(f.frame(20), [false, true, false]);
  assert.deepEqual(f.frame(15), [true, false, false]);
  assert.deepEqual(f.frame(65), [false, false, true]);
  assert.deepEqual(f.frame(26, 2160), [true, false, false]);
  assert.deepEqual(f.frame(100, 1080, true), [true, false, false], "item editing pins canonical geometry");
  assert.ok(f.meshes.every(mesh => mesh.userData.source.visible), "source render visibility is not rewritten");
  f.lods.dispose();
});

test("every shadow capture restores full geometry then the selected camera level; picks remain canonical", () => {
  const f = fixture(); f.frame(65);
  assert.equal(f.scene.children[1], f.root, "shadow markers bracket the existing scene group");
  const [before, after] = [f.scene.children[0], f.scene.children.at(-1)] as THREE.Mesh[];
  (before.onBeforeShadow as Function)();
  assert.deepEqual(f.meshes.map(mesh => mesh.visible), [true, false, false]);
  (after.onBeforeShadow as Function)();
  assert.deepEqual(f.meshes.map(mesh => mesh.visible), [false, false, true]);
  const ray = new THREE.Raycaster(new THREE.Vector3(0, 0, 5), new THREE.Vector3(0, 0, -1));
  f.root.updateMatrixWorld();
  const picked = ray.intersectObjects(f.root.children).filter(hit => hit.object.visible || f.lods.isCanonical(hit.object));
  assert.ok(picked.length);
  assert.ok(picked.every(hit => hit.object === f.meshes[0]));
  let disposed = 0; f.meshes[0].geometry.addEventListener("dispose", () => disposed++);
  f.lods.dispose();
  assert.deepEqual(f.scene.children, [f.root]);
  assert.equal(disposed, 0);
  assert.deepEqual(f.meshes.map(mesh => mesh.visible), [true, false, false]);
});

test("packet admission rejects missing, overlapping and invalid authored LOD references", () => {
  const source = { geometryLods: [definition], meshes: ["full", "coarse", "far"].map(id => ({ id, visible: true })) };
  assert.doesNotThrow(() => validateGeometryLods(source));
  for (const change of [
    (s: typeof source) => { s.geometryLods[0].levels[1].meshes = ["missing"]; },
    (s: typeof source) => { s.geometryLods[0].levels[1].meshes = ["full"]; },
    (s: typeof source) => { s.geometryLods[0].levels[2].distance = 19; },
    (s: typeof source) => { s.meshes[2].visible = false; },
    (s: typeof source) => { s.geometryLods[0].levels[1].screenCoverage = .2; },
  ]) {
    const altered = structuredClone(source); change(altered);
    assert.throws(() => validateGeometryLods(altered), /LOD/);
  }
});

test("the established per-tile foliage selector retains ownership of foliage-only groups", () => {
  const scene = new THREE.Scene(), root = new THREE.Group(); scene.add(root);
  const meshes = [0, 1].map(i => {
    const mesh = new THREE.InstancedMesh(new THREE.PlaneGeometry(), new THREE.MeshBasicMaterial(), 1);
    mesh.name = String(i); mesh.userData.foliage = { patch: "grass", level: i }; root.add(mesh); return mesh;
  });
  const lods = new PacketLods(scene, root, [{ id: "grass", levels: [0, 1].map(i => ({ distance: i * 20, batches: [String(i)] })) }]);
  meshes[0].visible = false; meshes[1].visible = true;
  lods.frame(new THREE.PerspectiveCamera(), 1080);
  assert.deepEqual(meshes.map(mesh => mesh.visible), [false, true]);
  assert.deepEqual(scene.children, [root]); lods.dispose();
});

test("retained packet objects can rebind thresholds and toggle editing without stale visibility or picking", () => {
  const f = fixture();
  const originalSources = f.meshes.map(mesh => mesh.userData.source);
  const originalGeometry = f.meshes.map(mesh => mesh.geometry);
  f.frame(65);
  f.lods.dispose();
  const edited = structuredClone(definition); edited.levels[1].distance = 80; edited.levels[2].distance = 150;
  const next = new PacketLods(f.scene, f.root, [edited]);
  next.frame(f.camera,1080);
  assert.deepEqual(f.meshes.map(mesh => mesh.visible),[true,false,false]);
  f.camera.position.z = 200; f.camera.updateMatrixWorld();
  next.frame(f.camera,1080);
  assert.deepEqual(f.meshes.map(mesh => mesh.visible),[false,false,true]);
  next.frame(f.camera,1080,true);
  assert.deepEqual(f.meshes.map(mesh => mesh.visible),[true,false,false]);
  next.frame(f.camera,1080,false);
  assert.deepEqual(f.meshes.map(mesh => mesh.visible),[false,false,true]);
  assert.deepEqual(f.meshes.map(mesh => mesh.geometry),originalGeometry);
  assert.deepEqual(f.meshes.map(mesh => mesh.userData.source),originalSources);
  assert.equal(f.scene.children.length,3,"one marker pair after every rebind");
  next.dispose();
});

test("explicit packet screen coverage takes precedence over distance thresholds", () => {
  const f = fixture();f.lods.dispose();
  const coverage = structuredClone(definition);
  coverage.levels.forEach((level,i) => { level.distance = i*10000; level.screenCoverage = [.2,.05,0][i]; });
  const next = new PacketLods(f.scene,f.root,[coverage]);
  f.camera.position.z = 100;f.camera.updateMatrixWorld();next.frame(f.camera,1080);
  assert.deepEqual(f.meshes.map(mesh=>mesh.visible),[false,false,true]);
  next.dispose();
});

test("shared authored coverage selection keeps the near-plane silhouette detailed", () => {
  const camera = new THREE.PerspectiveCamera(70, 1, .1, 100);
  camera.position.set(0, 0, 20); camera.updateMatrixWorld();
  const box = new THREE.Box3(new THREE.Vector3(-1,-1,-1), new THREE.Vector3(1,1,1));
  const hint = { kind: "coverage" as const, values: [.2,.05,0] };
  assert.equal(selectAuthoredLod(box,camera,1080,hint),1);
  camera.position.z = 1; camera.updateMatrixWorld();
  assert.equal(selectAuthoredLod(box,camera,1080,hint,2),0);
});
