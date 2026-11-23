import type * as THREE from "three";
import { CityOcclusion } from "./cityOcclusion.ts";
import { CityEdgeBlur } from "./cityEdgeBlur.ts";
import type { WorldSpecLighting } from "./worldSpecLighting.ts";
import { prepareFirstColor, shadowDepthStage, warmWorldColor } from "./worldWarmup.ts";
import { GlazingSides, glazingSideStage, warmGlazingSides } from "./glazing.ts";
import { legacyRenderDiagnostics, type RenderDiagnostics } from "./renderProfile.ts";

export type RenderLighting = Pick<WorldSpecLighting, "lights"> & {
  ambientOcclusion?: Parameters<CityOcclusion["render"]>[4];
};

function drawLighting(map: THREE.Object3D | null, override?: RenderLighting): RenderLighting | undefined {
  const record = map?.userData.worldSpecLighting;
  return override ?? (record?.version === 1 ? record : undefined);
}

function drawsBloom(map: THREE.Object3D | null): boolean {
  const record = map?.userData.worldSpecRender;
  return record?.version !== 1 || record.bloom !== false;
}

/** Draw the world before game-owned viewmodels and UI. */
export class WorldSpecRenderPass {
  private readonly occlusion: CityOcclusion;
  private readonly edges: CityEdgeBlur;
  private readonly glazing = new GlazingSides();
  /** Stand-in depth materials keep the prepared shadow programs alive for Three's shadow pass. */
  private shadowPrograms: THREE.Material[] = [];
  private shadowStage?: THREE.Object3D;

  private readonly outlines: boolean;
  constructor({ outlines = true, diagnostics = legacyRenderDiagnostics() }:
    { outlines?: boolean; diagnostics?: RenderDiagnostics } = {}) {
    this.outlines = outlines;
    this.occlusion = new CityOcclusion(diagnostics);
    this.edges = new CityEdgeBlur(diagnostics.effects);
  }

  private applyOutlineDefault(map: THREE.Object3D | null): void {
    const record = map?.userData.worldSpecRender;
    this.occlusion.setOutlines(this.outlines && (record?.version !== 1 || record.outlines !== false));
  }

  prepareFirstColor(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, map: THREE.Object3D,
    lighting?: RenderLighting, signal?: AbortSignal) {
    this.applyOutlineDefault(map);
    const target = (camera as THREE.PerspectiveCamera).isPerspectiveCamera
      ? this.occlusion.prepareColorTarget(renderer, drawLighting(map, lighting)?.ambientOcclusion, drawsBloom(map)) : undefined;
    // The colour submission covers the scene's own materials. These are the ones a pass swaps in.
    const shadowCasters = this.shadowStage;
    this.shadowStage = undefined;
    return prepareFirstColor(renderer, scene, camera, target ?? renderer.getRenderTarget(), signal, undefined,
      { variants: glazingSideStage(map, this.glazing), shadowCasters });
  }

  /** Read the scene's shadow casters for the next `prepareFirstColor`. Call while every
   * caster shows as the shadow pass draws it, before a camera selection hides any. */
  collectShadowCasters(renderer: THREE.WebGLRenderer, scene: THREE.Scene): void {
    for (const material of this.shadowPrograms) material.dispose();
    this.shadowPrograms = [];
    this.shadowStage = undefined;
    let casting = false;
    if (renderer.shadowMap.enabled) scene.traverseVisible(object => {
      casting ||= (object as THREE.Light).isLight === true && object.castShadow && !(object as THREE.PointLight).isPointLight;
    });
    if (!casting) return;
    const { stage, materials } = shadowDepthStage(renderer, scene);
    this.shadowPrograms = materials;
    this.shadowStage = stage;
  }

  /** Loading-screen preparation after the first world frame. Call with the
   * same scene, camera and render destination as a normal world draw. */
  warmup(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, map: THREE.Object3D,
    textureBudgetBytes = 32 * 1024 * 1024) {
    const perspective = (camera as THREE.PerspectiveCamera).isPerspectiveCamera;
    const target = (perspective && this.occlusion.colorTarget) || renderer.getRenderTarget();
    return warmWorldColor(renderer, scene, camera, map, target, textureBudgetBytes, async () => {
      await warmGlazingSides(renderer, scene, camera, map, target, this.glazing);
      if (perspective) await this.occlusion.warmExclusions(renderer, scene, camera, map);
    });
  }

  render(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera,
    sun: THREE.DirectionalLight, map: THREE.Object3D | null, hasOutlineExclusions: boolean,
    lightingOverride?: RenderLighting) {
    const extras = map?.userData;
    const lighting = drawLighting(map, lightingOverride);
    const record = extras?.worldSpecRender;
    const effects: unknown[] = record?.version === 1 && Array.isArray(record.effects) ? record.effects : [];
    const shadow = lighting?.lights?.find(light => light.role === "sun")?.shadow;
    this.applyOutlineDefault(map);
    this.glazing.render(renderer, () => {
      if ((camera as THREE.PerspectiveCamera).isPerspectiveCamera) {
        this.occlusion.render(renderer, scene, camera as THREE.PerspectiveCamera, sun, lighting?.ambientOcclusion, shadow,
          hasOutlineExclusions, effects.includes("paintedShopSurfaces"), drawsBloom(map),
          effects.includes("specularOutlineMask"));
      } else renderer.render(scene, camera);
    });
    this.edges.apply(renderer, effects.includes("displayAntialiasing"));
  }

  dispose(): void {
    for (const material of this.shadowPrograms) material.dispose();
    this.shadowPrograms = [];
    this.glazing.dispose();
    this.occlusion.dispose();
    this.edges.dispose();
  }
}
