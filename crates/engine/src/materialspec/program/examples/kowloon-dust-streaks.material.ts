// Pale dust washed down from an edge, as a weathering layer: the lightening twin of kowloon-grime-streaks, for dark
// surfaces where grime cannot show (window glass and painted frames under a window's head, dark shutters, a dark
// plinth). Rain carries the dust off the ledge above and leaves it in runs that dry pale: a film of the colour, laid
// over the surface rather than multiplied into it.
//
// It hangs in the zones the slot follows as `zone` (as grime-streaks does: `u` metres along the edge, `v` metres down
// from it, an identity per zone), reads the same cached streak tile, and stops where its zone ends. Nothing is drawn
// outside a zone.
import { defineLayer, param, zoneCoord, zoneExtent, sampleAlpha, valueNoise, seed, smoothstep, clamp, max, min, add, mul, sub, div, fract, vec2, vec3 } from "@superspec/material/v1";

const color = param.vec3({ default: vec3(0.62, 0.6, 0.55), range: [0, 1], label: "Dust colour" });
const strength = param.scalar({ default: 0.55, range: [0, 1], label: "Strength" });
const streakLength = param.scalar({ default: 0.6, range: [0.05, 5], unit: "m", label: "Longest run" });
const period = param.scalar({ default: 1.2, range: [0.2, 20], unit: "m", label: "Repeat along the edge" });
const film = param.scalar({ default: 0.25, range: [0, 1], label: "Film right under the edge" });
const filmDepth = param.scalar({ default: 0.08, range: [0.005, 0.5], unit: "m", label: "Film depth" });

const zone = zoneCoord("zone");
const extent = zoneExtent("zone");
const reach = mul(streakLength, add(0.7, mul(0.6, fract(mul(zone.z, 7.31)))));
const place = fract(mul(zone.z, 13.7));
// The tile is read from its last row up, its streak heads at the edge (see kowloon-grime-streaks).
const tile = vec2(add(div(zone.x, period), place), clamp(sub(1, div(zone.y, reach)), 0.03, 0.996));
const reached = sub(1, smoothstep(0.8, 1, div(zone.y, reach)));
const ended = sub(1, smoothstep(0.7, 1, div(zone.y, max(extent.y, 0.05))));
const patchy = add(0.5, mul(0.5, valueNoise(vec2(add(mul(zone.x, 2.1), mul(zone.z, 41)), 0.5), seed("dust-patchy"))));
const runs = mul(mul(sampleAlpha("superspec-material:grime-streaks.png", tile), mul(reached, ended)), patchy);
const head = mul(film, sub(1, smoothstep(mul(filmDepth, 0.3), filmDepth, zone.y)));

export default defineLayer({
  name: "Kowloon dust streaks",
  coverage: mul(extent.x, min(mul(add(runs, head), strength), 1)),
  baseColor: color,
  roughness: 0.9,
});
