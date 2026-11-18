// Rain runoff streaking down from a ledge, as a weathering layer: the Kowloon wall condition's runoff
// (worldspec/themes/kowloon/wall.rs, `runoff_at` and `deposit_detail`) measured from the feature the slot names
// `source` instead of authored ledge records. Below the source each column of the wall gets a streak of its own
// strength and length (up to `length`, 2.8 m by default), fading as (1 - drop / reach)^1.4, with a dark lip right
// under the source; the deposit is broken up by the wall condition's fine vertical grain, so it reads as streaks
// rather than a band. `flow` is how much water the source sheds (a floor line 1, a sill 0.8). It darkens the surface
// beneath by multiplication, toward the wall condition's grey-brown pigment.
import { defineLayer, param, featureDistance, smoothstep, pow, max, min, add, mul, sub, div, vec2, vec3, seed, surface, valueNoise } from "@superspec/material/v1";
import { fractal } from "./kowloon-noise.material";

const grime = param.scalar({ default: 0.6, range: [0, 1], label: "Grime" });
const flow = param.scalar({ default: 1, range: [0, 1], label: "Flow" });
const longest = param.scalar({ default: 2.8, range: [0.3, 6], unit: "m", label: "Longest streak" });
// How dark the deposit is against the surface beneath; the wall condition's own deposits are 1.
const strength = param.scalar({ default: 1.8, range: [0, 4], label: "Strength" });

const u = surface.x;
const z = surface.y;
const drop = featureDistance("source", 6);

// Each column's streak: its strength (strong where the noise across the wall is high) and how far it runs.
const streak = smoothstep(0.3, 0.78, valueNoise(vec2(mul(u, 3.4), 0.7), seed("runoff-streak")));
const column = add(0.2, mul(0.8, streak));
const reach = mul(
  add(0.3, mul(mul(sub(longest, 0.3), column), valueNoise(vec2(mul(u, 2.1), 1.3), seed("runoff-reach")))),
  add(0.4, mul(0.6, flow)));
const streaked = mul(column, pow(max(sub(1, div(drop, reach)), 0), 1.4));
// The lip right under the source, where water pools before it runs.
const beneath = mul(0.55, sub(1, smoothstep(0, 0.3, drop)));
const stain = mul(flow, min(add(streaked, beneath), 1));

// The wall condition's deposit detail: fine streaks a few centimeters across that wander a little, broken
// patches and a grain.
const wander = mul(sub(valueNoise(vec2(mul(u, 3.1), mul(z, 1.4)), seed("runoff-wander")), 0.5), 0.06);
const fine = fractal(vec2(mul(add(u, wander), 17), mul(z, 2.3)), seed("runoff-fine"));
const broken = fractal(vec2(mul(u, 13), mul(z, 11)), seed("runoff-broken"));
const grain = valueNoise(vec2(mul(u, 71), mul(z, 67)), seed("runoff-grain"));
const detail = add(add(add(0.12, mul(0.62, smoothstep(0.36, 0.68, fine))), mul(0.18, broken)), mul(0.08, grain));

export default defineLayer({
  name: "Kowloon runoff",
  coverage: min(mul(mul(mul(grime, min(mul(stain, 0.85), 1)), detail), strength), 1),
  baseColor: vec3(0.36, 0.34, 0.31),
  blend: { baseColor: "multiply" },
});
