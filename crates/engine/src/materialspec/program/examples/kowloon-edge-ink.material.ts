// Edges drawn as a painter inks them, as a weathering layer over any surface: along every exposed edge of the receiver
// (its convex creases and open rims: a wall's corners, a pier's arrises, the nose of a sill or a string course, a slab
// or coping edge, a kerb) a thin worn arris, paler and a little glossier, chipped here and there, and beside it a
// narrow line of grime fading out over a few centimeters; along every inside corner (concave creases, and edges lying
// on another surface where a project build supplies them) the same grime, a little heavier. The lines are uneven and
// broken along their length, never ruled, and upright edges carry more than level ones, so a corner reads as a soft
// pencil line. Where the slot follows `joint` (the seam where one surface of a wall meets the next, such as a rounded
// corner's start and end), no line or arris is drawn within `jointClear` of it. Convex creases are also rounded over in
// shading, toward the face beyond them, without new geometry (an open rim has no face beyond it to turn to). Nothing is
// read but the receiver's own edges and position, and the joints it is given.
import {
  defineLayer, param, vec2, vec3, add, sub, mul, div, mix, abs, max, min, smoothstep, dot, valueNoise, seed,
  creaseDistance, creaseNormal, creaseTangent, featureDistance, surface, normal, footprint,
} from "@superspec/material/v1";

const inkColor = param.vec3({ default: vec3(0.15, 0.135, 0.115), range: [0, 1], label: "Grime line colour" });
const inkStrength = param.scalar({ default: 0.5, range: [0, 1], label: "Grime line strength" });
const inkWidth = param.scalar({ default: 0.045, range: [0.005, 0.3], unit: "m", label: "Grime line width" });
const breakup = param.scalar({ default: 0.5, range: [0, 1], label: "Line breakup" });
const upright = param.scalar({ default: 0.5, range: [0, 1], label: "Emphasis on upright edges" });
const highlight = param.scalar({ default: 0.35, range: [0, 1], label: "Arris highlight" });
const highlightWidth = param.scalar({ default: 0.009, range: [0.002, 0.05], unit: "m", label: "Arris width" });
const wear = param.scalar({ default: 0.4, range: [0, 1], label: "Arris chipping" });
const radius = param.scalar({ default: 0.014, range: [0.002, 0.05], unit: "m", label: "Rounded edge radius" });
const jointClear = param.scalar({ default: 0.06, range: [0.005, 0.5], unit: "m", label: "Clear of a joint" });
const edgeAngle = param.scalar({ default: 35, range: [1, 179], unit: "deg", label: "Sharpest flat angle" });

const limit = add(mul(inkWidth, 1.6), footprint);
const exposed = creaseDistance("exposed", edgeAngle, limit);
const inside = min(creaseDistance("concave", edgeAngle, limit), creaseDistance("contact", edgeAngle, limit));
const along = creaseTangent("exposed", edgeAngle, limit);

// The line's width and weight wander along it; in places it breaks off.
const at = vec2(surface.x, surface.y);
const wide = mul(inkWidth, add(0.55, mul(0.9, valueNoise(mul(at, 2.7), seed("ink-width")))));
const broken = sub(1, mul(breakup, smoothstep(0.42, 0.72, valueNoise(add(mul(at, 5.3), vec2(3.1, 7.7)), seed("ink-breaks")))));
const grain = add(0.75, mul(0.25, valueNoise(mul(at, 38), seed("ink-grain"))));
const standing = mix(sub(1, upright), 1, abs(along.z));
// Beside an exposed edge the grime starts where the worn arris ends.
const beside = mul(mul(sub(1, smoothstep(mul(wide, 0.2), wide, exposed)), smoothstep(mul(highlightWidth, 0.6), mul(highlightWidth, 1.4), exposed)), standing);
const corner = sub(1, smoothstep(0, mul(wide, 1.3), inside));
const ink = mul(mul(mul(inkStrength, max(beside, corner)), broken), grain);

// The arris itself: worn paler and smoother, with chips where it is not.
const chips = smoothstep(0.55, 0.72, valueNoise(add(mul(at, 23), vec2(9.4, 1.6)), seed("ink-chips")));
const arris = mul(mul(highlight, sub(1, smoothstep(mul(highlightWidth, 0.5), highlightWidth, exposed))), sub(1, mul(wear, chips)));

// Rounding: the receiver normal turned toward the face beyond a convex crease.
const round = max(radius, 0.0005);
const middle = creaseNormal("convex", edgeAngle, add(round, footprint));
const beyond = sub(mul(middle, mul(2, dot(middle, normal))), normal);
const turned = sub(1, smoothstep(0, round, creaseDistance("convex", edgeAngle, add(round, footprint))));

// Beside a joint the wall runs on into the next surface: neither line nor arris.
const clear = smoothstep(mul(jointClear, 0.5), jointClear, featureDistance("joint", add(jointClear, 0.01)));
const both = max(add(ink, arris), 0.0001);

export default defineLayer({
  name: "Kowloon edge ink",
  coverage: mul(min(max(ink, arris), 1), clear),
  baseColor: mix(inkColor, vec3(0.82, 0.79, 0.72), div(arris, both)),
  roughness: mix(0.95, 0.45, div(arris, both)),
  normalOffset: mul(beyond, mul(turned, clear)),
});
