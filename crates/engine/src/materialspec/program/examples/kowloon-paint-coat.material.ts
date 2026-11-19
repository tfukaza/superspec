// A coat of emulsion paint over a Kowloon render, as a weathering layer: a colour (the reference
// shop house's cream by default) mottled a little, and broken where it has peeled into the grey
// patches of the surface beneath, which come in drifts of warped fractal noise on the part's
// pattern anchor. It reads nothing of the receiver but its position, so a wall that is resized keeps
// its patches where they were.
//
// Placed peels: where the slot follows `peel` (a closed outline the owner draws) and `peelInner` (the same outline
// `peelInset` further in; the two distances together say which side of the outline a texel is on), the coat has come
// off in a sheet inside the outline: a ragged, crisp-edged patch through which the render beneath shows with its own
// texture, ringed by a thin dark line of lifted paint. A slot that follows neither has none. The coat also thins to
// the render along exposed arrises (`arrisWear`), broken along their length.
import { defineLayer, param, smoothstep, step, floor, abs, min, max, hash, valueNoise, featureDistance, creaseDistance, select, lt, clamp, add, mul, sub, div, vec2, vec3, mix, seed, surface } from "@superspec/material/v1";
import { fractal, domainWarp } from "./kowloon-noise.material";

const color = param.vec3({ default: vec3(0.78, 0.71, 0.55), range: [0, 1], label: "Paint colour" });
const amount = param.scalar({ default: 1, range: [0, 1], label: "Paint amount" });
const peeling = param.scalar({ default: 0.35, range: [0, 1], label: "Peeling" });
const patchScale = param.scalar({ default: 2.4, range: [0.1, 8], unit: "1/m", label: "Patch frequency" });
const patches = param.scalar({ default: 0, range: [0, 1], label: "Repainted repairs" });
const unevenness = param.scalar({ default: 1, range: [0, 1], label: "Uneven tone" });
const peelShare = param.scalar({ default: 0.6, range: [0, 1], label: "Peeled share of a peel frame" });
const peelInset = param.scalar({ default: 0.05, range: [0.01, 0.3], unit: "m", label: "Peel frame inset" });
const bare = param.vec3({ default: vec3(0.42, 0.41, 0.38), range: [0, 1], label: "Bare render tone" });
const bareCover = param.scalar({ default: 0, range: [0, 1], label: "Bare render toned down" });
const arrisWear = param.scalar({ default: 0, range: [0, 1], label: "Paint worn off the arrises" });

const at = mul(vec2(surface.x, surface.y), patchScale);
const warped = add(at, mul(domainWarp(mul(at, 1.3), seed("paint-warp")), 0.5));
// Peeled where the field rises above a threshold that falls as `peeling` grows.
const field = fractal(add(warped, vec2(17.3, 5.9)), seed("paint-peel"));
const threshold = sub(0.84, mul(peeling, 0.3));
const intact = sub(1, smoothstep(threshold, add(threshold, 0.02), field));
const mottle = fractal(add(mul(at, 3.1), vec2(3.7, 41.2)), seed("paint-mottle"));

// Repainted repairs: where plaster was patched over the years the new paint never matched. Rectangles on a lattice
// of 2.1 by 1.9 m bays, most bays holding one, each a little lighter, darker, cooler or warmer than the coat, with a
// ragged brushed edge.
const bay = vec2(2.1, 1.9);
const cell = floor(div(vec2(surface.x, surface.y), bay));
const within = sub(vec2(surface.x, surface.y), mul(cell, bay));
const middle = add(vec2(0.7, 0.6), mul(vec2(hash(cell, seed("patch-x")), hash(cell, seed("patch-y"))), vec2(0.7, 0.7)));
const halfSize = add(vec2(0.22, 0.18), mul(vec2(hash(cell, seed("patch-w")), hash(cell, seed("patch-h"))), vec2(0.42, 0.34)));
const ragged = mul(sub(valueNoise(mul(vec2(surface.x, surface.y), 47), seed("patch-edge")), 0.5), 0.012);
const inside = sub(halfSize, abs(sub(within, middle)));
const patch = mul(mul(step(hash(cell, seed("patch-on")), mul(patches, 0.65)), smoothstep(0, 0.006, add(min(inside.x, inside.y), ragged))), intact);
const shade = sub(hash(cell, seed("patch-shade")), 0.5);
const warmth = sub(hash(cell, seed("patch-warmth")), 0.5);
const patchTone = add(vec3(add(1, mul(shade, 0.22))), mul(vec3(0.05, 0.01, -0.05), warmth));
// The coat is a little darker and dirtier toward the edge of a peeled patch, where it is lifting.
const rim = smoothstep(sub(threshold, 0.012), threshold, field);
// Placed peels: how far inside the outline a texel is (0 on it and outside it, up to the inset), then a broad field
// pulled down toward the outline, peeled where it stays above a threshold.
const peelReach = 3;
const peelEdge = featureDistance("peel", peelReach);
const peelDepth = select(lt(peelEdge, sub(peelReach, 0.05)),
  clamp(mul(add(sub(peelEdge, featureDistance("peelInner", peelReach)), peelInset), 0.5), 0, peelInset), 0);
const flat = vec2(surface.x, surface.y);
const sheet = add(mul(valueNoise(mul(flat, 2.6), seed("peel-broad")), 0.6), mul(valueNoise(mul(flat, 11), seed("peel-fine")), 0.4));
const peelField = mul(sheet, div(peelDepth, peelInset));
const peelThreshold = sub(0.64, mul(peelShare, 0.3));
const peeled = smoothstep(peelThreshold, add(peelThreshold, 0.008), peelField);
// The lifted edge: a thin dark line just outside the bare patch.
const lifted = mul(sub(1, peeled), smoothstep(sub(peelThreshold, 0.05), sub(peelThreshold, 0.008), peelField));
// Arrises: the coat thins to the render within a few centimetres of an exposed edge, in broken lengths.
const arris = creaseDistance("exposed", 35, 0.06);
const worn = mul(mul(arrisWear, sub(1, smoothstep(0.008, 0.035, arris))),
  smoothstep(0.45, 0.62, valueNoise(mul(flat, vec2(9, 3.5)), seed("paint-arris"))));
const coat = mul(mul(mix(mul(color, sub(1, mul(0.14, unevenness))), color, smoothstep(0.3, 0.7, mottle)), sub(1, mul(0.22, rim))),
  sub(1, mul(0.62, lifted)));

export default defineLayer({
  name: "Kowloon paint coat",
  // Inside a peel the render shows through, toned down toward `bare` by `bareCover` so its own texture stays.
  coverage: max(mul(mul(amount, intact), mul(sub(1, peeled), sub(1, worn))), mul(peeled, bareCover)),
  baseColor: mix(mul(coat, mix(vec3(1), patchTone, patch)), bare, peeled),
  roughness: 0.82,
});
