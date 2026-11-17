// The washroom plants' leaves and canopy hulls (TREES=1, roadmap stage 7), the recipe their foliage looks name. The leaf
// family evaluates it once per cluster in the vertex stage, at the cluster's centre with the canopy shade normal (from
// the metaball hull) as `normal`; the hull family per pixel with the hull's normals, so a far plant's impostor shades
// as its leaves do. Two greens in broad world-anchored patches, the sun's shade band on the canopy normal with an edge
// break, the cast shadow (the shadow proxy's and the room's) folded in. Its base color is the lit profile's albedo.
import { defineMaterial, param, vec3, mix, mul, add, div, fbm, seed, world, shadeBand, sunDirection, normal, edgeBreak } from "@superspec/material/v1";

const lit = param.vec3({ default: vec3(0.3, 0.52, 0.13), range: [0, 1], label: "Lit green" });
const deep = param.vec3({ default: vec3(0.16, 0.36, 0.09), range: [0, 1], label: "Deep green" });
const shadeTint = param.vec3({ default: vec3(0.3, 0.4, 0.68), range: [0, 1], label: "Shade tint" });
const threshold = param.scalar({ default: 0.05, range: [-1, 1], label: "Shade threshold" });
const softness = param.scalar({ default: 1.5, range: [0, 8], label: "Edge softness (pixels)" });
const range = param.scalar({ default: 0.05, range: [0, 1], label: "Gradient range" });

const edge = edgeBreak({ size: 0.5, amount: 0.12 });
const leaf = mix(deep, lit, fbm(div(world.xy, 1.3), seed("canopy"), 3));

export default defineMaterial({
  name: "Washroom painted canopy",
  shading: "painted",
  base: { baseColor: leaf, roughness: 0.7 },
  paint: mul(leaf, mix(shadeTint, vec3(1, 1, 1), shadeBand(normal, sunDirection, add(threshold, edge.offset), softness, range))),
});
