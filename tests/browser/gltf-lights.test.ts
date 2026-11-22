import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { GltfLights } from "../../browser/renderer/gltf-lights.ts";
import { GameLighting } from "../../browser/renderer/game-lighting.ts";
import { LightingError, parseLighting } from "../../browser/renderer/lighting.ts";

const hooks = registerHooks({ resolve(specifier, context, nextResolve) {
  return nextResolve(specifier.startsWith(".") && !/\.[a-z]+$/i.test(specifier) ? specifier + ".ts" : specifier, context);
} });
const { webGpuLighting, webGpuSceneProfile } = await import("../../browser/renderer/webgpu-lighting.ts");
const { resolveRenderDiagnostics, lightingOverridesQuery } = await import("../../adapters/three/src/renderProfile.ts");
hooks.deregister();

const lighting = parseLighting({ version: 1, lights: [{ id: "roof.sun", direction: [0, 0, 1], intensity: 2, color: [1, .5, .25] }],
  environment: { source: "none" } });

async function loadLights() {
  const lights = [{ type: "directional", name: "roof.sun", color: [1, .5, .25], intensity: 2 }, { type: "point" }, { type: "spot" }];
  const json = { asset: { version: "2.0" }, scene: 0, scenes: [{ nodes: [5] }], extensionsUsed: ["KHR_lights_punctual"],
    extensions: { KHR_lights_punctual: { lights } }, nodes: [
      { name: "roof.sun", extensions: { KHR_lights_punctual: { light: 0 } }, children: [4] },
      { name: "lamp", extensions: { KHR_lights_punctual: { light: 1 } } },
      { name: "spot", extensions: { KHR_lights_punctual: { light: 2 } } },
      // This light uses the same name, intensity and color as the exported sun.
      { name: "roof.sun", rotation: [0, 1, 0, 0], extensions: { KHR_lights_punctual: { light: 0 } } },
      { name: "attached" },
      { name: "domain-root", rotation: [-Math.SQRT1_2, 0, 0, Math.SQRT1_2], children: [0, 1, 2, 3] },
    ] };
  const { scene: root } = await new GLTFLoader().parseAsync(JSON.stringify(json), "");
  const [mirror, lamp, spot, second] = root.children[0].children as THREE.Light[];
  return { root, mirror, lamp, spot, second };
}

test("an invalid record never allocates WebGL lighting resources or attaches a preview rig", () => {
  const scene = new THREE.Scene();
  const renderer = new Proxy({} as THREE.WebGLRenderer, {
    get() { throw Error("renderer accessed before validating lighting"); },
  });
  assert.throws(() => new GameLighting(renderer, scene, new THREE.Camera(), { version: -1 }), LightingError);
  assert.deepEqual(scene.children, []);
});

test("plain assets use authored lights, with a default rig only when no lights were supplied", async () => {
  const { root, mirror, lamp, spot, second } = await loadLights();
  const policy = new GltfLights();
  assert.deepEqual(policy.select([root]), { useRig: false, authored: [mirror, lamp, spot, second] });
  assert.deepEqual(policy.select([new THREE.Group()]), { useRig: true, authored: [] });
});

test("only the recorded sun is suppressed and authored children and layers survive record changes", async () => {
  const { root, mirror, lamp, spot, second } = await loadLights();
  const policy = new GltfLights();
  mirror.layers.enable(2);
  const original = mirror.layers.mask;
  const child = root.getObjectByName("attached")!;
  assert.deepEqual(policy.select([root], lighting), { useRig: true, authored: [lamp, spot, second] });
  assert.equal(mirror.layers.mask, 0); assert.equal(child.parent, mirror);
  assert.ok(child.layers.test(new THREE.Camera().layers)); assert.equal(mirror.visible, true);
  const noSun = { ...lighting, lights: [] };
  assert.deepEqual(policy.select([root], noSun).authored, [lamp, spot, second]);
  assert.equal(mirror.layers.mask, 0);
  policy.select([], lighting);
  assert.equal(mirror.layers.mask, original);
  assert.deepEqual(policy.select([root]).authored, [mirror, lamp, spot, second]);
});

test("a matching light placed under a separately rotated parent remains an authored fill", async () => {
  const { root, mirror } = await loadLights();
  const other = new THREE.Group(), parent = new THREE.Group();
  parent.rotation.y = 1.2;
  const fill = mirror.clone();
  parent.add(fill); other.add(parent);
  const policy = new GltfLights();
  const active = policy.select([root, other], lighting);
  assert.ok(active.authored.includes(fill)); assert.ok(!active.authored.includes(mirror));
  assert.equal(fill.layers.mask, 1); assert.equal(mirror.layers.mask, 0);
  policy.restore();
});

test("portable WebGPU uses the same authored receivers and keeps embedded point and spot shadow cameras", async () => {
  for (const hasRecord of [false, true]) {
    const { root, mirror, lamp, spot, second } = await loadLights();
    const scene = new THREE.Scene(); scene.add(root);
    lamp.castShadow = spot.castShadow = true;
    const renderer = { shadowMap: { enabled: false } } as Parameters<typeof webGpuLighting>[0];
    const bounds = new THREE.Box3(new THREE.Vector3(-1,-1,-1), new THREE.Vector3(1,1,1));
    const active = webGpuLighting(renderer, scene, root, lighting, hasRecord, bounds, resolveRenderDiagnostics());
    try {
      const inView: THREE.Light[] = [];
      scene.traverseVisible(object => { if (object instanceof THREE.Light && object.layers.isEnabled(0)) inView.push(object); });
      assert.equal(inView.length, hasRecord ? 5 : 4);
      assert.ok(inView.includes(lamp) && inView.includes(spot) && inView.includes(second));
      assert.equal(inView.includes(mirror), !hasRecord);
      assert.ok(active.shadowCameras.has((lamp as THREE.PointLight).shadow.camera));
      assert.ok(active.shadowCameras.has((spot as THREE.SpotLight).shadow.camera));
      assert.ok(renderer.shadowMap.enabled);
    } finally { active.dispose(); }
    assert.equal(mirror.parent, root.children[0]); assert.equal(mirror.layers.mask, 1);
    assert.deepEqual(scene.children, [root]);
  }
});

test("portable WebGPU preflight relies on explicit illustration requests and authored lighting", () => {
  const defaults = resolveRenderDiagnostics();
  const query = lightingOverridesQuery(defaults);
  assert.equal(webGpuSceneProfile({}, query, defaults).missing, undefined);
  assert.equal(webGpuSceneProfile({}, query, {
    ...defaults, lighting: { ...defaults.lighting, paintedSpots: true },
  }).missing, "the requested illustration effects");
  assert.equal(webGpuSceneProfile({}, query, {
    ...defaults, effects: { ...defaults.effects, edgeBlur: true },
  }).missing, "the requested illustration effects");
  const withOcclusion = { extras: { superspec: { lighting: {
    version: 1, lights: [], ambientOcclusion: { enabled: true },
  } } } };
  assert.equal(webGpuSceneProfile(withOcclusion, query, defaults).missing, "the published illustration shadows or screen-space lighting");
  const withoutOcclusion = resolveRenderDiagnostics({ lighting: { ambientOcclusion: "off" } });
  assert.equal(webGpuSceneProfile(withOcclusion, lightingOverridesQuery(withoutOcclusion), withoutOcclusion).missing, undefined);
});
