import test, { after, before } from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { CityTextureResidency, type CityTextureBinding } from "../../adapters/three/src/cityTextureResidency.ts";

const animationFrames = new Map<number, FrameRequestCallback>();
const originalRaf = Object.getOwnPropertyDescriptor(globalThis, "requestAnimationFrame");
const originalCancel = Object.getOwnPropertyDescriptor(globalThis, "cancelAnimationFrame");
let nextFrame = 0;
before(() => {
  Object.defineProperty(globalThis, "requestAnimationFrame", { configurable: true, value: (callback: FrameRequestCallback) => {
    const id = ++nextFrame; animationFrames.set(id, callback); return id;
  } });
  Object.defineProperty(globalThis, "cancelAnimationFrame", { configurable: true, value: (id: number) => animationFrames.delete(id) });
});
after(() => {
  if (originalRaf) Object.defineProperty(globalThis, "requestAnimationFrame", originalRaf);
  else Reflect.deleteProperty(globalThis, "requestAnimationFrame");
  if (originalCancel) Object.defineProperty(globalThis, "cancelAnimationFrame", originalCancel);
  else Reflect.deleteProperty(globalThis, "cancelAnimationFrame");
});

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: Error) => void;
  const promise = new Promise<T>((accept, fail) => { resolve = accept; reject = fail; });
  return { promise, resolve, reject };
}
const tick = () => new Promise<void>(resolve => setImmediate(resolve));
function camera(distanceFromBox: number, fov = 70) {
  const value = new THREE.PerspectiveCamera(fov, 16 / 9, .1, 1000);
  value.position.z = distanceFromBox + 1;
  value.updateMatrixWorld(true);
  return value;
}
function fixture(count = 1) {
  const root = new THREE.Group(), geometry = new THREE.BoxGeometry(2, 2, 2);
  const materials = Array.from({ length: count }, () => new THREE.MeshStandardMaterial());
  const meshes = materials.map(material => new THREE.Mesh(geometry, material));
  root.add(...meshes);
  return { root, geometry, materials, meshes,
    dispose() { geometry.dispose(); for (const material of materials) material.dispose(); } };
}
function trackedTexture() {
  const texture = new THREE.Texture();
  let disposals = 0;
  texture.addEventListener("dispose", () => { disposals++; });
  return { texture, get disposals() { return disposals; } };
}
function binding(material: THREE.MeshStandardMaterial, slot: CityTextureBinding["slot"], low: THREE.Texture,
  loadHigh: CityTextureBinding["loadHigh"]): CityTextureBinding {
  material[slot] = low;
  return { slot, low, loadHigh };
}

test("uploads at most one near texture per frame and a rejected GPU upload retains the coarse material", async () => {
  const f = fixture(), slots = ["map", "normalMap", "roughnessMap"] as const;
  const lows = slots.map(() => new THREE.Texture()), highs = slots.map(() => trackedTexture());
  let uploads = 0;
  const controller = new CityTextureResidency(f.root, m => slots.map((slot, i) =>
    binding(m, slot, lows[i], async () => highs[i].texture)), undefined, 12, () => {
      uploads++;
      if (uploads === 2) throw Error("test upload failed");
    });
  try {
    controller.frame(camera(1), 1080); await tick();
    controller.frame(camera(1), 1080);
    assert.equal(uploads, 1);
    assert.equal(f.materials[0].map, lows[0]);
    await controller.settle();
    assert.equal(controller.stats().failed, 1);
    assert.equal(controller.stats().pending, 0);
    assert.equal(f.materials[0].normalMap, lows[1]);
    assert.equal(highs[0].disposals, 1);
  } finally { controller.dispose(); f.dispose(); for (const texture of lows) texture.dispose(); }
});

test("distant materials retain published textures and do no high-resolution work", async () => {
  const f = fixture(), low = new THREE.Texture(), high = trackedTexture();
  let loads = 0, bindings = 0;
  const controller = new CityTextureResidency(f.root, material => {
    bindings++; return [binding(material, "map", low, async () => { loads++; return high.texture; })];
  });
  try {
    controller.frame(camera(100), 1080); await controller.settle();
    assert.equal(bindings, 1); assert.equal(loads, 0); assert.equal(f.materials[0].map, low);
    assert.equal(high.disposals, 0);
  } finally { controller.dispose(); f.dispose(); low.dispose(); }
});

