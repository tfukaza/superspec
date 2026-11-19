// A Kowloon pavement of precast concrete flags, walked on for decades: every flag its own grey, a few of them newer
// and paler or cracked across, the joints dark and uneven, and the street's marks on top: blackened gum, oil and rust
// spots, damp clouds, and a band of grime where the paving meets a wall or a kerb (the receiver's contact edges, where
// a project build supplies them). It lies in world meters, so flags run on across separate receivers; a kerb is the
// same recipe with long narrow stones.
import {
  defineMaterial, layer, param, rectangularGrid, vec2, vec3, add, sub, mul, mix, abs, min, max,
  step, smoothstep, hash, valueNoise, cellular, creaseDistance, seed, world, footprint,
} from "@superspec/material/v1";

const flag = param.vec2({ default: vec2(0.55, 0.43), range: [0.1, 3], unit: "m", label: "Flag size" });
const stagger = param.scalar({ default: 0.5, range: [0, 0.99], label: "Row offset" });
const jointWidth = param.scalar({ default: 0.012, range: [0.002, 0.05], unit: "m", label: "Joint width" });
const stone = param.vec3({ default: vec3(0.39, 0.37, 0.3), range: [0, 1], label: "Flag colour" });
const variation = param.scalar({ default: 0.32, range: [0, 0.8], label: "Tone variation" });
const cracked = param.scalar({ default: 0.12, range: [0, 1], label: "Cracked flags" });
const fractures = param.scalar({ default: 0.6, range: [0, 1], label: "Fractures across flags" });
const spots = param.scalar({ default: 0.6, range: [0, 1], label: "Gum and oil spots" });
const grime = param.scalar({ default: 0.6, range: [0, 1], label: "Grime" });

const at = vec2(world.x, world.y);
const grid = rectangularGrid({ position: at, size: flag, rowOffset: stagger });
// Each flag: its tone, a few replaced ones paler, and a fine sandy grain.
const tone = add(sub(1, variation), mul(mul(variation, 1.25), grid.random));
const newer = step(0.93, hash(grid.cell, seed("flag-new")));
const grain = add(0.9, mul(0.2, valueNoise(mul(at, 210), seed("flag-grain"))));
const mottled = add(0.88, mul(0.24, valueNoise(mul(at, 5.5), seed("flag-mottle"))));
const faced = mul(mul(mix(stone, mul(stone, vec3(1.25, 1.24, 1.2)), newer), mul(tone, mottled)), grain);

// Joints, wider where their edges have crumbled; and a crack across some flags, wandering from side to side.
const crumbled = add(jointWidth, mul(mul(jointWidth, 1.5), smoothstep(0.55, 0.85, valueNoise(mul(at, 9), seed("flag-crumble")))));
const pixel = max(footprint, 0.001);
const joint = sub(1, smoothstep(mul(crumbled, 0.5), add(mul(crumbled, 0.5), pixel), grid.sideDistance));
const isCracked = step(sub(1, cracked), hash(grid.cell, seed("flag-cracked")));
const lean = sub(hash(grid.cell, seed("flag-lean")), 0.5);
const wobble = mul(sub(valueNoise(mul(at, 14), seed("flag-wobble")), 0.5), 0.05);
const line = abs(add(add(sub(grid.local.y, mul(flag.y, add(0.35, mul(0.3, hash(grid.cell, seed("flag-crack-at")))))), mul(sub(grid.local.x, mul(flag.x, 0.5)), lean)), wobble));
const flagCrack = mul(isCracked, sub(1, smoothstep(0.0015, add(0.004, pixel), line)));
// Fractures that run on across flags where the ground has settled: the level lines of a slow noise, in the weaker
// areas only, so they wander, branch off and stop.
const settling = add(valueNoise(add(mul(at, 1.1), vec2(4.4, 9.1)), seed("settle")), mul(sub(valueNoise(mul(at, 7), seed("settle-fine")), 0.5), 0.05));
const weak = smoothstep(0.52, 0.7, valueNoise(add(mul(at, 0.6), vec2(2.9, 0.4)), seed("settle-where")));
const fracture = mul(mul(fractures, weak), sub(1, smoothstep(0.003, add(0.007, mul(pixel, 1.1)), abs(sub(settling, 0.5)))));
const crack = max(flagCrack, fracture);

// The street's marks.
const gum = mul(spots, step(0.86, hash(mul(at, 6.3), seed("gum-cell"))));
const gumSpot = mul(gum, sub(1, smoothstep(0.1, 0.16, cellular(mul(at, 6.3), seed("gum")))));
const oil = mul(spots, smoothstep(0.74, 0.86, valueNoise(add(mul(at, 1.9), vec2(3.3, 8.8)), seed("oil"))));
const damp = smoothstep(0.42, 0.8, valueNoise(add(mul(at, 0.55), vec2(1.7, 6.1)), seed("damp")));
const contact = creaseDistance("contact", 30, 0.6);
const skirt = mul(sub(1, smoothstep(0.02, 0.45, contact)), add(0.45, mul(0.55, valueNoise(mul(at, 4.1), seed("skirt")))));
const soiled = mul(grime, min(add(add(mul(damp, 0.35), mul(skirt, 0.75)), mul(oil, 0.5)), 0.9));

export default defineMaterial({
  name: "Kowloon paving",
  base: { baseColor: faced, roughness: 0.9, metalness: 0, height: mul(sub(grain, 1), 0.0015) },
  layers: [
    layer({ id: "soiled", mask: soiled, baseColor: mul(faced, vec3(0.3, 0.29, 0.26)), roughness: 0.95 }),
    layer({ id: "gum", mask: gumSpot, baseColor: vec3(0.035, 0.034, 0.03), roughness: 0.7 }),
    layer({ id: "joints", mask: joint, baseColor: vec3(0.055, 0.052, 0.042), roughness: 1, height: { add: -0.004 } }),
    layer({ id: "cracks", mask: crack, baseColor: vec3(0.03, 0.03, 0.025), roughness: 1, height: { add: -0.003 } }),
  ],
});
