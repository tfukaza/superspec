// The washroom's lawn (GRASS=1, roadmap stage 6): a painted ground of greens with no cracks. Its base is a subtle mix of
// three close greens chosen by domain-warped fractal noise (as the concrete's tones are), with occasional organic patches of
// a deeper olive or a lime where two more warped fractals pass their rarity; a slow meadow-scale tint and the concrete's
// pigment drift go over it. Shaded by the sun's shade band with a world-anchored edge break. Its base color (without the
// shade) is what the scene's `ground.color` field holds, so the grass rooted on it takes the same greens.
import { defineMaterial, param, vec2, vec3, mix, mul, add, sub, div, world, shadeBand, sunDirection, normal, edgeBreak, seed, smoothstep } from "@superspec/material/v1";
import { domainWarp, pigment, fractal } from "./kowloon-noise.material";

// The meadow scale: a slow field over many meters tints the whole ground (and so its blades) warmer, toward
// yellow-green, or cooler, toward blue-green, subtly, on top of the bands below, so a large meadow drifts in hue the
// way a real one does rather than repeating one mottle.
const meadowScale = param.scalar({ default: 9, range: [1, 100], unit: "m", label: "Meadow hue scale" });
const meadowAmount = param.scalar({ default: 0.3, range: [0, 1], label: "Meadow hue amount" });
const warm = param.vec3({ default: vec3(1.18, 1.06, 0.55), range: [0, 2], label: "Meadow warm tint" });
const cool = param.vec3({ default: vec3(0.82, 0.97, 1.6), range: [0, 2], label: "Meadow cool tint" });

// Five bands, darkest first (an olive green) to the lightest (a saturated, near-neon yellow-lime): the middle three mix as
// the base, the outer two are the occasional patches.
const band0 = param.vec3({ default: vec3(0.23, 0.43, 0.05), range: [0, 1], label: "Band 1 (deepest)" });
const band1 = param.vec3({ default: vec3(0.26, 0.47, 0.06), range: [0, 1], label: "Band 2" });
const band2 = param.vec3({ default: vec3(0.29, 0.505, 0.075), range: [0, 1], label: "Band 3" });
const band3 = param.vec3({ default: vec3(0.34, 0.6, 0.06), range: [0, 1], label: "Band 4" });
const band4 = param.vec3({ default: vec3(0.42, 0.7, 0.03), range: [0, 1], label: "Band 5 (lightest)" });
const hueScale = param.scalar({ default: 1.6, range: [0.1, 10], label: "Hue mix frequency" });
const warpAmount = param.scalar({ default: 0.6, range: [0, 2], label: "Warp" });
const patchScale = param.scalar({ default: 0.9, range: [0.1, 6], label: "Patch frequency" });
const darkPatches = param.scalar({ default: 0.7, range: [0.3, 1], label: "Dark patch rarity" });
const lightPatches = param.scalar({ default: 0.67, range: [0.3, 1], label: "Light patch rarity" });
const patchSoftness = param.scalar({ default: 0.06, range: [0.005, 0.3], label: "Patch softness" });
const hueAmount = param.scalar({ default: 0.07, range: [0, 0.5], label: "Hue variation" });
const grain = param.scalar({ default: 0.05, range: [0, 0.3], label: "Grain" });
const grainScale = param.scalar({ default: 14, range: [2, 60], label: "Grain frequency" });
const hueAngle = param.scalar({ default: 70, range: [0, 360], unit: "deg", label: "Hue direction" });
const shadeTint = param.vec3({ default: vec3(0.3, 0.44, 0.66), range: [0, 1], label: "Shade tint" });
const threshold = param.scalar({ default: 0.76, range: [-1, 1], label: "Shade threshold" });
const softness = param.scalar({ default: 1.5, range: [0, 8], label: "Edge softness (pixels)" });
const range = param.scalar({ default: 0.3, range: [0, 1], label: "Gradient range" });

// The concrete's study coordinates: two meters per unit, squashed a little, so patches are not round.
const studyUv = div(world.xy, 2);
const noiseUv = mul(studyUv, vec2(0.85, 0.68));

// Patches of grass, not cracks: smooth fractal noise with its coordinates bent by more fractal noise (a domain warp), so
// every edge is an organic blob. The base is a subtle mix of the three middle bands, as the concrete's tones are; over it,
// occasional patches where a second warped fractal passes its rarity become the deepest band (dark) or the lightest
// (lime). No cells and no thresholds of one shared field, so patches never meet in lines.
const warped = add(noiseUv, mul(domainWarp(mul(noiseUv, 0.8), seed("warp")), warpAmount));
const hueField = fractal(add(mul(warped, hueScale), vec2(113.2, 67.8)), seed("hue-mix"));
const base = mix(mix(band1, band2, smoothstep(0.32, 0.5, hueField)), band3, smoothstep(0.5, 0.68, hueField));
const darkMask = smoothstep(darkPatches, add(darkPatches, patchSoftness), fractal(add(mul(warped, patchScale), vec2(41.2, 7.7)), seed("dark-patches")));
const lightMask = smoothstep(lightPatches, add(lightPatches, patchSoftness), fractal(add(mul(warped, mul(patchScale, 1.3)), vec2(9.4, 51.3)), seed("light-patches")));
const bands = mix(mix(base, band0, darkMask), band4, mul(lightMask, sub(1, darkMask)));
const meadow = smoothstep(0.3, 0.7, fractal(add(div(world.xy, meadowScale), vec2(7.3, 2.9)), seed("meadow")));
const meadowTint = mix(vec3(1, 1, 1), mix(cool, warm, meadow), meadowAmount);
// A fine grain over it all, as the concrete's darkness fields: a few percent lighter or darker, about 5 to 10 cm across.
const grained = mul(bands, add(1, mul(sub(fractal(add(mul(world.xy, grainScale), vec2(3.3, 8.8)), seed("grain")), 0.5), mul(grain, 2))));
const patched = mul(meadowTint, pigment(grained, mul(noiseUv, 1.8), hueAmount, hueAngle, seed("pigment")));

const ground = patched;

const edge = edgeBreak({ size: 0.35, amount: 0.05 });

export default defineMaterial({
  name: "Washroom painted lawn",
  shading: "painted",
  base: { baseColor: ground, roughness: 0.95 },
  paint: mul(ground, mix(shadeTint, vec3(1, 1, 1), shadeBand(normal, sunDirection, add(threshold, edge.offset), softness, range))),
});
