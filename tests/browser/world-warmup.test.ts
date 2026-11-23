import assert from "node:assert/strict";
import { test } from "node:test";
import * as THREE from "three";
import { warmupMesh } from "#render-engine";
import { prepareFirstColor, warmWorldColor, warmWorldTextures } from "../../adapters/three/src/worldWarmup.ts";
import { enableSinglePassGlazing, warmGlazingSides } from "../../adapters/three/src/glazing.ts";
import { markOutlineExclusions, registerOutlineDepth, warmExcludedDepth } from "../../adapters/three/src/outlineExclusions.ts";

function texture(width: number) {
  const tex = new THREE.DataTexture(new Uint8Array(width * width * 4), width, width);
  tex.generateMipmaps = true;
  tex.needsUpdate = true;
  return tex;
}

function coldColorFixture() {
  const stage = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  stage.fog = new THREE.Fog(0x112244, 1, 15);
  stage.environment = new THREE.Texture();
  stage.background = new THREE.Color(0x123456);
  stage.environmentRotation.y = .1;
  stage.environmentIntensity = .81;
  stage.add(new THREE.DirectionalLight());
  const target = new THREE.WebGLRenderTarget(8, 8), previous = new THREE.WebGLCubeRenderTarget(8);
  const writes: unknown[][] = [], reflected: string[] = [];
  const program = (name: string) => ({
    program: {} as object | undefined, getUniforms() { reflected.push(name); },
  });
  const existing = program("existing"), first = program("first"), second = program("second");
  const gl = { getExtension: (): object | null => ({ COMPLETION_STATUS_KHR: 123 }),
    isContextLost: () => false, getProgramParameter: (_handle: object, parameter: number) => { assert.equal(parameter, 123); return true; } };
  const renderer = {
    getContext: () => gl, info: { programs: [existing] }, getRenderTarget: () => previous,
    getActiveCubeFace: () => 2, getActiveMipmapLevel: () => 3,
    setRenderTarget(...args: unknown[]) { writes.push(args); },
    compile(source: THREE.Scene, view: THREE.Camera, lighting: THREE.Scene) {
      assert.equal(source, stage); assert.equal(view, camera);
      assert.notEqual(lighting, stage); assert.equal(lighting.children.length, 0);
      assert.equal(source.children.length, 1);
      assert.equal(lighting.environment, stage.environment); assert.equal(lighting.fog, stage.fog);
      assert.equal(lighting.background, stage.background); assert.equal(lighting.environmentIntensity, stage.environmentIntensity);
      assert.deepEqual(lighting.environmentRotation.toArray(), stage.environmentRotation.toArray());
      this.info.programs.push(first, second);
    },
  };
  const abort = new AbortController();
  const run = (wait = 2000) => prepareFirstColor(renderer as unknown as THREE.WebGLRenderer, stage, camera, target, abort.signal, wait);
  return { run, gl, abort, renderer, writes, target, previous, reflected, first, second };
}

test("early color reflects every new program after all links complete, with isolated lighting and host target restored during the wait", async () => {
  const f = coldColorFixture();
  let linked = false;
  f.gl.getProgramParameter = handle => handle === f.second.program || linked;
  const pending = f.run();
  assert.deepEqual(f.writes, [[f.target], [f.previous, 2, 3]]);
  assert.deepEqual(f.reflected, []);
  linked = true;
  assert.equal((await pending)?.programs, 2);
  assert.deepEqual(f.reflected, ["first", "second"]);
});

test("early color skips missing extension, lost contexts and cancellation, and bounds a driver's incomplete links", async () => {
  for (const mode of ["no-extension", "lost-at-start", "cancelled", "lost-while-waiting", "restored-while-waiting", "timeout"]) {
    const f = coldColorFixture();
    if (mode === "no-extension") f.gl.getExtension = () => null;
    if (mode === "lost-at-start") f.gl.isContextLost = () => true;
    if (mode === "cancelled") f.abort.abort();
    f.gl.getProgramParameter = () => false;
    const pending = f.run(mode === "timeout" ? 1 : 2000);
    if (mode === "lost-while-waiting") f.gl.isContextLost = () => true;
    if (mode === "restored-while-waiting") f.renderer.info = { programs: [] };
    assert.equal(await pending, undefined);
    assert.deepEqual(f.reflected, []);
    if (["no-extension", "lost-at-start", "cancelled"].includes(mode)) assert.deepEqual(f.writes, []);
  }
});

test("disposed and replaced native handles are never reflected and submission failures restore the target", async () => {
  const f = coldColorFixture();
  f.gl.getProgramParameter = () => false;
  const pending = f.run();
  f.first.program = undefined; f.second.program = {};
  await pending;
  assert.deepEqual(f.reflected, []);
  const error = coldColorFixture();
  error.renderer.compile = () => { throw Error("submission failed"); };
  await assert.rejects(error.run(), /submission failed/);
  assert.deepEqual(error.writes, [[error.target], [error.previous, 2, 3]]);
});

