import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { GameLighting, gltfLighting } from "../browser/renderer/game-lighting.ts";
import {
  applyMaterialAnisotropy, attachWorldSpecScene, CityShadowCache, createRenderGltfLoader,
  disposeCityTextureLod, disposeRenderGltfLoader, fitPublishedSun, lightingOverridesQuery,
  loadDeclaredLods, publishedTextureResidency, registerCityTextureLod, releaseObjects,
  streamsPublishedTextures, type ResolvedRenderProfile, type WorldSpecScene,
} from "#render-engine";
import { parseView, viewCamera } from "../browser/renderer/view.ts";
import { installConnectorDrag } from "./demos/connector-drag.ts";

/** Inspect published glTF directly, resolving external textures and buffers beside the entry file. */
export async function openGltf(url: URL, host: HTMLElement, query: URLSearchParams, graphics: ResolvedRenderProfile, preparing?: () => void) {
  const cleanup: (() => void)[] = [];
  const loading = new AbortController();
  const cancel = () => loading.abort();
  const dispose = () => {
    window.removeEventListener("pagehide", dispose);
    cancel();
    while (cleanup.length) {
      try { cleanup.pop()!(); }
      catch (error) { console.warn("Could not release a published viewer resource.", error); }
    }
  };
  window.addEventListener("pagehide", cancel, { once: true });
  try {
    const view = await createGltfView(url, host, query, graphics, preparing, loading.signal, release => cleanup.push(release));
    loading.signal.throwIfAborted();
    window.addEventListener("pagehide", dispose, { once: true });
    return { ...view, dispose };
  } catch (error) {
    dispose();
    throw error;
  } finally { window.removeEventListener("pagehide", cancel); }
}

