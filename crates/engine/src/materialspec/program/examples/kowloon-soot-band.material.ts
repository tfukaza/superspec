// The sooty plinth of a street wall, as a weathering layer: a dark band from the ground line (the feature the slot
// names `base`) up to `reach`, as traffic film, mop water and decades of hands leave it. Its top is ragged and wanders
// along the wall, the band is mottled and scuffed lighter in places, a damp line lies where it meets the pavement, and
// above it a haze of soot thins out in vertical streaks. A receiver without the feature shows nothing. It mixes its
// colour over the surface beneath (a deposit thick enough to hide the paint), so the strength is how opaque it gets.
import { defineLayer, param, featureDistance, smoothstep, valueNoise, exp, neg, add, mul, sub, div, min, max, vec2, vec3, seed, surface } from "@superspec/material/v1";
import { fractal } from "./kowloon-noise.material";

const color = param.vec3({ default: vec3(0.034, 0.037, 0.032), range: [0, 1], label: "Soot colour" });
const strength = param.scalar({ default: 0.92, range: [0, 1], label: "Strength" });
const reach = param.scalar({ default: 1, range: [0.05, 4], unit: "m", label: "Band height" });
const ragged = param.scalar({ default: 0.16, range: [0, 1], unit: "m", label: "Ragged top" });
const haze = param.scalar({ default: 1.1, range: [0.05, 4], unit: "m", label: "Haze above the band" });
const hazeStrength = param.scalar({ default: 0.5, range: [0, 1], label: "Haze strength" });

// Meters above the ground line, and along the wall on the pattern anchor (so the band runs on round a corner).
const above = featureDistance("base", 8);
const present = sub(1, smoothstep(7, 7.8, above));
const run = surface.x;

// The band's top: broad wander plus short drips and nicks.
const wander = sub(mul(fractal(vec2(mul(run, 0.9), 3.1), seed("soot-edge")), 2), 1);
const nicks = sub(valueNoise(vec2(mul(run, 19), 0.5), seed("soot-nicks")), 0.5);
const top = max(add(add(reach, mul(ragged, wander)), mul(mul(ragged, 0.35), nicks)), 0.02);
const band = sub(1, smoothstep(sub(top, 0.025), add(top, 0.025), above));

// Inside the band: mottled, scuffed lighter where it is rubbed, and darkest in the damp at the pavement.
const at = vec2(run, above);
const mottle = fractal(add(mul(at, vec2(2.3, 3.1)), vec2(7.7, 1.3)), seed("soot-mottle"));
const scuffs = smoothstep(0.6, 0.82, fractal(add(mul(at, vec2(7, 4.5)), vec2(31.4, 9.9)), seed("soot-scuffs")));
const damp = exp(neg(div(above, 0.09)));
const body = mul(band, min(add(sub(add(0.72, mul(0.28, mottle)), mul(scuffs, 0.3)), mul(damp, 0.4)), 1));

// Above it: soot thinning out with height, in columns of their own strength.
const columns = add(0.35, mul(0.65, valueNoise(vec2(mul(run, 5.3), mul(above, 0.35)), seed("soot-columns"))));
const lift = max(sub(above, top), 0);
const rising = mul(mul(mul(sub(1, band), exp(neg(div(lift, mul(haze, 0.4))))), columns), hazeStrength);

export default defineLayer({
  name: "Kowloon soot band",
  coverage: mul(mul(strength, present), max(body, rising)),
  baseColor: color,
  roughness: 0.95,
});
