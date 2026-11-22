import * as THREE from "three";
import { WebGPUBackend, WebGPURenderer } from "three/webgpu";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import { BlockLods } from "../adapters/three/src/blockLods.ts";
import { disposeCityTextureLod, registerCityTextureLod } from "../adapters/three/src/cityTextureLoader.ts";
import { loadDeclaredLods, type DeclaredLods } from "../adapters/three/src/declaredLods.ts";
import { lightingOverridesQuery, type ResolvedRenderProfile } from "../adapters/three/src/renderProfile.ts";
import { applyMaterialAnisotropy } from "../adapters/three/src/materialFiltering.ts";
import { createGltfLoader, disposeKtx2Loader } from "../browser/renderer/ktx2.ts";
import { releaseObjects } from "../browser/renderer/dispose-objects.ts";
import { publishedCityGeometry, publishedTextureResidency, streamsPublishedTextures } from "../browser/renderer/published-geometry.ts";
import { parseView, viewCamera } from "../browser/renderer/view.ts";
import { webGpuLighting, webGpuSceneProfile } from "../browser/renderer/webgpu-lighting.ts";
import { installConnectorDrag } from "./demos/connector-drag.ts";

export class WebGpuFallbackError extends Error {}
export class WebGpuUnavailable extends WebGpuFallbackError {}
export class UnsupportedWebGpuScene extends WebGpuFallbackError {}