test("promotion restores the exact authored high textures without changing material shader bindings", async () => {
  const f = fixture(), material = f.materials[0], slots = ["map", "normalMap", "metalnessMap", "roughnessMap"] as const;
  const lows = slots.map(() => new THREE.Texture()), highs = slots.map(() => new THREE.Texture());
  highs.forEach((texture, i) => {
    texture.channel = i % 2; texture.flipY = false; texture.anisotropy = 8;
    texture.repeat.set(2, 3); texture.offset.set(.2, .4); texture.rotation = .6;
    texture.colorSpace = i === 0 ? THREE.SRGBColorSpace : THREE.NoColorSpace;
  });
  const compile = material.onBeforeCompile = () => {};
  material.customProgramCacheKey = () => "authored-painterly-shader";
  material.normalScale.set(.3, .6); material.defines = { AUTHORED: 1 };
  const version = material.version, key = material.customProgramCacheKey();
  let redraws = 0;
  const controller = new CityTextureResidency(f.root, m => slots.map((slot, i) => binding(m, slot, lows[i], async () => highs[i])), () => redraws++);
  try {
    controller.frame(camera(2), 1080); await controller.settle();
    slots.forEach((slot, i) => {
      assert.equal(material[slot], highs[i]);
      assert.deepEqual(highs[i].repeat.toArray(), [2, 3]);
      assert.deepEqual(highs[i].offset.toArray(), [.2, .4]);
      assert.equal(highs[i].rotation, .6); assert.equal(highs[i].anisotropy, 8);
      assert.equal(highs[i].channel, i % 2); assert.equal(highs[i].flipY, false);
    });
    assert.equal(material.version, version); assert.equal(material.onBeforeCompile, compile);
    assert.equal(material.customProgramCacheKey(), key); assert.deepEqual(material.defines, { AUTHORED: 1 });
    assert.deepEqual(material.normalScale.toArray(), [.3, .6]);
    assert.equal(redraws, 0); assert.equal(animationFrames.size, 1);
    const callbacks = [...animationFrames.values()]; animationFrames.clear(); callbacks.forEach(callback => callback(0));
    assert.equal(redraws, 1);
  } finally { controller.dispose(); f.dispose(); lows.forEach(texture => texture.dispose()); }
});

test("distance thresholds use nearest bounds and retain hysteresis without repeating loads", async () => {
  const f = fixture(), low = new THREE.Texture(), high = trackedTexture();
  let loads = 0;
  const controller = new CityTextureResidency(f.root, m => [binding(m, "map", low, async () => { loads++; return high.texture; })]);
  try {
    controller.frame(camera(18.2), 1080); await controller.settle(); assert.equal(loads, 0);
    controller.frame(camera(17.8), 1080); await controller.settle(); assert.equal(f.materials[0].map, high.texture);
    controller.frame(camera(22.8), 1080); await controller.settle(); assert.equal(f.materials[0].map, high.texture);
    controller.frame(camera(23.3), 1080); await controller.settle(); assert.equal(f.materials[0].map, low);
    assert.equal(high.disposals, 1);
    controller.frame(camera(20), 1080); await controller.settle(); assert.equal(f.materials[0].map, low);
    controller.frame(camera(17), 1080); await controller.settle(); assert.equal(f.materials[0].map, high.texture);
    assert.equal(loads, 1, "CPU-decoded high texture can be uploaded again after a departure");
  } finally { controller.dispose(); f.dispose(); low.dispose(); }
});

test("FOV, zoom and drawing-buffer height preserve nearby projected detail", async () => {
  for (const [fov, zoom, height, expected] of [[70, 1, 1080, false], [40, 1, 1080, true], [70, 2, 1080, true], [70, 1, 2160, true]] as const) {
    const f = fixture(), low = new THREE.Texture(), high = new THREE.Texture();
    const controller = new CityTextureResidency(f.root, m => [binding(m, "map", low, async () => high)]);
    try {
      const view = camera(30, fov); view.zoom = zoom; view.updateProjectionMatrix();
      controller.frame(view, height); await controller.settle();
      assert.equal(f.materials[0].map === high, expected, `${fov} degree FOV, zoom ${zoom}, height ${height}`);
    } finally { controller.dispose(); f.dispose(); low.dispose(); }
  }
});

test("shared materials include every transformed mesh and instanced placement", async () => {
  const f = fixture(), low = new THREE.Texture(), high = new THREE.Texture(), material = f.materials[0];
  f.meshes[0].position.x = 500;
  const instances = new THREE.InstancedMesh(f.geometry, material, 2);
  instances.position.x = 100;
  instances.setMatrixAt(0, new THREE.Matrix4().makeTranslation(200, 0, 0));
  instances.setMatrixAt(1, new THREE.Matrix4().makeTranslation(-100, 0, 0));
  f.root.add(instances);
  let loadCalls = 0, bindingCalls = 0;
  const controller = new CityTextureResidency(f.root, m => {
    bindingCalls++; return [binding(m, "map", low, async () => { loadCalls++; return high; })];
  });
  try {
    controller.frame(camera(2), 1080); await controller.settle();
    assert.equal(material.map, high); assert.equal(loadCalls, 1); assert.equal(bindingCalls, 1);
    assert.equal(instances.material, material); assert.equal(f.meshes[0].material, material);
  } finally { controller.dispose(); instances.dispose(); f.dispose(); low.dispose(); }
});

