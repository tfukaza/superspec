import assert from "node:assert/strict";
import test from "node:test";
import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import { attachWorldSpecScene, opaqueProgramSort, resolveGraphicsSettings, resolveRenderDiagnostics } from "#render-engine";
import type { OpaqueRenderItem } from "../../adapters/three/src/opaqueProgramSort.ts";

function sceneFixture() {
  const scene = new THREE.Scene();
  const sun = new THREE.DirectionalLight();
  const actor = new THREE.Group();
  const root = new THREE.Group(), mesh = new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshStandardMaterial());
  root.userData.worldSpec = true;
  root.add(mesh);
  scene.add(sun, actor, root);
  const draws: { scene: THREE.Object3D; camera: THREE.Camera }[] = [];
  const opaqueSorts: unknown[] = [];
  const renderer = {
    setOpaqueSort: (sort: unknown) => opaqueSorts.push(sort),
    domElement: Object.assign(new EventTarget(), { width: 1920, height: 1080 }),
    capabilities: { maxTextureSize: 4096, getMaxAnisotropy: () => 8 },
    shadowMap: { type: THREE.BasicShadowMap, autoUpdate: false, enabled: false },
    render: (scene: THREE.Object3D, camera: THREE.Camera) => draws.push({ scene, camera }),
  } as unknown as THREE.WebGLRenderer;
  const parser = { json: {} };
  const gltf = { scene: root, scenes: [root], parser } as unknown as GLTF;
  const diagnostics = resolveRenderDiagnostics({ lighting: { fixtures: false }, visibility: { cityCulling: false } });
  return { actor, root, mesh, sun, gltf, renderer, draws, opaqueSorts, options: { scene, renderer, sun, diagnostics } };
}

test("opaque draws group by shader program inside Three's group and render order", () => {
  const programs = new Map<object, number>(), lookups: object[] = [];
  const renderer = { info: { render: { frame: 0 } }, properties: {
    has: (material: object) => programs.has(material),
    get: (material: object) => { lookups.push(material); return { currentProgram: { id: programs.get(material)! } }; },
  } };
  const sort = opaqueProgramSort(renderer);
  // Materials created in alternating order, as a city pack creates them.
  const materials = [7, 3, 7, 3, 7, 3].map((program, i) => { const material = { id: i + 1 }; programs.set(material, program); return material; });
  const undrawn = { id: 9 };
  let next = 0;
  const item = (material: { id: number }, extra: Partial<OpaqueRenderItem> = {}): OpaqueRenderItem =>
    ({ groupOrder: 0, renderOrder: 0, material, materialVariant: 0, z: 1, id: next++, ...extra });
  const items = [...materials.map(material => item(material)), item(undrawn)];
  const runs = (list: OpaqueRenderItem[]) => list.map(entry => programs.get(entry.material) ?? -1)
    .filter((program, i, all) => program !== all[i - 1]).length;
  const threeOrder = items.slice().sort((a, b) => a.material.id - b.material.id);
  assert.equal(runs(threeOrder), 7, "material order alternates the two programs");
  const sorted = items.slice().reverse().sort(sort);
  assert.deepEqual(sorted.map(entry => entry.material.id), [9, 2, 4, 6, 1, 3, 5]);
  assert.equal(runs(sorted), 3, "one run for each program, after the material that has not drawn yet");
  assert.equal(new Set(lookups).size, 6);
  assert.equal(lookups.length, 6, "a material's program is resolved once per render call");

  // Three's leading keys keep their precedence, and its trailing keys still break ties.
  const [a, b] = materials;
  const late = item(b, { renderOrder: 1 }), overlay = item(b, { groupOrder: 1, renderOrder: -1 });
  const nearer = item(a, { z: .5 }), farther = item(a, { z: 2 }), twin = item(a, { z: 2 });
  assert.deepEqual([overlay, late, farther, twin, nearer, item(b)].sort(sort).map(entry => entry.id),
    [next - 1, nearer.id, farther.id, twin.id, late.id, overlay.id]);
  // A material drawn by ordinary and instanced meshes reports one of its two programs.
  // Variant before material keeps the two real programs in two runs.
  const mixed = [item(a, { materialVariant: 2 }), item(materials[2], { materialVariant: 2 }), item(a), item(materials[2])];
  assert.deepEqual(mixed.slice().sort(sort).map(entry => [entry.material.id, entry.materialVariant]), [[1, 0], [3, 0], [1, 2], [3, 2]]);

  // The next render call observes relinked programs.
  programs.set(b, 8);
  assert.deepEqual([item(b), item(a)].sort(sort).map(entry => entry.material.id), [2, 1]);
  renderer.info.render.frame++;
  assert.deepEqual([item(b), item(a)].sort(sort).map(entry => entry.material.id), [1, 2]);
});

