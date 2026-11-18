// The washroom's lawn (GRASS=1, roadmap stage 6): a painted ground of five bands of green, close together as the
// concrete's tones are, so every difference is subtle, chosen by a fractal field and blended softly between
// neighbouring bands. The field is the Kowloon concrete's eroded, warped fracture (kowloon-noise.material.ts) with
// finer fractal noise added on top, so a band's edge wanders at every scale and the bands break into fragments rather
// than nesting as rings of a contour map. A slow meadow-scale tint and the concrete's pigment drift go over it. Shaded
// by the sun's shade band with a world-anchored edge break. Its base color (the bands without the shade) is what the
// scene's `ground.color` field holds, so the grass rooted on it (painted-grass.material.ts) takes the same greens and
// applies the same band at each root. Keep the band's parameters equal in both recipes.
import { defineMaterial, param, vec2, vec3, mix, mul, add, sub, div, neg, sin, cos, rotate, normalize, floor, fract, step, abs, clamp, world, shadeBand, sunDirection, normal, edgeBreak, seed, smoothstep } from "@superspec/material/v1";
import type { Scalar, ScalarInput, Vec2, Vec3 } from "@superspec/material/v1";
import { fracture, pigment, fractal, cellRandom } from "./kowloon-noise.material";

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
const strokeCell = param.scalar({ default: 0.12, range: [0.02, 1], unit: "m", label: "Stroke spacing" });
const strokeLength = param.scalar({ default: 0.85, range: [0.1, 1], label: "Stroke length (of its cell)" });
const strokeWidth = param.scalar({ default: 0.14, range: [0.01, 0.5], label: "Stroke width (of its cell)" });
const strokeTilt = param.scalar({ default: 0.7, range: [0, 3], label: "Stroke normal tilt" });
const strokeTone = param.scalar({ default: 0.1, range: [0, 0.5], label: "Stroke tone" });
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

// Grass strokes, so the ground reads as a grass field even from above, scattered as blades are: one stroke in each
// cell of a grid at a random place, turned any way, lighter or darker at random; three such layers of different
// cell sizes and offsets lie over one another, so no grid shows. A stroke is a thin pointed oval. Each lightens or
// darkens the paint a little (Stroke tone) and tips the shading normal across itself (Stroke normal tilt), so the
// shade band breaks stroke by stroke.
// The cell's random draw: x and y place the stroke, z turns it and picks lighter or darker.
const strokeDraw = (p: Vec2, size: ScalarInput, key: ScalarInput): Vec3 => cellRandom(floor(div(p, size)), key);
const strokeAngle = (r: Vec3): Scalar => mul(r.z, 6.2831853);
// The point in the stroke's own frame: x along it, y across, from its randomly placed centre.
const strokeLocal = (p: Vec2, size: ScalarInput, r: Vec3): Vec2 =>
  rotate(sub(p, mul(add(floor(div(p, size)), add(vec2(0.3, 0.3), mul(vec2(r.x, r.y), 0.4))), size)), strokeAngle(r));
const strokeAlong = (local: Vec2, size: ScalarInput): Scalar => div(local.x, mul(size, mul(strokeLength, 0.5)));
// Pointed: the half width narrows toward both ends.
const strokeHalfWidth = (along: ScalarInput, size: ScalarInput): Scalar => mul(mul(size, mul(strokeWidth, 0.5)), sub(1, mul(along, along)));
const strokeCover = (local: Vec2, along: ScalarInput, halfWidth: ScalarInput): Scalar =>
  mul(sub(1, smoothstep(0.85, 1, abs(along))), sub(1, smoothstep(mul(halfWidth, 0.6), halfWidth, abs(local.y))));
// x: coverage signed lighter (+) or darker (-); y and z: the across direction weighted by coverage.
const strokeOf = (cover: ScalarInput, r: Vec3): Vec3 =>
  vec3(mul(cover, sub(mul(step(0.5, fract(mul(r.z, 7.13))), 2), 1)), mul(neg(sin(strokeAngle(r))), cover), mul(cos(strokeAngle(r)), cover));
const strokeLayer = (p: Vec2, size: ScalarInput, r: Vec3): Vec3 =>
  strokeOf(strokeCover(strokeLocal(p, size, r), strokeAlong(strokeLocal(p, size, r), size),
    strokeHalfWidth(strokeAlong(strokeLocal(p, size, r), size), size)), r);
const p1 = world.xy;
const p2 = add(world.xy, vec2(0.37, 0.11));
const p3 = add(world.xy, vec2(0.13, 0.52));
const size2 = mul(strokeCell, 0.83);
const size3 = mul(strokeCell, 1.21);
const s1 = strokeLayer(p1, strokeCell, strokeDraw(p1, strokeCell, seed("strokes-a")));
const s2 = strokeLayer(p2, size2, strokeDraw(p2, size2, seed("strokes-b")));
const s3 = strokeLayer(p3, size3, strokeDraw(p3, size3, seed("strokes-c")));
const strokes = add(add(s1, s2), s3);
const tone = clamp(strokes.x, -1, 1);
const tip = mul(tone, strokeTilt);
const strokeNormal = normalize(add(normal, vec3(mul(strokes.y, tip), mul(strokes.z, tip), 0)));
const ground = mul(patched, add(1, mul(tone, strokeTone)));

const edge = edgeBreak({ size: 0.35, amount: 0.05 });

export default defineMaterial({
  name: "Washroom painted lawn",
  shading: "painted",
  base: { baseColor: ground, roughness: 0.95 },
  paint: mul(ground, mix(shadeTint, vec3(1, 1, 1), shadeBand(strokeNormal, sunDirection, add(threshold, edge.offset), softness, range))),
});
