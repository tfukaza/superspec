// Repairs and lost paint on a painted wall, as a weathering layer placed by the wall's owner: nothing appears but where
// the owner draws a frame.
//
// A repair is a hard-edged rectangle of new plaster or mismatched repaint: flat, a little lighter, greyer or warmer
// than the coat around it, with a thin dark outline where its edge stands proud and collects dirt. The owner draws its
// outline as the closed polyline the slot names `repair`, and the same outline `inset` (5 cm) further in as
// `repairInner`: a distance alone cannot say which side of an outline a texel is on, the two together can (inside,
// the outline is farther than the inner one by the inset; outside, nearer by it).
//
// A peel is where the coat has come off in a sheet: an irregular, crisp-edged patch of flat grey cement with a thin
// dark rim under the lifted paint. The owner draws the rectangle it stays within as `peel` and `peelInner` the same
// way; the patch's own ragged shape inside it comes from where it is, so a wall keeps its patches when it is rebaked.
import {
  defineLayer, param, featureDistance, smoothstep, select, lt, valueNoise, add, mul, sub, div, min, max, mix, clamp, vec2, vec3, seed, surface, world,
} from "@superspec/material/v1";
import type { Scalar, ScalarInput } from "@superspec/material/v1";

const color = param.vec3({ default: vec3(0.78, 0.71, 0.55), range: [0, 1], label: "Coat colour" });
const cement = param.vec3({ default: vec3(0.46, 0.45, 0.42), range: [0, 1], label: "Cement colour" });
const contrast = param.scalar({ default: 0.8, range: [0, 1], label: "Repair tone difference" });
const outline = param.scalar({ default: 0.85, range: [0, 1], label: "Outline strength" });
const outlineWidth = param.scalar({ default: 0.02, range: [0.002, 0.06], unit: "m", label: "Outline width" });
const inset = param.scalar({ default: 0.05, range: [0.01, 0.3], unit: "m", label: "Inner outline inset" });
const peelShare = param.scalar({ default: 0.6, range: [0, 1], label: "Peeled share of a peel frame" });

const reach = 3;
const at = vec2(surface.x, surface.y);

// How far inside an outline a texel is (0 on it and outside, up to the inset), and whether there is one at all.
const depthIn = (outer: ScalarInput, inner: ScalarInput): Scalar =>
  select(lt(outer, sub(reach, 0.05)), clamp(mul(add(sub(outer, inner), inset), 0.5), 0, inset), 0);

// Repairs.
const repairEdge = featureDistance("repair", reach);
const repairDepth = depthIn(repairEdge, featureDistance("repairInner", reach));
const ragged = mul(sub(valueNoise(mul(at, 60), seed("repair-edge")), 0.5), 0.006);
const repaired = smoothstep(0.001, 0.004, add(repairDepth, ragged));
// Its tone is one value over a patch (a slow field, read coarsely) and differs from patch to patch.
const kind = valueNoise(mul(at, 0.31), seed("repair-kind"));
const shade = valueNoise(add(mul(at, 0.37), vec2(5.1, 2.3)), seed("repair-shade"));
const greyed = mix(color, add(mul(cement, 0.75), mul(color, 0.35)), mul(contrast, add(0.3, mul(0.6, smoothstep(0.3, 0.7, kind)))));
const trowel = add(0.97, mul(0.06, valueNoise(mul(at, vec2(3, 26)), seed("repair-trowel"))));
const repairColor = mul(mul(greyed, add(1, mul(mul(contrast, 0.5), sub(shade, 0.42)))), trowel);
// The outline: a thin broken line on the edge itself, heavier along the lower edge of the tone field.
const lineBreak = add(0.55, mul(0.45, smoothstep(0.3, 0.6, valueNoise(mul(at, 9), seed("repair-line")))));
const repairLine = mul(mul(outline, sub(1, smoothstep(mul(outlineWidth, 0.3), outlineWidth, repairEdge))), mul(lineBreak, select(lt(repairEdge, sub(reach, 0.05)), 1, 0)));

// Peels.
const peelEdge = featureDistance("peel", reach);
const peelDepth = depthIn(peelEdge, featureDistance("peelInner", reach));
// Inside its frame the patch is where a broad field, pulled down toward the frame's edge, stays above a threshold.
const broad = add(mul(valueNoise(mul(at, 3.3), seed("peel-broad")), 0.62), mul(valueNoise(mul(at, 13), seed("peel-fine")), 0.38));
const field = mul(broad, div(peelDepth, inset));
const threshold = sub(0.52, mul(peelShare, 0.36));
const peeled = smoothstep(threshold, add(threshold, 0.012), field);
const rim = mul(peeled, sub(1, smoothstep(add(threshold, 0.012), add(threshold, 0.05), field)));
const grain = add(0.94, mul(0.1, valueNoise(mul(at, 41), seed("peel-grain"))));
const peelColor = mix(mul(cement, grain), mul(cement, 0.42), rim);

const lineColor = mul(color, vec3(0.3, 0.27, 0.23));
const repairLook = mix(repairColor, lineColor, min(repairLine, 1));
const cover = max(max(repaired, repairLine), peeled);

export default defineLayer({
  name: "Kowloon repairs",
  coverage: min(cover, 1),
  baseColor: mix(repairLook, peelColor, peeled),
  roughness: mix(0.8, 0.93, peeled),
});
