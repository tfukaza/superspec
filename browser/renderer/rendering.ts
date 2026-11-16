import * as THREE from "three";
import { AmbientOcclusion } from "./ambient-occlusion.ts";
import { DEFAULT_LIGHTING, defaultLighting, lightingFromQuery, sunOf, type Lighting } from "./lighting.ts";
import { ShadowStyle } from "./shadow-style.ts";

const TONE_MAPPING: Record<Lighting["toneMapping"], THREE.ToneMapping> = {
  aces: THREE.ACESFilmicToneMapping, neutral: THREE.NeutralToneMapping, none: THREE.NoToneMapping,
};

/**
 * Everything of a scene view that a lighting record decides: tone mapping and exposure, the environment's strength
 * and turn, the hemisphere light, the sun, whether and how it casts shadows, and ambient occlusion. Global settings of
 * the view, never part of a material. A viewer holds one of these and gives it the scene's record (`set`); the page's
 * parameters stay on top as session overrides.
 *
 * How the record's values are reached is this class's own choice and not in the record: the shadow map's size, type
 * and biases, and the occlusion's sample counts.
 */
export class SceneLighting {
  readonly ambient = new THREE.HemisphereLight();
  readonly sun = new THREE.DirectionalLight();
  /** The sun's shadow look. Every scene material is patched with it; it draws only while `styledShadows`. */
  readonly shadowStyle = new ShadowStyle(DEFAULT_LIGHTING.lights[0].shadow);
  /** Ambient occlusion while it is on and the renderer can composite; frames then go through it. */
  ambientOcclusion?: AmbientOcclusion;
  /** The record in effect: the scene's, with the session's overrides. */
  private resolved = defaultLighting();
  /** The session's exposure (the viewer's own control), which then replaces the record's. */
  private exposure?: number;
  private readonly direction = new THREE.Vector3(0, 0, 1);
  private readonly reported = new Set<string>();

  /**
   * `environment` is the built-in `studio` reflection environment, `query` the page's parameters and `excluded`
   * what ambient occlusion must not see (see `AmbientOcclusion`). The lights are added to `scene`.
   */
  constructor(private readonly renderer: THREE.WebGLRenderer, private readonly scene: THREE.Scene, private readonly camera: THREE.Camera,
    private readonly environment: THREE.Texture, private readonly query: URLSearchParams, private readonly excluded: () => THREE.Object3D[] = () => []) {
    this.ambient.position.set(0, 0, 1);
    // The sun does not move between records and a viewer renders on demand, so the shadow map is drawn when asked
    // (`shadowMap.needsUpdate`, see `fit`), not every frame.
    renderer.shadowMap.type = THREE.PCFSoftShadowMap;
    renderer.shadowMap.autoUpdate = false;
    this.sun.shadow.mapSize.set(2048, 2048);
    // A small bias against acne and a normal bias that keeps thin parts (ducts, pipes) attached to their shadows.
    this.sun.shadow.bias = -0.0003;
    this.sun.shadow.normalBias = 0.02;
    scene.add(this.ambient, this.sun, this.sun.target);
    this.set(defaultLighting());
  }

  /** The record in effect, with the session's overrides. */
  get lighting(): Lighting { return this.resolved; }
  /** Whether the sun casts shadows, so meshes cast and receive them. */
  get shadows(): boolean { return this.sun.castShadow; }
  /** Whether those shadows have the illustration look instead of three.js's plain one. */
  get styledShadows(): boolean { return this.shadowStyle.enabled; }