test("published world renders in the host scene and releases only its owned resources", async () => {
  const { actor, root, mesh, sun, gltf, renderer, draws, opaqueSorts, options } = sceneFixture();
  let geometryDisposals = 0, sunDisposals = 0;
  mesh.geometry.addEventListener("dispose", () => geometryDisposals++);
  sun.shadow.map = new THREE.WebGLRenderTarget(32, 32);
  sun.shadow.map.addEventListener("dispose", () => sunDisposals++);

  const attached = await attachWorldSpecScene(gltf, { ...options, sortOpaqueByProgram: true });
  assert.ok(attached);
  assert.equal(mesh.castShadow, true);
  assert.equal(renderer.shadowMap.type, THREE.PCFShadowMap);
  const camera = new THREE.OrthographicCamera();
  attached.render(camera);
  assert.deepEqual(draws, [{ scene: options.scene, camera }]);
  assert.deepEqual(opaqueSorts.map(sort => typeof sort), ["function"], "the world's opaque draws sort by program");
  attached.dispose();
  attached.dispose();
  assert.equal(opaqueSorts.length, 2);
  assert.equal(opaqueSorts[1], null, "the host renderer returns to Three's opaque order");
  assert.equal(root.parent, null);
  assert.equal(actor.parent, options.scene);
  assert.equal(sun.parent, options.scene);
  assert.equal(geometryDisposals, 1);
  assert.equal(sunDisposals, 0);
  assert.deepEqual(renderer.shadowMap, { type: THREE.BasicShadowMap, autoUpdate: false, enabled: false });
  sun.shadow.dispose();
});

test("a world cancelled during material preparation leaves the host ready for another map", async () => {
  const { actor, root, mesh, gltf, renderer, options } = sceneFixture();
  const abort = new AbortController();
  const reason = new Error("Superseded");
  root.userData.worldSpecConcreteDetail = { version: 1, texture: 0, metersPerTile: 1, mean: .5, strength: .3 };
  const texture = new THREE.Texture();
  let textureDisposals = 0, geometryDisposals = 0;
  texture.addEventListener("dispose", () => textureDisposals++);
  mesh.geometry.addEventListener("dispose", () => geometryDisposals++);
  gltf.parser.getDependency = async () => { abort.abort(reason); return texture; };

  await assert.rejects(attachWorldSpecScene(gltf, { ...options, signal: abort.signal }), reason);
  assert.equal(renderer.shadowMap.type, THREE.BasicShadowMap);
  assert.equal(textureDisposals, 1);
  assert.equal(geometryDisposals, 0);
  assert.deepEqual(options.scene.children, [options.sun, actor, root]);
});

test("a failed renderer configuration rolls back the unpublished world", async () => {
  const { actor, root, mesh, gltf, renderer, options } = sceneFixture();
  root.userData.worldSpecEnvironment = { version: 1, sky: { zenith: [0.2, 0.4, 0.6], horizon: [0.7, 0.7, 0.6] },
    fog: { near: 50, far: 100 } };
  const fog = options.scene.fog = new THREE.Fog(0xffffff, 1, 50);
  const reason = new Error("The renderer rejected the setting");
  Object.defineProperty(renderer.shadowMap, "autoUpdate", { get: () => false,
    set: (value: boolean) => { if (value) throw reason; } });
  let geometryDisposals = 0;
  mesh.geometry.addEventListener("dispose", () => geometryDisposals++);

  await assert.rejects(attachWorldSpecScene(gltf, options), reason);
  assert.deepEqual(options.scene.children, [options.sun, actor, root]);
  assert.equal(options.scene.fog, fog);
  assert.equal(renderer.shadowMap.type, THREE.BasicShadowMap);
  assert.equal(geometryDisposals, 0);
});