test("a late load after teleport does not promote the departed material and remains reusable", async () => {
  const f = fixture(), low = new THREE.Texture(), high = trackedTexture(), load = deferred<THREE.Texture>();
  let loads = 0;
  const controller = new CityTextureResidency(f.root, m => [binding(m, "map", low, () => { loads++; return load.promise; })]);
  try {
    controller.frame(camera(2), 1080); controller.frame(camera(100), 1080);
    load.resolve(high.texture); await controller.settle();
    assert.equal(f.materials[0].map, low);
    controller.frame(camera(2), 1080); await controller.settle();
    assert.equal(f.materials[0].map, high.texture); assert.equal(loads, 1);
  } finally { controller.dispose(); f.dispose(); low.dispose(); }
});

test("unbound high textures completed after teleport are cleaned on disposal", async () => {
  const f = fixture(), low = new THREE.Texture(), high = trackedTexture(), load = deferred<THREE.Texture>();
  const controller = new CityTextureResidency(f.root, m => [binding(m, "map", low, () => load.promise)]);
  try {
    controller.frame(camera(2), 1080); controller.frame(camera(100), 1080);
    load.resolve(high.texture); await controller.settle(); controller.dispose();
    assert.equal(f.materials[0].map, low); assert.equal(high.disposals, 1);
  } finally { controller.dispose(); f.dispose(); low.dispose(); }
});

test("shared high maps are disposed once only after their final active slot releases them", async () => {
  const f = fixture(2), lows = [new THREE.Texture(), new THREE.Texture()], high = trackedTexture();
  f.meshes[1].position.x = 30;
  const controller = new CityTextureResidency(f.root, m => {
    const i = f.materials.indexOf(m);
    return [binding(m, "metalnessMap", lows[i], async () => high.texture), binding(m, "roughnessMap", lows[i], async () => high.texture)];
  });
  try {
    const view = camera(0); view.position.x = 15;
    controller.frame(view, 1080); await controller.settle();
    assert.equal(f.materials[0].metalnessMap, high.texture); assert.equal(f.materials[1].roughnessMap, high.texture);
    view.position.x = -10; controller.frame(view, 1080); await controller.settle();
    assert.equal(f.materials[1].metalnessMap, lows[1]); assert.equal(f.materials[0].roughnessMap, high.texture);
    assert.equal(high.disposals, 0, "remaining material owns both packed-data slots");
    controller.dispose(); assert.equal(high.disposals, 1);
    controller.dispose(); assert.equal(high.disposals, 1);
  } finally { controller.dispose(); f.dispose(); lows.forEach(texture => texture.dispose()); }
});

test("in-flight completion after dispose neither binds nor double-disposes a high shared across jobs", async () => {
  const f = fixture(2), low = new THREE.Texture(), high = trackedTexture(), loads = [deferred<THREE.Texture>(), deferred<THREE.Texture>()];
  let redraws = 0;
  const controller = new CityTextureResidency(f.root, m => [binding(m, "map", low, () => loads[f.materials.indexOf(m)].promise)], () => redraws++);
  try {
    controller.frame(camera(2), 1080); controller.dispose();
    loads.forEach(load => load.resolve(high.texture)); await controller.settle();
    assert.equal(f.materials[0].map, low); assert.equal(f.materials[1].map, low);
    assert.equal(high.disposals, 1); assert.equal(redraws, 0); assert.equal(animationFrames.size, 0);
  } finally { controller.dispose(); f.dispose(); low.dispose(); }
});

test("failed high loads retain the published fallback and release successful sibling slots", async () => {
  const f = fixture(), low = new THREE.Texture(), high = trackedTexture();
  let attempts = 0;
  const warn = console.warn; console.warn = () => {};
  const controller = new CityTextureResidency(f.root, m => [binding(m, "map", low, async () => high.texture),
    binding(m, "normalMap", low, async () => { attempts++; throw Error("missing authored high image"); })]);
  try {
    controller.frame(camera(2), 1080); await controller.settle();
    controller.frame(camera(2), 1080); await controller.settle(); controller.dispose();
    assert.equal(f.materials[0].map, low); assert.equal(f.materials[0].normalMap, low);
    assert.equal(attempts, 1, "a missing image is not retried every frame"); assert.equal(high.disposals, 1);
  } finally { console.warn = warn; controller.dispose(); f.dispose(); low.dispose(); }
});

