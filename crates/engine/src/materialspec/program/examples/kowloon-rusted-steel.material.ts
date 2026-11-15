// Kowloon rusted steel, ported from the LevelSpec steel and rust study (metal_fragment.glsl,
// metal_oxide.glsl, metal_normal.glsl, metal_lighting.glsl, metal_output.glsl and
// metal_reflection.glsl). Edge, surface and raindrop oxide are independent; oxide color samples
// the catalog rust image triplanar with fully rotated lattice copies. Edges come from the
// receiver's own creases and open rims. View-dependent lighting is recorded as live operations.
import {
  defineMaterial, param, vec2, vec3, add, sub, mul, div, mix, min, max, smoothstep, step,
  sqrt, dot, clamp, fract, floor, length, hash, valueNoise, variedColor, triplanarWeights, seed,
  creaseDistance, creaseNormal, world, normal, footprint, boundsMin, boundsMax,
  modulateDiffuse, grazingFalloff, filterEnvironment, bandLuminance, scaleSpecular, lightFloor,
} from "@superspec/material/v1";

// Substrate: the study's bare steel. Galvanized is (0.258, 0.305, 0.318), metalness 0.85,
// roughness 0.33; painted is (0.042, 0.095, 0.085), metalness 0.16, roughness 0.29.
const substrate = param.vec3({ default: vec3(0.093, 0.1248, 0.1384), range: [0, 1], label: "Steel color" });
const metalness = param.scalar({ default: 0.92, range: [0, 1], label: "Metalness" });
const roughness = param.scalar({ default: 0.26, range: [0, 1], label: "Roughness" });
const edgeRust = param.scalar({ default: 1, range: [0, 1], label: "Edge rust" });
const surfaceRust = param.scalar({ default: 1, range: [0, 1], label: "Surface rust" });
const rainRust = param.scalar({ default: 1, range: [0, 1], label: "Raindrop rust" });
const polishedEdges = param.scalar({ default: 1, range: [0, 1], label: "Polished edges" });
const creaseAngle = param.scalar({ default: 30, range: [1, 179], unit: "deg", label: "Sharpest flat angle" });
const rustRepeat = param.scalar({ default: 2, range: [0.1, 20], label: "Oxide texture repeat per meter" });
const rustRoughness = param.scalar({ default: 0.92, range: [0, 1], label: "Oxide roughness" });
const oxideDark = param.vec3({ default: vec3(0.02122, 0.009134, 0.005605), range: [0, 1], label: "Oxide dark" });
const oxideLight = param.vec3({ default: vec3(0.215861, 0.08438, 0.0319), range: [0, 1], label: "Oxide light" });
const bandedGloss = param.scalar({ default: 1, range: [0, 1], label: "Banded gloss" });
const paintedValues = param.scalar({ default: 0, range: [0, 1], label: "Painted values" });
const silhouette = param.scalar({ default: 1, range: [0, 1], label: "Silhouette highlight" });

// The receiver's smallest half extent bounds edge bands on thin parts, like the study's boxes.
const halfExtent = mul(sub(boundsMax, boundsMin), 0.5);
const thinnest = max(min(halfExtent.x, min(halfExtent.y, halfExtent.z)), 0.0005);
const edgeLimit = add(0.0304, footprint);
const edgeDistance = creaseDistance("exposed", creaseAngle, edgeLimit);

// Edge, surface and raindrop coverage in world meters.
const fine = valueNoise(mul(world, 85), seed("fine"));
const edgeNoise = valueNoise(mul(world, 34), seed("edge"));
const edgeBand = min(mix(0.001, 0.022, mul(edgeNoise, edgeNoise)), mul(0.4, thinnest));
const edge = mul(
  sub(1, smoothstep(mul(edgeBand, 0.25), add(edgeBand, 0.0004), edgeDistance)),
  smoothstep(0.32, 0.62, add(edgeNoise, mul(sub(fine, 0.5), 0.19))),
);
const patchField = add(add(add(
  mul(valueNoise(mul(world, 3.1), seed("patch")), 0.68),
  mul(valueNoise(mul(world, 6.2), seed("patch")), 0.2)),
  mul(valueNoise(mul(world, 12.4), seed("patch")), 0.08)),
  mul(fine, 0.04));
const surfacePatches = smoothstep(0.61, 0.67, patchField);

// Raindrops on a tilted 3D lattice of 38 cells per meter, so marks also land on vertical faces.
const rainP = mul(add(add(
  mul(vec3(0.36, 0.8, -0.48), world.x),
  mul(vec3(-0.48, 0.6, 0.64), world.y)),
  mul(vec3(0.8, 0, 0.6), world.z)), 38);
