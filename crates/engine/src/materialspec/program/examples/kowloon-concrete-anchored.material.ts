// Kowloon concrete as an intrinsic base for weathering layers: the shipped recipe's aggregate,
// pigment, repair cutouts, construction joints, form-tie holes, chips and plaster, measured on the
// receiver's pattern anchor (`surface`, meters from the anchor) instead of its bounds. It reads
// nothing but its position and footprint, so every receiver and every size of one bakes the same
// pattern where they overlap, and its bake is shared between them. What depends on where a
// receiver stands lies in layers over it: top grime (kowloon-top-grime), ground stain and moss
// (kowloon-ground-stain, kowloon-moss) and worn edges (kowloon-concrete-edges). Defaults are the
// shipped recipe's. A narrow face is no longer kept plain (that read its bounds), and neither are
// the joints along a face's rim, which the edge layer's grooves stand in for.
import {
  defineMaterial, param, vec2, vec3, add, sub, mul, div, mix, abs, max, min, floor, smoothstep,
  softThreshold, select, and, not, ge, gt, length, valueNoise, variedColor,
  sampleColor, seed, footprint, surface, neg,
} from "@superspec/material/v1";
import type { Scalar, Vec2, Vec3 } from "@superspec/material/v1";
import {
  fractal, fracture, contour, shoulder, pigment, cellRandom, boxDistance, domainWarp, modulo,
} from "./kowloon-noise.material";

// Study controls.
const variation = param.bool({ default: true, label: "Texture variation" });
const darknessA = param.scalar({ default: 0.08, range: [0, 0.8], label: "Concrete A darkness" });
const hueAmount = param.scalar({ default: 0.05, range: [0, 0.3], label: "Hue variation" });
const hueAngle = param.scalar({ default: 60, range: [0, 360], unit: "deg", label: "Hue direction" });
const darknessB = param.scalar({ default: 0.1, range: [0, 0.8], label: "Concrete B darkness" });
const blendScale = param.scalar({ default: 9, range: [0.2, 16], label: "Blend frequency" });
const blendContrast = param.scalar({ default: 1, range: [1, 8], label: "Blend contrast" });
const jaggedness = param.scalar({ default: 0.8, range: [0, 2], label: "Edge jaggedness" });
const chipScale = param.scalar({ default: 0.24, range: [0.04, 0.8], unit: "m", label: "Chip pattern scale" });
const chipAmount = param.scalar({ default: 0.22, range: [0, 1], label: "Chip amount" });
const intactRoughness = param.scalar({ default: 0.3, range: [0, 1], label: "Intact roughness" });
const chippedRoughness = param.scalar({ default: 0.92, range: [0, 1], label: "Chipped roughness" });
const chipRelief = param.scalar({ default: 0.0035, range: [0, 0.006], unit: "m", label: "Chip relief" });
const cutouts = param.bool({ default: true, label: "Repair cutouts" });
const holes = param.bool({ default: true, label: "Form-tie holes" });
const plaster = param.bool({ default: false, label: "Plaster coat" });
const plasterChips = param.bool({ default: true, label: "Plaster loss" });
const ground = param.bool({ default: false, label: "Ground slab" });

// The study's recorded linear means of its two source images.
const meanA = vec3(0.37286770477657405, 0.3662141145046476, 0.3582086447939772);
const meanB = vec3(0.20798059217157364, 0.20320214220777086, 0.18970546072187944);

// Meters on the pattern anchor, two meters per study UV unit.
const meters = surface;
const studyUv = div(meters, 2);
const noiseUv = mul(studyUv, vec2(0.85, 0.68));
const wall = not(ground);

// Two varied source images with darkness fields, an eroded blend and pigment drift.
const sampleA = select(variation, variedColor("textures/concrete-plain.png", studyUv, seed("concrete-a"), 0.32, 1), sampleColor("textures/concrete-plain.png", studyUv));
const uvB = add(mul(studyUv, 1.13), vec2(3.7, 11.2));
const sampleB = mul(
  select(variation, variedColor("textures/concrete-fine-sand.png", uvB, seed("concrete-b"), 0.32, 1), sampleColor("textures/concrete-fine-sand.png", uvB)),
  mix(vec3(1), div(meanA, meanB), 0.9),
);
const darkA = contour(fracture(add(mul(noiseUv, 5.4), vec2(5.3, 17.1)), jaggedness, seed("dark-a")), 0.035);
const darkB = contour(fracture(add(mul(noiseUv, 8.28), vec2(73.4, 29.6)), jaggedness, seed("dark-b")), 0.035);
const blend = contour(fracture(add(mul(noiseUv, blendScale), vec2(113.2, 67.8)), jaggedness, seed("blend")), div(0.045, blendContrast));
const layered = pigment(
  mix(mul(sampleA, sub(1, mul(darknessA, darkA))), mul(sampleB, sub(1, mul(darknessB, darkB))), blend),
  noiseUv, hueAmount, hueAngle, seed("pigment"),
);

