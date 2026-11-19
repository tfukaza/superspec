import * as THREE from "three";

/**
 * The perspective camera's near plane, fitted to the view each frame. A depth buffer's precision falls off with the
 * square of the distance over the near plane (24 bits: one step is about distance² / near × 6e-8 m), so the fixed
 * 0.01 m near plane of `view.ts` left steps of about 2 cm at 60 m, and parts mounted within a few centimetres of a wall
 * (tiles, lettering, vents, brackets, plates) fought with it. The near plane is instead a small share of the distance to
 * what the camera looks at: at 62 m it is 0.19 m, a step of about 1.2 mm, and something would have to stand 300 times
 * nearer than the point looked at to be cut away. Close up it falls to `least`, so a small item can still be inspected
 * from a few centimetres; there the precision is fine at any near plane. While walking the eye has a body round it (its
 * radius is 0.35 m) and looks far down a street, so the near plane is `walking`.
 *
 * Reversed depth (three.js's `reversedDepthBuffer`) was weighed and not adopted: it gains precision only with a float
 * depth buffer, which the canvas a frame is drawn to without ambient occlusion does not have, and several of the
 * viewer's own shaders read or write depth the conventional way (the styled shadow's comparisons, the foliage's
 * shadow lookups and written depths, the stroke and blur passes).
 *
 * An orthographic camera's depth is linear and its precision is the same everywhere; it is left as it is.
 */
export const NEAR_PLANE = { share: 0.003, least: 0.02, most: 0.5, walking: 0.1 } as const;

/** Fits `camera.near` to the distance to `target` (null while walking); true when the projection changed. */
export function fitNearPlane(camera: THREE.Camera, target: THREE.Vector3 | null): boolean {
  const perspective = camera as THREE.PerspectiveCamera;
  if (!perspective.isPerspectiveCamera) return false;
  const near = target
    ? THREE.MathUtils.clamp(perspective.position.distanceTo(target) * NEAR_PLANE.share, NEAR_PLANE.least, NEAR_PLANE.most)
    : NEAR_PLANE.walking;
  if (perspective.near === near) return false;
  perspective.near = near;
  perspective.updateProjectionMatrix();
  return true;
}
