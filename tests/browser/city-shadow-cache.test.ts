import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { CityShadowCache } from "../../adapters/three/src/cityShadowCache.ts";

test("full shadow casters and view subsets are restored around every shadow pass", () => {
  const scene = new THREE.Scene(), city = new THREE.Group();
  const original = new THREE.Object3D(), subset = new THREE.Object3D();
  city.add(original, subset); scene.add(city);
  const sun = new THREE.DirectionalLight(), spot = new THREE.SpotLight(), camera = new THREE.PerspectiveCamera();
  const sunTarget = new THREE.WebGLRenderTarget(32, 32, { depthTexture: new THREE.DepthTexture(32, 32) });
  const spotTarget = new THREE.WebGLRenderTarget(16, 16, { depthTexture: new THREE.DepthTexture(16, 16) });
  let active = sunTarget, copies = 0, disposed = 0, failCopy = false, preparations = 0, viewRestores = 0;
  const renderer = {
    shadowMap: { type: THREE.PCFShadowMap }, domElement: new EventTarget(),
    getRenderTarget: () => active,
    setRenderTarget: (target: THREE.WebGLRenderTarget) => { active = target; },
    initRenderTarget: (target: THREE.WebGLRenderTarget) => { target.addEventListener("dispose", () => disposed++); },
    copyTextureToTexture: () => { if (failCopy) throw Error("copy failed"); copies++; },
  } as unknown as THREE.WebGLRenderer;
  const restoreView = () => { viewRestores++; original.visible = false; subset.visible = true; };
  const cache = new CityShadowCache(renderer, city, sun,
    () => { original.visible = true; subset.visible = false; }, [spot], restoreView,
    (shadowCamera, viewCamera) => {
      assert.equal(original.visible, true, "camera subsets are restored before light selection");
      assert.equal(subset.visible, false);
      assert.equal(shadowCamera, spot.shadow.camera);
      assert.equal(viewCamera, camera);
      preparations++;
    });
  const before = scene.children[0] as THREE.Mesh, after = scene.children[2] as THREE.Mesh;
  const begin = (light: THREE.DirectionalLight | THREE.SpotLight) => {
    before.onBeforeShadow(renderer, before, camera, light.shadow.camera, before.geometry, before.material as THREE.Material, null);
  };
  const finish = (light: THREE.DirectionalLight | THREE.SpotLight) => {
    after.onBeforeShadow(renderer, after, camera, light.shadow.camera, after.geometry, after.material as THREE.Material, null);
  };
  try {
    restoreView();
    begin(sun);
    assert.equal(original.visible, true);
    assert.equal(subset.visible, false);
    finish(sun);
    assert.equal(preparations, 0, "the directional shadow keeps its full casters");
    assert.equal(original.visible, false);
    assert.equal(subset.visible, true);

    active = spotTarget;
    begin(spot);
    assert.equal(original.visible, true, "the next light also receives complete casters");
    finish(spot);
    assert.equal(preparations, 1);
    assert.equal(original.visible, false);
    assert.equal(subset.visible, true);

    active = sunTarget;
    const restoresAfterCapture = viewRestores;
    begin(sun);
    assert.equal(city.visible, false, "a cache hit skips the static subtree");
    finish(sun);
    assert.equal(city.visible, true);
    assert.equal(original.visible, false);
    assert.equal(subset.visible, true);
    assert.equal(copies, 3);
    assert.equal(viewRestores, restoresAfterCapture, "depth reuse preserves the prepared camera selection");

    active = spotTarget;
    begin(spot);
    assert.equal(city.visible, false);
    finish(spot);
    assert.equal(preparations, 1, "reused depth skips instance selection");
    assert.equal(viewRestores, restoresAfterCapture);
    begin(spot);
    cache.finishPass();
    assert.equal(city.visible, true, "a failed render must restore visibility before the next camera list is built");
    assert.equal(viewRestores, restoresAfterCapture);

    // An interrupted rebuild has no after marker to publish its new projection.
    spot.shadow.matrix.makeTranslation(1, 0, 0);
    begin(spot);
    assert.equal(original.visible, true);
    begin(spot);
    assert.equal(original.visible, true, "a retry cannot reuse depth captured at the old projection");
    finish(spot);
    begin(spot);
    assert.equal(city.visible, false, "the completed retry supplies the next cache hit");
    finish(spot);

    // A failed reuse must be recoverable with a complete caster draw.
    failCopy = true;
    assert.throws(() => begin(spot), /copy failed/);
    failCopy = false;
    begin(spot);
    assert.equal(original.visible, true);
    finish(spot);

    sun.shadow.needsUpdate = spot.shadow.needsUpdate = false;
    cache.invalidate();
    assert.equal(disposed, 2);
    assert.ok(sun.shadow.needsUpdate && spot.shadow.needsUpdate);
    active = sunTarget;
    begin(sun);
    assert.equal(original.visible, true);
    failCopy = true;
    assert.throws(() => finish(sun), /copy failed/);
    assert.equal(original.visible, false, "failed capture also restores the view");
    assert.equal(subset.visible, true);
    failCopy = false;
    begin(sun);
    assert.equal(original.visible, true, "a failed copy is captured again");
    finish(sun);
    assert.equal(original.visible, false);
  } finally {
    cache.dispose(); sunTarget.dispose(); spotTarget.dispose();
  }
});