test("live presets update authored filtering and fixture budgets without replacing the world", async () => {
  const { gltf, mesh, renderer, root, options } = sceneFixture();
  const map = new THREE.Texture();
  map.anisotropy = 2;
  mesh.material.map = map;
  root.userData.shopLightPositions = [[0, 2, 0]];
  const diagnostics = resolveRenderDiagnostics({ lighting: { shadows: false }, visibility: { cityCulling: false } });
  const attached = await attachWorldSpecScene(gltf, { ...options, diagnostics, enableFixtureShadows: false });
  assert.ok(attached);
  const fixture = options.scene.children.find(child => child instanceof THREE.SpotLight)! as THREE.SpotLight;
  assert.deepEqual([map.anisotropy, fixture.castShadow, fixture.shadow.mapSize.x], [2, false, 512]);
  fixture.shadow.map = new THREE.WebGLRenderTarget(512, 512);
  let discarded = 0;
  fixture.shadow.map.addEventListener("dispose", () => discarded++);

  attached.setGraphics(resolveGraphicsSettings("very-high"), true);
  assert.deepEqual([map.anisotropy, fixture.castShadow, fixture.shadow.mapSize.x], [8, true, 1024]);
  assert.equal(fixture.shadow.map, null);
  assert.equal(discarded, 1);
  assert.equal(renderer.shadowMap.enabled, false);
  attached.setGraphics(resolveGraphicsSettings("medium"), false);
  assert.deepEqual([map.anisotropy, fixture.castShadow, fixture.shadow.mapSize.x], [2, false, 512]);
  assert.equal(root.parent, options.scene);

  attached.dispose();
});

test("first-texture preparation stores only what the camera's first frame draws and compiles nothing", async () => {
  const { gltf, root, renderer, draws, opaqueSorts, options } = sceneFixture();
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, .1, 100);
  options.scene.add(camera);
  const add = (z: number) => {
    const map = new THREE.DataTexture(new Uint8Array(4), 1, 1);
    map.needsUpdate = true;
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshStandardMaterial({ map }));
    mesh.position.z = z;
    root.add(mesh);
    return { mesh, map };
  };
  const ahead = add(-5), behind = add(5), hidden = add(-6), otherLayer = add(-7), inHiddenGroup = add(-8);
  hidden.mesh.visible = false;
  otherLayer.mesh.layers.set(3);
  const group = new THREE.Group();
  group.visible = false;
  root.add(group); group.add(inHiddenGroup.mesh);
  const stored: THREE.Texture[] = [];
  Object.assign(renderer, { properties: { has: (map: THREE.Texture) => stored.includes(map) },
    initTexture: (map: THREE.Texture) => stored.push(map),
    compile() { throw Error("Texture preparation compiled a shader"); }, compileAsync() { throw Error("Texture preparation compiled a shader"); } });
  const attached = await attachWorldSpecScene(gltf, options);
  assert.ok(attached);
  const stopped = new AbortController(), reason = new Error("Map replaced");
  stopped.abort(reason);
  await assert.rejects(async () => attached.prepareFirstTextures(camera, stopped.signal), reason);
  assert.deepEqual(stored, []);
  const steps: number[][] = [];
  assert.equal((await attached.prepareFirstTextures(camera, undefined, (...step) => steps.push(step))).uploadedTextures, 1);
  assert.deepEqual(stored, [ahead.mesh.material.map]);
  assert.ok(![behind, hidden, otherLayer, inHiddenGroup].some(({ map }) => stored.includes(map)));
  assert.deepEqual(steps, [[0, 1], [1, 1]]);
  assert.equal((await attached.prepareFirstTextures(camera)).uploadedTextures, 0, "a second preparation stores nothing again");
  assert.deepEqual(draws, [], "nothing was drawn");
  assert.deepEqual(opaqueSorts, [], "the host keeps Three's opaque order unless it asks for program sorting");
  attached.dispose();
});
