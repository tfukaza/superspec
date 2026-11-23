import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import { CityVisibility, mergeCityInstances, type CityInstanceGroups } from "./cityVisibility.ts";
import { loadCityLods } from "./cityLods.ts";
import { clusterCityGeometry } from "./cityClusters.ts";
import { CityFarHardware } from "./cityFarHardware.ts";
import { DistantGlazingDraws } from "./distantGlazing.ts";
import { GlazingRange } from "./glazingRange.ts";
import { CityVegetationLod } from "./cityVegetationLod.ts";
import { CityTextureResidency } from "./cityTextureResidency.ts";
import { cityTextureBindings, disposeCityTextureLod, hasCityTextureBindings } from "./cityTextureLoader.ts";
import type { DeclaredLods } from "./declaredLods.ts";
import type { BlockLods } from "./blockLods.ts";
import type { RenderDiagnostics } from "./renderProfile.ts";
import { releaseObjects } from "./releaseObjects.ts";

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
  { entryUrl, declared, blocks, invalidate, signal, geometryLodBias = 1, materialLodBias = 1, lodBandMergeTriangles, glazingDistance }: {
    entryUrl?: URL; declared?: DeclaredLods; blocks?: BlockLods; invalidate?: () => void; signal?: AbortSignal;
    geometryLodBias?: number; materialLodBias?: number;
    /** Extra triangles allowed per removed LOD band draw. Default CITY_LOD_MERGE_TRIANGLES. */
    lodBandMergeTriangles?: number;
    /** Metres beyond which window glass is left out. Default: all glass is drawn. */
    glazingDistance?: number;
  } = {}) {
  signal?.throwIfAborted();
  const { visibility: features, materials } = diagnostics;
  const culling = features.cityCulling && !hasMovableCityGeometry(gltf.scene);
  const lods = culling && entryUrl && features.lod
    ? await loadCityLods(gltf, entryUrl, signal) : new Map<THREE.BufferGeometry, readonly THREE.BufferGeometry[]>();
  if (signal?.aborted) {
    for (const [original, levels] of lods) for (const geometry of levels) if (geometry !== original) geometry.dispose();
    signal.throwIfAborted();
  }
  blocks?.keepCanonicalShadows();
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
      wideClusters: features.clusterDraws === "wide", lodMergeTriangles: lodBandMergeTriangles,
      occlusionSize: features.occlusionGrid === "coarse" ? [80, 48] : [160, 96] }) : undefined;
  const glazing = visibility && distantDetail && features.glazingGroups
    ? new DistantGlazingDraws(gltf.scene) : undefined;
  const vegetation = visibility && distantDetail && features.vegetationLod
    ? new CityVegetationLod(gltf.scene, lods) : undefined;
  const glassRange = visibility ? new GlazingRange(gltf.scene, glazingDistance ?? Infinity) : undefined;
  const restoreViewOverrides = () => { declared?.restore(); glazing?.restore(); glassRange?.restore(); vegetation?.restore(); blocks?.restore(); };
  const prepareView = (camera: THREE.Camera) => {
    restoreViewOverrides();
    const height = renderer.domElement.height, geometryHeight = height * geometryLodBias;
    if (blocks?.select(camera, geometryHeight)) visibility?.setHiddenGroups(blocks.hidden);
    visibility?.frame(camera, height, geometryLodBias);
    if (visibility && camera instanceof THREE.PerspectiveCamera) blocks?.cull(visibility.occludesBounds);
    vegetation?.frame(camera, geometryHeight, geometryLodBias);
    // Glass left out by range is hidden before the distant batches choose among what remains.
    glassRange?.frame(camera);
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
    setLodBias(geometry: number, material: number) {
      geometryLodBias = geometry;
      materialLodBias = material;
    },
    /** Draw-count budgets from the graphics settings. Undefined restores each default. */
    setDrawBudgets(mergeTriangles?: number, glassDistance?: number) {
      visibility?.setLodMergeTriangles(mergeTriangles);
      if (glassRange) glassRange.distance = glassDistance ?? Infinity;
    },
    updateAnisotropy: (updated: ReadonlySet<THREE.Texture>) => residency?.updateAnisotropy(updated),
    setBlockProxyEnabled: (enabled: boolean) => blocks?.setEnabled(enabled),
    blockProxyStats: () => blocks?.stats(),
    setSubmission: (mode: "ordinary" | "clusters") => visibility?.setSubmission(mode),
    dispose({ retainGltf = false, retainDeclared = false }: { retainGltf?: boolean; retainDeclared?: boolean } = {}) {
      if (disposed) return;
      disposed = true;
      residency?.dispose();
      if (!retainDeclared) declared?.dispose();
      if (!retainGltf) disposeCityTextureLod(gltf, { texturesReleased: !!residency });
      glazing?.dispose();
      glassRange?.dispose();
      vegetation?.dispose();
      visibility?.dispose();
      const proxyRoots = blocks?.dispose() ?? [];
      const geometries = new Set<THREE.BufferGeometry>();
      for (const [original, levels] of lods) {
        if (!retainGltf) geometries.add(original);
        for (const geometry of levels) if (!retainGltf || geometry !== original) geometries.add(geometry);
      }
      if (retainGltf) releaseObjects(proxyRoots, geometries, gltf.scenes);
      else {
        gltf.scene.removeFromParent();
        releaseObjects([...gltf.scenes, ...proxyRoots], geometries);
      }
    },
  };
}
