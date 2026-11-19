import * as THREE from "three";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";
import { AmbientOcclusion } from "./ambient-occlusion.ts";
import { DEFAULT_LIGHTING, contoursOf, defaultLighting, lightingFromQuery, paintedOf, statesAmbientOcclusion, sunOf, type Lighting } from "./lighting.ts";
import { ShadowStyle } from "./shadow-style.ts";
import { programSun, setPaintedFinishing } from "./shade.ts";
import { softwareOcclusionDefault } from "./software-occlusion.ts";

const TONE_MAPPING: Record<Lighting["toneMapping"], THREE.ToneMapping> = {
  // `none` is no curve: the exposure still multiplies (three.js's "no tone mapping" would drop the exposure too).
  aces: THREE.ACESFilmicToneMapping, neutral: THREE.NeutralToneMapping, none: THREE.LinearToneMapping,
};

/**
 * The record's built-in `studio` environment: three.js's neutral room, prefiltered for reflections. Built once per
 * renderer; the caller owns the returned GPU target.
 */
export function studioEnvironment(renderer: THREE.WebGLRenderer): THREE.WebGLRenderTarget {
  const room = new RoomEnvironment();
  const generator = new THREE.PMREMGenerator(renderer);
  try {
    return generator.fromScene(room);
  } finally {
    room.dispose();
    generator.dispose();
  }
}

/**
 * Everything of a scene view that a lighting record decides: tone mapping and exposure, the environment's strength
 * and turn, the hemisphere light, the sun, whether and how it casts shadows, and ambient occlusion. Global settings of
 * the view, never part of a material. A viewer holds one of these and gives it the scene's record (`set`); the page's
 * parameters stay on top as session overrides. The Studio viewer and the game runtime (`game-lighting.ts`)
 * both draw through this class, so a record gives the same picture in each.
 *
 * How the record's values are reached is this class's own choice and not in the record: the shadow map's size, type
 * and biases, and the occlusion's sample counts.
 */
export class SceneLighting {
  readonly ambient = new THREE.HemisphereLight();
  readonly sun = new THREE.DirectionalLight();
  /** The sun's shadow look. Every scene material is patched with it; it draws only while `styledShadows`. */
  readonly shadowStyle = new ShadowStyle(DEFAULT_LIGHTING.lights[0].shadow);
  /**
   * The composited frame, while ambient occlusion or contour lines are on and the renderer can composite; frames then
   * go through it. `occluding` and `outlining` say which of the two it draws.
   */
  ambientOcclusion?: AmbientOcclusion;
  occluding = false;
  outlining = false;
  /** Why ambient occlusion the record leaves to its default is not drawn here (`software-occlusion.ts`), or null. */
  occlusionDefault: string | null = null;
  /** The record in effect: the scene's, with the session's overrides. */
  private resolved = defaultLighting();
  /** The session's exposure (a host's own control), which then replaces the record's. */
  private exposure?: number;
  private readonly direction = new THREE.Vector3(0, 0, 1);
  private readonly reported = new Set<string>();
  /** What the lights hang from: the scene itself where the world is the record's Z-up frame, else a turned group. */
  private readonly frame: THREE.Object3D;

