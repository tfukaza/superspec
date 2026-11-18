// The washroom's lawn (GRASS=1, roadmap stage 6): a painted ground of two greens in broad patches, shaded by the sun's
// shade band with a world-anchored edge break, the room's cast shadows folded in. Its base color (the greens without the
// band) is what the scene's `ground.color` field holds, so the grass rooted on it (painted-grass.material.ts) takes the
// same paint and applies the same band at each root. Keep the band's parameters equal in both recipes.
import { defineMaterial, param, vec3, mix, mul, add, div, fbm, seed, world, shadeBand, sunDirection, normal, edgeBreak } from "@superspec/material/v1";

const lit = param.vec3({ default: vec3(0.40, 0.72, 0.03), range: [0, 1], label: "Lit green" });
const patch = param.vec3({ default: vec3(0.15, 0.47, 0.02), range: [0, 1], label: "Patch green" });
const shadeTint = param.vec3({ default: vec3(0.12, 0.38, 1.0), range: [0, 1], label: "Shade tint" });
const threshold = param.scalar({ default: 0.76, range: [-1, 1], label: "Shade threshold" });
const softness = param.scalar({ default: 1.5, range: [0, 8], label: "Edge softness (pixels)" });
const range = param.scalar({ default: 0.02, range: [0, 1], label: "Gradient range" });

const edge = edgeBreak({ size: 0.35, amount: 0.05 });
const ground = mix(patch, lit, fbm(div(world.xy, 1.6), seed("patches"), 3));

export default defineMaterial({
  name: "Washroom painted lawn",
  shading: "painted",
  base: { baseColor: ground, roughness: 0.95 },
  paint: mul(ground, mix(shadeTint, vec3(1, 1, 1), shadeBand(normal, sunDirection, add(threshold, edge.offset), softness, range))),
});
