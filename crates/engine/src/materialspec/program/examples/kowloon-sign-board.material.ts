// A painted shop sign's ground after years over a street: the catalog's board by the mesh's UVs (pale: the sign's own
// colour is its tint), chalked and bleached in clouds, flaked to a grey undercoat from its edges inward, streaked with
// soot from its top edge and grimy along its foot, with rust creeping in from the rim. The lettering over it is
// kowloon-sign-lettering.
import {
  defineMaterial, layer, param, vec2, vec3, add, sub, mul, div, mix, max, min, clamp, smoothstep, valueNoise, sampleColor,
  creaseDistance, seed, uv, surface, world, boundsMin, boundsMax,
} from "@superspec/material/v1";

const fading = param.scalar({ default: 0.5, range: [0, 1], label: "Chalking and fading" });
const flaking = param.scalar({ default: 0.45, range: [0, 1], label: "Flaked paint" });
const grime = param.scalar({ default: 0.6, range: [0, 1], label: "Grime" });
const rust = param.scalar({ default: 0.5, range: [0, 1], label: "Rim rust" });

const board = sampleColor("superspec-material:signboard.png", vec2(uv.x, sub(1, uv.y)));
const round = surface.x;
const at = vec2(surface.x, surface.y);
const upIt = clamp(div(sub(world.z, boundsMin.z), max(sub(boundsMax.z, boundsMin.z), 0.01)), 0, 1);
const rim = creaseDistance("exposed", 30, 0.2);

// Chalked paint: paler and a little uneven.
const cloud = valueNoise(mul(at, 2.6), seed("sign-fade"));
const chalked = mix(board, add(mul(board, 0.75), vec3(0.2, 0.19, 0.17)), mul(fading, add(0.25, mul(0.75, cloud))));

// Flakes: clusters, likelier toward the rim.
const cluster = valueNoise(add(mul(at, 5.5), vec2(2.2, 7.9)), seed("sign-flake-cluster"));
const chips = valueNoise(mul(at, 46), seed("sign-flake-chips"));
const nearRim = sub(1, smoothstep(0, 0.12, rim));
const flaked = smoothstep(sub(0.78, mul(flaking, add(add(0.1, mul(0.3, nearRim)), mul(0.16, cluster)))), 0.8, add(mul(cluster, 0.6), mul(chips, 0.4)));

// Soot run down from the top edge, dirt at the foot, rust from the rim.
const runs = smoothstep(0.35, 0.8, valueNoise(vec2(mul(round, 26), mul(world.z, 1.4)), seed("sign-runs")));
const head = mul(smoothstep(0.45, 1, upIt), add(0.2, mul(0.6, runs)));
const foot = mul(sub(1, smoothstep(0, 0.1, upIt)), 0.5);
const soiled = mul(grime, min(add(add(head, foot), mul(runs, 0.18)), 0.8));
const rusted = mul(rust, mul(sub(1, smoothstep(0.004, 0.05, rim)), smoothstep(0.35, 0.7, valueNoise(mul(at, 19), seed("sign-rust")))));

export default defineMaterial({
  name: "Kowloon sign board",
  base: { baseColor: chalked, roughness: add(0.55, mul(0.3, fading)), metalness: 0 },
  layers: [
    layer({ id: "flaked", mask: flaked, baseColor: vec3(0.42, 0.41, 0.36), roughness: 0.9, height: { add: -0.0004 } }),
    layer({ id: "soiled", mask: soiled, baseColor: mul(chalked, vec3(0.32, 0.29, 0.24)), roughness: 0.9 }),
    layer({ id: "rust", mask: rusted, baseColor: vec3(0.3, 0.13, 0.05), roughness: 0.95 }),
  ],
});
