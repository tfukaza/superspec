// Grime streaking down from an edge, as a weathering layer: a generic deposit that hangs in the zones the slot follows
// as `zone` (runoff under a storey's top, a sill, a coping, a balcony ledge, the drip of an air conditioner: whatever
// zones the geometry's owner states), with its colour, strength, length and how often it repeats.
//
// The owner's zones say where every texel hangs: `u` metres along its edge (running on from side to side and round a
// corner) and `v` metres below it along the surface's downhill, 0 at the edge. Nothing above an edge is in its zone,
// so streaks always start at their edge and never climb, and where two zones overlap the nearest edge above wins.
// The streaks themselves are one cached tile, `superspec-material:grime-streaks.png` (written by
// tools/grime/streak-pattern.mjs from the Kowloon wall condition's runoff): across it along the edge over one period,
// down it from the edge to the longest streak. Each zone takes its own place in the tile and its own length from its
// identity, so storeys and sills do not repeat one another; a column fades out where its zone ends (at the next edge
// or an opening's head below), and a dark lip lies right under every edge.
import { defineLayer, param, zoneCoord, zoneExtent, sampleAlpha, smoothstep, clamp, max, min, add, mul, sub, div, fract, vec2, vec3 } from "@superspec/material/v1";

const color = param.vec3({ default: vec3(0.36, 0.34, 0.31), range: [0, 1], label: "Grime colour" });
const strength = param.scalar({ default: 0.8, range: [0, 2], label: "Strength" });
const streakLength = param.scalar({ default: 2.8, range: [0.1, 10], unit: "m", label: "Longest streak" });
const period = param.scalar({ default: 4, range: [0.5, 20], unit: "m", label: "Repeat along the edge" });
const shift = param.scalar({ default: 0, range: [0, 100], unit: "m", label: "Pattern offset" });
const variety = param.scalar({ default: 0.5, range: [0, 1], label: "Length varies by zone" });
const lip = param.scalar({ default: 0.55, range: [0, 1], label: "Lip under the edge" });
const lipDepth = param.scalar({ default: 0.05, range: [0.005, 0.3], unit: "m", label: "Lip depth" });

const zone = zoneCoord("zone");
const extent = zoneExtent("zone");
// Each zone's own length (from 1 - variety to 1 + variety of the longest streak) and place in the tile.
const reach = mul(streakLength, add(sub(1, mul(variety, 0.5)), mul(variety, fract(mul(zone.z, 7.31)))));
const place = add(div(shift, period), fract(mul(zone.z, 13.7)));
// The tile is read from its last row up (an image's rows run upward here): its first row, the streaks' heads, at the
// edge. Both ends are kept clear of its wrap, so a streak is full strength right under its edge.
const tile = vec2(add(div(zone.x, period), place), clamp(sub(1, div(zone.y, reach)), 0.03, 0.996));
// Faded out toward the longest streak and toward where the zone ends in this column.
const reached = sub(1, smoothstep(0.85, 1, div(zone.y, reach)));
const ended = sub(1, smoothstep(0.7, 1, div(zone.y, max(extent.y, 0.05))));
const under = mul(lip, sub(1, smoothstep(mul(lipDepth, 0.3), lipDepth, zone.y)));
const streaks = mul(mul(sampleAlpha("superspec-material:grime-streaks.png", tile), mul(reached, ended)), strength);
const coverage = mul(extent.x, min(add(streaks, under), 1));

export default defineLayer({
  name: "Kowloon grime streaks",
  coverage,
  baseColor: color,
  blend: { baseColor: "multiply" },
});
