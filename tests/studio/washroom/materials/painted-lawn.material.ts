// The washroom's lawn (GRASS=1, roadmap stage 6): a painted ground layered as the Kowloon concrete layers its two
// images (kowloon-concrete.material.ts), with greens in place of the photographs: a lime and an emerald, each
// mottled darker by its own jagged field, mixed by an eroded blend field; then patches of sun-yellow and of deep teal
// in the same jagged manner, and the concrete's pigment drift over all. Shaded by the sun's shade band with a
// world-anchored edge break. Its base color (the layered greens without the band) is what the scene's `ground.color`
// field holds, so the grass rooted on it (painted-grass.material.ts) takes the same hues and applies the same band at
// each root. Keep the band's parameters equal in both recipes.
import { defineMaterial, param, vec2, vec3, mix, mul, add, sub, div, world, shadeBand, sunDirection, normal, edgeBreak, seed, smoothstep } from "@superspec/material/v1";
import { fracture, contour, pigment, fractal } from "./kowloon-noise.material";

// The meadow scale: a slow field over many meters tints the whole ground (and so its blades) warmer, toward
// yellow-green, or cooler, toward blue-green, subtly, on top of the fine patches below, so a large meadow drifts in hue
// the way a real one does rather than repeating one mottle.
const meadowScale = param.scalar({ default: 9, range: [1, 100], unit: "m", label: "Meadow hue scale" });
const meadowAmount = param.scalar({ default: 0.35, range: [0, 1], label: "Meadow hue amount" });
const warm = param.vec3({ default: vec3(1.18, 1.06, 0.55), range: [0, 2], label: "Meadow warm tint" });
const cool = param.vec3({ default: vec3(0.82, 0.97, 1.6), range: [0, 2], label: "Meadow cool tint" });

const lime = param.vec3({ default: vec3(0.40, 0.72, 0.03), range: [0, 1], label: "Green A (lime)" });
const emerald = param.vec3({ default: vec3(0.22, 0.56, 0.03), range: [0, 1], label: "Green B (emerald)" });
const sun = param.vec3({ default: vec3(0.52, 0.78, 0.04), range: [0, 1], label: "Sun-yellow patches" });
const deep = param.vec3({ default: vec3(0.10, 0.40, 0.06), range: [0, 1], label: "Deep teal patches" });
const darknessA = param.scalar({ default: 0.12, range: [0, 0.8], label: "Green A darkness" });
const darknessB = param.scalar({ default: 0.14, range: [0, 0.8], label: "Green B darkness" });
const blendScale = param.scalar({ default: 4.6, range: [0.2, 16], label: "Blend frequency" });
const jaggedness = param.scalar({ default: 0.9, range: [0, 2], label: "Edge jaggedness" });
const sunAmount = param.scalar({ default: 0.3, range: [0, 1], label: "Sun-yellow amount" });
const deepAmount = param.scalar({ default: 0.3, range: [0, 1], label: "Deep teal amount" });
const hueAmount = param.scalar({ default: 0.12, range: [0, 0.5], label: "Hue variation" });
const hueAngle = param.scalar({ default: 70, range: [0, 360], unit: "deg", label: "Hue direction" });
const shadeTint = param.vec3({ default: vec3(0.12, 0.38, 1.0), range: [0, 1], label: "Shade tint" });
const threshold = param.scalar({ default: 0.76, range: [-1, 1], label: "Shade threshold" });
const softness = param.scalar({ default: 1.5, range: [0, 8], label: "Edge softness (pixels)" });
const range = param.scalar({ default: 0.02, range: [0, 1], label: "Gradient range" });

// The concrete's study coordinates: two meters per unit, squashed a little, so patches are not round.
const studyUv = div(world.xy, 2);
const noiseUv = mul(studyUv, vec2(0.85, 0.68));

// Two greens, each with its own darkness field, mixed by an eroded blend (the concrete's two images).
const darkA = contour(fracture(add(mul(noiseUv, 9.7), vec2(5.3, 17.1)), jaggedness, seed("dark-a")), 0.035);
const darkB = contour(fracture(add(mul(noiseUv, 14.9), vec2(73.4, 29.6)), jaggedness, seed("dark-b")), 0.035);
const blend = contour(fracture(add(mul(noiseUv, blendScale), vec2(113.2, 67.8)), jaggedness, seed("blend")), 0.045);
const greens = mix(mul(lime, sub(1, mul(darknessA, darkA))), mul(emerald, sub(1, mul(darknessB, darkB))), blend);
// Sun-yellow and deep teal patches, each an eroded field at its own threshold.
const sunny = mul(contour(fracture(add(mul(noiseUv, 6.5), vec2(41.7, 8.2)), jaggedness, seed("sun-patches")), 0.04), sunAmount);
const deepPatch = mul(contour(fracture(add(mul(noiseUv, 7.9), vec2(17.9, 93.1)), jaggedness, seed("deep-patches")), 0.04), deepAmount);
const layered = mix(mix(greens, sun, mul(sunny, sub(1, blend))), deep, mul(deepPatch, blend));
const meadow = smoothstep(0.3, 0.7, fractal(add(div(world.xy, meadowScale), vec2(7.3, 2.9)), seed("meadow")));
const meadowTint = mix(vec3(1, 1, 1), mix(cool, warm, meadow), meadowAmount);
const ground = mul(meadowTint, pigment(layered, mul(noiseUv, 1.8), hueAmount, hueAngle, seed("pigment")));

const edge = edgeBreak({ size: 0.35, amount: 0.05 });

export default defineMaterial({
  name: "Washroom painted lawn",
  shading: "painted",
  base: { baseColor: ground, roughness: 0.95 },
  paint: mul(ground, mix(shadeTint, vec3(1, 1, 1), shadeBand(normal, sunDirection, add(threshold, edge.offset), softness, range))),
});
