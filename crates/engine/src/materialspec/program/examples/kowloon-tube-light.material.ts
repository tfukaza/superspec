// A lit fluorescent tube or lamp diffuser: a pale body that glows a warm white, a little dimmer and browner toward
// its ends where the tube has aged, with a film of dust that takes some of the light.
import { defineMaterial, param, vec3, sub, mul, div, mix, max, min, clamp, smoothstep, world, boundsMin, boundsMax } from "@superspec/material/v1";

const tint = param.vec3({ default: vec3(1, 0.9, 0.72), range: [0, 1], label: "Light colour" });
const glow = param.scalar({ default: 2.2, range: [0, 12], label: "Glow" });
const aged = param.scalar({ default: 0.4, range: [0, 1], label: "Darkened ends" });
const dust = param.scalar({ default: 0.25, range: [0, 1], label: "Dust" });

// Along the receiver's longest horizontal extent, 0 at one end and 1 at the other.
const extent = sub(boundsMax, boundsMin);
const longX = smoothstep(-0.001, 0.001, sub(extent.x, extent.y));
const along = clamp(mix(div(sub(world.y, boundsMin.y), max(extent.y, 0.01)), div(sub(world.x, boundsMin.x), max(extent.x, 0.01)), longX), 0, 1);
const ends = mul(aged, sub(1, smoothstep(0, 0.12, min(along, sub(1, along)))));
const lit = mul(sub(1, mul(ends, 0.7)), sub(1, mul(dust, 0.35)));

export default defineMaterial({
  name: "Kowloon tube light",
  base: {
    baseColor: mix(vec3(0.85, 0.84, 0.8), vec3(0.4, 0.33, 0.24), ends),
    roughness: 0.5,
    metalness: 0,
    emissive: mul(tint, mul(glow, lit)),
  },
});
