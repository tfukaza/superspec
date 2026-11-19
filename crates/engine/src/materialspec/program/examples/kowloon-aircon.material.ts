// An air conditioner's printed casing after years on a wall: the catalog's casing sheet (grilles, fans, labels) by the
// mesh's own UVs, under what weather adds. Each unit yellows by its own amount (chosen from where it hangs), carries a
// film of dust washed into streaks from its top, darker dirt toward its foot, and rust that starts along the bottom
// edge and climbs in runs. Nothing is read but the image, position and the receiver's bounds.
import {
  defineMaterial, layer, param, vec2, vec3, add, sub, mul, div, mix, max, min, clamp, smoothstep, hash, valueNoise,
  sampleColor, seed, uv, surface, world, boundsMin, boundsMax,
} from "@superspec/material/v1";

const yellowing = param.scalar({ default: 0.6, range: [0, 1], label: "Yellowing" });
const dust = param.scalar({ default: 0.6, range: [0, 1], label: "Dust and streaks" });
const rust = param.scalar({ default: 0.5, range: [0, 1], label: "Rust" });
const sheen = param.scalar({ default: 0.5, range: [0, 1], label: "Casing roughness" });

// Images read from their top row; mesh UVs run upward.
const print = sampleColor("superspec-material:aircon.png", vec2(uv.x, sub(1, uv.y)));
const unit = mul(boundsMin, 5.77);
const aged = mul(yellowing, add(0.25, mul(0.75, hash(unit, seed("aircon-age")))));
const casing = mul(print, mix(vec3(1), vec3(0.8, 0.74, 0.56), aged));

// Height in the unit, 0 at its foot and 1 at its top, and a coordinate round it for streaks.
const upIt = clamp(div(sub(world.z, boundsMin.z), max(sub(boundsMax.z, boundsMin.z), 0.01)), 0, 1);
const round = surface.x;
const threads = valueNoise(vec2(mul(round, 70), mul(world.z, 4)), seed("aircon-threads"));
const washed = smoothstep(0.35, 0.8, valueNoise(vec2(mul(round, 21), mul(world.z, 1.6)), seed("aircon-streaks")));
const film = mul(dust, min(add(add(0.18, mul(0.42, washed)), mul(0.35, sub(1, smoothstep(0, 0.45, upIt)))), 0.85));
const climb = add(0.1, mul(0.5, valueNoise(vec2(mul(round, 13), 0.5), seed("aircon-rust-reach"))));
const rusted = mul(rust, mul(sub(1, smoothstep(mul(climb, 0.2), climb, upIt)), add(0.35, mul(0.65, smoothstep(0.3, 0.7, threads)))));

export default defineMaterial({
  name: "Kowloon air conditioner casing",
  base: { baseColor: casing, roughness: sheen, metalness: 0.05 },
  layers: [
    layer({ id: "film", mask: film, baseColor: mul(casing, vec3(0.42, 0.39, 0.33)), roughness: 0.85 }),
    layer({ id: "rust", mask: rusted, baseColor: mix(vec3(0.07, 0.03, 0.014), vec3(0.3, 0.13, 0.05), threads), roughness: 0.95 }),
  ],
});
