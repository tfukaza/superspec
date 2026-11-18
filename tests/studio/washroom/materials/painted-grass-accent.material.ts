// The washroom lawn's accent grass (the painterly study's highlight population): sparse broad blades laid over the
// lawn, drawn a little above it. Evaluated per vertex at each blade's root, as painted-grass.material.ts is. A clump
// is one of two kinds by a fine noise at its root: most are highlights, a bright lime-yellow where the ground faces
// the sun and falling back toward the ground's own green where it faces away; the rest take another hue, a blue-green
// or a warm olive, near the ground's brightness, for variety. Shaded by the lawn's band with its shade tint, as the
// lawn and its grass are.
import { defineMaterial, param, vec2, vec3, mix, mul, add, lt, select, smoothstep, shadeBand, sunDirection, sampleField, placedWorld, edgeBreak, valueNoise, seed } from "@superspec/material/v1";

const highlight = param.vec3({ default: vec3(0.58, 0.82, 0.10), range: [0, 1], label: "Highlight" });
const cool = param.vec3({ default: vec3(0.16, 0.44, 0.30), range: [0, 1], label: "Cool hue" });
const warm = param.vec3({ default: vec3(0.42, 0.48, 0.10), range: [0, 1], label: "Warm hue" });
const highlights = param.scalar({ default: 0.6, range: [0, 1], label: "Share of highlights" });
const hueMix = param.scalar({ default: 0.7, range: [0, 1], label: "Hue strength" });
const shadeTint = param.vec3({ default: vec3(0.42, 0.62, 0.9), range: [0, 1], label: "Shade tint" });
const threshold = param.scalar({ default: 0.76, range: [-1, 1], label: "Shade threshold" });
const softness = param.scalar({ default: 1.5, range: [0, 8], label: "Edge softness (pixels)" });
const range = param.scalar({ default: 0.02, range: [0, 1], label: "Gradient range" });

const edge = edgeBreak({ size: 0.35, amount: 0.05 });
const ground = sampleField("ground.color", placedWorld.xy);
const groundNormal = sampleField("ground.normal", placedWorld.xy);
const lit = shadeBand(groundNormal, sunDirection, add(threshold, edge.offset), softness, range);
// One value per clump (its blades' roots lie within a few centimeters): fine noise at the root.
const pick = valueNoise(add(mul(placedWorld.xy, 23), vec2(4.1, 9.7)), seed("accent"));
const other = valueNoise(add(mul(placedWorld.xy, 31), vec2(12.3, 2.2)), seed("accent-hue"));
// A highlight blade is brightest facing the sun: there it is the highlight color, away from it a little over the ground.
const bright = mix(mul(ground, 1.12), highlight, smoothstep(0.2, 1, lit));
const hued = mix(ground, select(lt(other, 0.5), cool, warm), hueMix);
const color = select(lt(pick, highlights), bright, hued);

export default defineMaterial({
  name: "Washroom accent grass",
  shading: "painted",
  base: { baseColor: color, roughness: 0.8 },
  paint: mul(color, mix(shadeTint, vec3(1, 1, 1), lit)),
});
