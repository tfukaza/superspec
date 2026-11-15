import {
  defineMaterial, param, vec2, vec3, cellular, fbm, seed, select, mix, rotate, scale, surface,
  add, sub, mul, div, neg,
} from "@superspec/material/v1";
import { coverage } from "./common.material";

const flakeSize = param.scalar({ default: 0.025, range: [0.002, 0.3], unit: "m" });
const damage = param.scalar({ default: 0.55, range: [0.1, 0.9] });
const paint = param.vec3({ default: vec3(0.025, 0.14, 0.21), range: [0, 1] });
const polished = param.bool({ default: false });

const flakes = coverage(cellular(div(surface, flakeSize), seed("flakes")), damage, 0.07);
const scratches = fbm(scale(rotate(surface, 0.2), vec2(160, 8)), seed("scratches"), 3);

export default defineMaterial({
  name: "Painted metal",
  base: {
    baseColor: mix(paint, vec3(0.21, 0.16, 0.1), flakes),
    roughness: add(mix(select(polished, 0.2, 0.48), 0.88, flakes), mul(scratches, 0.04)),
    metalness: sub(1, mul(flakes, 0.7)),
    height: add(mul(neg(flakes), 0.0005), mul(scratches, 0.00002)),
  },
});