  /**
   * `environment` is the built-in `studio` reflection environment (`studioEnvironment`), `query` the page's
   * parameters and `excluded` what ambient occlusion must not see (see `AmbientOcclusion`). The lights are added to
   * `scene`. `up` names the world's up axis: the record's own Z (the Studio viewer), or Y, as a loaded glTF scene and
   * most three.js games have it; the record's directions are then turned with the world (x, y, z to x, z, -y).
   */
  constructor(private readonly renderer: THREE.WebGLRenderer, private readonly scene: THREE.Scene, private readonly camera: THREE.Camera,
    private readonly environment: THREE.Texture, private readonly query: URLSearchParams, private readonly excluded: () => THREE.Object3D[] = () => [],
    private readonly up: "z" | "y" = "z") {
    this.ambient.position.set(0, 0, 1);
    // The sun does not move between records and a viewer renders on demand, so the shadow map is drawn when asked
    // (`shadowMap.needsUpdate`, see `fit`), not every frame.
    renderer.shadowMap.type = THREE.PCFSoftShadowMap;
    renderer.shadowMap.autoUpdate = false;
    this.sun.shadow.mapSize.set(2048, 2048);
    // A small bias against acne and a normal bias that keeps thin parts (ducts, pipes) attached to their shadows.
    this.sun.shadow.bias = -0.0003;
    this.sun.shadow.normalBias = 0.02;
    if (up === "y") {
      this.frame = new THREE.Group();
      this.frame.rotation.x = -Math.PI / 2;
      scene.add(this.frame);
      // The shadow map keeps the roll it has in the record's frame (its rows along the domain's +Y), so the style's
      // noise, which lives in the map's plane, lies in the world as it does in a Z-up view.
      this.sun.shadow.camera.up.set(0, 0, -1);
      (this.shadowStyle.uniforms.msShadowUp.value as THREE.Vector3).set(0, 1, 0);
    } else this.frame = scene;
    this.frame.add(this.ambient, this.sun, this.sun.target);
    this.set(defaultLighting());
  }

  /** The record in effect, with the session's overrides. */
  get lighting(): Lighting { return this.resolved; }
  /** Whether the sun casts shadows, so meshes cast and receive them. */
  get shadows(): boolean { return this.sun.castShadow; }
  /** Whether those shadows have the illustration look instead of three.js's plain one. */
  get styledShadows(): boolean { return this.shadowStyle.enabled; }

  /**
   * Lights and renders the view by `record`. Call `fit` afterwards: the sun may have moved or started casting.
   * `stated` is the record as its project or file states it, before defaults were filled in (see `parseLighting`):
   * what it leaves out, a renderer may default by its own policy.
   */
  set(record: Lighting, stated?: unknown): void {
    const lighting = this.resolved = lightingFromQuery(record, this.query);
    const { renderer, scene } = this;
    renderer.toneMapping = TONE_MAPPING[lighting.toneMapping];
    renderer.toneMappingExposure = this.exposure ?? lighting.exposure;
    const source = lighting.environment.source;
    if (source !== "studio" && source !== "none") this.report(`environment.source "${source}" is not available in this viewer; using "studio".`);
    scene.environment = source === "none" ? null : this.environment;
    // The built-in environment is Y-up. In a Z-up world a quarter turn stands it up, then the record turns it about
    // +Z (the order matters: the turn is about the world's vertical, not the lying environment's). In a Y-up world
    // it stands already and the record's turn about the domain's +Z is one about the world's +Y.
    if (this.up === "y") scene.environmentRotation.set(0, lighting.environment.rotation, 0, "XYZ");
    else scene.environmentRotation.set(Math.PI / 2, 0, lighting.environment.rotation, "ZYX");
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
    // Programs read the record's sun (`sunDirection`) at the live stage: a moved sun redraws them, rebuilding nothing.
    if (sun) programSun.value.fromArray(sun.direction).normalize();
    else programSun.value.set(0, 0, 0);
    renderer.shadowMap.enabled = casting;
    this.sun.castShadow = casting;
    const shadow = sun?.shadow ?? DEFAULT_LIGHTING.lights[0].shadow;
    this.shadowStyle.set(casting ? shadow : { ...shadow, style: "plain" });

    const occlusion = lighting.ambientOcclusion;
    // The one renderer default: see `software-occlusion.ts`. It applies only where neither the record nor the page says.
    this.occlusionDefault = occlusion.enabled
      ? softwareOcclusionDefault(renderer, statesAmbientOcclusion(stated) || this.query.get("ao") === "on") : null;
    if (this.occlusionDefault) {
      occlusion.enabled = false;
      this.report(`ambientOcclusion is ${this.occlusionDefault}.`, "info");
    }
    // Contour lines are a pass over the same composited frame, with or without the occlusion.
    const contours = contoursOf(lighting);
    const unavailable = occlusion.enabled || contours.enabled ? AmbientOcclusion.unavailable(renderer) : null;
    if (unavailable && occlusion.enabled) this.report(`ambientOcclusion is not drawn: ${unavailable}.`);
    if (unavailable && contours.enabled) this.report(`contours are not drawn: ${unavailable}.`);
    this.occluding = occlusion.enabled && !unavailable;
    this.outlining = contours.enabled && !unavailable;
    if (this.occluding || this.outlining) {
      if (this.ambientOcclusion) {
        this.ambientOcclusion.set(this.occluding ? occlusion : null);
        this.ambientOcclusion.setContours(this.outlining ? contours : null);
      } else {
        this.ambientOcclusion = new AmbientOcclusion(renderer, scene, this.camera, this.occluding ? occlusion : null, this.excluded, this.outlining ? contours : null);
        const size = renderer.getSize(new THREE.Vector2());
        this.ambientOcclusion.setSize(size.x, size.y, renderer.getPixelRatio());
      }
    } else {
      this.ambientOcclusion?.dispose();
      this.ambientOcclusion = undefined;
    }
    // Painted materials: whether the tone curve and exposure apply is the scene's setting. Ambient occlusion is a pass
    // over the finished image here, so it cannot leave painted pixels out.
    const painted = paintedOf(lighting);
    setPaintedFinishing(painted);
    if (!painted.ambientOcclusion && this.occluding) this.report("painted.ambientOcclusion is false, but ambient occlusion is a pass over the whole image in this viewer and also darkens painted materials.");
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
    if (this.shadows) this.fitBounds(new THREE.Box3().setFromObject(models));
  }

