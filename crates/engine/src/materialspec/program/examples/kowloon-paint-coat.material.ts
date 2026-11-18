// A coat of emulsion paint over a Kowloon render, as a weathering layer: a colour (the reference
// shop house's cream by default) mottled a little, and broken where it has peeled into the grey
// patches of the surface beneath, which come in drifts of warped fractal noise on the part's
// pattern anchor. It reads nothing of the receiver but its position, so a wall that is resized keeps
// its patches where they were.
import { defineLayer, param, smoothstep, add, mul, sub, vec2, vec3, mix, seed, surface } from "@superspec/material/v1";
import { fractal, domainWarp } from "./kowloon-noise.material";

const color = param.vec3({ default: vec3(0.78, 0.71, 0.55), range: [0, 1], label: "Paint colour" });
const amount = param.scalar({ default: 1, range: [0, 1], label: "Paint amount" });
const peeling = param.scalar({ default: 0.35, range: [0, 1], label: "Peeling" });
const patchScale = param.scalar({ default: 0.7, range: [0.1, 4], unit: "1/m", label: "Patch frequency" });

const at = mul(vec2(surface.x, surface.y), patchScale);
const warped = add(at, mul(domainWarp(mul(at, 1.3), seed("paint-warp")), 0.5));
// Peeled where the field rises above a threshold that falls as `peeling` grows.
const field = fractal(add(warped, vec2(17.3, 5.9)), seed("paint-peel"));
const threshold = sub(0.82, mul(peeling, 0.3));
const intact = sub(1, smoothstep(threshold, add(threshold, 0.02), field));
const mottle = fractal(add(mul(at, 3.1), vec2(3.7, 41.2)), seed("paint-mottle"));

export default defineLayer({
  name: "Kowloon paint coat",
  coverage: mul(amount, intact),
  baseColor: mix(mul(color, 0.86), color, smoothstep(0.3, 0.7, mottle)),
  roughness: 0.82,
});
