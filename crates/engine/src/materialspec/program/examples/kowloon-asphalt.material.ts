// An old asphalt street: a dark binder full of pale aggregate, worn greyer along the wheel tracks, cut and patched in
// darker rectangles where services were dug up, crazed with fine cracks, spotted with oil, and silted pale along the
// gutter (the receiver's contact edges with a kerb, where a project build supplies them). It lies in world meters, so
// the street runs on across receivers. `tracks` gives the direction traffic runs in.
import {
  defineMaterial, layer, param, vec2, vec3, add, sub, mul, div, mix, abs, min, max, floor, step, smoothstep, dot,
  hash, valueNoise, creaseDistance, seed, world, footprint,
} from "@superspec/material/v1";

const binder = param.vec3({ default: vec3(0.06, 0.062, 0.058), range: [0, 1], label: "Asphalt colour" });
const aggregate = param.scalar({ default: 0.6, range: [0, 1], label: "Aggregate" });
const wearing = param.scalar({ default: 0.5, range: [0, 1], label: "Worn tracks" });
const tracks = param.vec2({ default: vec2(0, 1), range: [-1, 1], label: "Traffic direction" });
const patches = param.scalar({ default: 0.3, range: [0, 1], label: "Patched cuts" });
const cracks = param.scalar({ default: 0.5, range: [0, 1], label: "Cracks" });
const stains = param.scalar({ default: 0.5, range: [0, 1], label: "Oil and silt" });

const at = vec2(world.x, world.y);
// Stones in the binder, and broad uneven fading.
const stones = smoothstep(0.62, 0.8, valueNoise(mul(at, 95), seed("asphalt-stones")));
const fines = valueNoise(mul(at, 310), seed("asphalt-fines"));
const faded = add(0.85, mul(0.4, valueNoise(mul(at, 0.8), seed("asphalt-fade"))));
// Wheel tracks: two polished bands per lane, 3 m apart, across the traffic direction.
const lane = dot(at, vec2(tracks.y, sub(0, tracks.x)));
const inLane = sub(lane, mul(floor(div(lane, 3)), 3));
const track = mul(wearing, max(sub(1, smoothstep(0.15, 0.45, abs(sub(inLane, 0.75)))), sub(1, smoothstep(0.15, 0.45, abs(sub(inLane, 2.25))))));
const surfaceTone = mul(binder, mul(faded, add(1, mul(track, 0.5))));
const stony = mix(surfaceTone, vec3(0.27, 0.26, 0.23), mul(mul(aggregate, stones), add(0.5, mul(0.5, track))));
const grained = mul(stony, add(0.9, mul(0.2, fines)));

// Reinstated cuts: darker, smoother rectangles on a coarse lattice, some long and thin.
const cut = floor(div(at, vec2(2.6, 3.4)));
const here = sub(at, mul(cut, vec2(2.6, 3.4)));
const middle = add(vec2(0.9, 1.2), mul(vec2(hash(cut, seed("cut-x")), hash(cut, seed("cut-y"))), vec2(0.8, 1)));
const halfSize = add(vec2(0.25, 0.3), mul(vec2(hash(cut, seed("cut-w")), hash(cut, seed("cut-h"))), vec2(0.5, 0.8)));
const inside = sub(halfSize, abs(sub(here, middle)));
const patch = mul(step(sub(1, patches), hash(cut, seed("cut-on"))), smoothstep(0, 0.012, min(inside.x, inside.y)));
const patchRim = mul(patch, sub(1, smoothstep(0.012, 0.03, min(inside.x, inside.y))));

// Crazing: the walls between cells of a warped cellular field, in the weaker areas only.
const warped = add(mul(at, 1.6), mul(vec2(valueNoise(mul(at, 2.3), seed("craze-wx")), valueNoise(add(mul(at, 2.3), 9.1), seed("craze-wy"))), 0.6));
const weak = smoothstep(0.5, 0.75, valueNoise(mul(at, 0.45), seed("craze-where")));
const pixel = max(mul(footprint, 1.6), 0.002);
// The level lines of two noises, crossing: a mesh of cracks that wander and close on themselves.
const crazeA = abs(sub(valueNoise(warped, seed("craze-a")), 0.5));
const crazeB = abs(sub(valueNoise(add(mul(warped, 1.37), vec2(11.3, 4.9)), seed("craze-b")), 0.5));
const crazing = mul(mul(cracks, weak), sub(1, smoothstep(0.004, add(0.01, pixel), min(crazeA, crazeB))));

// Oil in dark soft spots; silt and litter dust pale along the gutter.
const oil = mul(stains, smoothstep(0.7, 0.85, valueNoise(add(mul(at, 1.3), vec2(5.5, 2.2)), seed("oil"))));
const gutter = creaseDistance("contact", 30, 0.8);
const silt = mul(mul(stains, sub(1, smoothstep(0.03, 0.6, gutter))), add(0.4, mul(0.6, valueNoise(mul(at, vec2(1.1, 6)), seed("silt")))));

export default defineMaterial({
  name: "Kowloon asphalt",
  base: { baseColor: grained, roughness: sub(0.95, mul(track, 0.2)), metalness: 0, height: mul(stones, 0.0012) },
  layers: [
    layer({ id: "patch", mask: mul(patch, 0.75), baseColor: mul(binder, 0.62), roughness: 0.85 }),
    layer({ id: "rim", mask: patchRim, baseColor: mul(binder, 0.35), roughness: 0.9, height: { add: -0.002 } }),
    layer({ id: "silt", mask: mul(silt, 0.7), baseColor: vec3(0.3, 0.28, 0.23), roughness: 1 }),
    layer({ id: "oil", mask: mul(oil, 0.7), baseColor: vec3(0.012, 0.012, 0.012), roughness: 0.5 }),
    layer({ id: "crazing", mask: crazing, baseColor: vec3(0.012, 0.012, 0.011), roughness: 1, height: { add: -0.002 } }),
  ],
});
