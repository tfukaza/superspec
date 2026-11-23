import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { CityVegetationLod } from "../../adapters/three/src/cityVegetationLod.ts";

function fixture(instanced = false) {
  const root = new THREE.Group(), near = new THREE.PlaneGeometry(1, 1, 4, 4);
  const material = new THREE.MeshStandardMaterial({ transparent: true, side: THREE.DoubleSide });
  material.userData = { castsShadow: false, worldSpecSurface: { family: "Vegetation", recipe: "Weed underpainting" } };
  const reduced = new THREE.BufferGeometry();
  for (const [name, attribute] of Object.entries(near.attributes)) reduced.setAttribute(name, attribute);
  reduced.setIndex([0, 5, 1]);
  reduced.userData.cityLodErrorMeters = .08;
  const mesh = instanced ? new THREE.InstancedMesh(near, material, 2) : new THREE.Mesh(near, material);
  if (mesh instanceof THREE.InstancedMesh) {
    mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -80));
    mesh.setMatrixAt(1, new THREE.Matrix4().makeTranslation(1, 0, -90));
  } else mesh.position.z = -80;
  mesh.renderOrder = 7;
  root.add(mesh);
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, .1, 500);
  const lods = new Map([[near, [reduced]]]);
  return { root, near, reduced, material, mesh, camera, lods };
}

test("weed camera LOD preserves transparent ordering, source picking and borrowed resources", () => {
  const f = fixture(), { root, near, reduced, material, mesh, camera, lods } = f;
  root.updateMatrixWorld(true);
  const originalRaycast = mesh.raycast, id = mesh.id;
  const raycaster = new THREE.Raycaster(new THREE.Vector3(), new THREE.Vector3(.4, -.4, -80).normalize());
  const sourceHits = raycaster.intersectObject(mesh);
  assert.ok(sourceHits.length);
  const controller = new CityVegetationLod(root, lods);
  controller.frame(camera, 1080);
  assert.equal(mesh.geometry, reduced);
  assert.equal(mesh.id, id);
  assert.equal(mesh.material, material);
  assert.equal(mesh.renderOrder, 7);
  assert.equal(mesh.visible, true);
  assert.equal(root.children.length, 1);
  const activeHits = raycaster.intersectObject(mesh);
  assert.deepEqual(activeHits.map(hit => [hit.distance, hit.faceIndex]), sourceHits.map(hit => [hit.distance, hit.faceIndex]));
  assert.equal(mesh.geometry, reduced, "picking restores the selected render geometry");
  controller.restore();
  assert.equal(mesh.geometry, near);
  controller.frame(camera, 1080);
  let disposed = 0;
  for (const resource of [near, reduced, material]) resource.addEventListener("dispose", () => disposed++);
  controller.dispose();
  assert.equal(mesh.geometry, near);
  assert.equal(mesh.raycast, originalRaycast);
  assert.equal(disposed, 0, "the controller does not own source or loaded LOD resources");
});

test("whole-instance bounds protect nearby weeds and preserve source instance order", () => {
  const { root, near, reduced, mesh, camera, lods } = fixture(true);
  assert.ok(mesh instanceof THREE.InstancedMesh);
  const matrices = mesh.instanceMatrix.array.slice();
  root.updateMatrixWorld(true);
  const raycaster = new THREE.Raycaster(new THREE.Vector3(), new THREE.Vector3(.4, -.4, -80).normalize());
  const originalHits = raycaster.intersectObject(mesh).map(hit => [hit.instanceId, hit.distance, hit.faceIndex]);
  assert.ok(originalHits.length);
  const controller = new CityVegetationLod(root, lods);
  controller.frame(camera, 1080);
  assert.equal(mesh.geometry, reduced);
  assert.deepEqual(mesh.instanceMatrix.array, matrices);
  assert.equal(mesh.count, 2);
  assert.deepEqual(raycaster.intersectObject(mesh).map(hit => [hit.instanceId, hit.distance, hit.faceIndex]), originalHits);
  camera.position.z = -65;
  controller.frame(camera, 1080);
  assert.equal(mesh.geometry, near, "nearest instance remains fully detailed");
  camera.position.z = 0;
  controller.frame(camera, 1080);
  assert.equal(mesh.geometry, reduced);
  mesh.instanceMatrix.needsUpdate = true;
  controller.frame(camera, 1080);
  assert.equal(mesh.geometry, near, "changed source instance data requires reconstruction");
  controller.dispose();
});

