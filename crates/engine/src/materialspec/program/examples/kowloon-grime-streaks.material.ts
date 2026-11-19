// Grime streaking down from an edge, as a weathering layer: a generic deposit that follows the feature the slot
// names `source` (a ledge, a sill, a roofline, the drip line under an air conditioner: any polyline its geometry's
// owner reports, so its endpoints are the edge's), with its colour, strength, length and how often it repeats.
//
// Nothing is evaluated per texel but the mapping: the streaks themselves are one cached tile,
// `superspec-material:grime-streaks.png` (written by tools/grime/streak-pattern.mjs from the Kowloon wall
// condition's runoff), in edge space: across it along the edge over one period, down it from the edge to the longest
// streak. Each texel finds where it hangs from the edge: its distance to the edge, turned into a drop along the
// surface's downhill direction (gravity projected onto the surface), and its position along the edge, moved back up
// that downhill line. So streaks fall with gravity whatever the edge's slope (the tile is skewed on a sloping ledge),
// a longer or shorter run stretches the same tile, and every edge of every wall repeats it. A point above its edge
// reads the same distance: the feature the slot names `guard` (below) tells the two sides apart.
import { defineLayer, param, featureDistance, featureTangent, sampleAlpha, smoothstep, select, lt, clamp, normalize, dot, cross, abs, max, min, add, mul, sub, div, vec2, vec3, world, normal, gravity } from "@superspec/material/v1";

const color = param.vec3({ default: vec3(0.36, 0.34, 0.31), range: [0, 1], label: "Grime colour" });
const strength = param.scalar({ default: 0.8, range: [0, 2], label: "Strength" });
const streakLength = param.scalar({ default: 2.8, range: [0.1, 10], unit: "m", label: "Longest streak" });
const period = param.scalar({ default: 4, range: [0.5, 20], unit: "m", label: "Repeat along the edge" });
const shift = param.scalar({ default: 0, range: [0, 100], unit: "m", label: "Pattern offset" });

const guardLift = param.scalar({ default: 0.05, range: [0.005, 1], unit: "m", label: "Guard offset above the edge" });

const distance = featureDistance("source", 10);
const along = featureTangent("source", 10);
// Which side of the edge: a distance alone cannot say. Where the owner also draws the same edge `guardLift` uphill as
// the feature the slot names `guard`, the two distances differ by the cosine of the angle off straight below: 1 under
// the edge, 0 level with its ends, negative above it. Streaks then hang only below the edge, fanning out a little past
// its ends as they fall, instead of ringing each end and climbing the wall above. Without a guard every side is below.
const guarded = featureDistance("guard", 12);
const beneath = select(lt(guarded, 11.5), clamp(div(sub(guarded, distance), guardLift), 0, 1), 1);
const under = smoothstep(0.9, 0.985, beneath);
// Downhill on the surface (straight down a flat one, so it is never zero), and the direction across the edge in the
// surface: a unit vector where the receiver has the edge, zero where it has none (then nothing is drawn).
const downhill = normalize(add(sub(gravity, mul(normal, dot(gravity, normal))), vec3(0, 0, -1e-4)));
const across = cross(normal, along);
const facing = dot(across, downhill);
// The drop along the downhill line from the edge to this point, and where on the edge that line starts.
const drop = div(mul(distance, max(beneath, 0.5)), max(abs(facing), 0.2));
const start = sub(dot(world, along), mul(drop, dot(downhill, along)));
const tile = vec2(div(add(start, shift), period), min(div(drop, streakLength), 0.999));
// Beyond the longest streak, or where the receiver has no such edge, nothing.
const reached = sub(1, smoothstep(0.97, 1, div(drop, streakLength)));
const coverage = mul(mul(mul(sampleAlpha("superspec-material:grime-streaks.png", tile), reached), under), strength);

export default defineLayer({
  name: "Kowloon grime streaks",
  coverage: min(coverage, 1),
  baseColor: color,
  blend: { baseColor: "multiply" },
});
