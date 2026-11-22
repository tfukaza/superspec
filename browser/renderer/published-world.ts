import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { bindWorldSpecScene } from "../../adapters/three/src/bindScene.ts";
import { ShopLights } from "../../adapters/three/src/shopLights.ts";
import { WorldSpecRenderPass } from "../../adapters/three/src/renderPass.ts";
import { markOutlineExclusions } from "../../adapters/three/src/outlineExclusions.ts";
import { CityShadowCache } from "../../adapters/three/src/cityShadowCache.ts";
import type { DeclaredLods } from "../../adapters/three/src/declaredLods.ts";
import type { GraphicsSettings } from "../../adapters/three/src/graphicsSettings.ts";
import type { RenderDiagnostics } from "../../adapters/three/src/renderProfile.ts";
import { BlockLods } from "../../adapters/three/src/blockLods.ts";
import { publishedCityGeometry } from "./published-geometry.ts";
import { publishedSky } from "./published-sky.ts";
import { sunOf } from "./lighting.ts";
import type { GameLighting } from "./game-lighting.ts";

export { publishedTextureResidency } from "./published-geometry.ts";

/** Klali fits the sun to the playable footprint. Far scenery otherwise consumes shadow resolution. */
function fitMarketSun(host: GameLighting, bounds?: { minX: number; maxX: number; minZ: number; maxZ: number }) {
  const light = sunOf(host.lighting);
  if (!bounds || !light?.castsShadow || ![bounds.minX, bounds.maxX, bounds.minZ, bounds.maxZ].every(Number.isFinite)) return;
  // Work in the light's Z-up parent. Game Y spans -6 to 14 metres in the playable volume.
  const centre = new THREE.Vector3((bounds.minX + bounds.maxX) / 2, -(bounds.minZ + bounds.maxZ) / 2, 4);
  const direction = new THREE.Vector3(...light.direction).normalize();
  const right = new THREE.Vector3(0, 0, 1).cross(direction).normalize();
  if (right.lengthSq() === 0) right.set(1, 0, 0);
  const up = direction.clone().cross(right);
  let reach = 0, depth = 0;
  for (const x of [bounds.minX, bounds.maxX]) for (const z of [bounds.minZ, bounds.maxZ]) for (const y of [-6, 14]) {
    const corner = new THREE.Vector3(x, -z, y).sub(centre);
    reach = Math.max(reach, Math.abs(corner.dot(right)), Math.abs(corner.dot(up)));
    depth = Math.max(depth, Math.abs(corner.dot(direction)));
  }
  const sun = host.view.sun;
  // Three orients the shadow camera in world coordinates.
  sun.shadow.camera.up.set(0, 1, 0);
  sun.target.position.copy(centre);
  sun.position.copy(centre).addScaledVector(direction, 200);
  sun.target.updateWorldMatrix(true, false);
  sun.updateWorldMatrix(true, false);
  const camera = sun.shadow.camera;
  Object.assign(camera, { left: -reach, right: reach, bottom: -reach, top: reach, near: 198 - depth, far: 202 + depth });
  camera.updateProjectionMatrix();
  sun.shadow.bias = -0.05 / (camera.far - camera.near);
  sun.shadow.normalBias = 0;
  sun.shadow.radius = 0.85;
}

