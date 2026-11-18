// The washroom's lawn (GRASS=1, roadmap stage 6): a painted ground of five bands of green, close together as the
// concrete's tones are, so every difference is subtle, chosen by a fractal field and blended softly between
// neighbouring bands. The field is the Kowloon concrete's eroded, warped fracture (kowloon-noise.material.ts) with
// finer fractal noise added on top, so a band's edge wanders at every scale and the bands break into fragments rather
// than nesting as rings of a contour map. A slow meadow-scale tint and the concrete's pigment drift go over it. Shaded
// by the sun's shade band with a world-anchored edge break. Its base color (the bands without the shade) is what the
// scene's `ground.color` field holds, so the grass rooted on it (painted-grass.material.ts) takes the same greens and
// applies the same band at each root. Keep the band's parameters equal in both recipes.
import { defineMaterial, param, vec2, vec3, mix, mul, add, sub, div, neg, sin, cos, rotate, normalize, valueNoise, world, shadeBand, sunDirection, normal, edgeBreak, seed, smoothstep } from "@superspec/material/v1";
import type { Scalar, ScalarInput } from "@superspec/material/v1";
import { fracture, pigment, fractal } from "./kowloon-noise.material";

// The meadow scale: a slow field over many meters tints the whole ground (and so its blades) warmer, toward
// yellow-green, or cooler, toward blue-green, subtly, on top of the bands below, so a large meadow drifts in hue the
// way a real one does rather than repeating one mottle.
const meadowScale = param.scalar({ default: 9, range: [1, 100], unit: "m", label: "Meadow hue scale" });
const meadowAmount = param.scalar({ default: 0.3, range: [0, 1], label: "Meadow hue amount" });
const warm = param.vec3({ default: vec3(1.18, 1.06, 0.55), range: [0, 2], label: "Meadow warm tint" });
const cool = param.vec3({ default: vec3(0.82, 0.97, 1.6), range: [0, 2], label: "Meadow cool tint" });

// Five bands, darkest first, each about 5 % lighter than the last and turning from a deep olive green to a soft yellow-green.
const band0 = param.vec3({ default: vec3(0.27, 0.44, 0.05), range: [0, 1], label: "Band 1 (deepest)" });
const band1 = param.vec3({ default: vec3(0.28, 0.475, 0.06), range: [0, 1], label: "Band 2" });
const band2 = param.vec3({ default: vec3(0.29, 0.505, 0.075), range: [0, 1], label: "Band 3" });
const band3 = param.vec3({ default: vec3(0.30, 0.53, 0.095), range: [0, 1], label: "Band 4" });
const band4 = param.vec3({ default: vec3(0.32, 0.55, 0.09), range: [0, 1], label: "Band 5 (lightest)" });
const bandScale = param.scalar({ default: 3.2, range: [0.2, 16], label: "Band frequency" });
const bandSoftness = param.scalar({ default: 0.07, range: [0.005, 0.2], label: "Band softness" });
const breakup = param.scalar({ default: 0.35, range: [0, 1], label: "Band breakup" });
const jaggedness = param.scalar({ default: 0.9, range: [0, 2], label: "Edge jaggedness" });
const regionScale = param.scalar({ default: 0.5, range: [0.05, 4], label: "Region frequency" });
const regionWeight = param.scalar({ default: 1.1, range: [0, 3], label: "Region strength" });
const flowScale = param.scalar({ default: 1.6, range: [0.2, 20], unit: "m", label: "Stroke direction scale" });
const strokeLength = param.scalar({ default: 6, range: [1, 60], label: "Stroke length frequency" });
const strokeWidth = param.scalar({ default: 32, range: [5, 300], label: "Stroke width frequency" });
const strokeTilt = param.scalar({ default: 1.4, range: [0, 3], label: "Stroke normal tilt" });
const strokeTone = param.scalar({ default: 0.18, range: [0, 0.5], label: "Stroke tone" });
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