const rainCell = floor(rainP);
const chance = hash(add(rainCell, 33.5), seed("rain"));
const dropCenter = add(0.4, mul(0.2, vec3(hash(rainCell, seed("rain")), hash(add(rainCell, 11), seed("rain")), hash(add(rainCell, 62), seed("rain")))));
const dropRadius = mix(0.14, 0.34, chance);
const dropDistance = add(length(sub(fract(rainP), dropCenter)), mul(sub(fine, 0.5), 0.12));
// Drops are millimeters wide. When a sample spans a large part of a lattice cell, coverage fades
// toward the field's mean (51 % of cells hold a drop covering about 13 % of the cell) instead of
// growing each drop to the sample's size.
const sharpRain = mul(step(0.49, chance), sub(1, smoothstep(mul(dropRadius, 0.65), dropRadius, dropDistance)));
const rain = mix(sharpRain, 0.065, smoothstep(0.15, 0.8, mul(footprint, 38)));

const coverage = sub(1, mul(mul(
  sub(1, mul(edgeRust, edge)),
  sub(1, mul(surfaceRust, surfacePatches))),
  sub(1, mul(rainRust, rain))));

// Oxide color: triplanar varied samples of the catalog rust image, toned between two browns.
const rustP = mul(world, rustRepeat);
const axis = triplanarWeights(normal, 4);
const oxideSample = add(add(
  mul(variedColor("superspec-material:rust.png", vec2(rustP.z, rustP.y), seed("oxide-x"), 6.283185, 0), axis.x),
  mul(variedColor("superspec-material:rust.png", vec2(rustP.x, rustP.z), seed("oxide-y"), 6.283185, 0), axis.y)),
  mul(variedColor("superspec-material:rust.png", vec2(rustP.x, rustP.y), seed("oxide-z"), 6.283185, 0), axis.z));
const tone = clamp(mul(sqrt(dot(oxideSample, vec3(0.2126, 0.7152, 0.0722))), 1.7), 0, 1);
const oxideColor = mix(oxideDark, oxideLight, tone);

// Polished rims: an 8 mm band (narrower on thin parts) of exposed metal turns glossy and rounds.
const roundWidth = min(0.008, mul(0.45, thinnest));
const exposedMetal = mul(polishedEdges, sub(1, coverage));
const polishedRim = mul(exposedMetal, sub(1, smoothstep(0, roundWidth, edgeDistance)));
const rimRoughness = mix(roughness, 0.13, polishedRim);
const rimMiddle = creaseNormal("convex", creaseAngle, add(roundWidth, footprint));

// Live response: diffuse-only grain, the silhouette lift, brighter specular and a light floor on
// polished rims and silhouettes (metal_lighting.glsl), and banded gloss on exposed metal.
const diffuseGrain = add(1, mul(sub(valueNoise(mul(world, 51), seed("diffuse")), 0.5), 0.04));
const glossWeight = mul(bandedGloss, sub(1, coverage));
const silhouetteMask = mul(silhouette, sub(1, coverage));

export default defineMaterial({
  name: "Kowloon rusted steel",
  base: {
    baseColor: mix(substrate, oxideColor, coverage),
    roughness: mix(rimRoughness, rustRoughness, coverage),
    metalness: mix(metalness, 0, coverage),
    // The study's rim bend after normal mapping: the shaded normal plus the direction across the
    // rim (the adjacent face's normal), weighted by the polished band (metal_normal.glsl).
    normalOffset: mul(sub(mul(rimMiddle, mul(2, dot(rimMiddle, normal))), normal), polishedRim),
  },
  live: [
    modulateDiffuse({ factor: diffuseGrain, mask: sub(1, coverage) }),
    grazingFalloff({ from: 0.3, to: 0.06, mask: silhouetteMask }),
    scaleSpecular({ factor: add(1, mul(0.45, polishedRim)) }),
    lightFloor({ red: 0.105, green: 0.111, blue: 0.12, mask: polishedRim }),
    lightFloor({ red: 0.052, green: 0.057, blue: 0.065, mask: silhouetteMask, from: 0.3, to: 0.06 }),
    filterEnvironment({ roughness: rimRoughness, spread: mul(0.22, glossWeight) }),
    // Painted values soften dark bands, sharpen bright ones and lift the shadows (metal_reflection.glsl).
    bandLuminance({
      spacing: div(1, 5.5), softness: mix(0.04, 0.48, paintedValues), mask: glossWeight,
      brightSoftness: mix(0.04, 0.035, paintedValues),
      liftRed: mul(0.105, paintedValues), liftGreen: mul(0.135, paintedValues), liftBlue: mul(0.16, paintedValues),
    }),
  ],
});

