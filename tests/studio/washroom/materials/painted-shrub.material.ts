// A painted shrub canopy, begun from the painterly-shader study's illustration meadow (github.com/tfukaza/lab,
// experiments/painterly-shader, preset "illustration-meadow"): the shade position t = (n·s - offset) / 2 + 1/2 cut into
// four flat bands. Its palette is deeper and less saturated than the study's lime gradient, and each band is varied
// inside the way the washroom concrete is (kowloon-noise: two eroded darkness fields and a pigment hue drift), so a
// band reads as painted rather than as a flat fill. The study's jag (noise biting the terminator) is added to the
// band's threshold by the plant families (browser/renderer/foliage-plants.ts, `jag=`). shadeBand with a range of 1.4
// is within a few percent of the study's linear t at the band edges, and folds in the cast shadow as min(t, visibility).
// Leaves evaluate this once per cluster (so the variation steps from dab to dab), the hull per pixel.
import { defineMaterial, param, vec2, vec3, mix, mul, clamp, floor, step, add, sub, fbm, div, seed, world, shadeBand, sunDirection, normal } from "@superspec/material/v1";
import { fracture, contour, pigment } from "./kowloon-noise.material";

const bandOffset = param.scalar({ default: -0.31, range: [-1, 1], label: "Band offset" });
const tintStrength = param.scalar({ default: 0.25, range: [0, 1], label: "Tint strength" });
const darkness = param.scalar({ default: 0.2, range: [0, 0.6], label: "Mottling darkness" });
const hueAmount = param.scalar({ default: 0.18, range: [0, 0.5], label: "Hue variation" });
const mottleScale = param.scalar({ default: 0.35, range: [0.05, 2], unit: "m", label: "Mottling scale" });

// Six band colors (sRGB #2a5a52, #336a51, #3c754f, #528650, #6c9b52, #8fb85a), linear: a cool teal shade to a sunlit
// green, the shade steps closer together than the lit ones (the shade side reads as one soft mass, as painted).
const band0 = vec3(0.0232, 0.1022, 0.0844);
const band1 = vec3(0.0331, 0.1441, 0.0823);
const band2 = vec3(0.0452, 0.1779, 0.0782);
const band3 = vec3(0.0844, 0.2384, 0.0802);
const band4 = vec3(0.15, 0.3277, 0.0844);
const band5 = vec3(0.2747, 0.4793, 0.1022);
// A warm straw (#d6cf7a), not the study's lime.
const tint = vec3(0.672, 0.624, 0.195);

const t = shadeBand(normal, sunDirection, bandOffset, 0, 1.4);
const level = floor(clamp(mul(t, 6), 0, 5.999));
const banded = mix(mix(mix(mix(mix(band0, band1, step(1, level)), band2, step(2, level)), band3, step(3, level)), band4, step(4, level)), band5, step(5, level));

// Canopy-plane coordinates in mottling units: world XY sheared by height, so the sides are not streaked.
const at = div(vec2(add(world.x, mul(world.z, 0.6)), sub(world.y, mul(world.z, 0.4))), mottleScale);
// The concrete's two eroded darkness fields at their own scales, then its pigment hue drift.
const darkA = contour(fracture(add(mul(at, 1.7), vec2(5.3, 17.1)), 0.8, seed("shrub-dark-a")), 0.035);
const darkB = contour(fracture(add(mul(at, 3.1), vec2(73.4, 29.6)), 0.8, seed("shrub-dark-b")), 0.035);
const mottled = mul(mul(banded, sub(1, mul(darkness, darkA))), sub(1, mul(mul(darkness, 0.6), darkB)));
const drifted = pigment(mottled, at, hueAmount, 40, seed("shrub-pigment"));
// The study's tint noise (contrast 1.6 about its middle), world-anchored.
const tintValue = clamp(add(mul(sub(fbm(mul(at, 0.7), seed("shrub-tint"), 3), 0.5), 1.6), 0.5), 0, 1);
const color = mul(drifted, mix(vec3(1, 1, 1), tint, mul(tintValue, tintStrength)));

export default defineMaterial({
  name: "Painted shrub",
  shading: "painted",
  base: { baseColor: band2, roughness: 0.7 },
  paint: color,
});