// The band field at two scales: broad regions a few meters across (an eroded fracture at regionScale) that set where
// the field runs lighter or darker, and clumps of 20 to 50 cm (the fracture at bandScale) within them; finer fractal
// noise pushes each band edge in and out: fragments, not rings.
const region = fracture(add(mul(noiseUv, regionScale), vec2(31.7, 5.9)), jaggedness, seed("regions"));
const broad = fracture(add(mul(noiseUv, bandScale), vec2(113.2, 67.8)), jaggedness, seed("bands"));
const fine = sub(fractal(add(mul(noiseUv, mul(bandScale, 3.1)), vec2(9.4, 51.3)), seed("bands-fine")), 0.5);
const field = add(add(add(mul(sub(broad, 0.5), 1.4), mul(sub(region, 0.5), regionWeight)), 0.5), mul(fine, breakup));
// The ramp: each band laid over the ones below it from its threshold (0.2, 0.4, 0.6, 0.8), blended over the softness.
const bandAt = (at: ScalarInput): Scalar => smoothstep(sub(at, bandSoftness), add(at, bandSoftness), field);
const bands = mix(mix(mix(mix(band0, band1, bandAt(0.2)), band2, bandAt(0.4)), band3, bandAt(0.6)), band4, bandAt(0.8));
const meadow = smoothstep(0.3, 0.7, fractal(add(div(world.xy, meadowScale), vec2(7.3, 2.9)), seed("meadow")));
const meadowTint = mix(vec3(1, 1, 1), mix(cool, warm, meadow), meadowAmount);
// A fine grain over it all, as the concrete's darkness fields: a few percent lighter or darker, about 5 to 10 cm across.
const grained = mul(bands, add(1, mul(sub(fractal(add(mul(world.xy, grainScale), vec2(3.3, 8.8)), seed("grain")), 0.5), mul(grain, 2))));
const patched = mul(meadowTint, pigment(grained, mul(noiseUv, 1.8), hueAmount, hueAngle, seed("pigment")));

// Grass strokes, so the ground reads as a grass field even from above: streaks of noise drawn long along one of three
// fixed directions (combed grass), the directions blended by a soft mask over the ground (stroke direction scale),
// sharpened into strokes. Each stroke lightens or darkens the paint a little and tips the shading normal across
// itself, so the shade band breaks along strokes. Fixed turns, not a varying one: a turn that varies over the ground
// would swirl the pattern far from the origin.
const streakAt = (angle: ScalarInput, key: ScalarInput): Scalar =>
  smoothstep(0.3, 0.7, valueNoise(mul(rotate(world.xy, angle), vec2(strokeLength, strokeWidth)), key));
const sa = streakAt(0.5, seed("strokes-a"));
const sb = streakAt(2.6, seed("strokes-b"));
const sc = streakAt(4.4, seed("strokes-c"));
const pick = fractal(add(div(world.xy, flowScale), vec2(17.3, 2.1)), seed("flow"));
const wa = smoothstep(0.55, 0.7, pick);
const wc = sub(1, smoothstep(0.3, 0.45, pick));
const streak = mix(mix(sb, sa, wa), sc, wc);
// Across the stroke, as each direction's across is blended.
const acrossX = mix(mix(neg(sin(2.6)), neg(sin(0.5)), wa), neg(sin(4.4)), wc);
const acrossY = mix(mix(cos(2.6), cos(0.5), wa), cos(4.4), wc);
const tip = mul(sub(streak, 0.5), strokeTilt);
const strokeNormal = normalize(add(normal, vec3(mul(acrossX, tip), mul(acrossY, tip), 0)));
const ground = mul(patched, add(1, mul(sub(streak, 0.5), strokeTone)));

const edge = edgeBreak({ size: 0.35, amount: 0.05 });

export default defineMaterial({
  name: "Washroom painted lawn",
  shading: "painted",
  base: { baseColor: ground, roughness: 0.95 },
  paint: mul(ground, mix(shadeTint, vec3(1, 1, 1), shadeBand(strokeNormal, sunDirection, add(threshold, edge.offset), softness, range))),
});