/** Restore delivery records that GLTFLoader preserves as extras on a WorldSpec scene. */
export async function publishedWorld(gltf: GLTF, scene: THREE.Scene, renderer: THREE.WebGLRenderer, diagnostics: RenderDiagnostics, host: GameLighting,
  entryUrl?: URL, invalidate?: () => void, declared?: DeclaredLods,
  graphics?: Pick<GraphicsSettings, "fixtureShadowMapSize" | "geometryLodBias" | "materialLodBias">) {
  const extras = gltf.scene.userData;
  if (extras.worldSpec !== true) return;
  const environment = extras.worldSpecEnvironment?.version === 1 ? extras.worldSpecEnvironment : extras.klali;
  const stoa = gltf.scene.name === "Stoa";
  if (stoa && extras.worldSpecRender === undefined) {
    extras.worldSpecRender = { version: 1, effects: ["paintedShopSurfaces", "displayAntialiasing"], outlines: false };
  }
  const sun = host.view.sun;
  fitMarketSun(host, environment?.bounds);
  sun.shadow.autoUpdate = false;
  sun.shadow.needsUpdate = true;
  // The sun can reuse its static map while local lights track the active fixtures.
  renderer.shadowMap.type = THREE.PCFShadowMap;
  renderer.shadowMap.autoUpdate = true;
  const caster = new THREE.MeshDepthMaterial({ polygonOffset: true, polygonOffsetFactor: 2.5, polygonOffsetUnits: 4 });
  const blocks = diagnostics.visibility.blockProxy && diagnostics.visibility.lod ? await BlockLods.load(gltf) : undefined;
  let hasOutlineExclusions = false;
  const bind = async () => {
    await bindWorldSpecScene(gltf, { renderer, sun, caster, isCurrent: () => true, diagnostics, lighting: host.lighting,
      distantGlazingPixels: 32 });
    // Declared node variants are temporarily attached here so their authored
    // outline exclusions survive when that level becomes visible later.
    hasOutlineExclusions = markOutlineExclusions(gltf.scene);
  };
  if (declared) await declared.withVariants(bind); else await bind();
  const geometry = await publishedCityGeometry(gltf, renderer, diagnostics, {
    entryUrl, invalidate, declared, blocks, geometryLodBias: graphics?.geometryLodBias, materialLodBias: graphics?.materialLodBias,
  });
  const sky = publishedSky(scene, environment, host.lighting);
  const { fixtures, shopLight, paintedSpots, shadowCache, shadowInstanceCull } = diagnostics.lighting;
  const shadowMapSize = graphics?.fixtureShadowMapSize === undefined ? undefined
    : Math.min(graphics.fixtureShadowMapSize, renderer.capabilities.maxTextureSize);
  const lights = new ShopLights(scene, fixtures ? extras.shopLightPositions : [],
    fixtures && stoa && shopLight !== "baseline", paintedSpots,
    { staticShadows: shadowCache, stableSlots: true, shadowMapSize });
  if (!diagnostics.lighting.shadows) for (const light of lights.shadowLights) light.castShadow = false;
  renderer.shadowMap.enabled ||= lights.shadowLights.some(light => light.castShadow);
  // Restore complete casters inside the shadow pass, after the camera draw list
  // has been collected. Off-camera geometry still contributes full-detail shadows.
  let activeCamera: THREE.Camera | undefined;
  const shadows = new CityShadowCache(renderer, gltf.scene, sun, geometry.restoreCanonical, lights.shadowLights,
    () => { if (activeCamera) geometry.prepareView(activeCamera); },
    shadowInstanceCull ? geometry.shadowFrame : undefined);
  const worldPass = new WorldSpecRenderPass({ diagnostics });
  const refreshSun = () => { sun.shadow.needsUpdate = true; lights.invalidateShadows(); };
  renderer.domElement.addEventListener("webglcontextrestored", refreshSun);
  const updateLighting = (camera: THREE.Camera) => {
    lights.update(camera.position);
    sky?.update(camera);
  };
  return {
    async prepareFirstColor(camera: THREE.Camera, signal: AbortSignal) {
      // View selection supplies the real geometry and light set. Streaming and
      // frame counters advance only when the viewer subsequently draws.
      geometry.prepareView(camera);
      try {
        updateLighting(camera);
        return await worldPass.prepareFirstColor(renderer, scene, camera, gltf.scene, host.lighting, signal);
      } finally { geometry.finishView(); }
    },
    warmup(camera: THREE.Camera) {
      const prepare = () => worldPass.warmup(renderer, scene, camera, gltf.scene, diagnostics.loading.textureWarmupMiB * 1024 * 1024);
      return declared ? declared.withVariants(prepare) : prepare();
    },
    settle: geometry.settle,
    textureStats: geometry.textureStats,
    invalidateShadows: shadows.invalidate,
    setBlockProxyEnabled: geometry.setBlockProxyEnabled,
    blockProxyStats: geometry.blockProxyStats,
    setSubmission: geometry.setSubmission,
    render(camera: THREE.Camera) {
      activeCamera = camera;
      geometry.beginFrame(camera);
      updateLighting(camera);
      try { worldPass.render(renderer, scene, camera, sun, gltf.scene, hasOutlineExclusions, host.lighting); }
      finally {
        try { shadows.finishPass(); }
        finally { geometry.finishView(); }
      }
    },
    dispose() {
      renderer.domElement.removeEventListener("webglcontextrestored", refreshSun);
      shadows.dispose();
      worldPass.dispose();
      lights.dispose();
      sky?.dispose();
      caster.dispose();
      sun.shadow.dispose();
      geometry.dispose();
    },
  };
}
