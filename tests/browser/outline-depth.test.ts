import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { outlineDepthMaterial, outlineDepthLimit, registerOutlineDepth, markOutlineExclusions, renderColorAndExclusions, renderExcludedDepth } from "../../adapters/three/src/outlineExclusions.ts";

test("exclusion depth uses a second render state after resolved color, including a recursive reflection", () => {
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(), reflection = new THREE.PerspectiveCamera();
  const before = scene.onBeforeRender, after = scene.onAfterRender;
  let depth = 0, pass = "color";
  const events: string[] = [];
  const renderer = { render(root: THREE.Scene, view: THREE.Camera) {
    assert.equal(root, scene);
    depth++;
    Reflect.apply(root.onBeforeRender, root, [renderer, root, view, null]);
    const name = view === reflection ? "reflection" : pass;
    events.push(`draw ${name} ${depth}`);
    if (name === "color") renderer.render(root, reflection);
    events.push(`resolve ${name} ${depth}`);
    Reflect.apply(root.onAfterRender, root, [renderer, root, view]);
    depth--;
  } } as unknown as THREE.WebGLRenderer;
  renderColorAndExclusions(renderer, scene, camera, () => {
    assert.equal(scene.onBeforeRender, before);
    assert.equal(scene.onAfterRender, after);
    pass = "excluded";
    renderer.render(scene, camera);
  });
  assert.deepEqual(events, ["draw color 1", "draw reflection 2", "resolve reflection 2", "resolve color 1",
    "draw excluded 2", "resolve excluded 2"]);
});

test("host scene callbacks and renderers without completion hooks retain sequential exclusions", () => {
  for (const mode of ["custom-before", "custom-after", "no-hooks", "replaced"]) {
    const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(), events: string[] = [];
    const custom = () => { events.push("host callback"); };
    if (mode === "custom-before") scene.onBeforeRender = custom;
    if (mode === "custom-after") scene.onAfterRender = custom;
    const renderer = { render() {
      events.push("color start");
      if (mode === "replaced") scene.onAfterRender = custom;
      if (mode !== "no-hooks") {
        Reflect.apply(scene.onBeforeRender, scene, [renderer, scene, camera, null]);
        Reflect.apply(scene.onAfterRender, scene, [renderer, scene, camera]);
      }
      events.push("color end");
    } } as unknown as THREE.WebGLRenderer;
    renderColorAndExclusions(renderer, scene, camera, () => events.push("exclusions"));
    assert.deepEqual(events, mode === "no-hooks" ? ["color start", "color end", "exclusions"]
      : ["color start", "host callback", "color end", "exclusions"]);
    if (mode === "replaced") assert.equal(scene.onAfterRender, custom);
  }
});

test("color and exclusion failures release scene hooks and preserve the thrown error", () => {
  for (const failing of ["color", "excluded"]) {
    const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
    const before = scene.onBeforeRender, after = scene.onAfterRender, error = new Error(failing);
    let completedColor = false, attemptedDepth = false;
    const renderer = { render() {
      Reflect.apply(scene.onBeforeRender, scene, [renderer, scene, camera, null]);
      if (failing === "color") throw error;
      Reflect.apply(scene.onAfterRender, scene, [renderer, scene, camera]);
      completedColor = true;
    } } as unknown as THREE.WebGLRenderer;
    assert.throws(() => renderColorAndExclusions(renderer, scene, camera, () => { attemptedDepth = true; throw error; }),
      caught => caught === error);
    assert.equal(attemptedDepth, failing === "excluded");
    assert.equal(completedColor, failing === "excluded");
    assert.equal(scene.onBeforeRender, before);
    assert.equal(scene.onAfterRender, after);
  }
});

test("admitted opaque exclusion materials preserve raster state and reuse depth shaders", () => {
  const cache = new Map<THREE.Material, THREE.MeshDepthMaterial>();
  const surface = new THREE.MeshStandardMaterial({ side: THREE.DoubleSide, polygonOffset: true,
    polygonOffsetFactor: -2, polygonOffsetUnits: 3, depthFunc: THREE.LessDepth });
  assert.equal(outlineDepthMaterial(surface, cache), surface);
  registerOutlineDepth(surface);
  const depth = outlineDepthMaterial(surface, cache);
  assert.ok(depth instanceof THREE.MeshDepthMaterial);
  for (const field of ["side", "polygonOffset", "polygonOffsetFactor", "polygonOffsetUnits", "depthFunc"] as const)
    assert.equal(depth[field], surface[field]);
  assert.equal(depth.colorWrite, false);
  surface.visible = false;
  assert.equal(outlineDepthMaterial(surface, cache), depth);
  assert.equal(depth.visible, false);
  for (const material of cache.values()) material.dispose();
  surface.dispose();
});

