// The dirt a rendered street wall carries everywhere, as a weathering layer over its paint: rain-washed grime in long
// vertical streaks of uneven strength, broad damp clouds, and soot gathered round the wall's openings (the receiver's
// `opening-edge` features: darkest at the edge, fading over `sootReach`). It darkens the surface beneath by
// multiplication and reads only the pattern anchor and that feature, so a resized wall keeps its dirt where it was and
// a wall without openings is only rain-washed. `settled` adds what lies on ledges (by the way a face looks, in world
// meters): sills, string courses and balcony slabs. Streaks from particular edges are kowloon-grime-streaks.
import { defineLayer, param, featureDistance, smoothstep, valueNoise, exp, neg, dot, add, mul, sub, div, min, max, mix, vec2, vec3, seed, surface, world, normal, gravity } from "@superspec/material/v1";
import { fractal } from "./kowloon-noise.material";

const color = param.vec3({ default: vec3(0.36, 0.34, 0.31), range: [0, 1], label: "Dirt colour" });
const sootColor = param.vec3({ default: vec3(0.24, 0.21, 0.17), range: [0, 1], label: "Soot colour" });
const amount = param.scalar({ default: 0.5, range: [0, 1], label: "Rain-washed dirt" });
const clouds = param.scalar({ default: 0.4, range: [0, 1], label: "Damp clouds" });
const soot = param.scalar({ default: 0.6, range: [0, 1], label: "Soot round openings" });
const sootReach = param.scalar({ default: 0.65, range: [0.05, 2], unit: "m", label: "Soot reach" });
const settled = param.scalar({ default: 0, range: [0, 1], label: "Dirt settled on ledges" });
const mottle = param.scalar({ default: 0, range: [0, 1], label: "Fine mottling" });

const at = vec2(surface.x, surface.y);
// Rain: broad columns, each broken into finer threads that wander a little.
const columns = smoothstep(0.38, 0.86, fractal(add(mul(at, vec2(2.2, 0.45)), vec2(3.3, 8.1)), seed("dirt-columns")));
const wander = vec2(mul(valueNoise(mul(at, vec2(1.3, 2.1)), seed("dirt-wander")), 0.8), 0);
const threads = valueNoise(add(mul(at, vec2(17, 0.9)), wander), seed("dirt-threads"));
const hairs = valueNoise(add(mul(at, vec2(43, 1.7)), mul(wander, 2.3)), seed("dirt-hairs"));
const grain = valueNoise(mul(at, 61), seed("dirt-grain"));
const rain = mul(columns, add(add(add(0.15, mul(0.4, smoothstep(0.35, 0.7, threads))), mul(0.35, smoothstep(0.5, 0.72, hairs))), mul(0.1, grain)));
// Fine mottling: specks and blooms a few centimeters across, everywhere, crisp at their edges.
const specks = smoothstep(0.56, 0.64, fractal(add(mul(at, 11), vec2(6.6, 2.4)), seed("dirt-specks")));
const blooms = smoothstep(0.5, 0.62, fractal(add(mul(at, 27), vec2(1.2, 9.8)), seed("dirt-blooms")));
const mottled = mul(mottle, add(mul(0.3, specks), mul(0.2, blooms)));
// Damp: soft clouds a few meters across.
const cloud = smoothstep(0.45, 0.8, fractal(add(mul(at, vec2(0.42, 0.36)), vec2(11.9, 4.2)), seed("dirt-clouds")));

// Soot round the openings, mottled.
const edge = featureDistance("opening-edge", 4);
const within = sub(1, smoothstep(mul(sootReach, 0.7), sootReach, edge));
const sooted = mul(mul(mul(soot, within), exp(neg(div(edge, mul(sootReach, 0.22))))),
  add(0.58, mul(0.42, fractal(add(mul(at, 3.7), vec2(5.1, 17.7)), seed("dirt-soot")))));

// On a ledge the dirt settles instead of washing off: faces that look up are filmed over, patchily, and faces that
// look down stay damp and dark.
const lookingUp = dot(normal, neg(gravity));
const lying = mul(mul(settled, smoothstep(0.3, 0.85, lookingUp)), add(0.55, mul(0.45, fractal(add(mul(vec2(surface.x, surface.y), 2.9), vec2(8.3, 1.9)), seed("dirt-settled")))));
const beneath = mul(mul(settled, 0.6), smoothstep(0.4, 0.9, neg(lookingUp)));

const washed = add(add(add(mul(mul(amount, 0.75), rain), mul(mul(clouds, 0.45), cloud)), max(lying, beneath)), mottled);

export default defineLayer({
  name: "Kowloon wall dirt",
  coverage: min(add(washed, sooted), 1),
  baseColor: mix(color, sootColor, div(sooted, add(add(washed, sooted), 0.0001))),
  blend: { baseColor: "multiply" },
});
