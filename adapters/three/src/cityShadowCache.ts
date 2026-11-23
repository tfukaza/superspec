import * as THREE from "three";

/** resetState zeroes these GL registers, then clears Three's record of their values.
 * Retain the defaults in Three's cache so the next depth blit needs no GL readbacks. */
export function rememberUnpackDefaults(renderer: THREE.WebGLRenderer): void {
  const gl = renderer.getContext() as WebGL2RenderingContext;
  renderer.state.pixelStorei(gl.UNPACK_ROW_LENGTH, 0);
  renderer.state.pixelStorei(gl.UNPACK_IMAGE_HEIGHT, 0);
  renderer.state.pixelStorei(gl.UNPACK_SKIP_PIXELS, 0);
  renderer.state.pixelStorei(gl.UNPACK_SKIP_ROWS, 0);
  renderer.state.pixelStorei(gl.UNPACK_SKIP_IMAGES, 0);
}

type StaticDepth = {
  matrix: THREE.Matrix4;
  source?: THREE.WebGLRenderTarget;
  saved?: THREE.WebGLRenderTarget;
  /** The target of a complete caster draw which was not copied. */
  uncopied?: THREE.WebGLRenderTarget;
  cameraMask: number;
};

type ShadowPass =
  | { mode: "reuse" }
  | { mode: "casters"; capture?: { entry: StaticDepth; target: THREE.WebGLRenderTarget } };

/**
 * Each shadow pass visits children in scene order. Draw the fixed city first and save
 * its depth, then let live actors draw against it. On subsequent frames a depth blit
 * replaces just the city's draw calls, retaining the sun's normal PCF and material cutouts.
 *
 * The saved depth is a second map of the light's size. A light that redraws its map every
 * frame is copied on its first pass. A map drawn on request (`autoUpdate` false) is copied
 * only once a second pass arrives with the same projection, so a light that is drawn once
 * and then left alone never allocates a copy.
 */
export class CityShadowCache {
  private readonly before: THREE.Mesh;
  private readonly after: THREE.Mesh;
  private readonly entries = new Map<THREE.Camera, StaticDepth>();
  private pass?: ShadowPass;
  private readonly shadowStates: readonly THREE.LightShadow[];
  private readonly lights: ReadonlyMap<THREE.Camera, THREE.Light>;