test("cutout, transparent, displaced and custom vertex surfaces retain their materials", () => {
  const cache = new Map<THREE.Material, THREE.MeshDepthMaterial>();
  const cases = [
    new THREE.MeshStandardMaterial({ alphaTest: 0.5 }),
    new THREE.MeshStandardMaterial({ transparent: true }),
    new THREE.MeshStandardMaterial({ alphaHash: true }),
    new THREE.MeshStandardMaterial({ alphaToCoverage: true }),
    new THREE.MeshStandardMaterial({ displacementMap: new THREE.Texture() }),
    new THREE.MeshStandardMaterial(),
  ];
  for (const material of cases) registerOutlineDepth(material);
  cases.at(-1)!.onBeforeCompile = shader => { shader.vertexShader += "\n// custom deformation"; };
  for (const material of cases) assert.equal(outlineDepthMaterial(material, cache), material);
  assert.equal(cache.size, 0);
  for (const material of cases) { material.displacementMap?.dispose(); material.dispose(); }
});

test("exclusion render failures restore material identity, camera and shadow state", () => {
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  const surface = new THREE.MeshStandardMaterial();
  registerOutlineDepth(surface);
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(), surface);
  mesh.userData.worldSpecOutline = false;
  scene.add(mesh);
  markOutlineExclusions(scene);
  const cache = new Map<THREE.Material, THREE.MeshDepthMaterial>();
  const target = new THREE.WebGLRenderTarget(8, 8);
  const background = scene.background = new THREE.Color("red");
  const renderer = {
    shadowMap: { autoUpdate: true, needsUpdate: true },
    setRenderTarget() {}, clearDepth() {},
    render() {
      assert.ok(mesh.material instanceof THREE.MeshDepthMaterial);
      assert.equal(scene.background, null);
      throw new Error("interrupted render");
    },
  } as unknown as THREE.WebGLRenderer;
  const layers = camera.layers.mask;
  assert.throws(() => renderExcludedDepth(renderer, scene, camera, target, cache), /interrupted render/);
  assert.equal(mesh.material, surface);
  assert.equal(scene.background, background);
  assert.equal(camera.layers.mask, layers);
  assert.equal(renderer.shadowMap.autoUpdate, true);
  assert.equal(renderer.shadowMap.needsUpdate, true);
  for (const material of cache.values()) material.dispose();
  mesh.geometry.dispose(); surface.dispose(); target.dispose();
});

test("outline footprint culling keeps crossing bounds, near instances and unsupported shaders", () => {
  const camera = new THREE.PerspectiveCamera(90, 2, 0.1, 500);
  const limit = outlineDepthLimit(camera, 2000, 1000);
  assert.ok(Math.abs(limit - 25) < 1e-10, "a five-centimetre pixel at 25m cannot carry an outline");
  assert.ok(Math.abs(outlineDepthLimit(camera, 4000, 1000) - 50) < 1e-10,
    "a stretched viewport retains exclusions until even its narrower pixel spans five centimetres");
  const scene = new THREE.Scene(), geometry = new THREE.BoxGeometry(2, 2, 2);
  const material = new THREE.MeshStandardMaterial(), unknown = new THREE.MeshStandardMaterial();
  registerOutlineDepth(material);
  const far = new THREE.Mesh(geometry, material); far.position.z = -30;
  const crossing = new THREE.Mesh(geometry, material); crossing.position.z = -25;
  const custom = new THREE.Mesh(geometry, unknown); custom.position.z = -100;
  const instances = new THREE.InstancedMesh(geometry, material, 2);
  instances.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -100));
  instances.setMatrixAt(1, new THREE.Matrix4().makeTranslation(0, 0, -20));
  for (const mesh of [far, crossing, custom, instances]) { mesh.userData.worldSpecOutline = false; scene.add(mesh); }
  markOutlineExclusions(scene);
  scene.updateMatrixWorld(true); camera.updateMatrixWorld(true);
  const target = new THREE.WebGLRenderTarget(8, 8);
  const renderer = {
    shadowMap: { autoUpdate: true, needsUpdate: false }, setRenderTarget() {}, clearDepth() {},
    render() {
      assert.equal(far.visible, false);
      assert.equal(crossing.visible, true);
      assert.equal(custom.visible, true);
      assert.equal(instances.visible, true);
      throw Error("interrupted distance pass");
    },
  } as unknown as THREE.WebGLRenderer;
  assert.throws(() => renderExcludedDepth(renderer, scene, camera, target, undefined, limit), /interrupted distance pass/);
  assert.ok([far, crossing, custom, instances].every(mesh => mesh.visible));
  instances.dispose(); geometry.dispose(); material.dispose(); unknown.dispose(); target.dispose();
});