test("prepare nearby complete material bindings within budget, including mips and optional finish maps", async () => {
  const near = texture(4), far = texture(4), detail = texture(2), bound = texture(1);
  const nearMaterial = new THREE.MeshStandardMaterial({ map: near, roughnessMap: near, metalnessMap: bound });
  Object.assign(nearMaterial, { worldSpecFinishDetail: detail });
  const farMaterial = new THREE.MeshStandardMaterial({ map: far });
  const root = new THREE.Group(), geometry = new THREE.BoxGeometry(1, 1, 1);
  const add = (material: THREE.Material, x: number) => {
    const mesh = new THREE.Mesh(geometry, material);
    mesh.position.x = x;
    root.add(mesh);
    return mesh;
  };
  add(farMaterial, 50);
  add(nearMaterial, 100);
  add(nearMaterial, 3).visible = false;
  const resident = new Set([bound]), uploaded: THREE.Texture[] = [];
  const renderer = { properties: { has: (t: THREE.Texture) => resident.has(t) },
    initTexture(t: THREE.Texture) { resident.add(t); uploaded.push(t); } } as unknown as THREE.WebGLRenderer;
  const eye = new THREE.Vector3();
  for (const budget of [NaN, 0, -1])
    assert.deepEqual(await warmWorldTextures(renderer, root, eye, budget), { uploadedTextures: 0, estimatedUploadBytes: 0 });
  assert.deepEqual(await warmWorldTextures(renderer, root, eye, 64), { uploadedTextures: 0, estimatedUploadBytes: 0 });
  assert.deepEqual(await warmWorldTextures(renderer, root, eye, 104), { uploadedTextures: 2, estimatedUploadBytes: 104 });
  assert.deepEqual(uploaded, [near, detail]);
  assert.deepEqual(await warmWorldTextures(renderer, root, eye, 104), { uploadedTextures: 1, estimatedUploadBytes: 84 });
  assert.deepEqual(uploaded, [near, detail, far]);
  const manual = texture(4), wide = texture(4);
  manual.generateMipmaps = false;
  manual.mipmaps = [manual.image, texture(2).image, texture(1).image];
  wide.internalFormat = "RGBA16F";
  add(new THREE.MeshStandardMaterial({ map: manual, normalMap: wide }), 1);
  assert.deepEqual(await warmWorldTextures(renderer, root, eye, 1024), { uploadedTextures: 0, estimatedUploadBytes: 0 });
});

test("world preparation compiles against the active color pass, restores the host target before await and resolves new variants", async () => {
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(), map = new THREE.Group();
  scene.add(map);
  const host = new THREE.WebGLCubeRenderTarget(16), world = new THREE.WebGLRenderTarget(32, 16);
  const targets: unknown[][] = [];
  let linked = 0, release!: () => void;
  const programs: { getUniforms(): void }[] = [{ getUniforms() { throw Error("Already initialized program"); } }];
  const renderer = {
    getRenderTarget: () => host, getActiveCubeFace: () => 2, getActiveMipmapLevel: () => 3,
    setRenderTarget(...args: unknown[]) { targets.push(args); },
    compileAsync(targetScene: THREE.Scene, view: THREE.Camera) {
      assert.equal(targetScene, scene); assert.equal(view, camera);
      programs.push({ getUniforms() { linked++; } });
      return new Promise<void>(done => { release = done; });
    }, info: { programs },
  } as unknown as THREE.WebGLRenderer;
  const result = warmWorldColor(renderer, scene, camera, map, world, 0);
  assert.deepEqual(targets, [[world], [host, 2, 3]]);
  assert.equal(linked, 0);
  release();
  const stats = await result;
  assert.equal(linked, 1); assert.equal(stats.programsBefore, 1); assert.equal(stats.programsAfter, 2);
  Object.assign(renderer, { compileAsync() { throw Error("Failed to start compile"); } });
  await assert.rejects(() => warmWorldColor(renderer, scene, camera, map, null, 0), /Failed to start compile/);
  assert.deepEqual(targets.slice(-2), [[null], [host, 2, 3]]);
});

