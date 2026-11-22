import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { enableSinglePassGlazing, GlazingSides, warmGlazingSides } from "../../adapters/three/src/glazing.ts";

// WebGLRenderer passes null for a mesh without material groups.
const ungrouped = null!;

function glazing(options: { distantPixels?: number } = {}) {
  const material = new THREE.MeshStandardMaterial({ transparent: true, side: THREE.DoubleSide });
  material.userData.worldSpecSurface = { recipe: "architectural-glazing" };
  enableSinglePassGlazing(material, options);
  return material;
}
function draw(material: THREE.MeshStandardMaterial, mesh: THREE.Mesh, camera: THREE.Camera, renderer = {} as THREE.WebGLRenderer) {
  mesh.updateMatrixWorld(true); camera.updateMatrixWorld(true);
  material.onBeforeRender(renderer, new THREE.Scene(), camera, mesh.geometry, mesh, ungrouped);
}

test("front-facing panes skip only the empty back pass and keep back normal-map rendering", () => {
  const material = glazing(), geometry = new THREE.PlaneGeometry(), mesh = new THREE.Mesh(geometry, material);
  const camera = new THREE.PerspectiveCamera(); camera.position.z = 5;
  draw(material, mesh, camera);
  assert.equal(material.forceSinglePass, true);
  const version = material.version;
  draw(material, mesh, camera);
  assert.equal(material.version, version, "stable views reuse their program");
  camera.position.z = -5;
  draw(material, mesh, camera);
  assert.equal(material.forceSinglePass, false);
  assert.ok(material.version > version);
  camera.position.z = 0;
  draw(material, mesh, camera);
  assert.equal(material.forceSinglePass, false, "edge-on classification stays conservative");
  geometry.dispose(); material.dispose();
});

test("opposite winding and instances on opposite camera sides retain two-pass ordering", () => {
  const material = glazing(), geometry = new THREE.PlaneGeometry();
  geometry.setIndex([0, 2, 1, 2, 3, 1, 1, 2, 0, 1, 3, 2]);
  const mesh = new THREE.Mesh(geometry, material), camera = new THREE.PerspectiveCamera(); camera.position.z = 5;
  draw(material, mesh, camera);
  assert.equal(material.forceSinglePass, false);
  const plane = new THREE.PlaneGeometry(), instances = new THREE.InstancedMesh(plane, material, 2);
  instances.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, 0));
  instances.setMatrixAt(1, new THREE.Matrix4().makeTranslation(0, 0, 10));
  draw(material, instances, camera);
  assert.equal(material.forceSinglePass, false);
  instances.count = 1;
  draw(material, instances, camera);
  assert.equal(material.forceSinglePass, true, "visibility subsets consider only their selected placements");
  instances.dispose(); geometry.dispose(); plane.dispose(); material.dispose();
});

test("updated geometry and displacement invalidate single-pass admission", () => {
  const material = glazing(), geometry = new THREE.PlaneGeometry(), mesh = new THREE.Mesh(geometry, material);
  const camera = new THREE.PerspectiveCamera(); camera.position.z = 5;
  draw(material, mesh, camera);
  assert.equal(material.forceSinglePass, true);
  geometry.setIndex([1, 2, 0, 1, 3, 2]);
  draw(material, mesh, camera);
  assert.equal(material.forceSinglePass, false);
  geometry.setIndex([0, 2, 1, 2, 3, 1]);
  material.displacementMap = new THREE.Texture();
  draw(material, mesh, camera);
  assert.equal(material.forceSinglePass, false);
  material.displacementMap.dispose(); geometry.dispose(); material.dispose();
});

test("far approximation respects nearest distance, zoom, pane extent and physical viewport size", () => {
  const material = glazing({ distantPixels: 32 }), geometry = new THREE.PlaneGeometry();
  const mesh = new THREE.Mesh(geometry, material), camera = new THREE.PerspectiveCamera(90, 1920 / 1080, 0.1, 1000);
  mesh.rotation.y = Math.PI;
  mesh.position.z = -40;
  let scale = 1;
  const renderer = { getCurrentViewport: (viewport: THREE.Vector4) => viewport.set(0, 0, 1920 * scale, 1080 * scale) } as THREE.WebGLRenderer;
  draw(material, mesh, camera, renderer);
  assert.equal(material.forceSinglePass, true, "small distant back-facing pane can use the optional approximation");
  mesh.position.z = -10;
  draw(material, mesh, camera, renderer);
  assert.equal(material.forceSinglePass, false);
  mesh.position.z = -40; camera.zoom = 4; camera.updateProjectionMatrix();
  draw(material, mesh, camera, renderer);
  assert.equal(material.forceSinglePass, false);
  camera.zoom = 1; camera.updateProjectionMatrix(); scale = 4;
  draw(material, mesh, camera, renderer);
  assert.equal(material.forceSinglePass, false);
  scale = 1; mesh.scale.setScalar(4);
  draw(material, mesh, camera, renderer);
  assert.equal(material.forceSinglePass, false, "large glass areas retain their full blend ordering");
  geometry.dispose(); material.dispose();
});