test("depth is copied only for a light that redraws, and released when the light stops casting", () => {
  const scene = new THREE.Scene(), city = new THREE.Group();
  scene.add(city);
  const sun = new THREE.DirectionalLight(), spot = new THREE.SpotLight(), camera = new THREE.PerspectiveCamera();
  sun.castShadow = spot.castShadow = true;
  // The host draws the sun's map on request. The fixture redraws every frame.
  sun.shadow.autoUpdate = false;
  const sunTarget = new THREE.WebGLRenderTarget(32, 32, { depthTexture: new THREE.DepthTexture(32, 32) });
  const spotTarget = new THREE.WebGLRenderTarget(16, 16, { depthTexture: new THREE.DepthTexture(16, 16) });
  let active = sunTarget, copies = 0, allocated = 0, disposed = 0, casterDraws = 0;
  const renderer = {
    shadowMap: { type: THREE.PCFShadowMap, enabled: true }, domElement: new EventTarget(),
    getRenderTarget: () => active,
    setRenderTarget: (target: THREE.WebGLRenderTarget) => { active = target; },
    initRenderTarget: (target: THREE.WebGLRenderTarget) => { allocated++; target.addEventListener("dispose", () => disposed++); },
    copyTextureToTexture: () => { copies++; },
  } as unknown as THREE.WebGLRenderer;
  const cache = new CityShadowCache(renderer, city, sun, () => { casterDraws++; }, [spot]);
  const before = scene.children[0] as THREE.Mesh, after = scene.children[2] as THREE.Mesh;
  const pass = (light: THREE.DirectionalLight | THREE.SpotLight) => {
    active = light === sun ? sunTarget : spotTarget;
    before.onBeforeShadow(renderer, before, camera, light.shadow.camera, before.geometry, before.material as THREE.Material, null);
    const reused = !city.visible;
    after.onBeforeShadow(renderer, after, camera, light.shadow.camera, after.geometry, after.material as THREE.Material, null);
    assert.equal(city.visible, true);
    return reused;
  };
  try {
    assert.equal(pass(sun), false);
    assert.deepEqual([allocated, copies, casterDraws], [0, 0, 1], "a map drawn once keeps no second copy");
    cache.releaseUnused();
    assert.equal(pass(sun), false, "an uncopied projection draws its casters again");
    assert.deepEqual([allocated, copies, casterDraws], [1, 1, 2], "the repeated request shows the copy will be read");
    assert.equal(pass(sun), true);
    assert.deepEqual([allocated, copies, casterDraws], [1, 2, 2]);

    sun.shadow.matrix.makeTranslation(1, 0, 0);
    assert.equal(pass(sun), false);
    assert.deepEqual([allocated, disposed, copies], [1, 1, 2], "depth saved for the old projection is released, not replaced");

    assert.equal(pass(spot), false);
    assert.deepEqual([allocated, copies], [2, 3], "a light that redraws every frame is copied at once");
    assert.equal(pass(spot), true);
    cache.releaseUnused();
    assert.equal(disposed, 1, "casting lights keep their copies");
    spot.castShadow = false;
    cache.releaseUnused();
    assert.equal(disposed, 2);
    spot.castShadow = true;
    assert.equal(pass(spot), false, "a light that casts again starts from complete casters");
    assert.deepEqual([allocated, disposed], [3, 2]);
    renderer.shadowMap.enabled = false;
    cache.releaseUnused();
    assert.equal(disposed, 3, "disabled shadow maps keep no saved depth");
  } finally {
    cache.dispose(); sunTarget.dispose(); spotTarget.dispose();
  }
  assert.equal(disposed, allocated);
});

test("local shadow selection leaves live actors after the static cache and restores failed or uncached passes", () => {
  const scene = new THREE.Scene(), city = new THREE.Group(), actor = new THREE.Mesh();
  actor.castShadow = true;
  scene.add(actor, city);
  const sun = new THREE.DirectionalLight(), spot = new THREE.SpotLight(), camera = new THREE.PerspectiveCamera();
  let full = false, preparations = 0, failPrepare = false;
  const renderer = {
    shadowMap: { type: THREE.PCFShadowMap }, domElement: new EventTarget(),
    getRenderTarget: () => null,
  } as unknown as THREE.WebGLRenderer;
  const cache = new CityShadowCache(renderer, city, sun, () => { full = true; }, [spot],
    () => { full = false; }, () => {
      assert.equal(full, true);
      preparations++;
      if (failPrepare) throw Error("selection failed");
    });
  const before = scene.children[0] as THREE.Mesh, after = scene.children[2] as THREE.Mesh;
  const begin = () => before.onBeforeShadow(renderer, before, camera, spot.shadow.camera,
    before.geometry, before.material as THREE.Material, null);
  const finish = () => after.onBeforeShadow(renderer, after, camera, spot.shadow.camera,
    after.geometry, after.material as THREE.Material, null);
  try {
    assert.deepEqual(scene.children, [before, city, after, actor], "live actors draw after the captured static depth");
    begin();
    assert.equal(preparations, 1, "an uncached spotlight still uses complete light-frustum casters");
    assert.equal(full, true);
    finish();
    assert.equal(full, false);
    assert.ok(actor.visible && actor.castShadow, "actor visibility and shadow flags remain independent");

    renderer.shadowMap.type = THREE.VSMShadowMap;
    begin();
    assert.equal(preparations, 1, "VSM receiver drawing keeps the ordinary full subtree");
    assert.equal(full, true);
    finish();
    assert.equal(full, false);

    renderer.shadowMap.type = THREE.PCFShadowMap;
    failPrepare = true;
    assert.throws(begin, /selection failed/);
    assert.equal(full, false, "failed instance selection restores the main view without an after hook");
    assert.ok(city.visible && actor.visible && actor.castShadow);
    failPrepare = false;
    begin(); finish();
    assert.equal(full, false, "the next pass recovers after selection failure");
  } finally {
    cache.dispose();
  }
});
