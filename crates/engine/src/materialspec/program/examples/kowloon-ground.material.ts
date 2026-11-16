// What the Kowloon ground weathering layers share: the shipped concrete's ground-contact study,
// measured from the feature a slot names `base` (where a surface meets the ground) and along the
// pattern anchor, instead of the receiver's bounds. See kowloon-ground-stain and kowloon-moss.
import { fractal } from "./kowloon-noise.material";
import { add, mul, sub, div, smoothstep, valueNoise, seed, surface, vec2, featureDistance } from "@superspec/material/v1";
import type { Scalar, ScalarInput } from "@superspec/material/v1";

/** Meters above the ground line, read out to `limit`. */
export const aboveBase = (limit: ScalarInput): Scalar => featureDistance("base", limit);

/** Fine grain of the ground band. */
export const groundGrain = valueNoise(add(mul(div(surface, 2), 280), 7.1), seed("ground-grain"));

/** Clumps along the wall. */
export const clumps = fractal(add(mul(vec2(surface.x, 0), 3.8), vec2(9.2, 22)), seed("ground-clumps"));

/** How far up the stain reaches. */
const stainLimit = (damp: ScalarInput): Scalar =>
  add(add(add(0.06, mul(mul(clumps, clumps), 0.3)), mul(damp, 0.12)), mul(groundGrain, 0.025));

/** The dark band of damp ground contact: `damp` in [0, 1], `above` meters above the ground. */
export const groundStain = (damp: ScalarInput, above: ScalarInput): Scalar =>
  mul(sub(1, smoothstep(mul(stainLimit(damp), 0.18), stainLimit(damp), above)), smoothstep(0.2, 0.64, add(clumps, mul(damp, 0.15))));
