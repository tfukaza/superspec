import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import { createRenderGltfLoader } from "../../adapters/three/src/gltfLoader.ts";
import { bindWorldSpecScene } from "../../adapters/three/src/bindScene.ts";
import { publishedCityGeometry } from "../../adapters/three/src/publishedGeometry.ts";
import { resolveRenderDiagnostics } from "../../adapters/three/src/renderProfile.ts";
import { prepareFirstColor, shadowDepthStage } from "../../adapters/three/src/worldWarmup.ts";
import { GlazingSides, glazingSideStage } from "../../adapters/three/src/glazing.ts";
import { tintedPartsGlb, type Batch, type Vec3 } from "./tinted-instances.fixture.ts";

/** Draws the synthetic tinted-part pack through the published path on the page's GPU and
 * reports the pixels at world points the test chooses. The test owns every expectation. */
const WIDTH = 960, HEIGHT = 640;

async function open(parts: Batch[], panes: Batch[], roles: boolean) {
  const canvas = document.createElement("canvas");
  document.getElementById("viewport")!.replaceChildren(canvas);
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: false, preserveDrawingBuffer: true });
  renderer.setPixelRatio(1);
  renderer.setSize(WIDTH, HEIGHT, false);
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFShadowMap;
  const gltf = await createRenderGltfLoader().parseAsync(tintedPartsGlb(parts, panes, { roles }), "") as GLTF;
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0, 0, 0);
  // Ambient light of pi returns a matte surface's own colour. The sun shines straight down -Z.
  const ambient = new THREE.AmbientLight(0xffffff, Math.PI), sun = new THREE.DirectionalLight(0xffffff, 0);
  sun.position.set(0, 0, 20);
  sun.target.position.set(0, 0, -10);
  sun.castShadow = true;
  sun.shadow.mapSize.set(2048, 2048);
  Object.assign(sun.shadow.camera, { left: -12, right: 12, top: 12, bottom: -12, near: 1, far: 60 });
  sun.shadow.camera.updateProjectionMatrix();
  // A matte wall one metre behind the parts receives their shadows. It is not part of the pack.
  const wall = new THREE.Mesh(new THREE.PlaneGeometry(60, 30), new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 1, metalness: 0 }));
  wall.position.z = -9;
  wall.receiveShadow = true;
  const camera = new THREE.PerspectiveCamera(60, WIDTH / HEIGHT, .1, 200);
  scene.add(ambient, sun, sun.target, wall, camera, gltf.scene);
  scene.updateMatrixWorld(true);
  const diagnostics = resolveRenderDiagnostics();
  const caster = new THREE.MeshDepthMaterial();
  await bindWorldSpecScene(gltf, { renderer, sun, caster, isCurrent: () => true, diagnostics });
  const geometry = await publishedCityGeometry(gltf, renderer, diagnostics);
  const sides = new GlazingSides();

  // First-frame preparation, as attachWorldSpecScene orders it.
  geometry.restoreCanonical();
  const shadows = shadowDepthStage(renderer, scene);
  geometry.prepareView(camera);
  const prepared = await prepareFirstColor(renderer, scene, camera, null, undefined, undefined,
    { variants: glazingSideStage(gltf.scene, sides), shadowCasters: shadows.stage });
  geometry.finishView();
  const programs = () => renderer.info.programs?.length ?? 0;
  const preparedPrograms = programs();
  const known = new Set(renderer.info.programs);
  /** The shader and vertex variant of each program linked since preparation, for a failure message. */
  const latePrograms = () => (renderer.info.programs ?? []).filter(program => !known.has(program)).map(program => program.name + " " + program.cacheKey.slice(0, 300));

  const gl = renderer.getContext();
  /** `castShadows` redraws the sun's shadow map first, as the first frame does. The sun does not
   * move, so later frames reuse the map, as the published world's shadow cache does. */
  const draw = (eye: Vec3, sunlight: number, points: Vec3[], castShadows: boolean) => {
    camera.position.set(...eye);
    camera.updateMatrixWorld(true);
    sun.intensity = sunlight;
    ambient.intensity = sunlight ? .2 * Math.PI : Math.PI;
    const tintedDepthDraws: number[] = [];
    const direct = renderer.renderBufferDirect.bind(renderer);
    renderer.renderBufferDirect = (view, fog, drawn, material, object, group) => {
      if (object instanceof THREE.InstancedMesh && object.instanceColor && (material as THREE.MeshDepthMaterial).isMeshDepthMaterial)
        tintedDepthDraws.push(object.count);
      return direct(view, fog, drawn, material, object, group);
    };
    try {
      if (castShadows) {
        // The sun's pass draws canonical casters culled to the light. Its colour is drawn over below.
        geometry.restoreCanonical();
        sun.shadow.updateMatrices(sun);
        geometry.shadowFrame(sun.shadow.camera, camera);
        renderer.shadowMap.autoUpdate = true;
        sides.render(renderer, () => renderer.render(scene, camera));
      }
      renderer.shadowMap.autoUpdate = false;
      renderer.shadowMap.needsUpdate = false;
      geometry.beginFrame(camera);
      sides.render(renderer, () => renderer.render(scene, camera));
    } finally { renderer.renderBufferDirect = direct; }
    const pixel = new Uint8Array(4), projected = new THREE.Vector3();
    const samples = points.map(point => {
      projected.set(...point).project(camera);
      const x = Math.floor((projected.x * .5 + .5) * WIDTH), y = Math.floor((projected.y * .5 + .5) * HEIGHT);
      if (Math.abs(projected.x) >= 1 || Math.abs(projected.y) >= 1 || Math.abs(projected.z) >= 1) return null;
      gl.readPixels(x, y, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
      return [pixel[0], pixel[1], pixel[2]];
    });
    // The instanced draws of the camera's selection: how many instances each submits and whether it is tinted.
    const batches: { count: number; tinted: boolean; source: boolean }[] = [];
    gltf.scene.traverseVisible(object => {
      if (object instanceof THREE.InstancedMesh) batches.push({ count: object.count, tinted: !!object.instanceColor, source: !!object.name && object.parent === gltf.scene });
    });
    geometry.finishView();
    return { samples, batches, tintedDepthDraws, programs: programs(), latePrograms: latePrograms() };
  };
  return { draw, prepared: prepared?.programs ?? null, preparedPrograms, gpu: (() => {
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    return info ? String(gl.getParameter(info.UNMASKED_RENDERER_WEBGL)) : "unknown";
  })() };
}

let session: Awaited<ReturnType<typeof open>> | undefined;
const driver = {
  async open(parts: Batch[], panes: Batch[], roles = true) {
    session = await open(parts, panes, roles);
    return { prepared: session.prepared, preparedPrograms: session.preparedPrograms, gpu: session.gpu };
  },
  draw: (eye: Vec3, sunlight: number, points: Vec3[], castShadows = false) => session!.draw(eye, sunlight, points, castShadows),
};
declare global { interface Window { driver?: typeof driver } }
window.driver = driver;
