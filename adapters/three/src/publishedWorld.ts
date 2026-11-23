import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import { bindWorldSpecScene } from "./bindScene.ts";
import { ShopLights } from "./shopLights.ts";
import { WorldSpecRenderPass, type RenderLighting } from "./renderPass.ts";
import { markOutlineExclusions } from "./outlineExclusions.ts";
import { CityShadowCache } from "./cityShadowCache.ts";
import { installOpaqueProgramSort } from "./opaqueProgramSort.ts";
import { warmWorldTextures } from "./worldWarmup.ts";
import { loadDeclaredLods, type DeclaredLods } from "./declaredLods.ts";
import type { GraphicsSettings } from "./graphicsSettings.ts";
import { materialAnisotropyController } from "./materialFiltering.ts";
import { resolveRenderDiagnostics, type RenderDiagnostics } from "./renderProfile.ts";
import { BlockLods } from "./blockLods.ts";
import { publishedCityGeometry } from "./publishedGeometry.ts";
import { publishedSky } from "./publishedSky.ts";
import { releaseObjects } from "./releaseObjects.ts";
import type { WorldSpecLighting } from "./worldSpecLighting.ts";

type PublishedLighting = WorldSpecLighting & Pick<RenderLighting, "ambientOcclusion">;
type PlayableBounds = { minX: number; maxX: number; minZ: number; maxZ: number };

function worldEnvironment(gltf: GLTF): { bounds?: PlayableBounds } | undefined {
  const extras = gltf.scene.userData;
  return extras.worldSpecEnvironment?.version === 1 ? extras.worldSpecEnvironment : extras.klali;
}

/** Fit the sun to the playable footprint in the light parent's coordinate frame. */
export function fitPublishedSun(sun: THREE.DirectionalLight, gltf: GLTF, lighting: WorldSpecLighting, upAxis: "y" | "z" = "z") {
  const bounds = worldEnvironment(gltf)?.bounds;
  const light = lighting.lights.find(light => light.kind === "directional" && light.role === "sun");
  if (!bounds || !light?.castsShadow || ![bounds.minX, bounds.maxX, bounds.minZ, bounds.maxZ].every(Number.isFinite)) return false;
  // The exported volume's vertical span in game coordinates is -6 to 14 metres.
  const point = (x: number, y: number, z: number) => upAxis === "z" ? new THREE.Vector3(x, -z, y) : new THREE.Vector3(x, y, z);
  const centre = point((bounds.minX + bounds.maxX) / 2, 4, (bounds.minZ + bounds.maxZ) / 2);
  const [x, y, z] = light.direction;
  const direction = point(x, z, -y).normalize();
  const right = point(0, 1, 0).cross(direction).normalize();
  if (right.lengthSq() === 0) right.set(1, 0, 0);
  const up = direction.clone().cross(right);
  let reach = 0, depth = 0;
  for (const x of [bounds.minX, bounds.maxX]) for (const z of [bounds.minZ, bounds.maxZ]) for (const y of [-6, 14]) {
    const corner = point(x, y, z).sub(centre);
    reach = Math.max(reach, Math.abs(corner.dot(right)), Math.abs(corner.dot(up)));
    depth = Math.max(depth, Math.abs(corner.dot(direction)));
  }
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
  return true;
}

export interface WorldSpecSceneOptions {
  scene: THREE.Scene;
  renderer: THREE.WebGLRenderer;
  sun: THREE.DirectionalLight;
  lighting?: PublishedLighting;
  entryUrl?: URL;
  invalidate?: () => void;
  declared?: DeclaredLods;
  diagnostics?: RenderDiagnostics;
  graphics?: Pick<GraphicsSettings, "fixtureShadowMapSize" | "geometryLodBias" | "materialLodBias" | "materialAnisotropy">
    & Partial<Pick<GraphicsSettings, "lodBandMergeTriangles" | "glazingDistance">>;
  signal?: AbortSignal;
  /** Install the sky and fog described by the export. */
  installSky?: boolean;
  /** Allow shadow-casting fixtures to enable the renderer's global shadow maps. */
  enableFixtureShadows?: boolean;
  /** Redraw local shadows for actors while reusing the static world's depth. */
  dynamicCasters?: boolean;
  /** Group the renderer's opaque draws by shader program. Default false: where two
   * materials' surfaces coincide, the later draw wins, so a new order changes those pixels. */
  sortOpaqueByProgram?: boolean;
}

/** Bind the published world to a host-owned renderer, scene and sun. Attach the
 * glTF root directly to the scene first. A successful controller owns the glTF
 * resources and detached LODs; a rejected attachment leaves them with the host. */