test("identical high loaders shared by slots and materials do not duplicate decoding", async () => {
  const f = fixture(2), low = new THREE.Texture(), high = trackedTexture(), result = deferred<THREE.Texture>();
  let loads = 0;
  const sharedLoad = () => { loads++; return result.promise; };
  const controller = new CityTextureResidency(f.root, m => [binding(m, "metalnessMap", low, sharedLoad), binding(m, "roughnessMap", low, sharedLoad)]);
  try {
    controller.frame(camera(2), 1080); result.resolve(high.texture); await controller.settle();
    assert.equal(loads, 1);
    f.materials.forEach(m => { assert.equal(m.metalnessMap, high.texture); assert.equal(m.roughnessMap, high.texture); });
  } finally { controller.dispose(); f.dispose(); low.dispose(); }
});

test("bounded async pumping drains all nearby materials without another frame", async () => {
  const f = fixture(7), low = new THREE.Texture(), highs = f.materials.map(() => new THREE.Texture());
  const loads = f.materials.map(() => deferred<THREE.Texture>()), started: number[] = [];
  f.meshes.forEach((mesh, i) => { mesh.position.x = i; });
  const controller = new CityTextureResidency(f.root, m => {
    const i = f.materials.indexOf(m);
    return [binding(m, "map", low, () => { started.push(i); return loads[i].promise; })];
  });
  try {
    controller.frame(camera(2), 1080); await tick(); assert.deepEqual(started, [0, 1, 2, 3]);
    loads[0].resolve(highs[0]); await tick(); assert.deepEqual(started, [0, 1, 2, 3, 4]);
    loads[1].resolve(highs[1]); await tick(); assert.deepEqual(started, [0, 1, 2, 3, 4, 5]);
    loads[2].resolve(highs[2]); await tick(); assert.deepEqual(started, [0, 1, 2, 3, 4, 5, 6]);
    loads.forEach((load, i) => load.resolve(highs[i])); await controller.settle();
    f.materials.forEach((material, i) => assert.equal(material.map, highs[i]));
  } finally { controller.dispose(); f.dispose(); low.dispose(); }
});

test("a camera cut prioritizes newly nearby work over stale queued placements", async () => {
  const f = fixture(8), low = new THREE.Texture(), highs = f.materials.map(() => new THREE.Texture());
  const loads = f.materials.map(() => deferred<THREE.Texture>()), started: number[] = [];
  f.meshes.forEach((mesh, i) => { mesh.position.x = i === 7 ? 100 : i; });
  const controller = new CityTextureResidency(f.root, m => {
    const i = f.materials.indexOf(m);
    return [binding(m, "map", low, () => { started.push(i); return loads[i].promise; })];
  });
  try {
    const view = camera(2);
    controller.frame(view, 1080); await tick(); assert.deepEqual(started, [0, 1, 2, 3]);
    view.position.x = 100; controller.frame(view, 1080);
    loads[0].resolve(highs[0]); await tick(); assert.deepEqual(started, [0, 1, 2, 3, 7]);
    loads.forEach((load, i) => load.resolve(highs[i])); await controller.settle();
    assert.equal(f.materials[7].map, highs[7]);
    f.materials.slice(0, 7).forEach(material => assert.equal(material.map, low));
    assert.deepEqual(started, [0, 1, 2, 3, 7], "obsolete queued materials do not consume decode slots");
  } finally { controller.dispose(); f.dispose(); low.dispose(); }
});

test("a failed image can recover on a later approach without retrying on every nearby frame", async () => {
  const f = fixture(), low = new THREE.Texture(), high = trackedTexture();
  let attempts = 0;
  const warn = console.warn; console.warn = () => {};
  const controller = new CityTextureResidency(f.root, m => [binding(m, "map", low, async () => {
    if (++attempts === 1) throw Error("temporary fetch error");
    return high.texture;
  })]);
  try {
    controller.frame(camera(2), 1080); await controller.settle();
    controller.frame(camera(2), 1080); await controller.settle();
    assert.equal(attempts, 1); assert.equal(f.materials[0].map, low);
    controller.frame(camera(100), 1080); controller.frame(camera(2), 1080); await controller.settle();
    assert.equal(attempts, 2); assert.equal(f.materials[0].map, high.texture);
  } finally { console.warn = warn; controller.dispose(); f.dispose(); low.dispose(); }
});
