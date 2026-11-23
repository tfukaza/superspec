import * as THREE from "three";
import { parseLighting, type Lighting } from "./lighting.ts";
import { studioReflections } from "#render-engine";
import { SceneLighting } from "./rendering.ts";
import { GltfLights } from "./gltf-lights.ts";

/**
 * Lighting for a three.js game: the lights, shadow look and ambient occlusion of a SuperSpec lighting record
 * (`lighting.ts`), set up on the game's own scene and renderer exactly as the Studio viewer draws them. It owns no
 * rendering code of its own: it holds the viewer's `SceneLighting` and only adds what a host that did not build its
 * scene from a SuperSpec packet needs (reading the record from a GLB, adopting loaded meshes, a Y-up world).
 *
 * A SuperSpec GLB carries the record under `extras.superspec.lighting` and its sun again as a `KHR_lights_punctual`
 * light, for consumers that know nothing of the record. Use:
 *
 *     const gltf = await createRenderGltfLoader(renderer, { transcoderPath: "basis/" }).loadAsync(url);
 *     scene.add(gltf.scene);
 *     const lighting = new GameLighting(renderer, scene, camera, gltfLighting(gltf));
 *     lighting.adopt(gltf.scene);
 *     ... each frame, or when something changed: lighting.render();
 *     ... after renderer.setSize: lighting.resize();
 *
 * The renderer's output color space, antialiasing and pixel ratio stay the host's (the Studio uses sRGB output and an
 * antialiased canvas). What the record states that the renderer cannot do is said once on the console with the
 * fallback taken, and never stops the scene: ambient occlusion without float render targets, an environment other
 * than the built-in `studio` room, and ambient occlusion left to its default on a software renderer
 * (`software-occlusion.ts`).
 */

/** What a loaded glTF offers this module: three.js's `GLTF` result, or anything shaped like its parts. */
export interface LoadedGltf { scene?: { userData?: Record<string, unknown> }; userData?: Record<string, unknown>; parser?: { json?: unknown } }

/**
 * The lighting record a SuperSpec GLB carries, as its file states it. Without
 * a record, GameLighting uses the file's lights or the shipped preview rig.
 */
export function gltfLighting(gltf: LoadedGltf): unknown {
  type Extras = { superspec?: { lighting?: unknown } } | undefined;
  // GLTFLoader copies the document's `extras` onto its result's `userData`; the parsed document has them too.
  return (gltf.userData as Extras)?.superspec?.lighting ?? (gltf.parser?.json as { extras?: Extras } | undefined)?.extras?.superspec?.lighting
    ?? gltf.scene?.userData?.worldSpecLighting;
}

export interface GameLightingOptions {
  /** Use the published city's screen-space finishing. */
  city?: boolean;
  /**
   * The world's up axis. `y` (the default) is a loaded glTF scene's and three.js's own; `z` is the record's and the
   * SuperSpec packet's frame. The record's directions are turned with the world.
   */
  up?: "y" | "z";
  /**
   * Session overrides in the Studio's page-parameter form (`ao=off`, `shadowstyle=plain`, ...; see
   * `lightingFromQuery`), for a debug menu or a quality setting. They sit on top of every record given later.
   */
  overrides?: URLSearchParams;
  /** Requested map size for the preview rig's sun, limited by the renderer. */
  sunShadowMapSize?: number;
  /** What ambient occlusion must not see: overlays that are drawn in the image but cast no occlusion. */
  excluded?: () => THREE.Object3D[];
}

export class GameLighting {
  /** The same view the Studio viewer holds: its lights, shadow style and ambient occlusion. */
  readonly view: SceneLighting;
  private readonly environment: THREE.WebGLRenderTarget;
  private readonly adopted = new Set<THREE.Object3D>();
  private readonly embedded = new GltfLights();
  private stated?: unknown;
  private record: Lighting;
  private useRig?: boolean;
  private authoredShadows = false;
  private readonly city: boolean;
  private readonly query: URLSearchParams;