test("a near placement prevents distant approximation for the entire instanced draw", () => {
  const material = glazing({ distantPixels: 32 }), geometry = new THREE.PlaneGeometry();
  geometry.rotateY(Math.PI);
  const mesh = new THREE.InstancedMesh(geometry, material, 2), camera = new THREE.PerspectiveCamera(90, 1920 / 1080, 0.1, 1000);
  const renderer = { getCurrentViewport: (viewport: THREE.Vector4) => viewport.set(0, 0, 1920, 1080) } as THREE.WebGLRenderer;
  mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -40));
  mesh.setMatrixAt(1, new THREE.Matrix4().makeTranslation(0, 0, -10));
  draw(material, mesh, camera, renderer);
  assert.equal(material.forceSinglePass, false);
  mesh.count = 1;
  draw(material, mesh, camera, renderer);
  assert.equal(material.forceSinglePass, true);
  mesh.dispose(); geometry.dispose(); material.dispose();
});

// Reproduce WebGLRenderer's transparent render-item dispatch; keep a snapshot
// of the concrete material and side that the driver sees at each submission.
function renderItem(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, mesh: THREE.Mesh) {
  mesh.updateMatrixWorld(true); camera.updateMatrixWorld(true);
  const material = mesh.material as THREE.Material;
  material.onBeforeRender(renderer, scene, camera, mesh.geometry, mesh, ungrouped);
  const submit = () => renderer.renderBufferDirect(camera, scene, mesh.geometry, material, mesh, ungrouped);
  if (material.transparent && material.side === THREE.DoubleSide && !material.forceSinglePass) {
    material.side = THREE.BackSide; material.needsUpdate = true; submit();
    material.side = THREE.FrontSide; material.needsUpdate = true; submit();
    material.side = THREE.DoubleSide;
  } else submit();
}

function captureRenderer(renderBufferDirect: THREE.WebGLRenderer["renderBufferDirect"]): THREE.WebGLRenderer {
  return { renderBufferDirect } as THREE.WebGLRenderer;
}

test("stable sides keep the sorted object slot, live material state and native back-face shading", () => {
  const glass = glazing(), geometry = new THREE.PlaneGeometry(), other = new THREE.MeshBasicMaterial({ transparent: true });
  const front = new THREE.Mesh(geometry, glass), back = new THREE.Mesh(geometry, glass), decal = new THREE.Mesh(geometry, other);
  back.rotation.y = Math.PI;
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(), sides = new GlazingSides();
  camera.position.z = 5;
  const draws: { material: THREE.Material; side: THREE.Side; object: THREE.Object3D }[] = [];
  const renderer = captureRenderer((_camera, _scene, _geometry, material, object) => {
    draws.push({ material, side: material.side, object });
    assert.equal(glass.side, THREE.DoubleSide);
    assert.equal(glass.forceSinglePass, false);
  });
  const original = renderer.renderBufferDirect, version = glass.version;
  function frame() {
    for (const mesh of [back, decal, front]) renderItem(renderer, scene, camera, mesh);
  }
  sides.render(renderer, frame);
  assert.equal(renderer.renderBufferDirect, original);
  assert.deepEqual(draws.map(draw => draw.side), [THREE.BackSide, THREE.FrontSide, THREE.FrontSide, THREE.DoubleSide]);
  assert.deepEqual(draws.map(draw => draw.object), [back, back, decal, front]);
  assert.equal(draws[2].material, other);
  assert.equal(glass.version, version, "rendering alternate sides leaves source shader revisions intact");
  const views = [draws[0].material, draws[1].material, draws[3].material];
  assert.equal(new Set([glass, ...views].map(material => material.uuid)).size, 4);
  glass.normalMap = new THREE.Texture(); glass.opacity = .42; glass.needsUpdate = true;
  draws.length = 0;
  sides.render(renderer, frame);
  for (const [index, view] of views.entries()) {
    assert.equal(draws[[0, 1, 3][index]].material, view);
    assert.ok(view instanceof THREE.MeshStandardMaterial);
    assert.equal(view.normalMap, glass.normalMap); assert.equal(view.opacity, .42); assert.equal(view.version, glass.version);
  }
  let released = 0;
  for (const view of views) view.addEventListener("dispose", () => released++);
  glass.dispose(); sides.dispose(); assert.equal(released, 3);
  glass.normalMap.dispose(); geometry.dispose(); other.dispose();
});

