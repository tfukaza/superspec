// Honey-colored bench planks: grain along the plank, a tone per plank, pale worn edges and
// darker handling grime.
import {
  defineMaterial, layer, param, vec2, vec3, add, sub, mul, div, mix, abs, dot, floor, fract, smoothstep,
  fbm, hash, valueNoise, seed, directionalCoordinates, edgeWear, roundedEdges, world,
} from "@superspec/material/v1";

const light = param.vec3({ default: vec3(0.34, 0.14, 0.035), range: [0, 1], label: "Light wood" });
const dark = param.vec3({ default: vec3(0.12, 0.045, 0.012), range: [0, 1], label: "Dark grain" });
const direction = param.vec3({ default: vec3(0, 1, 0), range: [-1, 1], label: "Grain direction" });
const across = param.vec3({ default: vec3(1, 0, 0), range: [-1, 1], label: "Direction across the planks" });
const plankPitch = param.scalar({ default: 0.155, range: [0.02, 2], unit: "m", label: "Plank spacing" });
const plankStart = param.scalar({ default: 0.0225, range: [-10, 10], unit: "m", label: "First plank edge" });

const plank = floor(div(sub(dot(world, across), plankStart), plankPitch));
const shade = hash(plank, seed("plank"));
const grain = directionalCoordinates({ direction: direction, scale: vec2(0.45, 0.02) });
// Each plank is cut from a different part of the tree.
const cut = add(grain.coordinates, vec2(mul(shade, 53), mul(plank, 19.7)));
const growth = fbm(mul(cut, vec2(1, 0.22)), seed("growth"), 3);
const rings = abs(sub(fract(mul(growth, 7)), 0.5));
const line = smoothstep(0.55, 0.95, mul(rings, 2));
const fiber = valueNoise(mul(cut, vec2(4, 2.5)), seed("fiber"));
const grime = fbm(div(world, 0.25), seed("grime"), 3);

const wear = edgeWear({ width: 0.012, depth: 0.0008, breakup: 0.05, jitter: 0.7 });
const edges = roundedEdges({ radius: 0.006 });

export default defineMaterial({
  name: "Washroom bench wood",
  base: {
    baseColor: mul(mix(light, dark, add(mul(line, 0.7), mul(fiber, 0.15))), add(0.75, mul(shade, 0.5))),
    roughness: add(0.5, mul(line, 0.2)),
    metalness: 0,
    height: sub(mul(fiber, 0.0002), mul(line, 0.0005)),
  },
  layers: [
    layer({ id: "grime", mask: mul(smoothstep(0.5, 0.8, grime), 0.5), baseColor: vec3(0.16, 0.1, 0.05), roughness: 0.75 }),
    layer({ id: "wear", mask: mul(wear.coverage, 0.7), baseColor: vec3(0.45, 0.25, 0.09), roughness: 0.8, height: { add: -0.0008 } }),
  ],
  channels: { receiverNormal: edges.receiverNormal },
});