async function createGltfView(url: URL, host: HTMLElement, query: URLSearchParams, graphics: ResolvedRenderProfile, preparing: (() => void) | undefined,
  loading: AbortSignal, onDispose: (release: () => void) => void) {
  const { settings, diagnostics } = graphics;
  const renderer = new THREE.WebGLRenderer({ antialias: graphics.parameters.gpu.antialiasing === "msaa" });
  if (settings.renderScale !== 1) renderer.setPixelRatio(settings.renderScale);
  onDispose(() => renderer.domElement.remove());
  onDispose(() => renderer.dispose());
  onDispose(() => disposeRenderGltfLoader(renderer));
  host.append(renderer.domElement);
  const scene = new THREE.Scene();
  const view = parseView(query);
  const camera = viewCamera(view);
  // SceneView uses SuperSpec Z-up coordinates. glTF has +Y up.
  const yUp = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), -Math.PI / 2);
  camera.up.applyQuaternion(yUp);
  camera.position.applyQuaternion(yUp);
  const controls = new OrbitControls(camera, renderer.domElement);
  onDispose(() => controls.dispose());
  controls.enabled = false;
  controls.target.fromArray(view.target).applyQuaternion(yUp);
  const manager = new THREE.LoadingManager();
  const abortRequests = () => manager.abort();
  loading.addEventListener("abort", abortRequests, { once: true });
  onDispose(() => loading.removeEventListener("abort", abortRequests));
  const loader = createRenderGltfLoader(renderer, { transcoderPath: "basis/", manager, graphicsPreset: graphics.preset });
  const streamTextures = streamsPublishedTextures(diagnostics);
  if (streamTextures) registerCityTextureLod(loader);
  const gltf = await loader.loadAsync(url.href);
  let world: WorldSpecScene | undefined, textureResidency: ReturnType<typeof publishedTextureResidency>;
  let declared: Awaited<ReturnType<typeof loadDeclaredLods>> | undefined;
  onDispose(() => {
    if (world) return; // Published geometry takes ownership when world attachment succeeds.
    textureResidency?.dispose(); declared?.dispose();
    disposeCityTextureLod(gltf, { texturesReleased: !!textureResidency });
    releaseObjects(gltf.scenes);
  });
  loading.throwIfAborted();
  scene.add(gltf.scene);
  const isWorldSpec = gltf.scene.userData.worldSpec === true;
  declared = diagnostics.visibility.lod && gltf.parser.json.extensionsUsed?.includes("MSFT_lod")
    ? await loadDeclaredLods(gltf) : undefined;
  loading.throwIfAborted();
  const lighting = new GameLighting(renderer, scene, camera, gltfLighting(gltf),
    { overrides: lightingOverridesQuery(diagnostics), sunShadowMapSize: settings.sunShadowMapSize });
  onDispose(() => lighting.dispose());
  const anisotropy = settings.materialAnisotropy;
  const adopt = () => {
    lighting.adopt(gltf.scene, !isWorldSpec);
    if (!isWorldSpec && anisotropy !== undefined) applyMaterialAnisotropy(gltf.scene, anisotropy, renderer.capabilities.getMaxAnisotropy());
  };
  if (declared) { await declared.withVariants(adopt); lighting.fit(); } else adopt();
  let preparingWorld = true;
  const invalidate = diagnostics.instrumentation.gpuProbe ? undefined : () => { if (!preparingWorld && !loading.aborted) render(); };
  if (isWorldSpec) {
    const { sun } = lighting.view;
    fitPublishedSun(sun, gltf, lighting.lighting);
    sun.shadow.autoUpdate = false;
    sun.shadow.needsUpdate = true;
    world = await attachWorldSpecScene(gltf, { scene, renderer, diagnostics, sun, lighting: lighting.lighting,
      entryUrl: url, invalidate, declared, graphics: settings, signal: loading });
    if (world) onDispose(world.dispose);
  }
  loading.throwIfAborted();
  textureResidency = !world && streamTextures ? publishedTextureResidency(gltf, renderer, invalidate, declared) : undefined;
  const applyCameraLods = () => {
    const height = renderer.domElement.height;
    declared?.frame(camera, height * settings.geometryLodBias, height * settings.materialLodBias);
  };
  const shadows = !world && declared ? new CityShadowCache(renderer, gltf.scene, lighting.view.sun,
    () => declared?.restore(), [], applyCameraLods, undefined, false) : undefined;
  if (shadows) onDispose(() => shadows.dispose());
  if (!view.fixed) {
    const box = new THREE.Box3().setFromObject(gltf.scene);
    if (!box.isEmpty()) {
      box.getCenter(controls.target);
      camera.position.copy(controls.target).addScaledVector(new THREE.Vector3(.8, .9, 1.2), Math.max(1, box.getSize(new THREE.Vector3()).length()));
    }
  }
  controls.update();
  const render = () => {
    if (world) { world.render(camera); return; }
    applyCameraLods();
    textureResidency?.frame(camera, renderer.domElement.height * settings.materialLodBias);
    try { lighting.render(); } finally { shadows?.finishPass(); }
  };
  let viewportWidth = 0, viewportHeight = 0;
  const configureViewport = () => {
    const { clientWidth: width, clientHeight: height } = host;
    if (!width || !height) {
      viewportWidth = viewportHeight = 0;
      return false;
    }
    if (width === viewportWidth && height === viewportHeight) return false;
    viewportWidth = width;
    viewportHeight = height;
    renderer.setSize(width, height);
    if (camera instanceof THREE.PerspectiveCamera) camera.aspect = width / height;
    else { camera.right = view.height * width / height / 2; camera.left = -camera.right; }
    camera.updateProjectionMatrix();
    lighting.resize();
    return true;
  };
  const resize = (redraw = false) => {
    const changed = configureViewport();
    if ((changed || (redraw && viewportHeight > 0)) && !renderer.getContext().isContextLost()) render();
  };
  const observer = new ResizeObserver(() => resize());
  const redrawRestored = () => resize(true);
  const warmWorld = diagnostics.loading.warmup ? world : undefined;
  const hasViewport = configureViewport();
  let warmup, initialColor;
  if (warmWorld) preparing?.();
  // Every preset links the first view's shaders off the main thread before drawing it. Without the
  // extension the first draw links them itself, as it always did. The fuller warm-up after the
  // first frame stays a separate choice, which the light presets and phones leave off.
  if (world && hasViewport && diagnostics.loading.firstFrameShaders && diagnostics.loading.shaderWarmup === "parallel"
      && renderer.extensions.has("KHR_parallel_shader_compile")) {
    preparing?.();
    try { initialColor = await world.prepareFirstColor(camera, loading); }
    catch (error) { console.warn("Initial shader preparation failed. Continuing with the first draw.", error); }
  }
  loading.throwIfAborted();
  let firstTextures;
  if (world && hasViewport && !renderer.getContext().isContextLost()) {
    // Every preset stores the first view's textures in short slices, so the first frame does not block on them.
    preparing?.();
    try { firstTextures = await world.prepareFirstTextures(camera, loading); }
    catch (error) {
      loading.throwIfAborted();
      console.warn("Initial texture preparation failed. Continuing with the first draw.", error);
    }
  }
  resize(true);
  if (warmWorld && !renderer.getContext().isContextLost()) {
    warmup = { ...await warmWorld.warmup(camera), initialColor };
    loading.throwIfAborted();
    // Texture requests and viewport changes can arrive before the holders detach.
    resize(true);
  }
  preparingWorld = false;
  controls.enabled = true;
  const stopDrag = installConnectorDrag(gltf.scene, camera, renderer.domElement, controls, () => {
    world?.invalidateShadows();
    render();
  });
  if (stopDrag) onDispose(stopDrag);
  if (diagnostics.instrumentation.gpuProbe) Object.assign(window, { __worldSpecView: { backend: "webgl", renderer, scene, camera, controls, render, warmup, initialColor, firstTextures,
    graphics: { preset: graphics.preset, ...settings },
    settle: world?.settle ?? (() => textureResidency?.settle()), textureStats: world?.textureStats ?? (() => textureResidency?.stats()),
    setSubmission: world?.setSubmission, setBlockProxyEnabled: world?.setBlockProxyEnabled, blockProxyStats: world?.blockProxyStats,
    declaredLods: declared && (() => declared?.stats()) } });
  const probe = window as Window & { __worldSpecView?: { renderer: unknown } };
  onDispose(() => { if (probe.__worldSpecView?.renderer === renderer) delete probe.__worldSpecView; });
  onDispose(() => {
    observer.disconnect();
    renderer.domElement.removeEventListener("webglcontextrestored", redrawRestored);
    controls.removeEventListener("change", render);
  });
  observer.observe(host);
  renderer.domElement.addEventListener("webglcontextrestored", redrawRestored);
  controls.addEventListener("change", render);
  const share = () => {
    const zUp = yUp.clone().invert();
    const parameters = new URLSearchParams(query);
    parameters.set("camera", camera.position.clone().applyQuaternion(zUp).toArray().join(","));
    parameters.set("target", controls.target.clone().applyQuaternion(zUp).toArray().join(","));
    if (camera instanceof THREE.OrthographicCamera) parameters.set("view-height", String(view.height / camera.zoom));
    return parameters;
  };
  return { share, paintedLighting: !!world };
}
