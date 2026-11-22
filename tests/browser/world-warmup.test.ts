import assert from "node:assert/strict";
import { test } from "node:test";
import * as THREE from "three";
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
  for (const budget of [NaN, Infinity, -1])
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
