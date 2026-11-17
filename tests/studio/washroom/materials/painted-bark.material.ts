// The washroom plants' trunks and branches (TREES=1, roadmap stage 7): a painted bark of two browns streaked along the
// stem, shaded by the sun's band on the bark's normal with the cast shadow folded in (the canopy's shadow proxy shades
// the trunk under it). The bark family evaluates it per pixel. Its base color is the lit profile's albedo.
import { defineMaterial, param, vec3, mix, mul, add, div, fbm, seed, world, shadeBand, sunDirection, normal, edgeBreak } from "@superspec/material/v1";

const light = param.vec3({ default: vec3(0.33, 0.25, 0.17), range: [0, 1], label: "Light bark" });
const dark = param.vec3({ default: vec3(0.17, 0.12, 0.08), range: [0, 1], label: "Dark bark" });
const shadeTint = param.vec3({ default: vec3(0.36, 0.38, 0.62), range: [0, 1], label: "Shade tint" });
const threshold = param.scalar({ default: 0.1, range: [-1, 1], label: "Shade threshold" });
const softness = param.scalar({ default: 1.5, range: [0, 8], label: "Edge softness (pixels)" });
const range = param.scalar({ default: 0.04, range: [0, 1], label: "Gradient range" });

const edge = edgeBreak({ size: 0.3, amount: 0.08 });
const bark = mix(dark, light, fbm(div(world.xy, 0.08), seed("bark"), 3));

export default defineMaterial({
  name: "Washroom painted bark",
  shading: "painted",
  base: { baseColor: bark, roughness: 0.9 },
  paint: mul(bark, mix(shadeTint, vec3(1, 1, 1), shadeBand(normal, sunDirection, add(threshold, edge.offset), softness, range))),
});
