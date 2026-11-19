// A lit fluorescent tube or lamp diffuser: a pale body that glows a warm white, a little dimmer and browner toward
// its ends where the tube has aged, with a film of dust that takes some of the light.
import { defineMaterial, param, vec2, vec3, add, sub, mul, div, mix, abs, max, min, clamp, smoothstep, valueNoise, seed, world, boundsMin, boundsMax } from "@superspec/material/v1";

const tint = param.vec3({ default: vec3(1, 0.9, 0.72), range: [0, 1], label: "Light colour" });
const glow = param.scalar({ default: 2.2, range: [0, 12], label: "Glow" });
const aged = param.scalar({ default: 0.4, range: [0, 1], label: "Darkened ends" });
const dust = param.scalar({ default: 0.25, range: [0, 1], label: "Dust" });
// For the surfaces of a lit room rather than the tube itself: the glow falls off away from the lamp under its
// ceiling (toward the floor and the room's ends), unevenly, so a room seen head-on has depth.
const falloff = param.scalar({ default: 0, range: [0, 1], label: "Room falloff" });

// Along the receiver's longest horizontal extent, 0 at one end and 1 at the other.
const extent = sub(boundsMax, boundsMin);
const longX = smoothstep(-0.001, 0.001, sub(extent.x, extent.y));
const along = clamp(mix(div(sub(world.y, boundsMin.y), max(extent.y, 0.01)), div(sub(world.x, boundsMin.x), max(extent.x, 0.01)), longX), 0, 1);
const ends = mul(aged, sub(1, smoothstep(0, 0.12, min(along, sub(1, along)))));
const lit = mul(sub(1, mul(ends, 0.7)), sub(1, mul(dust, 0.35)));
// A room: brightest high in the middle, under the lamp; a slow mottle keeps walls from reading flat.
const up = clamp(div(sub(world.z, boundsMin.z), max(extent.z, 0.01)), 0, 1);
const middle = sub(1, mul(abs(sub(along, 0.5)), 1.6));
const mottle = add(0.8, mul(0.4, valueNoise(mul(vec2(add(world.x, world.y), world.z), 1.3), seed("room-mottle"))));
const pool = mul(mul(add(0.25, mul(0.75, mul(up, up))), add(0.45, mul(0.55, middle))), mottle);
const room = mix(1, pool, falloff);

export default defineMaterial({
  name: "Kowloon tube light",
  base: {
    baseColor: mul(mix(vec3(0.85, 0.84, 0.8), vec3(0.4, 0.33, 0.24), ends), mix(1, add(0.55, mul(0.45, pool)), falloff)),
    roughness: 0.5,
    metalness: 0,
    emissive: mul(tint, mul(glow, mul(lit, room))),
  },
});
