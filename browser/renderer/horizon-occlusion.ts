import * as THREE from "three";
import type { AmbientOcclusionSettings, LightShadow } from "./lighting.ts";

export { HORIZON_OCCLUSION_GLSL } from "../../adapters/three/src/horizonOcclusionShader.ts";

/** The uniforms `HORIZON_OCCLUSION_GLSL` reads. */
export function horizonUniforms(): Record<string, THREE.IUniform> {
  return {
    aoDepth: { value: null }, aoInvProjection: { value: new THREE.Matrix4() }, aoTexel: { value: new THREE.Vector2() },
    aoFocal: { value: new THREE.Vector2() }, aoRadius: { value: 1 }, aoThickness: { value: 0.3 }, aoIntensity: { value: 0 },
    aoTint: { value: 0 }, aoHue: { value: new THREE.Vector3(1, 1, 1) },
  };
}

/** The sun shadow's hue at its saturation, brightest channel 1: the shadow's tone before its darkness (`shadow-style.ts`). */
export function shadowHue(shadow: Pick<LightShadow, "tint" | "tintStrength"> | undefined, target = new THREE.Vector3()): THREE.Vector3 {
  if (!shadow) return target.set(1, 1, 1);
  const [r, g, b] = shadow.tint, peak = Math.max(r, g, b, 1e-6), keep = 1 - shadow.tintStrength;
  return target.set(r / peak + (1 - r / peak) * keep, g / peak + (1 - g / peak) * keep, b / peak + (1 - b / peak) * keep);
}

/** Sets the uniforms for a frame: the record's values, the shadow hue, the camera and the depth to read. */
export function updateHorizon(uniforms: Record<string, THREE.IUniform>, settings: AmbientOcclusionSettings, shadow: LightShadow | undefined,
  camera: THREE.Camera, depth: THREE.Texture, width: number, height: number): void {
  uniforms.aoDepth.value = depth;
  (uniforms.aoInvProjection.value as THREE.Matrix4).copy(camera.projectionMatrixInverse);
  const projection = camera.projectionMatrix.elements;
  (uniforms.aoFocal.value as THREE.Vector2).set(projection[0], projection[5]);
  (uniforms.aoTexel.value as THREE.Vector2).set(1 / Math.max(1, width), 1 / Math.max(1, height));
  uniforms.aoRadius.value = settings.radius;
  uniforms.aoThickness.value = settings.thickness;
  uniforms.aoIntensity.value = settings.enabled ? settings.intensity : 0;
  uniforms.aoTint.value = settings.tint ?? 0;
  shadowHue(shadow, uniforms.aoHue.value as THREE.Vector3);
}
