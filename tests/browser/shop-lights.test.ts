import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { Scene, Vector3, WebGLRenderTarget } from "three";

// The adapter ships bundled modules with extensionless source imports. Resolve
// those imports for this source-level test without changing its distribution.
const hooks = registerHooks({ resolve(specifier, context, nextResolve) {
  if (specifier.startsWith(".") && !/\.[a-z]+$/i.test(specifier)) return nextResolve(specifier + ".ts", context);
  return nextResolve(specifier, context);
} });
const { ShopLights } = await import("../../adapters/three/src/shopLights.ts");
hooks.deregister();

const fixtures = [[0, 2, 0], [10, 2, 0], [20, 2, 0], [30, 2, 0]];

test("bench bulkhead stays on when orbiting above the soffit", () => {
  const fixture = {
    kind: "planter-ceiling", fixture: "ceiling-bulkhead", position: [0, 2.5, 0], target: [0, 0, 0],
    ceilingY: 2.7, floorY: 0, color: 0xffdfb2, candela: 728, range: 2.7, angle: 0.83, penumbra: 0.88,
  };
  for (const cameraFloorCull of [true, false]) {
    const lights = new ShopLights(new Scene(), [{ ...fixture, cameraFloorCull }]);
    try {
      lights.update(new Vector3(4, 2, 4));
      assert.equal(lights.shadowLights[0].intensity, fixture.candela);
      lights.update(new Vector3(4, 4, 4));
      assert.equal(lights.shadowLights[0].intensity, cameraFloorCull ? 0 : fixture.candela);
    } finally {
      lights.dispose();
    }
  }
});

function finishShadowPass(lights: InstanceType<typeof ShopLights>): void {
  for (const light of lights.shadowLights) {
    light.shadow.map ??= new WebGLRenderTarget(512, 512);
    light.shadow.needsUpdate = false;
  }
}

test("static fixture shadows survive camera movement and brightness changes", () => {
  const lights = new ShopLights(new Scene(), fixtures, false, false, { staticShadows: true });
  try {
    lights.update(new Vector3(0, 1, 0));
    assert.ok(lights.shadowLights.every(light => light.shadow.needsUpdate && !light.shadow.autoUpdate));
    const before = lights.shadowLights.map(light => light.intensity);
    finishShadowPass(lights);
    lights.update(new Vector3(0.5, 1, 0));
    assert.ok(lights.shadowLights.every(light => !light.shadow.needsUpdate && !light.shadow.autoUpdate));
    assert.notDeepEqual(lights.shadowLights.map(light => light.intensity), before);
  } finally { lights.dispose(); }
});

test("reassigning static shadow slots redraws the new fixture projection", () => {
  const lights = new ShopLights(new Scene(), fixtures, false, false, { staticShadows: true });
  try {
    lights.update(new Vector3(0, 1, 0));
    finishShadowPass(lights);
    lights.update(new Vector3(30, 1, 0));
    assert.deepEqual(lights.shadowLights.map(light => light.position.x), [30, 20, 10]);
    assert.ok(lights.shadowLights.every(light => light.shadow.needsUpdate));
  } finally { lights.dispose(); }
});

test("explicit invalidation survives an unchanged eye and the next selection update", () => {
  const lights = new ShopLights(new Scene(), fixtures, false, false, { staticShadows: true });
  try {
    const eye = new Vector3(0, 1, 0);
    lights.update(eye);
    finishShadowPass(lights);
    lights.invalidateShadows();
    lights.update(eye);
    assert.ok(lights.shadowLights.every(light => light.shadow.needsUpdate));
    lights.update(new Vector3(0.5, 1, 0));
    assert.ok(lights.shadowLights.every(light => light.shadow.needsUpdate));
  } finally { lights.dispose(); }
});

test("ordinary game fixtures keep updating shadows for moving actors", () => {
  const lights = new ShopLights(new Scene(), fixtures, false, false);
  try {
    lights.update(new Vector3(0, 1, 0));
    finishShadowPass(lights);
    assert.ok(lights.shadowLights.some(light => light.intensity > 0));
    for (const light of lights.shadowLights) assert.equal(light.shadow.autoUpdate, light.intensity > 0);
  } finally { lights.dispose(); }
});

test("stable shadow slots refresh only newly selected fixtures", () => {
  const lights = new ShopLights(new Scene(), fixtures, false, false, { staticShadows: true, stableSlots: true });
  try {
    lights.update(new Vector3(0, 1, 0));
    finishShadowPass(lights);
    lights.update(new Vector3(30, 1, 0));
    assert.deepEqual(lights.shadowLights.map(light => light.position.x), [30, 10, 20]);
    assert.deepEqual(lights.shadowLights.map(light => light.shadow.needsUpdate), [true, false, false]);
    assert.ok(lights.shadowLights[0].intensity > 0);
  } finally { lights.dispose(); }
});

test("a dark fixture waits to capture its shadow until it contributes", () => {
  const lights = new ShopLights(new Scene(), fixtures, false, false, { staticShadows: true, stableSlots: true });
  try {
    lights.update(new Vector3(0, 1, 0));
    finishShadowPass(lights);
    lights.update(new Vector3(100, 1, 0));
    assert.ok(lights.shadowLights.every(light => light.intensity === 0 && !light.shadow.needsUpdate));
    lights.update(new Vector3(30, 1, 0));
    assert.deepEqual(lights.shadowLights.map(light => light.shadow.needsUpdate), [true, false, false]);
  } finally { lights.dispose(); }
});
