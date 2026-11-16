// A painted ground for the washroom slab (PAINTED=1, roadmap stage 5): two flat paints, lit and in shade, split by the
// sun's shade band with a world-anchored edge break, so the boundary bites into the floor and moves with the sun. The
// sun's cast shadows fold into the same factor. Its base color, the lit paint, is the baked fallback a renderer without
// the program shows.
import { defineMaterial, param, vec3, mix, add, shadeBand, sunDirection, normal, edgeBreak } from "@superspec/material/v1";

const lit = param.vec3({ default: vec3(0.62, 0.55, 0.4), range: [0, 1], label: "Lit paint" });
const shade = param.vec3({ default: vec3(0.18, 0.2, 0.32), range: [0, 1], label: "Shade paint" });
const threshold = param.scalar({ default: 0.55, range: [-1, 1], label: "Shade threshold" });
const softness = param.scalar({ default: 1.5, range: [0, 8], label: "Edge softness (pixels)" });
const range = param.scalar({ default: 0.03, range: [0, 1], label: "Gradient range" });

const edge = edgeBreak({ size: 0.35, amount: 0.12 });

export default defineMaterial({
  name: "Washroom painted ground",
  shading: "painted",
  base: { baseColor: lit, roughness: 0.9 },
  paint: mix(shade, lit, shadeBand(normal, sunDirection, add(threshold, edge.offset), softness, range)),
});
