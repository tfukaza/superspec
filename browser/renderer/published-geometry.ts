import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { CityVisibility, mergeCityInstances, type CityInstanceGroups } from "../../adapters/three/src/cityVisibility.ts";
import { loadCityLods } from "../../adapters/three/src/cityLods.ts";
import { clusterCityGeometry } from "../../adapters/three/src/cityClusters.ts";
import { CityFarHardware } from "../../adapters/three/src/cityFarHardware.ts";
import { DistantGlazingDraws } from "../../adapters/three/src/distantGlazing.ts";
import { CityVegetationLod } from "../../adapters/three/src/cityVegetationLod.ts";
import { CityTextureResidency } from "../../adapters/three/src/cityTextureResidency.ts";
import { cityTextureBindings, disposeCityTextureLod, hasCityTextureBindings } from "../../adapters/three/src/cityTextureLoader.ts";
import type { DeclaredLods } from "../../adapters/three/src/declaredLods.ts";
import type { BlockLods } from "../../adapters/three/src/blockLods.ts";
import type { RenderDiagnostics } from "../../adapters/three/src/renderProfile.ts";
import { releaseObjects } from "./dispose-objects.ts";

type CityGeometryRenderer = Pick<THREE.WebGLRenderer, "initTexture" | "domElement">;

export function streamsPublishedTextures({ visibility, materials }: RenderDiagnostics): boolean {
  return visibility.cityCulling && visibility.detail === "default" && materials.textureLod;
}

export function publishedTextureResidency(gltf: GLTF, renderer: CityGeometryRenderer, invalidate?: () => void, declared?: DeclaredLods) {
  if (!hasCityTextureBindings(gltf)) return;
  const movingReceivers = declared?.textureReceivers() ?? new Set<THREE.Mesh>();
  if (hasMovableCityGeometry(gltf.scene))
    gltf.scene.traverse(object => { if (object instanceof THREE.Mesh) movingReceivers.add(object); });
  return new CityTextureResidency(gltf.scene, material => cityTextureBindings(gltf, material), invalidate, 12,
    texture => renderer.initTexture(texture), movingReceivers);
}

function hasMovableCityGeometry(root: THREE.Object3D): boolean {
  return root.userData.modernIndustrialCableDrag === 1;
}

/** Camera selection for published WorldSpec geometry, independent of the rendering backend.
 * The controller owns the GLTF scene's geometry, material, texture and loaded LOD resources.
 * Restore canonical geometry for shadow passes, then reapply the view before its queued draws. */
export async function publishedCityGeometry(gltf: GLTF, renderer: CityGeometryRenderer, diagnostics: RenderDiagnostics,
  { entryUrl, declared, blocks, invalidate, geometryLodBias = 1, materialLodBias = 1 }: {
    entryUrl?: URL; declared?: DeclaredLods; blocks?: BlockLods; invalidate?: () => void;
    geometryLodBias?: number; materialLodBias?: number;
  }) {
  const { visibility: features, materials } = diagnostics;
  blocks?.keepCanonicalShadows();
  const culling = features.cityCulling && !hasMovableCityGeometry(gltf.scene);
  const lods = culling && entryUrl && features.lod
    ? await loadCityLods(gltf, entryUrl) : new Map();
  const residency = features.cityCulling && materials.textureLod
    ? publishedTextureResidency(gltf, renderer, invalidate, declared) : undefined;
  const instanceGroups: CityInstanceGroups | undefined = blocks ? new WeakMap() : undefined;
  if (culling && features.merge) mergeCityInstances(gltf.scene, blocks?.groupFor, instanceGroups);
  const distantDetail = features.detail === "default";
  const farHardware = culling && distantDetail && features.hardwareGroups
    ? new CityFarHardware() : undefined;
  const clusterPairs = culling && features.clusters ? clusterCityGeometry(gltf.scene, lods, farHardware, instanceGroups) : undefined;
  const detail = features.detail === "full" ? {} : features.detail === "balanced"
    ? { disconnectedErrorPixels: 4, distantDetailErrorPixels: 10, distantArchitectureErrorPixels: 3 }
    : { disconnectedErrorPixels: 8, distantDetailErrorPixels: 28, distantArchitectureErrorPixels: 6,
      detailCull: { maxComponentPixels: 28, maxCoveragePixels: 4096, minDistance: 30 } };
  const visibility = culling ? new CityVisibility(gltf.scene, features.instanceCulling, lods,
    { ...detail, clusterPairs, farHardware, groupFor: blocks?.groupFor, instanceGroups, coalesceClusters: features.clusterDraws !== "off",
      wideClusters: features.clusterDraws === "wide",
      occlusionSize: features.occlusionGrid === "coarse" ? [80, 48] : [160, 96] }) : undefined;
  const glazing = visibility && distantDetail && features.glazingGroups
    ? new DistantGlazingDraws(gltf.scene) : undefined;
  const vegetation = visibility && distantDetail && features.vegetationLod
    ? new CityVegetationLod(gltf.scene, lods) : undefined;
  const restoreViewOverrides = () => { declared?.restore(); glazing?.restore(); vegetation?.restore(); blocks?.restore(); };
  const prepareView = (camera: THREE.Camera) => {
    restoreViewOverrides();
    const height = renderer.domElement.height, geometryHeight = height * geometryLodBias;
    if (blocks?.select(camera, geometryHeight)) visibility?.setHiddenGroups(blocks.hidden);
    visibility?.frame(camera, height, geometryLodBias);
    if (visibility && camera instanceof THREE.PerspectiveCamera) blocks?.cull(visibility.occludesBounds);
    vegetation?.frame(camera, geometryHeight);
    glazing?.frame(camera, height, renderer.domElement.width);
    declared?.frame(camera, geometryHeight, height * materialLodBias);
  };
  let disposed = false;
  return {
    beginFrame(camera: THREE.Camera) {
      blocks?.beginFrame();
      prepareView(camera);
      residency?.frame(camera, renderer.domElement.height * materialLodBias);
    },
    finishView: () => vegetation?.restore(),
    restoreCanonical() { restoreViewOverrides(); visibility?.restore(); },
    prepareView,
    shadowFrame: (shadowCamera: THREE.Camera, viewCamera?: THREE.Camera) => visibility?.shadowFrame(shadowCamera, viewCamera),
    settle: () => residency?.settle(),
    textureStats: () => residency?.stats(),
    setBlockProxyEnabled: (enabled: boolean) => blocks?.setEnabled(enabled),
    blockProxyStats: () => blocks?.stats(),
    setSubmission: (mode: "ordinary" | "clusters") => visibility?.setSubmission(mode),
    dispose() {
      if (disposed) return;
      disposed = true;
      residency?.dispose();
      declared?.dispose();
      disposeCityTextureLod(gltf, { texturesReleased: !!residency });
      glazing?.dispose();
      vegetation?.dispose();
      visibility?.dispose();
      const proxyRoots = blocks?.dispose() ?? [];
      const geometries = new Set<THREE.BufferGeometry>();
      for (const [original, levels] of lods) { geometries.add(original); for (const geometry of levels) geometries.add(geometry); }
      releaseObjects([...gltf.scenes, ...proxyRoots], geometries);
    },
  };
}
