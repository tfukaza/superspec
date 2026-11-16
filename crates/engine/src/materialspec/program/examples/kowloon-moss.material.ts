// Moss islands in the damp band at a Kowloon wall's foot, as a weathering layer: the shipped
// concrete's ground moss, following the feature the slot names `base`, rough and raised over the
// surface beneath. Where the occluder service supplies `occlusion` and `sunExposure`, moss grows
// further into sheltered corners and away from open sky; without them (an open wall: no
// occlusion, half the sky) it is the shipped moss.
import { defineLayer, param, smoothstep, external, add, mul, sub, vec2, vec3, seed, surface } from "@superspec/material/v1";
import { fractal } from "./kowloon-noise.material";
import { aboveBase, groundGrain, groundStain } from "./kowloon-ground.material";

const dampness = param.scalar({ default: 0.5, range: [0, 1], label: "Ground dampness" });
const shade = param.scalar({ default: 0.15, range: [0, 1], label: "Shelter response" });

const damp = smoothstep(0.5, 1, dampness);
const above = aboveBase(0.25);
const stain = groundStain(damp, above);
const islands = fractal(add(mul(vec2(surface.x, mul(above, 2.2)), 8), vec2(31.1, 14.6)), seed("ground-islands"));
const shelter = mul(shade, add(mul(external("occlusion", 0), 0.5), sub(0.5, external("sunExposure", 0.5))));
const moss = mul(mul(
  smoothstep(0.64, 0.79, add(add(add(islands, mul(damp, 0.12)), mul(groundGrain, 0.09)), shelter)),
  sub(1, smoothstep(0.015, add(0.11, mul(damp, 0.09)), above))),
  smoothstep(0.08, 0.4, stain));

export default defineLayer({
  name: "Kowloon moss",
  coverage: moss,
  baseColor: vec3(0.073, 0.091, 0.022),
  roughness: 0.95,
  height: add(0.0005, mul(groundGrain, 0.0016)),
  blend: { height: "add" },
});
