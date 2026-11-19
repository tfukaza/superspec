// Painted sign lettering that has weathered: the catalog's glyph sheet by the mesh's UVs, cut out along its strokes
// (pale: the ink's own colour is its tint), with paint lost in flakes so strokes break up, the rest chalked and
// soiled. `wear` 0 is fresh lettering.
import {
  defineMaterial, param, vec2, vec3, add, sub, mul, mix, smoothstep, valueNoise, sampleColor, sampleAlpha, alphaCutout,
  seed, uv, surface, world,
} from "@superspec/material/v1";

const wear = param.scalar({ default: 0.45, range: [0, 1], label: "Lost paint" });
const fading = param.scalar({ default: 0.4, range: [0, 1], label: "Chalking" });

// The glyph cells are addressed from the sheet's top row, as the sign's mesh maps them.
const sheet = vec2(uv.x, uv.y);
const ink = sampleColor("superspec-material:lettering.png", sheet);
const stroke = sampleAlpha("superspec-material:lettering.png", sheet);
const round = surface.x;
const at = vec2(surface.x, surface.y);
// Flakes: small and fine-edged, so a stroke loses paint along its length and at its edges (where the cutout is
// thin) but never a whole stroke; a stroke survives where the field stays under the threshold.
const cluster = valueNoise(mul(at, 14), seed("letter-cluster"));
const chips = valueNoise(mul(at, 90), seed("letter-chips"));
const edge = sub(1, smoothstep(0.5, 0.85, stroke));
const flake = add(add(mul(cluster, 0.45), mul(chips, 0.55)), mul(edge, 0.25));
const kept = sub(1, smoothstep(sub(0.9, mul(wear, 0.34)), sub(0.93, mul(wear, 0.3)), flake));
const cloud = valueNoise(mul(at, 3.1), seed("letter-fade"));
const chalked = mix(ink, add(mul(ink, 0.7), vec3(0.22, 0.21, 0.19)), mul(fading, add(0.3, mul(0.7, cloud))));

export default defineMaterial({
  name: "Kowloon sign lettering",
  base: { baseColor: chalked, roughness: 0.8, metalness: 0, opacity: alphaCutout(mul(stroke, kept), 0.5) },
});
