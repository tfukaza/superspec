import * as THREE from "three";
import { parseLighting, type Lighting } from "./lighting.ts";
import { SceneLighting, studioEnvironment } from "./rendering.ts";

/**
 * Lighting for a three.js game: the lights, shadow look and ambient occlusion of a SuperSpec lighting record
 * (`lighting.ts`), set up on the game's own scene and renderer exactly as the Studio viewer draws them. It owns no
 * rendering code of its own: it holds the viewer's `SceneLighting` and only adds what a host that did not build its
 * scene from a SuperSpec packet needs (reading the record from a GLB, adopting loaded meshes, a Y-up world).
 *
 * A SuperSpec GLB carries the record under `extras.superspec.lighting` and its sun again as a `KHR_lights_punctual`
 * light, for consumers that know nothing of the record. Use:
 *
 *     const gltf = await createGltfLoader(renderer).loadAsync(url);   // ktx2.ts
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
export interface LoadedGltf { userData?: Record<string, unknown>; parser?: { json?: unknown } }

/**
 * The lighting record a SuperSpec GLB carries (`extras.superspec.lighting`), as its file states it, or undefined when
 * it has none and so the shipped look. Give the result to `GameLighting`, which admits it.
 */
export function gltfLighting(gltf: LoadedGltf): unknown {
  type Extras = { superspec?: { lighting?: unknown } } | undefined;
  // GLTFLoader copies the document's `extras` onto its result's `userData`; the parsed document has them too.
  return (gltf.userData as Extras)?.superspec?.lighting ?? (gltf.parser?.json as { extras?: Extras } | undefined)?.extras?.superspec?.lighting;
}

export interface GameLightingOptions {
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
  /** What ambient occlusion must not see: overlays that are drawn in the image but cast no occlusion. */
  excluded?: () => THREE.Object3D[];
}

export class GameLighting {
  /** The same view the Studio viewer holds: its lights, shadow style and ambient occlusion. */
  readonly view: SceneLighting;
  private readonly environment: THREE.WebGLRenderTarget;
  private readonly adopted = new Set<THREE.Object3D>();

  /**
   * `record` is a lighting record as stated (parsed JSON, or `gltfLighting`'s result); undefined is the shipped look.
   * An invalid record is refused with a `LightingError` naming every field.
   */
  constructor(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, record?: unknown, options: GameLightingOptions = {}) {
    this.environment = studioEnvironment(renderer);
    this.view = new SceneLighting(renderer, scene, camera, this.environment.texture, options.overrides ?? new URLSearchParams(), options.excluded, options.up ?? "y");
    this.set(record);
  }

  /** The record in effect, complete, with the overrides and this renderer's defaults applied. */
  get lighting(): Lighting { return this.view.lighting; }

  /** Lights the scene by another record (as stated; undefined is the shipped look). Values change in place. */
  set(record?: unknown): void {
    this.view.set(parseLighting(record ?? { version: 1 }), record);
    for (const root of this.adopted) this.dress(root);
    this.fit();
  }

  /**
   * Takes loaded models into the lit scene: their meshes cast and receive the sun's shadows as the record says, their
   * standard materials get the shadow look, and the punctual lights an exported GLB carries for other consumers are
   * removed (this module lights the scene from the whole record; both would light it twice). Call it once per loaded
   * root, after adding it to the scene, and again for materials replaced later.
   */
  adopt(root: THREE.Object3D): void {
    const lights: THREE.Object3D[] = [];
    root.traverse(object => { if ((object as THREE.Light).isLight) lights.push(object); });
    for (const light of lights) {
      light.removeFromParent();
      (light as THREE.Light).dispose();
    }
    const styled = this.styled;
    root.traverse(object => {
      const mesh = object as THREE.Mesh;
      if (!mesh.isMesh) return;
      for (const material of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
        if (!(material as THREE.MeshStandardMaterial).isMeshStandardMaterial || styled.has(material)) continue;
        styled.add(material);
        this.view.shadowStyle.apply(material);
      }
    });
    this.adopted.add(root);
    this.dress(root);
    this.fit();
  }
  private readonly styled = new WeakSet<THREE.Material>();

  /** Stops tending a root that left the scene. */
  release(root: THREE.Object3D): void {
    this.adopted.delete(root);
    this.fit();
  }

  /**
   * Fits the sun's shadow to the adopted models and draws its shadow map anew. Call it when they moved: the map is
   * drawn on demand, not every frame.
   */
  fit(): void {
    const bounds = new THREE.Box3();
    for (const root of this.adopted) { root.updateWorldMatrix(true, true); bounds.expandByObject(root); }
    this.view.fitBounds(bounds);
  }

  /** Draws one frame: through the ambient occlusion when it is on, else straight to the canvas. */
  render(): void { this.view.render(); }

  /** Follows the renderer's size; call after `renderer.setSize` or a pixel ratio change. */
  resize(): void { this.view.resize(); }

  private dress(root: THREE.Object3D): void {
    const shadows = this.view.shadows;
    root.traverse(object => { if ((object as THREE.Mesh).isMesh) object.castShadow = object.receiveShadow = shadows; });
  }

  dispose(): void {
    this.adopted.clear();
    this.view.dispose();
    this.environment.dispose();
  }
}