  /** `fit` for bounds already known, in world coordinates. */
  fitBounds(bounds: THREE.Box3): void {
    if (!this.shadows || bounds.isEmpty()) return;
    const sphere = bounds.getBoundingSphere(new THREE.Sphere());
    const radius = Math.max(sphere.radius, 0.05);
    // The lights stand in the record's frame.
    if (this.frame !== this.scene) { this.frame.updateWorldMatrix(true, false); this.frame.worldToLocal(sphere.center); }
    this.sun.target.position.copy(sphere.center);
    this.sun.position.copy(sphere.center).addScaledVector(this.direction, radius * 3);
    Object.assign(this.sun.shadow.camera, { left: -radius, right: radius, top: radius, bottom: -radius, near: radius * 0.5, far: radius * 5.5 });
    this.sun.shadow.camera.updateProjectionMatrix();
    this.sun.updateMatrixWorld();
    this.sun.target.updateMatrixWorld();
    this.shadowStyle.fit(this.sun);
    this.renderer.shadowMap.needsUpdate = true;
  }

  /** Draws one frame: through the composited frame (ambient occlusion, contour lines) when one is on, else straight to the canvas. */
  render(): void {
    if (this.ambientOcclusion) this.ambientOcclusion.render();
    else this.renderer.render(this.scene, this.camera);
  }

  /** Follows the renderer's size (call after `renderer.setSize`): the occlusion's targets are sized with the canvas. */
  resize(): void {
    const size = this.renderer.getSize(new THREE.Vector2());
    this.ambientOcclusion?.setSize(size.x, size.y, this.renderer.getPixelRatio());
  }

  /** A setting this viewer cannot honor is said once, with its fallback, and never blocks the scene. */
  private report(message: string, level: "warn" | "info" = "warn"): void {
    if (this.reported.has(message)) return;
    this.reported.add(message);
    console[level](`Lighting: ${message}`);
  }

  dispose(): void {
    this.ambientOcclusion?.dispose();
    this.ambientOcclusion = undefined;
    this.frame.remove(this.ambient, this.sun, this.sun.target);
    if (this.frame !== this.scene) this.scene.remove(this.frame);
    this.sun.shadow.dispose();
  }
}