  constructor(private renderer: THREE.WebGLRenderer, private city: THREE.Object3D, sun: THREE.DirectionalLight, restoreVisibility: () => void,
    localLights: readonly THREE.SpotLight[] = [], private restoreView?: () => void,
    prepareShadow?: (shadowCamera: THREE.Camera, viewCamera: THREE.Camera) => void, cacheDepth = true) {
    const scene = city.parent!;
    this.shadowStates = [sun, ...localLights].map(light => light.shadow);
    this.lights = new Map([sun, ...localLights].map(light => [light.shadow.camera as THREE.Camera, light] as const));
    const shadows = new Map([sun, ...localLights].map(light => [light.shadow.camera as THREE.Camera, light.shadow] as const));
    const localCameras = new Set<THREE.Camera>(localLights.map(light => light.shadow.camera));
    const restoreCasters = (shadowCamera: THREE.Camera, viewCamera: THREE.Camera) => {
      this.pass = { mode: "casters" };
      try {
        restoreVisibility();
        // The camera render list is already built. This callback may select
        // full-detail instances against the light frustum before city traversal.
        // VSM also draws receiving surfaces, so keep its ordinary full subtree.
        if (localCameras.has(shadowCamera) && renderer.shadowMap.type !== THREE.VSMShadowMap)
          prepareShadow?.(shadowCamera, viewCamera);
      } catch (error) {
        this.finishPass();
        throw error;
      }
    };
    // Three invokes mesh shadow hooks even when the geometry has zero vertices.
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.Float32BufferAttribute([], 3));
    this.before = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial());
    this.after = new THREE.Mesh(geometry, this.before.material);
    for (const marker of [this.before, this.after]) {
      marker.castShadow = true;
      marker.frustumCulled = false;
      marker.layers.enableAll();
    }
    // Shadow traversal is depth-first. Surround the static subtree before any actors,
    // including those which were already in the scene when the map was replaced.
    scene.children.splice(scene.children.indexOf(city), 1);
    scene.add(this.before, this.after);
    scene.children.splice(-2);
    scene.children.unshift(this.before, city, this.after);
    this.before.onBeforeShadow = (_, __, camera, shadowCamera) => {
      // A renderer exception can interrupt the traversal between the two markers.
      this.finishPass();
      if (!city.visible || city.parent !== scene) return;
      const live = renderer.getRenderTarget();
      const shadow = shadows.get(shadowCamera);
      if (!cacheDepth || !shadow || !live?.depthTexture || renderer.shadowMap.type === THREE.VSMShadowMap) {
        restoreCasters(shadowCamera, camera);
        return;
      }
      let entry = this.entries.get(shadowCamera);
      if (!entry) {
        entry = { matrix: new THREE.Matrix4(), cameraMask: 0 };
        this.entries.set(shadowCamera, entry);
      }
      if (!entry.saved || entry.source !== live || entry.cameraMask !== camera.layers.mask ||
          entry.saved.width !== live.width || entry.saved.height !== live.height || !entry.matrix.equals(shadow.matrix)) {
        // A second pass with an unchanged projection shows that the light redraws on request.
        const copy = shadow.autoUpdate || (entry.uncopied === live && entry.cameraMask === camera.layers.mask
          && entry.matrix.equals(shadow.matrix));
        // Include off-camera casters when rebuilding the complete static depth.
        entry.source = entry.uncopied = undefined;
        if (!copy) {
          // Depth saved for another projection cannot serve this one.
          entry.saved?.dispose();
          entry.saved = undefined;
        }
        restoreCasters(shadowCamera, camera);
        this.pass = { mode: "casters", capture: copy ? { entry, target: live } : undefined };
        if (!copy) entry.uncopied = live;
        entry.cameraMask = camera.layers.mask;
        entry.matrix.copy(shadow.matrix);
      } else {
        try {
          renderer.copyTextureToTexture(entry.saved.depthTexture!, live.depthTexture);
        } catch (error) {
          entry.source = undefined;
          throw error;
        } finally {
          renderer.setRenderTarget(live);
        }
        // The camera draw list already exists. Skip the entire cached subtree for this pass.
        city.visible = false;
        this.pass = { mode: "reuse" };
      }
    };
    this.after.onBeforeShadow = () => {
      try {
        if (this.pass?.mode === "casters" && this.pass.capture) {
          const { entry, target } = this.pass.capture;
          this.captureDepth(entry, target);
        }
      } finally {
        this.finishPass();
      }
    };
    renderer.domElement.addEventListener("webglcontextrestored", this.invalidate);
  }

  /** Refresh static caster depth after an edit, even when every light is fixed. */
  invalidate = (): void => {
    this.finishPass();
    for (const entry of this.entries.values()) entry.saved?.dispose();
    this.entries.clear();
    for (const shadow of this.shadowStates) shadow.needsUpdate = true;
  };

  /** Release the depth saved for lights that no longer draw a shadow map. Call once a frame.
   * A light that casts again redraws its casters and saves a new copy. */
  releaseUnused(): void {
    const enabled = this.renderer.shadowMap.enabled;
    for (const [camera, entry] of this.entries) {
      const light = this.lights.get(camera);
      if (enabled && light?.castShadow && light.visible) continue;
      entry.saved?.dispose();
      this.entries.delete(camera);
    }
  }

  private captureDepth(entry: StaticDepth, target: THREE.WebGLRenderTarget): void {
    try {
      if (!entry.saved || entry.saved.width !== target.width || entry.saved.height !== target.height) {
        entry.saved?.dispose();
        entry.saved = target.clone();
        this.renderer.initRenderTarget(entry.saved);
      }
      this.renderer.copyTextureToTexture(target.depthTexture!, entry.saved.depthTexture!);
      entry.source = target;
    } catch (error) {
      entry.saved?.dispose();
      entry.saved = undefined;
      throw error;
    } finally {
      this.renderer.setRenderTarget(target);
    }
  }

  /** Unwind an interrupted shadow traversal before the next camera draw list is built. */
  finishPass(): void {
    const pass = this.pass;
    this.pass = undefined;
    if (pass?.mode === "reuse") this.city.visible = true;
    if (pass?.mode === "casters") this.restoreView?.();
  }

  dispose(): void {
    this.renderer.domElement.removeEventListener("webglcontextrestored", this.invalidate);
    this.invalidate();
    this.before.removeFromParent();
    this.after.removeFromParent();
    this.before.geometry.dispose();
    (this.before.material as THREE.Material).dispose();
  }
}
