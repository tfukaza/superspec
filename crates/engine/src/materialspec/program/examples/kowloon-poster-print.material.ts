// Printed paper that has hung outdoors: the catalog's poster sheet by the mesh's own UVs (a fixture that draws a
// notice, a calendar or a price card names its cell there), bleached toward old paper in clouds, streaked by rain from
// its top and soiled along its edges. Notices pasted straight onto a wall are the kowloon-notices layer.
import {
  defineMaterial, param, vec2, vec3, add, sub, mul, mix, min, smoothstep, valueNoise, sampleColor, creaseDistance, seed, uv, world,
} from "@superspec/material/v1";

const fading = param.scalar({ default: 0.4, range: [0, 1], label: "Fading" });
const grime = param.scalar({ default: 0.5, range: [0, 1], label: "Grime" });

// Images read from their top row; mesh UVs run upward.
const print = sampleColor("superspec-material:posters-atlas.jpg", vec2(uv.x, sub(1, uv.y)));
const round = add(world.x, world.y);
const cloud = valueNoise(mul(vec2(round, world.z), 7), seed("print-fade"));
const bleached = mix(print, vec3(0.62, 0.58, 0.47), mul(fading, add(0.3, mul(0.7, cloud))));
const runs = smoothstep(0.45, 0.8, valueNoise(vec2(mul(round, 60), mul(world.z, 3)), seed("print-runs")));
const rim = sub(1, smoothstep(0.002, 0.03, creaseDistance("exposed", 30, 0.05)));
const soiled = mul(grime, min(add(add(mul(0.3, runs), mul(0.25, sub(1, cloud))), mul(0.4, rim)), 0.8));

export default defineMaterial({
  name: "Kowloon poster print",
  base: { baseColor: mul(bleached, sub(1, soiled)), roughness: 0.9, metalness: 0 },
});