test("nested glazing scopes and exceptions restore the host renderer and material", () => {
  const glass = glazing(), geometry = new THREE.PlaneGeometry(), mesh = new THREE.Mesh(geometry, glass);
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(), sides = new GlazingSides(), nested = new GlazingSides();
  camera.position.z = -5;
  const draws: THREE.Side[] = [];
  const renderer = captureRenderer((_camera, _scene, _geometry, material) => { draws.push(material.side); });
  const original = renderer.renderBufferDirect;
  sides.render(renderer, () => {
    const outer = renderer.renderBufferDirect;
    nested.render(renderer, () => renderItem(renderer, scene, camera, mesh));
    assert.equal(renderer.renderBufferDirect, outer);
    renderItem(renderer, scene, camera, mesh);
  });
  assert.deepEqual(draws, [THREE.BackSide, THREE.FrontSide, THREE.BackSide, THREE.FrontSide]);
  assert.throws(() => sides.render(renderer, () => {
    draw(glass, mesh, camera, renderer);
    assert.equal(glass.forceSinglePass, true);
    throw new Error("interrupted after classification");
  }), /interrupted/);
  assert.equal(glass.forceSinglePass, false); assert.equal(renderer.renderBufferDirect, original);
  renderer.renderBufferDirect = () => { throw new Error("interrupted at submission"); };
  const failing = renderer.renderBufferDirect;
  assert.throws(() => sides.render(renderer, () => renderItem(renderer, scene, camera, mesh)), /at submission/);
  assert.equal(glass.forceSinglePass, false); assert.equal(renderer.renderBufferDirect, failing);
  let released = 0;
  for (const view of sides.views(glass)) view.addEventListener("dispose", () => released++);
  sides.dispose(); nested.dispose(); glass.dispose(); assert.equal(released, 3); geometry.dispose();
});

test("custom render and shader hooks retain original material identity", () => {
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(), geometry = new THREE.PlaneGeometry(), sides = new GlazingSides();
  camera.position.z = -5;
  const customize: ((glass: THREE.Material) => void)[] = [
    glass => { glass.onBeforeRender = glass.onBeforeRender.bind(glass); },
    glass => { glass.onBeforeCompile = glass.onBeforeCompile.bind(glass); },
    glass => { glass.customProgramCacheKey = glass.customProgramCacheKey.bind(glass); },
  ];
  for (const install of customize) {
    const glass = glazing(), mesh = new THREE.Mesh(geometry, glass);
    install(glass);
    const draws: THREE.Side[] = [];
    const renderer = captureRenderer((_camera, _scene, _geometry, material) => {
      assert.equal(material, glass); draws.push(material.side);
    });
    sides.render(renderer, () => renderItem(renderer, scene, camera, mesh));
    assert.deepEqual(draws, [THREE.BackSide, THREE.FrontSide]); glass.dispose();
  }
  sides.dispose(); geometry.dispose();
});

test("loading prepares the actual side materials with their instanced and ordinary geometry", async () => {
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(), map = new THREE.Group();
  const glass = glazing(), geometry = new THREE.PlaneGeometry(), sides = new GlazingSides();
  const ordinary = new THREE.Mesh(geometry, glass), instanced = new THREE.InstancedMesh(geometry, glass, 2);
  map.add(ordinary, instanced); scene.add(map);
  const renderer = {
    getRenderTarget: () => null, getActiveCubeFace: () => 0, getActiveMipmapLevel: () => 0, setRenderTarget() {},
    async compileAsync(stage: THREE.Group, view: THREE.Camera, targetScene: THREE.Scene) {
      assert.equal(targetScene, scene); assert.equal(view, camera);
      assert.deepEqual(stage.children.map(mesh => mesh instanceof THREE.InstancedMesh), [false, true]);
      for (const mesh of stage.children as THREE.Mesh[]) {
        assert.equal(mesh.geometry, geometry); assert.equal(mesh.material, sides.views(glass));
        assert.deepEqual(mesh.material.map(material => material.side), [THREE.FrontSide, THREE.BackSide, THREE.DoubleSide]);
        assert.ok(mesh.material.every(material => material.forceSinglePass));
      }
    },
  } as unknown as THREE.WebGLRenderer;
  await warmGlazingSides(renderer, scene, camera, map, null, sides);
  assert.equal(glass.forceSinglePass, false); assert.equal(ordinary.material, glass); assert.equal(instanced.material, glass);
  sides.dispose(); glass.dispose(); instanced.dispose(); geometry.dispose();
});