export async function attachWorldSpecScene(gltf: GLTF, {
  scene, renderer, sun, entryUrl, invalidate, diagnostics = resolveRenderDiagnostics(),
  lighting, declared, graphics, signal, installSky = true, enableFixtureShadows = true, dynamicCasters = false,
  sortOpaqueByProgram = false,
}: WorldSpecSceneOptions) {
  const extras = gltf.scene.userData;
  if (extras.worldSpec !== true) return;
  if (gltf.scene.parent !== scene) throw Error("Attach the WorldSpec glTF root directly to its render scene first.");
  signal?.throwIfAborted();
  const record = lighting ?? (extras.worldSpecLighting?.version === 1 ? extras.worldSpecLighting as PublishedLighting : undefined);
  const stoa = gltf.scene.name === "Stoa";
  if (stoa && extras.worldSpecRender === undefined) {
    extras.worldSpecRender = { version: 1, effects: ["paintedShopSurfaces", "displayAntialiasing"], outlines: false };
  }
  const caster = new THREE.MeshDepthMaterial({ polygonOffset: true, polygonOffsetFactor: 2.5, polygonOffsetUnits: 4 });
  let blocks: BlockLods | undefined;
  let geometry: Awaited<ReturnType<typeof publishedCityGeometry>> | undefined;
  const declaredByHost = declared !== undefined;
  let transferred = false;
  const cleanup = [
    () => caster.dispose(),
    () => {
      if (geometry) geometry.dispose({ retainGltf: !transferred, retainDeclared: !transferred && declaredByHost });
      else {
        if (!declaredByHost) declared?.dispose();
        releaseObjects(blocks?.dispose() ?? [], [], gltf.scenes);
      }
    },
  ];
  const dispose = () => {
    const errors: unknown[] = [];
    while (cleanup.length) {
      try { cleanup.pop()!(); }
      catch (error) { errors.push(error); }
    }
    if (errors.length) throw new AggregateError(errors, "Could not release a WorldSpec scene.");
  };
  let hasOutlineExclusions = false;
  let applyAnisotropy: ReturnType<typeof materialAnisotropyController> | undefined;
  try {
    if (diagnostics.visibility.lod) {
      if (!declared && gltf.parser.json.extensionsUsed?.includes("MSFT_lod")) declared = await loadDeclaredLods(gltf);
      signal?.throwIfAborted();
      if (diagnostics.visibility.blockProxy) blocks = await BlockLods.load(gltf);
    }
    signal?.throwIfAborted();
    const bind = async () => {
      await bindWorldSpecScene(gltf, { renderer, sun, caster, isCurrent: () => !signal?.aborted, diagnostics, lighting: record,
        distantGlazingPixels: 32 });
      signal?.throwIfAborted();
      // Some authored outline exclusions belong to detached LOD variants.
      hasOutlineExclusions = markOutlineExclusions(gltf.scene);
      applyAnisotropy = materialAnisotropyController(gltf.scene);
    };
    if (declared) await declared.withVariants(bind); else await bind();
    geometry = await publishedCityGeometry(gltf, renderer, diagnostics, {
      entryUrl, invalidate, signal, declared, blocks, geometryLodBias: graphics?.geometryLodBias, materialLodBias: graphics?.materialLodBias,
      lodBandMergeTriangles: graphics?.lodBandMergeTriangles, glazingDistance: graphics?.glazingDistance,
    });
    const selected = geometry;
    signal?.throwIfAborted();
    if (gltf.scene.parent !== scene) throw Error("The WorldSpec glTF root left its render scene during attachment.");

    const { fixtures, shopLight, paintedSpots, shadowCache, shadowInstanceCull } = diagnostics.lighting;
    const fixtureSize = (size?: number) => size ? Math.min(size, renderer.capabilities.maxTextureSize) : undefined;
    const sky = installSky ? publishedSky(scene, worldEnvironment(gltf), record) : undefined;
    if (sky) cleanup.push(sky.dispose);
    const lights = new ShopLights(scene, fixtures ? extras.shopLightPositions : [],
      fixtures && stoa && shopLight !== "baseline", paintedSpots,
      { staticShadows: shadowCache && !dynamicCasters, stableSlots: shadowCache, shadowMapSize: fixtureSize(graphics?.fixtureShadowMapSize) });
    cleanup.push(() => lights.dispose());
    if (!diagnostics.lighting.shadows || graphics?.fixtureShadowMapSize === 0)
      for (const light of lights.shadowLights) light.castShadow = false;
    const { type, autoUpdate, enabled } = renderer.shadowMap;
    let enabledForFixtures = false;
    const enableShadows = () => {
      if (enableFixtureShadows && !renderer.shadowMap.enabled && lights.shadowLights.some(light => light.castShadow)) {
        renderer.shadowMap.enabled = true;
        enabledForFixtures = true;
      }
    };
    cleanup.push(() => {
      renderer.shadowMap.type = type;
      renderer.shadowMap.autoUpdate = autoUpdate;
      if (enabledForFixtures) renderer.shadowMap.enabled = enabled;
      sun.shadow.needsUpdate = true;
    });
    // Live sun updates retain actor shadows while reusing the static depth.
    renderer.shadowMap.type = THREE.PCFShadowMap;
    renderer.shadowMap.autoUpdate = true;
    enableShadows();
    sun.shadow.needsUpdate = true;
    // Shadow passes need canonical casters even when the camera culled them.
    let activeCamera: THREE.Camera | undefined;
    const shadows = new CityShadowCache(renderer, gltf.scene, sun, selected.restoreCanonical, lights.shadowLights,
      () => { if (activeCamera) selected.prepareView(activeCamera); },
      shadowInstanceCull ? selected.shadowFrame : undefined, shadowCache);
    cleanup.push(() => shadows.dispose());
    const worldPass = new WorldSpecRenderPass({ diagnostics });
    cleanup.push(() => worldPass.dispose());
    if (sortOpaqueByProgram) cleanup.push(installOpaqueProgramSort(renderer));
    const refreshSun = () => { sun.shadow.needsUpdate = true; lights.invalidateShadows(); };
    renderer.domElement.addEventListener("webglcontextrestored", refreshSun);
    cleanup.push(() => renderer.domElement.removeEventListener("webglcontextrestored", refreshSun));
    const updateLighting = (camera: THREE.Camera) => {
      lights.update(camera.position);
      sky?.update(camera);
    };
    const beginFrame = (camera: THREE.Camera) => {
      activeCamera = camera;
      selected.beginFrame(camera);
      updateLighting(camera);
    };
    const finishFrame = () => {
      try { shadows.finishPass(); shadows.releaseUnused(); }
      finally { activeCamera = undefined; selected.finishView(); }
    };
    let anisotropy: number | undefined;
    const updateAnisotropy = (samples?: number) => {
      if (anisotropy === samples) return;
      anisotropy = samples;
      if (applyAnisotropy) selected.updateAnisotropy(applyAnisotropy(samples, renderer.capabilities.getMaxAnisotropy()));
    };
    updateAnisotropy(graphics?.materialAnisotropy);

    transferred = true;
    return {
      async prepareFirstColor(camera: THREE.Camera, signal: AbortSignal) {
        // Shadow passes draw the canonical casters, so read them before the camera hides any.
        selected.restoreCanonical();
        worldPass.collectShadowCasters(renderer, scene);
        // Preparation uses the real view and lights without advancing the streaming queue.
        selected.prepareView(camera);
        try {
          updateLighting(camera);
          return await worldPass.prepareFirstColor(renderer, scene, camera, gltf.scene, record, signal);
        } finally { selected.finishView(); }
      },
      /** Store the textures of the meshes this camera's first frame draws, a few
       * milliseconds at a time, and compile nothing. Call before the first render so
       * that frame does not upload them all in one block. Draw nothing meanwhile. */
      prepareFirstTextures(camera: THREE.Camera, signal?: AbortSignal,
        onProgress?: (uploaded: number, total: number) => void) {
        // The camera's LOD, culling and declared variants decide which meshes draw.
        selected.prepareView(camera);
        try {
          camera.updateWorldMatrix(true, false);
          const frustum = new THREE.Frustum().setFromProjectionMatrix(
            new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse),
            camera.coordinateSystem, camera.reversedDepth);
          const draws = (mesh: THREE.Mesh) => {
            if (!mesh.layers.test(camera.layers) || (mesh.frustumCulled && !frustum.intersectsObject(mesh))) return false;
            for (let node: THREE.Object3D | null = mesh; node; node = node.parent) if (!node.visible) return false;
            return true;
          };
          // The meshes are read synchronously. Only the uploads wait.
          return warmWorldTextures(renderer, gltf.scene, camera.getWorldPosition(new THREE.Vector3()), Infinity,
            { draws, signal, onProgress });
        } finally { selected.finishView(); }
      },
      warmup(camera: THREE.Camera) {
        const prepare = () => worldPass.warmup(renderer, scene, camera, gltf.scene, diagnostics.loading.textureWarmupMiB * 1024 * 1024);
        return declared ? declared.withVariants(prepare) : prepare();
      },
      settle: selected.settle,
      textureStats: selected.textureStats,
      invalidateShadows: shadows.invalidate,
      setBlockProxyEnabled: selected.setBlockProxyEnabled,
      blockProxyStats: selected.blockProxyStats,
      setSubmission: selected.setSubmission,
      hasSky: sky !== undefined,
      setGraphics(settings: GraphicsSettings, fixtureShadows = diagnostics.lighting.shadows) {
        if (!cleanup.length) return;
        selected.setLodBias(settings.geometryLodBias, settings.materialLodBias);
        selected.setDrawBudgets(settings.lodBandMergeTriangles, settings.glazingDistance);
        updateAnisotropy(settings.materialAnisotropy);
        if (lights.setShadowSettings(fixtureShadows && settings.fixtureShadowMapSize !== 0,
          fixtureSize(settings.fixtureShadowMapSize))) shadows.invalidate();
        enableShadows();
      },
      beginFrame,
      finishFrame,
      render(camera: THREE.Camera) {
        try {
          beginFrame(camera);
          worldPass.render(renderer, scene, camera, sun, gltf.scene, hasOutlineExclusions, record);
        } finally { finishFrame(); }
      },
      dispose,
    };
  } catch (error) {
    try { dispose(); }
    catch (cleanupError) { throw new AggregateError([error, cleanupError], "Could not attach a WorldSpec scene."); }
    throw error;
  }
}

export type WorldSpecScene = NonNullable<Awaited<ReturnType<typeof attachWorldSpecScene>>>;