test("a compile-only mesh uses replacement geometry and preserves shader variants without copying instance buffers", () => {
  const source = new THREE.InstancedMesh(new THREE.BoxGeometry(), new THREE.MeshStandardMaterial(), 4);
  const geometry = new THREE.SphereGeometry(), material = new THREE.MeshStandardMaterial();
  source.castShadow = source.receiveShadow = true;
  source.setColorAt(0, new THREE.Color(0xcc9044));
  source.morphTexture = texture(2);
  const stage = warmupMesh(source, material, geometry);
  assert.ok(stage instanceof THREE.InstancedMesh);
  assert.equal(stage.geometry, geometry);
  assert.equal(stage.material, material);
  assert.equal(stage.instanceMatrix.count, 0);
  assert.equal(stage.instanceColor, source.instanceColor);
  assert.equal(stage.morphTexture, source.morphTexture);
  assert.equal(stage.castShadow, source.castShadow);
  assert.equal(stage.receiveShadow, source.receiveShadow);
  assert.notEqual(source.geometry, geometry);
  assert.notEqual(source.material, material);
});

test("warm camera-dependent glazing and exclusion variants without changing source meshes, layers or lighting", async () => {
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(), map = new THREE.Group();
  const sun = new THREE.DirectionalLight(), geometry = new THREE.BoxGeometry();
  const glass = new THREE.MeshStandardMaterial({ transparent: true, side: THREE.DoubleSide });
  glass.userData.worldSpecSurface = { recipe: "architectural-glazing" };
  enableSinglePassGlazing(glass);
  const panes = new THREE.InstancedMesh(geometry, glass, 4);
  const decal = new THREE.MeshStandardMaterial({ transparent: true, map: texture(4) });
  const ordinary = new THREE.MeshStandardMaterial(); registerOutlineDepth(ordinary);
  const sign = new THREE.Mesh(geometry, [decal, ordinary]);
  sign.userData.worldSpecOutline = false;
  scene.add(map, sun); map.add(panes, sign);
  assert.equal(markOutlineExclusions(map), true);
  const target = new THREE.WebGLRenderTarget(32, 32);
  const calls: string[] = [];
  const renderer = {
    getRenderTarget: () => null, getActiveCubeFace: () => 0, getActiveMipmapLevel: () => 0,
    setRenderTarget() {},
    async compileAsync(stage: THREE.Group, view: THREE.Camera, targetScene: THREE.Scene) {
      assert.equal(targetScene, scene);
      assert.equal(stage.children.length, 1);
      const receiver = stage.children[0] as THREE.Mesh;
      if (receiver instanceof THREE.InstancedMesh) {
        calls.push("glass");
        assert.equal(receiver.material, glass);
        assert.equal(glass.forceSinglePass, true);
        assert.equal(view, camera); assert.ok(view.layers.test(sun.layers));
      } else {
        calls.push("exclusions");
        assert.equal(view.layers.test(sun.layers), false);
        assert.ok(view.layers.test(sign.layers)); assert.ok(camera.layers.test(sun.layers));
        assert.ok(Array.isArray(receiver.material)); assert.equal(receiver.material[0], decal);
        assert.ok(receiver.material[1] instanceof THREE.MeshDepthMaterial);
      }
    },
  } as unknown as THREE.WebGLRenderer;
  await warmGlazingSides(renderer, scene, camera, map, target);
  await warmExcludedDepth(renderer, scene, camera, map, target, new Map());
  assert.deepEqual(calls, ["glass", "exclusions"]);
  assert.equal(glass.forceSinglePass, false);
  assert.deepEqual(sign.material, [decal, ordinary]);
  assert.equal(camera.layers.mask, 1);
  assert.deepEqual(map.children, [panes, sign]);
});

test("transcoded mip arrays are released after the first upload and never transferred again", async () => {
  const { releaseMipsAfterUpload, releaseSceneMipsAfterUpload } = await import("../../adapters/three/src/uploadedMips.ts");
  const levels = () => [4, 2, 1].map(size => ({ data: new Uint8Array(size * size), width: size, height: size }));
  const compressed = new THREE.CompressedTexture(levels(), 4, 4, THREE.RGBA_ASTC_4x4_Format);
  const view = compressed.clone();
  let hostUpdates = 0;
  compressed.onUpdate = () => { hostUpdates++; };
  const hostUpdate = compressed.onUpdate;
  const bitmap = new THREE.Texture({ width: 4, height: 4 });
  const scene = new THREE.Group();
  scene.add(new THREE.Mesh(new THREE.BufferGeometry(), new THREE.MeshStandardMaterial({ map: compressed, normalMap: bitmap })),
    new THREE.Mesh(new THREE.BufferGeometry(), [new THREE.MeshStandardMaterial({ map: view })]));

  assert.equal(releaseMipsAfterUpload(bitmap), false, "a browser image owns no mip arrays");
  assert.equal(releaseSceneMipsAfterUpload([scene]), 2);
  assert.deepEqual(compressed.mipmaps.map(mip => mip.data.byteLength), [16, 4, 1], "the first upload still has every level");
  assert.equal(compressed.source.dataReady, true);

  // WebGL calls onUpdate once the levels are stored.
  compressed.onUpdate!(compressed);
  assert.equal(hostUpdates, 1);
  assert.equal(compressed.onUpdate, hostUpdate, "the host's own callback is restored");
  assert.deepEqual(compressed.mipmaps.map(mip => [mip.data.byteLength, mip.width, mip.height]), [[0, 4, 4], [0, 2, 2], [0, 1, 1]]);
  assert.ok(compressed.mipmaps.every(mip => mip.data instanceof Uint8Array));
  assert.deepEqual(view.mipmaps.map(mip => mip.data.byteLength), [0, 0, 0], "a clone of the same source holds no second copy");
  assert.equal(view.source.dataReady, false, "a later sampler change allocates nothing and transfers no level");

  const first = new Uint8Array(64);
  const data = new THREE.DataTexture(first, 4, 4);
  data.mipmaps = [{ data: first, width: 4, height: 4 }, { data: new Uint8Array(16), width: 2, height: 2 }];
  assert.equal(releaseMipsAfterUpload(data), true);
  data.onUpdate!(data);
  assert.equal((data.image.data as Uint8Array).byteLength, 0, "the base level shared with the image is released too");
});

