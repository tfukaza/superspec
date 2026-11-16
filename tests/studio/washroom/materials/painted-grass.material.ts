// The washroom lawn's grass (GRASS=1, roadmap stage 6), the recipe its foliage look names. Drawn by the grass shader
// families, it is evaluated once per vertex at each blade's root (`placedWorld` is the root there): the blade takes the
// ground's paint under its root (`ground.color`, the lawn's greens) and shades it by the ground's normal there with the
// lawn's own band, so a blade matches the ground beneath it and reads only where it crosses a shade boundary. The band's
// parameters equal painted-lawn.material.ts's. Its base color is the lit profile's albedo and the baked fallback.
import { defineMaterial, param, vec3, mix, mul, add, shadeBand, sunDirection, sampleField, placedWorld, edgeBreak } from "@superspec/material/v1";

const shadeTint = param.vec3({ default: vec3(0.36, 0.44, 0.74), range: [0, 1], label: "Shade tint" });
const threshold = param.scalar({ default: 0.76, range: [-1, 1], label: "Shade threshold" });
const softness = param.scalar({ default: 1.5, range: [0, 8], label: "Edge softness (pixels)" });
const range = param.scalar({ default: 0.02, range: [0, 1], label: "Gradient range" });
const blade = param.vec3({ default: vec3(0.12, 0.3, 0.06), range: [0, 1], label: "Lit blade albedo" });

const edge = edgeBreak({ size: 0.35, amount: 0.05 });
const ground = sampleField("ground.color", placedWorld.xy);
const groundNormal = sampleField("ground.normal", placedWorld.xy);

export default defineMaterial({
  name: "Washroom painted grass",
  shading: "painted",
  base: { baseColor: blade, roughness: 0.8 },
  paint: mul(ground, mix(shadeTint, vec3(1, 1, 1), shadeBand(groundNormal, sunDirection, add(threshold, edge.offset), softness, range))),
});