// Filled repair rectangles inside 2 m x 1.6 m construction panels laid from the anchor.
const panel = floor(div(meters, vec2(2, 1.6)));
const panelRandom = cellRandom(add(panel, vec2(81.3, 42.7)), seed("cutouts"));
const cutoutActive = and(and(cutouts, wall), ge(panelRandom.x, 0.28));
const cutoutDistance = select(
  cutoutActive,
  boxDistance(
    sub(meters, add(mul(add(panel, 0.5), vec2(2, 1.6)), mul(sub(vec2(panelRandom.y, panelRandom.z), 0.5), vec2(0.35, 0.25)))),
    add(vec2(0.3, 0.22), mul(vec2(panelRandom.z, panelRandom.y), vec2(0.25, 0.19))),
  ),
  100,
);
const cutoutCoverage = sub(1, smoothstep(-0.003, 0.003, cutoutDistance));
const repaired = mul(layered, add(vec3(1), mul(cutoutCoverage, add(
  vec3(mul(sub(mul(panelRandom.z, 2), 1), 0.14)),
  mul(vec3(0.045, 0.02, -0.025), sub(mul(panelRandom.y, 2), 1)),
))));

// Construction joints every 2 m from the anchor and the repair outlines, with chipped shoulders.
const seamDistance = min(abs(sub(modulo(add(meters.x, 1), 2), 1)), abs(cutoutDistance));
const seamGrain = valueNoise(add(mul(meters, 137), vec2(3.1, 8.7)), seed("seam-grain"));
const chippedWidth = add(add(0.009, mul(smoothstep(0.4, 0.78, fractal(mul(meters, 22), seed("seam-pockets"))), 0.037)), mul(sub(seamGrain, 0.5), 0.006));
const pixel = max(mul(footprint, 0.65), 0.0006);
const groove = sub(1, smoothstep(0.002, add(0.006, pixel), seamDistance));
const seamChip = sub(1, smoothstep(mul(chippedWidth, 0.4), add(chippedWidth, pixel), seamDistance));
const seamHeight = sub(
  mul(-0.006, sub(1, smoothstep(0.002, 0.006, seamDistance))),
  mul(mul(0.004, shoulder(seamDistance, chippedWidth)), add(0.5, mul(0.5, seamGrain))),
);

// Form-tie holes on a 1 m grid from the anchor: normal relief only.
const holeCenter = add(floor(meters), 0.5);
const holeRadius = add(length(sub(meters, holeCenter)), mul(sub(valueNoise(mul(meters, 190), seed("holes")), 0.5), 0.003));
const holeHeight = select(holes, mul(-0.012, sub(1, smoothstep(0.017, 0.028, holeRadius))), 0);

// Walls carry seams and holes; the slab carries the study's fine ground relief.
const reliefHeight = select(wall, add(seamHeight, holeHeight), add(mul(fractal(mul(studyUv, 65), seed("slab")), 0.0006), mul(valueNoise(mul(studyUv, 420), seed("slab-grain")), 0.00018)));
const reliefDarkness = select(wall, add(mul(groove, 0.24), mul(seamChip, 0.055)), 0);
const reliefExposed = select(wall, mul(seamChip, sub(1, groove)), 0);

