import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import { prepareRadialTangents } from "./radialTangent";
import { styleWorldShadows, type WorldSpecLighting } from "./worldSpecLighting";
import { applyConcreteDetail, type ConcreteDetail } from "./concreteDetail";
import { applyBounce, probeTexture, type BounceRecord } from "./worldSpecBounce";
import { installPaintedLawns, stylePaintedStroke } from "./paintedLawn";
import { createReflectionField, applyReflectionField } from "./reflectionField";
import { applyEdgeFinish } from "./edgeFinish";
import { applyWorldSpecMaterial } from "./worldSpecMaterials";
import { registerOutlineDepth } from "./outlineExclusions";
import { writeOutlineSpecular } from "./overlapOutline";
import { enableSinglePassGlazing } from "./glazing";
import { narrowDataTextureChannels } from "./textureChannels";
import { legacyRenderDiagnostics, resolveRenderDiagnostics, type RenderDiagnostics } from "./renderProfile";

/** Bind optional WorldSpec and MaterialSpec extras to an already loaded glTF scene. */
export async function bindWorldSpecScene(gltf: GLTF, options: {
  renderer?: THREE.WebGLRenderer;
  sun: THREE.DirectionalLight;
  caster: THREE.MeshDepthMaterial;
  isCurrent: () => boolean;
  query?: URLSearchParams;
  diagnostics?: RenderDiagnostics;
  lighting?: WorldSpecLighting;
  distantGlazingPixels?: number;
  distantHardwareBounce?: boolean;
}): Promise<boolean> {
  const parsed = gltf.scene;
  const render = parsed.userData.worldSpecRender;
  const maskSpecular = render?.version === 1 && render.effects?.includes("specularOutlineMask");
  const renderer = options.renderer;
  const { materials: settings, lighting: lightingSettings } = options.diagnostics ??
    (options.query ? resolveRenderDiagnostics(undefined, options.query) : legacyRenderDiagnostics());
  const detail = parsed.userData.worldSpecConcreteDetail as ConcreteDetail | undefined;
  let grain: THREE.Texture | undefined;
  if (settings.grain && detail?.version === 1 && Number.isInteger(detail.texture) && detail.metersPerTile > 0
      && Number.isFinite(detail.mean) && Number.isFinite(detail.strength)) {
    try {
      grain = await gltf.parser.getDependency("texture", detail.texture) as THREE.Texture;
      grain.anisotropy = Math.min(8, renderer?.capabilities.getMaxAnisotropy() ?? 1);
    } catch (error) {
      console.warn("Using the baked concrete atlas:", error);
    }
    if (!options.isCurrent()) {
      grain?.dispose(); return false;
    }
  }
  const bounce = parsed.userData.worldSpecBounce as BounceRecord | undefined;
  let probes: THREE.Data3DTexture | undefined;
  if (bounce?.version === 1 && settings.bounce !== "off") {
    try {
      const buffer = await gltf.parser.getDependency("bufferView",bounce.bufferView);
      probes = probeTexture(bounce,buffer);
    } catch (error) {
      console.warn("WorldSpec indirect lighting unavailable",error);
    }
    if (!options.isCurrent()) {
      probes?.dispose(); grain?.dispose(); return false;
    }
  }
  const lighting: WorldSpecLighting | undefined = options.lighting ??
    (parsed.userData.worldSpecLighting?.version === 1 ? parsed.userData.worldSpecLighting : undefined);
  const sun = lighting?.lights.find(light => light.kind === "directional" && light.role === "sun");
  if (sun?.shadow?.style === "illustration" && parsed.userData.paintedLawn) {
    try { await installPaintedLawns(gltf); }
    catch (error) { console.warn("Using static painted lawn", error); }
    if (!options.isCurrent()) {
      probes?.dispose(); grain?.dispose(); return false;
    }
  }
  const detailMaps = new Map<number,THREE.Texture>();
  if (parsed.userData.worldSpecFinishes) {
    const slots = new Set<number>();
    parsed.traverse(obj => {
      if (!(obj instanceof THREE.Mesh)) return;
      for (const m of Array.isArray(obj.material) ? obj.material : [obj.material]) {
        const slot = m.userData.worldSpecSurface?.detailTexture;
        if (Number.isInteger(slot)) slots.add(slot);
      }
    });
    await Promise.all([...slots].map(async slot => {
      try { detailMaps.set(slot,await gltf.parser.getDependency("texture",slot)); }
      catch(error) { console.warn("Missing painted finish texture",slot,error); }
    }));
    if (!options.isCurrent()) {
      for (const tex of detailMaps.values()) tex.dispose();
      probes?.dispose(); grain?.dispose(); return false;
    }
  }
  const field = settings.reflectionProbes
    ? createReflectionField(parsed.userData.worldSpecReflectionProbes) : undefined;
  const library = parsed.userData.worldSpecFinishes;
  const styled = new Set<THREE.MeshStandardMaterial>();
  const channelMaterials = new Set<THREE.Material>();
  parsed.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return;
    const materials = Array.isArray(object.material) ? object.material : [object.material];
    object.castShadow = materials.some(material => material.userData.castsShadow !== false);
    object.receiveShadow = true;
    if (materials.some(material => material.userData.materialSpec?.live?.some(
      (operation: { id: string; arguments?: { radial?: { value?: number } } }) =>
        operation.id === "virtualSky" && (operation.arguments?.radial?.value ?? 0) > 0))) {
      prepareRadialTangents(object.geometry);
    }
    for (const material of materials) {
      if (material instanceof THREE.MeshStandardMaterial && !styled.has(material)) {
        const ordinaryGeometry = material.onBeforeCompile === THREE.Material.prototype.onBeforeCompile;
        const ordinaryChannels = ordinaryGeometry && material.onBeforeRender === THREE.Material.prototype.onBeforeRender;
        const stableGlazing = ordinaryChannels && material.customProgramCacheKey === THREE.Material.prototype.customProgramCacheKey;
        if (sun?.shadow?.style === "illustration") {
          const paintCamera = parsed.userData.worldSpecRender?.version === 1 && lightingSettings.paintedShadows ? options.sun.shadow.camera : undefined;
          styleWorldShadows(material, sun.shadow, paintCamera);
        }
        const surface = material.userData.worldSpecSurface;
        const finish = library?.finishes?.[surface?.finish];
        const legacyBench = material.name === "builtin:kitchen-worktop" && settings.worktopReflection !== "default";
        const local = field && !legacyBench && (surface?.reflection === "box" || surface?.reflection === "masonry");
        applyWorldSpecMaterial(material, sun?.shadow, renderer?.capabilities?.getMaxAnisotropy?.(), finish, !!local, settings);
        const wear = library?.wear?.[surface?.wear];
        const finishDetail=detailMaps.get(surface?.detailTexture);
        if (finishDetail) Object.assign(material,{worldSpecFinishDetail:finishDetail});
        if (wear && (finish || material.userData.materialSpecEdge)) applyEdgeFinish(material,finish,wear,finishDetail);
        if (field && local) {
          applyReflectionField(material,field,finish?.reflection ?? .3,!finish);
          Object.assign(material,{ worldSpecProbeTexture:field.texture });
        }
        if (grain && detail && (material.userData.worldSpecConcrete === true || material.userData.worldSpecAggregate === true)) {
          for (const texture of [material.map, material.normalMap])
            if (texture) texture.anisotropy = Math.max(texture.anisotropy, grain.anisotropy);
          applyConcreteDetail(material, grain, detail);
        }
        if (probes && bounce) applyBounce(material, probes, bounce, settings.bounce === "painted",
          { distantHardware: options.distantHardwareBounce ?? settings.hardwareBounce === "coarse" });
        if (material.userData.paintedStroke !== undefined) stylePaintedStroke(material, material.userData.paintedStroke);
        if (maskSpecular) writeOutlineSpecular(material);
        if (ordinaryGeometry) registerOutlineDepth(material);
        if (ordinaryChannels) channelMaterials.add(material);
        if (ordinaryGeometry && settings.singlePassGlazing !== "off")
          enableSinglePassGlazing(material, {
            distantPixels: settings.singlePassGlazing === "exact" ? 0 : options.distantGlazingPixels,
            stableSides: stableGlazing,
          });
        styled.add(material);
      }
    }
    // Use the material cutout when casting lettering and perforated detail.
    if (!materials.some(m => m.alphaTest > 0 || m.transparent)) object.customDepthMaterial = options.caster;
  });
  if (renderer?.capabilities.isWebGL2 && settings.textureChannels === "narrow") narrowDataTextureChannels(gltf, channelMaterials);
  return true;
}
