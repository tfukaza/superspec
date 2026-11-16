import {
  defineMaterial, layer, param, tileAndGrout, groundWeathering, runoffStreaks, modulateDiffuse,
  vec2, vec3, fbm, seed, surface, div,
} from "@superspec/material/v1";

// Tile and Grout expresses clean square, staggered rectangular and worn tile with one group.
const tileSize = param.vec2({ default: vec2(0.3, 0.15), range: [0.05, 1], unit: "m" });
const stagger = param.scalar({ default: 0.5, range: [0, 0.99] });
const wear = param.scalar({ default: 0.18, range: [0, 0.6] });
const glaze = param.vec3({ default: vec3(0.78, 0.76, 0.7), range: [0, 1], label: "Glaze color" });
const glazeRoughness = param.scalar({ default: 0.18, range: [0, 1], label: "Glaze roughness" });
const missing = param.scalar({ default: 0.04, range: [0, 1], label: "Missing tiles" });

const tiles = tileAndGrout({ size: tileSize, rowOffset: stagger, groutWidth: 0.008, chipAmount: wear, missingChance: missing });
const ground = groundWeathering({ scale: 0.6, stainAmount: 0.45 });
const streaks = runoffStreaks({ width: 0.04, length: 0.9, strength: 0.6 });
const grain = fbm(div(surface, 0.002), seed("glaze-grain"), 2);

export default defineMaterial({
  name: "Tiled courtyard",
  base: { baseColor: glaze, roughness: glazeRoughness, metalness: 0 },
  layers: [
    layer({ id: "grout", mask: tiles.joint, baseColor: vec3(0.32, 0.31, 0.29), roughness: 0.85, height: { add: -0.003 } }),
    layer({ id: "chips", mask: tiles.chip, baseColor: vec3(0.55, 0.5, 0.42), roughness: 0.7, height: { add: -0.0015 } }),
    layer({ id: "substrate", mask: tiles.substrate, baseColor: vec3(0.4, 0.38, 0.34), roughness: 0.9 }),
    layer({ id: "stain", mask: ground.stain, baseColor: vec3(0.16, 0.18, 0.12), roughness: 0.95 }),
    layer({ id: "runoff", mask: streaks.coverage, baseColor: vec3(0.2, 0.2, 0.19) }),
  ],
  live: [modulateDiffuse({ factor: 0.92, mask: grain })],
});