test("an unlimited budget stores each drawable mesh's textures once, in time slices, and stops when aborted", async t => {
  const root = new THREE.Group();
  const loaded = () => texture(2);
  const shared = loaded(), wide = loaded(), unloaded = new THREE.Texture(), target = new THREE.WebGLRenderTarget(2, 2);
  wide.internalFormat = "RGBA16F";
  const maps = [loaded(), loaded(), loaded(), loaded()], hiddenMap = loaded();
  const add = (material: THREE.Material, x: number) => {
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(), material);
    mesh.position.x = x; root.add(mesh); return mesh;
  };
  add(new THREE.MeshStandardMaterial({ map: maps[0], normalMap: shared, aoMap: unloaded, emissiveMap: target.texture }), 1);
  add(new THREE.MeshStandardMaterial({ map: maps[1], normalMap: shared, roughnessMap: wide }), 2);
  add(new THREE.MeshStandardMaterial({ map: maps[2], normalMap: maps[3] }), 3);
  add(new THREE.MeshStandardMaterial({ map: hiddenMap }), 0).visible = false;
  const drawable = [maps[0], shared, maps[1], wide, maps[2], maps[3]];

  // Each upload takes 5 ms of a clock that only uploads advance.
  let clock = 0;
  t.mock.method(performance, "now", () => clock);
  const waits: number[] = [];
  let duringWait: (() => void) | undefined;
  const timeout = globalThis.setTimeout;
  t.mock.method(globalThis, "setTimeout", (done: () => void) => timeout(() => { waits.push(uploaded.length); duringWait?.(); done(); }, 0));
  const resident = new Set<THREE.Texture>(), uploaded: THREE.Texture[] = [];
  const renderer = { properties: { has: (map: THREE.Texture) => resident.has(map) },
    initTexture(map: THREE.Texture) { assert.ok(!resident.has(map), "stored twice"); resident.add(map); uploaded.push(map); clock += 5; },
  } as unknown as THREE.WebGLRenderer;
  const eye = new THREE.Vector3(), draws = (mesh: THREE.Mesh) => mesh.visible;

  const abort = new AbortController(), reason = new Error("Map replaced");
  duringWait = () => abort.abort(reason);
  await assert.rejects(warmWorldTextures(renderer, root, eye, Infinity, { draws, signal: abort.signal }), reason);
  assert.deepEqual(uploaded, drawable.slice(0, 3), "three 5 ms uploads fill a 12 ms slice, and the abort lands in the wait after it");
  duringWait = undefined;

  const progress: number[][] = [];
  waits.length = 0;
  assert.deepEqual(await warmWorldTextures(renderer, root, eye, Infinity, { draws, sliceMs: 4, onProgress: (...step) => progress.push(step) }),
    { uploadedTextures: 3, estimatedUploadBytes: 2 * 20 }, "the half-float map is stored without a size estimate");
  assert.deepEqual(uploaded, drawable, "nearest material first, without the hidden mesh, the unloaded image or the render target");
  assert.deepEqual(waits, [4, 5], "a 4 ms slice waits before every upload after its first");
  assert.deepEqual(progress, [[0, 3], [1, 3], [2, 3], [3, 3]]);
  assert.deepEqual(await warmWorldTextures(renderer, root, eye, Infinity, { draws }), { uploadedTextures: 0, estimatedUploadBytes: 0 });
  assert.deepEqual(await warmWorldTextures(renderer, root, eye, Infinity), { uploadedTextures: 1, estimatedUploadBytes: 20 });
  assert.equal(uploaded.at(-1), hiddenMap, "without a filter the hidden mesh is prepared as before");
  target.dispose();
});
