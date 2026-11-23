import assert from "node:assert/strict";
import { test } from "node:test";
import * as THREE from "three";
import { prepareFirstColor, shadowDepthStage } from "../../adapters/three/src/worldWarmup.ts";
import { enableSinglePassGlazing, GlazingSides, glazingSideStage } from "../../adapters/three/src/glazing.ts";
import { recordingRenderer } from "./first-frame-programs.fixture.ts";

/** A sunlit street corner: an opaque wall, a cutout fence drawn from both sides, instanced
 * crates, a hidden distance variant, and a window pane facing the camera. */
function glazedStreet() {
  const scene = new THREE.Scene(), map = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, .1, 100);
  camera.position.set(0, 1, 6);
  const sun = new THREE.DirectionalLight(0xffffff, 2);
  sun.position.set(3, 8, 4);
  sun.castShadow = true;
  const texture = () => { const map = new THREE.DataTexture(new Uint8Array(4), 1, 1); map.needsUpdate = true; return map; };
  const caster = <T extends THREE.Mesh>(mesh: T, x: number) => { mesh.position.x = x; mesh.castShadow = mesh.receiveShadow = true; map.add(mesh); return mesh; };
  caster(new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshStandardMaterial({ map: texture() })), -2);
  caster(new THREE.Mesh(new THREE.PlaneGeometry(), new THREE.MeshStandardMaterial({ map: texture(), alphaTest: .5, side: THREE.DoubleSide })), -1);
  caster(new THREE.InstancedMesh(new THREE.BoxGeometry(.3, .3, .3), new THREE.MeshStandardMaterial(), 2), 1);
  const hidden = caster(new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshStandardMaterial({ alphaMap: texture(), alphaTest: .2 })), 2);
  hidden.visible = false;
  const glass = new THREE.MeshStandardMaterial({ transparent: true, opacity: .4, side: THREE.DoubleSide });
  glass.userData.worldSpecSurface = { recipe: "architectural-glazing" };
  enableSinglePassGlazing(glass, { stableSides: true });
  const pane = new THREE.Mesh(new THREE.PlaneGeometry(), glass);
  map.add(pane);
  scene.add(sun, map, camera);
  scene.updateMatrixWorld(true);
  return { scene, map, camera, hidden };
}

test("after first-frame preparation a glazed, shadowed scene's first render links no program", async () => {
  for (const prepareSubstitutes of [false, true]) {
    const { renderer, stats } = recordingRenderer();
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFShadowMap;
    const { scene, map, camera, hidden } = glazedStreet();
    const sides = new GlazingSides();
    const shadows = shadowDepthStage(renderer, scene);
    const prepared = await prepareFirstColor(renderer, scene, camera, null, undefined, undefined,
      prepareSubstitutes ? { variants: glazingSideStage(map, sides), shadowCasters: shadows.stage } : {});
    assert.ok(prepared && prepared.programs > 0);
    assert.equal(stats.linked, prepared.programs);
    assert.deepEqual(scene.children.length, 3, "compile-only meshes leave the scene");

    sides.render(renderer, () => renderer.render(scene, camera));
    const firstFrame = stats.linked - prepared.programs;
    if (prepareSubstitutes) assert.equal(firstFrame, 0, "the first frame found every program it used");
    // The colour submission alone misses the pane's single-pass side and each shadow depth variant.
    else assert.ok(firstFrame >= 4, `the unprepared control linked ${firstFrame} programs while drawing`);

    // A variant that was hidden during preparation was left out, and links when it first draws.
    hidden.visible = true;
    sides.render(renderer, () => renderer.render(scene, camera));
    if (prepareSubstitutes) assert.ok(stats.linked - prepared.programs > 0, "a hidden caster's depth program was not prepared");
    for (const material of shadows.materials) material.dispose();
    sides.dispose();
    renderer.dispose();
  }
});
