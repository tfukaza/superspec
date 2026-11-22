import {
  defineMaterial,
  param,
  vec2,
  vec3,
  add,
  sub,
  mul,
  mix,
  dot,
  smoothstep,
  valueNoise,
  sampleColor,
  seed,
  world,
  normal,
  creaseDistance,
  creaseNormal,
  footprint,
} from "@superspec/material/v1";

const sleeve = param.scalar({ default: 0, range: [0, 1], label: "Galvanized coupling" });
const along = vec2(world.x, world.z);
const broad = valueNoise(mul(along, 16), seed("oxide-islands"));
const drift = vec2(broad, valueNoise(mul(along, 17), seed("scale-edges")));
const flakes = valueNoise(add(mul(along, 83), mul(drift, 3.8)), seed("broken-oxide"));
const pits = valueNoise(mul(along, 224), seed("oxide-pits"));
// Retain some local pigment variation from the original weathered steel.
const source = sampleColor("superspec-material:wall-metal-weathered.png", along);
const warm = dot(source, vec3(0.6, -0.4, -0.2));
const field = add(add(mul(broad, 0.37), mul(flakes, 0.4)), add(mul(pits, 0.23), mul(warm, 0.14)));
const distanceToSeat = creaseDistance("contact", 0, 0.28);
const nearSeat = sub(1, smoothstep(0.04, 0.28, distanceToSeat));
const threshold = mix(sub(0.625, mul(nearSeat, 0.19)), 0.74, sleeve);
const oxideMask = smoothstep(threshold, add(threshold, 0.065), field);
const rim = sub(1, smoothstep(0.0005, 0.006, creaseDistance("exposed", 30, 0.018)));
const cover = mul(oxideMask, sub(1, mul(rim, 0.91)));
const castMottle = smoothstep(0.33, 0.71, add(mul(flakes, 0.56), mul(pits, 0.44)));
const zinc = mix(vec3(0.48, 0.54, 0.56), vec3(0.59, 0.64, 0.64), castMottle);
const steel = mix(vec3(0.25, 0.285, 0.296), zinc, sleeve);
const rust = mix(
  vec3(0.15, 0.064, 0.034),
  vec3(0.33, 0.14, 0.075),
  smoothstep(0.26, 0.76, add(mul(flakes, 0.48), mul(pits, 0.52))),
);
const rub = mul(rim, smoothstep(0.32, 0.7, add(mul(broad, 0.65), mul(flakes, 0.35))));
const silver = mix(mul(steel, add(0.92, mul(broad, 0.16))), vec3(0.71, 0.73, 0.73), mul(rub, 0.72));
const abrasive = sub(valueNoise(mul(along, vec2(70, 14)), seed("rubbed-steel")), 0.5);
const crease = creaseNormal("convex", 30, add(0.006, footprint));
const bend = mul(sub(mul(crease, mul(2, dot(crease, normal))), normal), rim);

export default defineMaterial({
  name: "Oxidized service steel",
  base: {
    baseColor: mul(
      add(0.76, mul(0.24, smoothstep(0.035, 0.078, distanceToSeat))),
      mix(
        mix(silver, rust, cover),
        vec3(0.46, 0.4, 0.32),
        mul(cover, mul(smoothstep(0.74, 0.91, pits), 0.58)),
      ),
    ),
    metalness: mix(mix(0.43, 0.5, sleeve), 0.09, cover),
    roughness: mix(mix(mix(0.64, 0.69, sleeve), 0.32, rub), add(0.69, mul(pits, 0.14)), cover),
    normalOffset: add(
      bend,
      vec3(mul(abrasive, mix(0.024, 0.04, cover)), 0, mul(sub(flakes, 0.5), mul(cover, 0.035))),
    ),
  },
});