// Optional plaster coat: eroded exposure of the substrate, mottled plaster color, a thin lip.
const plasterP = add(mul(studyUv, vec2(2.4, 2)), vec2(28.1, 4.7));
const plasterBroad = fractal(add(plasterP, mul(domainWarp(plasterP, seed("plaster")), mul(0.18, jaggedness))), seed("plaster"));
const plasterField = add(plasterBroad, mul(mul(plasterDetail(plasterP), jaggedness), sub(1, smoothstep(0.025, 0.08, abs(sub(plasterBroad, 0.6))))));
const exposedHeight = select(plaster, select(plasterChips, smoothstep(0.598, 0.602, plasterField), 0), 1);
const exposedColor = select(plaster, select(plasterChips, softThreshold(plasterField, 0.6, 0.004), 0), 1);
const plasterColor = pigment(
  mul(variedColor("superspec-material:building-plaster-plain.png", studyUv, seed("plaster-image"), 0.32, 1), sub(1, mul(darknessA, fractal(add(mul(studyUv, vec2(3.7, 2.8)), 19.2), seed("plaster-mottling"))))),
  noiseUv, hueAmount, hueAngle, seed("pigment"),
);
const plasterHeight = mul(add(add(0.004, mul(valueNoise(mul(studyUv, 480), seed("plaster-grain")), 0.00016)), mul(fractal(mul(studyUv, 65), seed("plaster-fine")), 0.00035)), sub(1, exposedHeight));

// Chips: an eroded fracture field at its own threshold, crisp for relief, filtered for shading.
const chipP = add(mul(studyUv, div(2, chipScale)), vec2(18.7, 42.3));
const chipThreshold = mix(0.93, 0.53, chipAmount);
const chipBroad = fracture(chipP, jaggedness, seed("chips"));
const chipGrit = add(add(
  mul(sub(valueNoise(add(mul(chipP, 9), 11.3), seed("chips")), 0.5), 0.15),
  mul(sub(valueNoise(add(mul(chipP, 24), 37.1), seed("chips")), 0.5), 0.11)),
  mul(sub(valueNoise(add(mul(chipP, 61), 72.4), seed("chips")), 0.5), 0.07));
const chipField = add(chipBroad, mul(mul(chipGrit, sub(1, smoothstep(0.025, 0.11, abs(sub(chipBroad, chipThreshold))))), jaggedness));
const chipsOn = gt(chipAmount, 0);
const chipRelieved = mul(select(chipsOn, smoothstep(sub(chipThreshold, 0.012), add(chipThreshold, 0.012), chipField), 0), exposedHeight);
const chipShaded = mul(select(chipsOn, softThreshold(chipField, chipThreshold, 0.044), 0), exposedColor);
const chipGrain = add(mul(valueNoise(mul(studyUv, 420), seed("chip-grain")), 0.45), mul(valueNoise(mul(studyUv, 127), seed("chip-grain-b")), 0.55));
const chipHeight = mul(mul(neg(chipRelieved), chipRelief), sub(1.35, mul(chipGrain, 0.7)));

// Color composition, in the study's order; grime, stains, moss and edge wear are layers.
const relieved = add(mul(repaired, sub(1, min(reliefDarkness, 0.85))), mul(vec3(0.012, 0.011, 0.009), reliefExposed));
const coated = select(plaster, mix(plasterColor, relieved, exposedColor), relieved);
const chipped = mul(coated, mix(1, add(0.59, mul(chipGrain, 0.29)), chipShaded));

const surfaceHeight = add(
  select(plaster, add(mul(reliefHeight, exposedHeight), plasterHeight), reliefHeight),
  chipHeight);
const intact = mix(intactRoughness, 0.96, select(plaster, sub(1, exposedColor), 0));

export default defineMaterial({
  name: "Kowloon concrete, anchored base",
  base: {
    baseColor: clamp01(chipped),
    roughness: mix(intact, chippedRoughness, chipShaded),
    metalness: 0,
    height: surfaceHeight,
  },
});

function plasterDetail(p: Vec2): Scalar {
  return add(add(add(add(
    mul(sub(valueNoise(mul(p, 13), seed("plaster-detail")), 0.5), 0.055),
    mul(sub(valueNoise(add(mul(p, 31), 17.3), seed("plaster-detail")), 0.5), 0.015)),
    mul(sub(valueNoise(add(mul(p, 83), 39.1), seed("plaster-detail")), 0.5), 0.028)),
    mul(sub(valueNoise(add(mul(p, 173), 81.7), seed("plaster-detail")), 0.5), 0.012)),
    mul(sub(valueNoise(add(mul(p, 347), 13.9), seed("plaster-detail")), 0.5), 0.004));
}

function clamp01(color: Vec3): Vec3 {
  return min(max(color, vec3(0)), vec3(1));
}