  /** Lights and renders the view by `record`. Call `fit` afterwards: the sun may have moved or started casting. */
  set(record: Lighting): void {
    const lighting = this.resolved = lightingFromQuery(record, this.query);
    const { renderer, scene } = this;
    renderer.toneMapping = TONE_MAPPING[lighting.toneMapping];
    renderer.toneMappingExposure = this.exposure ?? lighting.exposure;
    const source = lighting.environment.source;
    if (source !== "studio" && source !== "none") this.report(`environment.source "${source}" is not available in this viewer; using "studio".`);
    scene.environment = source === "none" ? null : this.environment;
    // The built-in environment is Y-up: a quarter turn stands it up, then the record turns it about +Z.
    scene.environmentRotation.set(Math.PI / 2, 0, lighting.environment.rotation);
    scene.environmentIntensity = lighting.environment.intensity;
    this.ambient.color.setRGB(...lighting.ambient.sky, THREE.LinearSRGBColorSpace);
    this.ambient.groundColor.setRGB(...lighting.ambient.ground, THREE.LinearSRGBColorSpace);
    this.ambient.intensity = lighting.ambient.intensity;

    const sun = sunOf(lighting);
    this.sun.visible = sun !== undefined;
    const casting = sun?.castsShadow === true;
    if (sun) {
      this.sun.color.setRGB(...sun.color, THREE.LinearSRGBColorSpace);
      this.sun.intensity = sun.intensity;
      this.direction.fromArray(sun.direction).normalize();
      this.sun.position.copy(this.sun.target.position).addScaledVector(this.direction, 10);
    }
    renderer.shadowMap.enabled = casting;
    this.sun.castShadow = casting;
    const shadow = sun?.shadow ?? DEFAULT_LIGHTING.lights[0].shadow;
    this.shadowStyle.set(casting ? shadow : { ...shadow, style: "plain" });

    const occlusion = lighting.ambientOcclusion;
    const unavailable = occlusion.enabled ? AmbientOcclusion.unavailable(renderer) : null;
    if (unavailable) this.report(`ambientOcclusion is not drawn: ${unavailable}.`);
    if (occlusion.enabled && !unavailable) {
      if (this.ambientOcclusion) this.ambientOcclusion.set(occlusion);
      else {
        this.ambientOcclusion = new AmbientOcclusion(renderer, scene, this.camera, occlusion, this.excluded);
        const size = renderer.getSize(new THREE.Vector2());
        this.ambientOcclusion.setSize(size.x, size.y, renderer.getPixelRatio());
      }
    } else {
      this.ambientOcclusion?.dispose();
      this.ambientOcclusion = undefined;
    }
  }

  /** The session's exposure, which replaces the record's until the page is left. */
  setExposure(value: number): void {
    this.exposure = value;
    this.renderer.toneMappingExposure = value;
  }

  /**
   * Fits the sun's shadow camera to what `models` spans and asks for one new shadow map. The light's direction is the
   * record's; only where the light stands along it changes.
   */
  fit(models: THREE.Object3D): void {
    if (!this.shadows) return;
    const bounds = new THREE.Box3().setFromObject(models);
    if (bounds.isEmpty()) return;
    const sphere = bounds.getBoundingSphere(new THREE.Sphere());
    const radius = Math.max(sphere.radius, 0.05);
    this.sun.target.position.copy(sphere.center);
    this.sun.position.copy(sphere.center).addScaledVector(this.direction, radius * 3);
    Object.assign(this.sun.shadow.camera, { left: -radius, right: radius, top: radius, bottom: -radius, near: radius * 0.5, far: radius * 5.5 });
    this.sun.shadow.camera.updateProjectionMatrix();
    this.sun.target.updateMatrixWorld();
    this.shadowStyle.fit(this.sun);
    this.renderer.shadowMap.needsUpdate = true;
  }

  /** A setting this viewer cannot honor is said once, with its fallback, and never blocks the scene. */
  private report(message: string): void {
    if (this.reported.has(message)) return;
    this.reported.add(message);
    console.warn(`Lighting: ${message}`);
  }

  dispose(): void {
    this.ambientOcclusion?.dispose();
    this.ambientOcclusion = undefined;
    this.scene.remove(this.ambient, this.sun, this.sun.target);
    this.sun.shadow.dispose();
  }
}
