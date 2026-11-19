// Small glazed wall tiles as an old Kowloon facade wears them (the spandrels under a shop house's windows): every
// tile a little off its neighbours in tone, a few replaced over the years with whatever was to hand, the glaze faded
// in clouds, chips at the edges and a missing tile here and there showing the bedding, and joints black with dirt
// that has crept onto the faces beside them and run down in streaks. Water from the sill or ledge above a tiled panel
// runs down its vertical joints: from the panel's top edge each of some joints carries a dark thread of its own length,
// widest where it starts and thinning as it falls.
import {
  defineMaterial, layer, param, tileAndGrout, rectangularGrid, vec2, vec3, add, sub, mul, div, mix, min, max, clamp, floor,
  step, smoothstep, hash, valueNoise, seed, surface, world, boundsMax,
} from "@superspec/material/v1";

const tileSize = param.scalar({ default: 0.1, range: [0.03, 0.6], unit: "m", label: "Tile size" });
const jointWidth = param.scalar({ default: 0.006, range: [0.001, 0.03], unit: "m", label: "Joint width" });
const glaze = param.vec3({ default: vec3(0.205, 0.283, 0.235), range: [0, 1], label: "Glaze colour" });
const spare = param.vec3({ default: vec3(0.46, 0.45, 0.36), range: [0, 1], label: "Replacement glaze" });
const replaced = param.scalar({ default: 0.05, range: [0, 1], label: "Replaced tiles" });
const variation = param.scalar({ default: 0.22, range: [0, 0.6], label: "Tone variation" });
const fade = param.scalar({ default: 0.3, range: [0, 1], label: "Fading" });
const wear = param.scalar({ default: 0.22, range: [0, 0.6], label: "Chipped edges" });
const missing = param.scalar({ default: 0.02, range: [0, 1], label: "Missing tiles" });
const grime = param.scalar({ default: 0.6, range: [0, 1], label: "Grime" });
const glazeRoughness = param.scalar({ default: 0.32, range: [0, 1], label: "Glaze roughness" });
const jointRuns = param.scalar({ default: 0.55, range: [0, 1], label: "Joints with runs" });
const runReach = param.scalar({ default: 0.6, range: [0.05, 3], unit: "m", label: "Longest joint run" });

const size = vec2(tileSize, tileSize);
const tiles = tileAndGrout({ size, groutWidth: jointWidth, chipAmount: wear, chipSize: 0.012, missingChance: missing });
const grid = rectangularGrid({ size });

// Each tile's own tone, and the odd one in another glaze.
const tone = add(sub(1, variation), mul(variation, grid.random));
const odd = step(sub(1, replaced), hash(grid.cell, seed("replaced")));
const clouds = smoothstep(0.25, 0.8, valueNoise(mul(surface, 1.4), seed("glaze-fade")));
const faced = mul(mix(glaze, spare, odd), tone);
const faded = mix(faced, add(mul(faced, 0.6), vec3(0.19, 0.19, 0.17)), mul(fade, clouds));

// Dirt: out of the joints onto the faces, in patches, and washed down in streaks.
const beside = sub(1, smoothstep(0.002, min(mul(tileSize, 0.24), 0.018), grid.sideDistance));
const streaks = smoothstep(0.35, 0.8, valueNoise(mul(surface, vec2(4.5, 0.8)), seed("tile-streaks")));
const patches = smoothstep(0.5, 0.8, valueNoise(add(mul(surface, 2.3), vec2(4.1, 9.7)), seed("tile-patches")));
// Plus a broad soot band under the panel's top, where the sill above drips.
const headSoot = mul(sub(1, smoothstep(0.05, 0.35, max(sub(boundsMax.z, world.z), 0))), add(0.4, mul(0.6, streaks)));
const film = mul(grime, min(add(add(add(mul(beside, add(0.1, mul(0.25, streaks))), mul(patches, 0.12)), mul(streaks, 0.12)), mul(headSoot, 0.2)), 0.85));

// Runs down the vertical joints from the panel's top (surface x runs along the wall, y up it).
const joint = floor(add(div(surface.x, tileSize), 0.5));
const toJoint = mul(sub(div(surface.x, tileSize), sub(joint, 0.5)), tileSize);
const offJoint = min(toJoint, sub(tileSize, toJoint));
const runLength = mul(runReach, add(0.15, mul(0.85, hash(vec2(joint, 3.1), seed("joint-run")))));
const fall = clamp(sub(1, div(max(sub(boundsMax.z, world.z), 0), runLength)), 0, 1);
const runOn = step(hash(vec2(joint, 7.7), seed("joint-on")), jointRuns);
const thread = add(0.6, mul(0.4, valueNoise(vec2(mul(joint, 5.3), mul(world.z, 7)), seed("joint-thread"))));
const runCore = sub(1, smoothstep(mul(jointWidth, 0.5), add(jointWidth, mul(tileSize, mul(0.32, fall))), offJoint));
const runs = mul(mul(mul(grime, runOn), fall), mul(runCore, thread));

export default defineMaterial({
  name: "Kowloon wall tile",
  base: { baseColor: faded, roughness: glazeRoughness, metalness: 0 },
  layers: [
    layer({ id: "film", mask: film, baseColor: vec3(0.085, 0.078, 0.062), roughness: 0.88 }),
    layer({ id: "runs", mask: min(mul(runs, 1.3), 0.95), baseColor: vec3(0.045, 0.038, 0.028), roughness: 0.9 }),
    layer({ id: "chips", mask: tiles.chip, baseColor: vec3(0.42, 0.39, 0.33), roughness: 0.8, height: { add: -0.0012 } }),
    layer({ id: "bedding", mask: tiles.substrate, baseColor: vec3(0.3, 0.285, 0.25), roughness: 0.95 }),
    layer({ id: "joints", mask: tiles.joint, baseColor: mix(vec3(0.16, 0.15, 0.13), vec3(0.115, 0.106, 0.09), grime), roughness: 0.95, height: { add: -0.0025 } }),
  ],
});