/** Inspect a published glTF through a genuine WebGPU device. Only explicitly portable views omit declared effects. */
export async function openWebGpuGltf(url: URL, host: HTMLElement, query: URLSearchParams, graphics: ResolvedRenderProfile,
  options: { auto: boolean; onLost: (reason: string) => void }) {
  const { settings, diagnostics } = graphics;
  const view = parseView(query);
  if (!("gpu" in navigator) || !navigator.gpu) throw new WebGpuUnavailable("WebGPU is unavailable in this browser or browsing context.");
  let renderer: WebGPURenderer;
  try { renderer = new WebGPURenderer({ antialias: true }); }
  catch (cause) { throw new WebGpuUnavailable("Could not create a WebGPU renderer.", { cause }); }
  const scene = new THREE.Scene();
  const camera = viewCamera(view);
  const yUp = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), -Math.PI / 2);
  camera.up.applyQuaternion(yUp);
  camera.position.applyQuaternion(yUp);
  let controls: OrbitControls | undefined;
  let gltf: GLTF | undefined, declared: DeclaredLods | undefined, blocks: BlockLods | undefined;
  let geometry: Awaited<ReturnType<typeof publishedCityGeometry>> | undefined;
  let residency: ReturnType<typeof publishedTextureResidency>;
  let lighting: ReturnType<typeof webGpuLighting> | undefined;
  let observer: ResizeObserver | undefined, stopDrag: (() => void) | undefined;
  let disposed = false, ready = false, lostReason: string | undefined;
  let redrawId = 0;
  const probeWindow = window as Window & { __worldSpecView?: { renderer: unknown } };
  function dispose() {
    if (disposed) return;
    disposed = true;
    window.removeEventListener("pagehide", dispose);
    cancelAnimationFrame(redrawId); observer?.disconnect(); stopDrag?.();
    controls?.removeEventListener("change", render); controls?.dispose();
    if (probeWindow.__worldSpecView?.renderer === renderer) delete probeWindow.__worldSpecView;
    lighting?.dispose();
    if (geometry) geometry.dispose();
    else {
      residency?.dispose();
      declared?.dispose();
      if (gltf) {
        disposeCityTextureLod(gltf, { texturesReleased: !!residency });
        releaseObjects([...gltf.scenes, ...(blocks?.dispose() ?? [])]);
      }
    }
    disposeKtx2Loader(renderer);
    renderer.domElement.remove();
    // Three's dispose() re-enters init via setAnimationLoop(null) on an uninitialized renderer.
    if (renderer.initialized) void renderer.dispose().catch(error => console.warn("Could not finish releasing the WebGPU renderer.", error));
  }
  function render() {
    if (disposed || lostReason) return;
    camera.updateMatrixWorld();
    if (geometry) geometry.beginFrame(camera);
    else { applyCameraLods(); residency?.frame(camera, renderer.domElement.height * settings.materialLodBias); }
    lighting?.update(camera.position);
    try { renderer.render(scene, camera); }
    finally { geometry?.finishView(); }
  }
  function applyCameraLods() {
    const height = renderer.domElement.height;
    declared?.frame(camera, height * settings.geometryLodBias, height * settings.materialLodBias);
  }
  function invalidate() {
    if (!ready || redrawId || disposed || diagnostics.instrumentation.gpuProbe) return;
    redrawId = requestAnimationFrame(() => { redrawId = 0; render(); });
  }
  function reportFailure(reason: string) {
    if (disposed || lostReason) return;
    lostReason = reason;
    if (ready) { dispose(); options.onLost(lostReason); }
  }
  const originalLoss = renderer.onDeviceLost.bind(renderer);
  renderer.onDeviceLost = info => {
    if (disposed || lostReason) return;
    originalLoss(info);
    reportFailure(info.message || info.reason || "The WebGPU device was lost.");
  };
  const originalError = renderer.onError.bind(renderer);
  renderer.onError = event => {
    if (disposed || lostReason) return;
    originalError(event);
    // r186 passes an object at runtime; its type declaration still describes a string.
    const detail = event as unknown as { message?: string; type?: string } | string;
    const message = typeof detail === "string" ? detail : detail.message || detail.type || "GPU validation failed";
    reportFailure("WebGPU rendering failed: " + message.split("\n")[0].slice(0, 180));
  };
  function checkDevice() {
    if (lostReason) throw new WebGpuUnavailable(lostReason);
  }
  try {
    try { await renderer.init(); }
    catch (cause) { throw new WebGpuUnavailable("The browser could not initialize a WebGPU device.", { cause }); }
    if (!(renderer.backend instanceof WebGPUBackend)) throw new WebGpuUnavailable("The browser selected a WebGL fallback device.");
    checkDevice();
    // Three handles browser/device resets, but deliberately suppresses explicit device.destroy().
    // The latter is also a real loss when something other than this preview owns the teardown.
    const backend = renderer.backend as WebGPUBackend & { device: {
      lost: Promise<{ reason: string; message: string }>; queue: { onSubmittedWorkDone(): Promise<void> };
    } };
    void backend.device.lost.then(info => {
      if (info.reason === "destroyed" && !disposed)
        renderer.onDeviceLost({ api: "WebGPU", reason: info.reason, message: info.message || "The WebGPU device was destroyed.", originalEvent: info });
    });
    const loader = createGltfLoader(renderer);
    const lightingQuery = lightingOverridesQuery(diagnostics);
    let profile: ReturnType<typeof webGpuSceneProfile> | undefined;
    loader.register(parser => ({ name: "SuperSpecWebGPUProfile", beforeRoot: async () => {
      profile = webGpuSceneProfile(parser.json, lightingQuery, diagnostics);
      if (profile.missing && options.auto) throw new UnsupportedWebGpuScene(`WebGPU does not yet implement ${profile.missing}.`);
    } }));
    const streamTextures = streamsPublishedTextures(diagnostics);
    if (streamTextures) registerCityTextureLod(loader);
    gltf = await loader.loadAsync(url.href);
    checkDevice();
    if (!profile) throw Error("The glTF loader did not inspect the published scene.");
    scene.add(gltf.scene);
    const bounds = new THREE.Box3().setFromObject(gltf.scene);
    controls = new OrbitControls(camera, renderer.domElement);
    controls.target.fromArray(view.target).applyQuaternion(yUp);
    if (!view.fixed && !bounds.isEmpty()) {
      bounds.getCenter(controls.target);
      camera.position.copy(controls.target).addScaledVector(new THREE.Vector3(.8, .9, 1.2), Math.max(1, bounds.getSize(new THREE.Vector3()).length()));
    }
    controls.update();
    const model = gltf.scene;
    const isWorld = model.userData.worldSpec === true;
    if (diagnostics.visibility.lod && gltf.parser.json.extensionsUsed?.includes("MSFT_lod")) declared = await loadDeclaredLods(gltf);
    if (isWorld && diagnostics.visibility.lod && diagnostics.visibility.blockProxy) blocks = await BlockLods.load(gltf);
    try { lighting = webGpuLighting(renderer, scene, model, profile.lighting, profile.hasRecord, bounds, diagnostics, settings); }
    catch (cause) { throw new WebGpuUnavailable("WebGPU could not initialize scene lighting.", { cause }); }
    const anisotropy = settings.materialAnisotropy;
    const dress = () => {
      model.traverse(object => {
        if (!(object instanceof THREE.Mesh)) return;
        const materials = Array.isArray(object.material) ? object.material : [object.material];
        object.castShadow = materials.some(material => material.userData.castsShadow !== false);
        object.receiveShadow = true;
      });
      if (anisotropy !== undefined) applyMaterialAnisotropy(model, anisotropy, renderer.getMaxAnisotropy());
    };
    if (declared) await declared.withVariants(dress); else await dress();
    if (isWorld) geometry = await publishedCityGeometry(gltf, renderer, diagnostics, { entryUrl: url, declared, blocks, invalidate,
      geometryLodBias: settings.geometryLodBias, materialLodBias: settings.materialLodBias });
    else if (streamTextures) residency = publishedTextureResidency(gltf, renderer, invalidate, declared);
    // Three's WebGPU shadow node recursively renders this scene with the light's camera.
    // Its color draw list has already been collected. Restore the full casters for that pass.
    scene.onBeforeRender = (_renderer, _scene, passCamera) => {
      if (!lighting?.shadowCameras.has(passCamera)) return;
      if (geometry) {
        geometry.restoreCanonical();
        if (diagnostics.lighting.shadowInstanceCull) geometry.shadowFrame(passCamera, camera);
      } else declared?.restore();
    };
    scene.onAfterRender = (_renderer, _scene, passCamera) => {
      if (!lighting?.shadowCameras.has(passCamera)) return;
      if (geometry) geometry.prepareView(camera); else applyCameraLods();
    };
    const resize = () => {
      if (disposed) return;
      const { clientWidth: width, clientHeight: height } = host;
      if (!width || !height) return;
      renderer.setSize(width, height);
      if (camera instanceof THREE.PerspectiveCamera) camera.aspect = width / height;
      else { camera.right = view.height * width / height / 2; camera.left = -camera.right; }
      camera.updateProjectionMatrix(); render();
    };
    checkDevice();
    host.append(renderer.domElement);
    if (settings.renderScale !== 1) renderer.setPixelRatio(settings.renderScale);
    try {
      resize();
      await backend.device.queue.onSubmittedWorkDone();
    } catch (cause) { throw new WebGpuUnavailable("WebGPU could not render the scene's first frame.", { cause }); }
    checkDevice();
    stopDrag = installConnectorDrag(model, camera, renderer.domElement, controls, () => { lighting?.invalidate(); render(); });
    controls.addEventListener("change", render);
    observer = new ResizeObserver(resize); observer.observe(host);
    window.addEventListener("pagehide", dispose, { once: true });
    ready = true;
    if (diagnostics.instrumentation.gpuProbe) Object.assign(probeWindow, { __worldSpecView: {
      backend: "webgpu", renderer, scene, camera, controls, render, portable: !!profile.missing,
      graphics: { preset: graphics.preset, ...settings },
      settle: async () => { await (geometry?.settle() ?? residency?.settle()); await backend.device.queue.onSubmittedWorkDone(); },
      textureStats: geometry?.textureStats ?? (() => residency?.stats()), declaredLods: declared && (() => declared?.stats()),
      setSubmission: geometry?.setSubmission, setBlockProxyEnabled: geometry?.setBlockProxyEnabled, blockProxyStats: geometry?.blockProxyStats,
    } });
    const target = controls.target;
    const share = () => {
      const zUp = yUp.clone().invert(), parameters = new URLSearchParams(query);
      parameters.set("camera", camera.position.clone().applyQuaternion(zUp).toArray().join(","));
      parameters.set("target", target.clone().applyQuaternion(zUp).toArray().join(","));
      if (camera instanceof THREE.OrthographicCamera) parameters.set("view-height", String(view.height / camera.zoom));
      return parameters;
    };
    return { dispose, share, portable: !!profile.missing };
  } catch (error) {
    dispose();
    if (lostReason) throw new WebGpuUnavailable(lostReason, { cause: error });
    throw error;
  }
}
