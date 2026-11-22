// The avenue trees' bark: painted as an illustration's, not textured. A warm olive grey-brown (the anime-tree concept's:
// lit about #706756, shade #37352b) in four flat steps from a clear
// light side to a dark side (three shade bands of the sun on the bark's normal, their edges a little broken), with broad
// vertical brush strokes (bold furrows a few centimeters wide, running up the stem, no fine noise), darker toward the
// forks under the crown and with a little warm bounce from the paving at the foot. Limbs take the same paint as they
// thin. The bark family evaluates it per pixel; its base color is the lit profile's albedo.
import { defineMaterial, param, vec2, vec3, mix, mul, add, sub, div, fbm, seed, world, shadeBand, sunDirection, normal, edgeBreak, smoothstep } from "@superspec/material/v1";

const light = param.vec3({ default: vec3(0.17, 0.14, 0.095), range: [0, 1], label: "Lit bark" });
const stroke = param.vec3({ default: vec3(0.1, 0.085, 0.06), range: [0, 1], label: "Furrow strokes" });
const shade = param.vec3({ default: vec3(0.31, 0.3, 0.27), range: [0, 1], label: "Dark side tint" });
const bounce = param.vec3({ default: vec3(1.18, 1.04, 0.86), range: [0, 2], label: "Warm bounce at the foot" });
const strokeWidth = param.scalar({ default: 0.07, range: [0.02, 0.4], unit: "m", label: "Stroke width" });
const strokeLength = param.scalar({ default: 1.1, range: [0.2, 4], unit: "m", label: "Stroke length" });
const strokes = param.scalar({ default: 0.45, range: [0, 1], label: "Share of strokes" });
const crown = param.vec2({ default: vec2(2.6, 4.6), range: [0, 20], unit: "m", label: "Darkening toward the crown (from, to)" });
const crownShade = param.scalar({ default: 0.62, range: [0, 1], label: "Bark under the crown" });
const softness = param.scalar({ default: 1.5, range: [0, 8], label: "Edge softness (pixels)" });

// Strokes run up the stem: a field stretched along z, cut into bold marks.
const across = div(add(world.x, mul(world.y, 0.8)), strokeWidth);
const field = fbm(vec2(across, div(world.z, strokeLength)), seed("strokes"), 2);
const marked = smoothstep(sub(strokes, 0.04), add(strokes, 0.04), field);
const bark = mix(stroke, light, marked);
// Darker toward the forks under the crown, warmer at the foot.
const height = world.z;
const under = mix(1, crownShade, smoothstep(crown.x, crown.y, height));
const foot = sub(1, smoothstep(0.35, 1.3, height));
const tinted = mul(mul(bark, under), mix(vec3(1, 1, 1), bounce, foot));

// Four steps from the dark side to the light: three bands of the sun, each edge a little broken.
const edge = edgeBreak({ size: 0.22, amount: 0.07 });
const low = shadeBand(normal, sunDirection, add(-0.35, edge.offset), softness, 0.02);
const middle = shadeBand(normal, sunDirection, add(0.1, edge.offset), softness, 0.02);
const high = shadeBand(normal, sunDirection, add(0.55, edge.offset), softness, 0.02);
const lit = div(add(add(low, middle), high), 3);

export default defineMaterial({
  name: "Avenue painted bark",
  shading: "painted",
  base: { baseColor: tinted, roughness: 0.9 },
  paint: mul(tinted, mix(shade, vec3(1, 1, 1), lit)),
});
