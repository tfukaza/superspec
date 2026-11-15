import {
  defineMaterial, param, vec2, vec3, fbm, noise, seed, mix, runoff, surface,
  add, sub, mul, div,
} from "@superspec/material/v1";
import { coverage, tinted } from "./common.material";

// Chip scale and depth are physical: meters.
const chipSize = param.scalar({ default: 0.08, range: [0.005, 0.5], unit: "m" });
const chipDepth = param.scalar({ default: 0.002, range: [0, 0.02], unit: "m" });
const coverageAmount = param.scalar({ default: 0.48, range: [0.1, 0.9] });
const tint = param.vec3({ default: vec3(0.43, 0.40, 0.35), range: [0, 1] });

const aggregate = fbm(div(surface, 0.012), seed("aggregate"), 4);
const warped = add(
  div(surface, chipSize),
  mul(vec2(noise(div(surface, 0.09), seed("warp-x")), noise(div(surface, 0.09), seed("warp-y"))), 0.4),
);
const chips = coverage(fbm(warped, seed("chips"), 4), coverageAmount, 0.08);
const runoffMask = runoff(0.025, 0.7);

export default defineMaterial({
  name: "Chipped concrete",
  base: {
    baseColor: mul(mix(tinted(tint, aggregate), mul(tint, 0.58), chips), sub(1, mul(runoffMask, 0.25))),
    roughness: mix(0.68, 0.96, chips),
    metalness: 0,
    height: sub(mul(aggregate, 0.0003), mul(chips, chipDepth)),
  },
});