  /**
   * `record` is parsed JSON or `gltfLighting`'s result. An absent record gives
   * embedded lights precedence over the shipped preview rig.
   * An invalid record is refused with a `LightingError` naming every field.
   */
  constructor(private readonly renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera,
    record?: unknown, options: GameLightingOptions = {}) {
    this.record = parseLighting(record ?? { version: 1 });
    this.stated = record;
    this.environment = studioReflections(renderer);
    this.city = options.city === true;
    this.query = options.overrides ?? new URLSearchParams();
    this.view = new SceneLighting(renderer, scene, camera, this.environment.texture, this.query, options.excluded, options.up ?? "y");
    if (options.sunShadowMapSize !== undefined) {
      const size = Math.min(options.sunShadowMapSize, renderer.capabilities.maxTextureSize);
      this.view.sun.shadow.mapSize.set(size, size);
    }
    this.syncLights(true);
  }

  /** The record in effect, complete, with the overrides and this renderer's defaults applied. */
  get lighting(): Lighting { return this.view.lighting; }

  /** Lights the scene by another record. An absent record restores authored lighting. */
  set(record?: unknown): void {
    this.record = parseLighting(record ?? { version: 1 });
    this.stated = record;
    this.syncLights(true);
  }

  /**
   * Takes loaded models into the lit scene: their meshes cast and receive the sun's shadows as the record says, their
   * standard materials get the shadow look. Authored lights remain in the model.
   * A lighting record supplies its own sun and ambient, and an unlit file receives the preview rig. Call once per loaded
   * root, after adding it to the scene, and again for materials replaced later. Pass false when the WorldSpec
   * adapter supplies the shadow shaders.
   */
  adopt(root: THREE.Object3D, styleMaterials = true): void {
    const styled = this.styled;
    if (styleMaterials) root.traverse(object => {
      const mesh = object as THREE.Mesh;
      if (!mesh.isMesh) return;
      for (const material of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
        if (!(material as THREE.MeshStandardMaterial).isMeshStandardMaterial || styled.has(material)) continue;
        styled.add(material);
        this.view.shadowStyle.apply(material);
      }
    });
    this.adopted.add(root);
    this.syncLights(false);
  }
  private readonly styled = new WeakSet<THREE.Material>();

  /** Stops tending a root that left the scene. */
  release(root: THREE.Object3D): void {
    this.adopted.delete(root);
    this.syncLights(false);
  }

  private syncLights(recordChanged: boolean): void {
    const { useRig, authored } = this.embedded.select(this.adopted, this.stated == null ? undefined : this.record);
    if (recordChanged || useRig !== this.useRig) {
      this.view.set(useRig ? this.record : { ...this.record, lights: [] }, this.stated, this.city);
      this.view.ambient.visible = useRig;
      this.useRig = useRig;
    }
    this.authoredShadows = this.query.get("shadows") !== "off" && authored.some(light => light.castShadow);
    this.renderer.shadowMap.enabled = this.view.shadows || this.authoredShadows;
    for (const root of this.adopted) this.dress(root);
    this.fit();
  }

  /**
   * Fits the sun's shadow to the adopted models and draws its shadow map anew. Call it when they moved: the map is
   * drawn on demand, not every frame.
   */
  fit(): void {
    if (this.authoredShadows) this.renderer.shadowMap.needsUpdate = true;
    const bounds = new THREE.Box3();
    for (const root of this.adopted) { root.updateWorldMatrix(true, true); bounds.expandByObject(root); }
    this.view.fitBounds(bounds);
  }

  /** Draws one frame: through the ambient occlusion when it is on, else straight to the canvas. */
  render(): void { this.view.render(); }

  /** Follows the renderer's size; call after `renderer.setSize` or a pixel ratio change. */
  resize(): void { this.view.resize(); }

  private dress(root: THREE.Object3D): void {
    const shadows = this.view.shadows || this.authoredShadows;
    root.traverse(object => { if ((object as THREE.Mesh).isMesh) object.castShadow = object.receiveShadow = shadows; });
  }

  dispose(): void {
    this.embedded.restore();
    this.adopted.clear();
    this.view.dispose();
    this.environment.dispose();
  }
}
