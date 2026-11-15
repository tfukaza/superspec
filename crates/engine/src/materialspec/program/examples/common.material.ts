import { smoothstep, add, sub, mul } from "@superspec/material/v1";
import type { Scalar, ScalarInput, Vec3 } from "@superspec/material/v1";

/** A smooth threshold: 0 below `threshold - softness`, 1 above `threshold + softness`. */
export const coverage = (field: ScalarInput, threshold: ScalarInput, softness: ScalarInput): Scalar =>
  smoothstep(sub(threshold, softness), add(threshold, softness), field);

/** Brightens or darkens a base color by a detail value centered on 0.5. */
export function tinted(base: Vec3, detail: ScalarInput): Vec3 {
  return mul(base, add(0.85, mul(detail, 0.3)));
}