test("projected error accounts for drawing resolution and additional host scale", () => {
  const f = fixture();
  const controller = new CityVegetationLod(f.root, f.lods);
  controller.frame(f.camera, 1080);
  assert.equal(f.mesh.geometry, f.reduced);
  controller.frame(f.camera, 2160);
  assert.equal(f.mesh.geometry, f.near);
  controller.dispose();
  f.root.scale.setScalar(2);
  const scaled = new CityVegetationLod(f.root, f.lods);
  scaled.frame(f.camera, 2160);
  assert.equal(f.mesh.geometry, f.near, "world error scales with the host, not just the depth");
  scaled.frame(f.camera, 1080);
  assert.equal(f.mesh.geometry, f.reduced);
  scaled.frame(new THREE.OrthographicCamera(), 1080);
  assert.equal(f.mesh.geometry, f.near);
  scaled.dispose();
});

test("a lower preset brings the full-motif distance closer, to no less than twelve metres", () => {
  // At 540 px a 3 cm error projects to under a pixel from 15 m, so only the distance gate decides.
  const f = fixture();
  f.reduced.userData.cityLodErrorMeters = .03;
  f.mesh.position.z = -19;
  const controller = new CityVegetationLod(f.root, f.lods);
  const at = (z: number, nearScale?: number) => {
    f.camera.position.z = z;
    controller.frame(f.camera, 540, nearScale);
    return f.mesh.geometry === f.reduced;
  };
  // The weed's bounds begin 19 m ahead: inside the default 24 m.
  assert.equal(at(0), false);
  assert.equal(at(0, 1), false);
  assert.equal(at(0, 1.4), false, "a higher preset never shortens the distance");
  // 24 m * 0.75 = 18 m, so 19 m is beyond it.
  assert.equal(at(0, .75), true);
  controller.restore();
  // 24 m * 0.3 would be 7.2 m; the floor holds 12 m. From z = -6 the weed is 13 m ahead, from -8.5 it is 10.5 m.
  assert.equal(at(-6, .3), true);
  assert.equal(at(-8.5, .3), false);
  controller.dispose();
});

test("material replacement and shader invalidation return the authored geometry", () => {
  const f = fixture(), controller = new CityVegetationLod(f.root, f.lods);
  controller.frame(f.camera, 1080);
  assert.equal(f.mesh.geometry, f.reduced);
  f.mesh.material = new THREE.MeshStandardMaterial();
  controller.frame(f.camera, 1080);
  assert.equal(f.mesh.geometry, f.near);
  f.mesh.material = f.material;
  controller.frame(f.camera, 1080);
  assert.equal(f.mesh.geometry, f.reduced);
  f.material.needsUpdate = true;
  controller.frame(f.camera, 1080);
  assert.equal(f.mesh.geometry, f.near);
  controller.dispose();
});

test("continuous turf, casters, custom callbacks and incompatible levels remain untouched", () => {
  const cases = [
    (f: ReturnType<typeof fixture>) => { f.material.userData.grassField = true; },
    (f: ReturnType<typeof fixture>) => { f.material.userData.worldSpecSurface.recipe = "Rolling olive turf"; },
    (f: ReturnType<typeof fixture>) => { f.mesh.castShadow = true; },
    (f: ReturnType<typeof fixture>) => { f.mesh.onBeforeRender = () => {}; },
    (f: ReturnType<typeof fixture>) => { f.mesh.raycast = () => {}; },
    (f: ReturnType<typeof fixture>) => { f.reduced.setIndex([]); },
    (f: ReturnType<typeof fixture>) => { delete f.reduced.userData.cityLodErrorMeters; },
    (f: ReturnType<typeof fixture>) => { f.reduced.setAttribute("normal", f.near.attributes.normal.clone()); },
  ];
  for (const alter of cases) {
    const f = fixture();
    alter(f);
    const raycast = f.mesh.raycast;
    const controller = new CityVegetationLod(f.root, f.lods);
    controller.frame(f.camera, 1080);
    assert.equal(f.mesh.geometry, f.near);
    assert.equal(f.mesh.raycast, raycast);
    controller.dispose();
  }
});
