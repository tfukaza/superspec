// Wear rising from the foot of a door, a shutter or a gate, as a weathering layer: where boots, mops and splashing
// rain reach, the paint is chipped and the steel beneath has rusted, worst right at the foot and thinning out upward in
// a ragged front, with chips scattered above it and along the leaf's edges. It follows the feature the slot names
// `foot` (the line along the bottom of the leaf), and its colour is the bare surface's: rust by default.
import { defineLayer, param, featureDistance, creaseDistance, smoothstep, valueNoise, add, mul, sub, min, max, vec2, vec3, seed, surface } from "@superspec/material/v1";

const color = param.vec3({ default: vec3(0.33, 0.17, 0.08), range: [0, 1], label: "Bare colour" });
const reach = param.scalar({ default: 0.35, range: [0.02, 2], unit: "m", label: "How high it reaches" });
const strength = param.scalar({ default: 0.85, range: [0, 1], label: "Strength" });
const chips = param.scalar({ default: 0.5, range: [0, 1], label: "Chips above and on the edges" });

const at = vec2(surface.x, surface.y);
const up = featureDistance("foot", 3);
// Only the leaf above a foot line: a slot's layers reach every receiver of the slot, so nothing is drawn further than
// a door's height from a foot.
const near = sub(1, smoothstep(2.1, 2.4, up));
// A ragged front: the reach wanders along the foot.
const front = mul(reach, add(0.45, mul(0.75, valueNoise(vec2(mul(surface.x, 3.5), 0.5), seed("foot-front")))));
const fine = valueNoise(mul(at, 55), seed("foot-fine"));
const foot = sub(1, smoothstep(mul(front, 0.4), front, add(up, mul(sub(fine, 0.5), 0.06))));
// Chips: small and crisp, a few above the front and more along the leaf's edges.
const edges = sub(1, smoothstep(0.004, 0.03, creaseDistance("exposed", 35, 0.04)));
// More chips the more `chips` asks, and far more of them along the edges, where the leaf is knocked.
const field = add(mul(valueNoise(mul(at, 38), seed("foot-chips")), 0.6), mul(valueNoise(mul(at, 9), seed("foot-chip-groups")), 0.4));
const cut = sub(0.8, mul(chips, 0.06));
const chipped = mul(mul(min(chips, 1), smoothstep(cut, add(cut, 0.02), field)), mul(near, add(0.12, mul(0.88, edges))));

export default defineLayer({
  name: "Kowloon foot wear",
  coverage: mul(strength, min(max(foot, chipped), 1)),
  baseColor: color,
  roughness: 0.95,
});
