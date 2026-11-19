// Old window glass seen from the street by day: dark, a little reflective, never the same twice. The room behind is
// painted into it rather than modelled: curtains drawn part way from either side in one of a few faded cloths, a drape
// or blind across some windows, a lit tube under the ceiling of a few, and the dark of the room deepening upward.
// Over that lies the dirt of the glass itself: a dusty film in rain-washed streaks, and grime packed along every pane's
// edge (the receiver's open boundaries, so each pane of a glazed window gets its own rim) and thickest along the foot.
//
// A receiver is one window's glazing: what it shows is chosen from where its bounds are, so every window differs and
// keeps its look when rebaked. Nothing is read but position, bounds and the receiver's own edges.
import {
  defineMaterial, param, vec2, vec3, add, sub, mul, div, mix, abs, max, min, clamp, smoothstep, step, select, lt,
  dot, cross, hash, valueNoise, creaseDistance, seed, world, normal, boundsMin, boundsMax,
} from "@superspec/material/v1";

const tint = param.vec3({ default: vec3(0.03, 0.042, 0.04), range: [0, 1], label: "Glass colour" });
const interior = param.scalar({ default: 0.6, range: [0, 1], label: "Room behind" });
const curtains = param.scalar({ default: 0.7, range: [0, 1], label: "Share with curtains" });
const lamps = param.scalar({ default: 0.2, range: [0, 1], label: "Share with a lit tube" });
const sky = param.scalar({ default: 0.5, range: [0, 1], label: "Sky sheen" });
const dirt = param.scalar({ default: 0.55, range: [0, 1], label: "Dirt" });
const gloss = param.scalar({ default: 0.1, range: [0, 1], label: "Clean roughness" });
const glow = param.scalar({ default: 1.5, range: [0, 8], label: "Lit tube glow" });
// How much of the room behind shows through the glass itself (0: the glass is opaque and the room is painted into it).
// Through clear glass a host draws what stands behind; dirt and the sky's sheen stay on the glass.
const transmission = param.scalar({ default: 0, range: [0, 0.9], label: "See-through" });

// Where the texel is in its window: meters from one side along the glass, and 0 to 1 across and up it.
const extent = sub(boundsMax, boundsMin);
const tangent = cross(vec3(0, 0, 1), normal);
const span = max(dot(abs(tangent), extent), 0.01);
const offset0 = dot(sub(world, boundsMin), tangent);
const run = select(lt(offset0, 0), add(offset0, span), offset0);
const acrossIt = clamp(div(run, span), 0, 1);
const upIt = clamp(div(sub(world.z, boundsMin.z), max(extent.z, 0.01)), 0, 1);
const key = mul(boundsMin, 7.31);

// Curtains from either side, each its own width, in folds.
const leftWidth = add(0.08, mul(0.2, hash(key, seed("curtain-left"))));
const rightWidth = add(0.08, mul(0.2, hash(key, seed("curtain-right"))));
const drawn = step(hash(key, seed("curtain-drawn")), curtains);
const curtain = mul(drawn, max(sub(1, smoothstep(sub(leftWidth, 0.015), leftWidth, acrossIt)), smoothstep(sub(1, rightWidth), add(sub(1, rightWidth), 0.015), acrossIt)));
const folds = add(0.55, mul(0.45, valueNoise(vec2(mul(run, 26), mul(world.z, 0.6)), seed("curtain-folds"))));
const cloth = mix(mix(vec3(0.8, 0.74, 0.56), vec3(0.42, 0.52, 0.6), step(0.6, hash(key, seed("cloth-a")))), vec3(0.72, 0.5, 0.42), step(0.8, hash(key, seed("cloth-b"))));
// A drape or blind across the whole window in some, further back and dimmer.
const draped = mul(step(hash(key, seed("drape")), 0.4), smoothstep(0.25, 0.4, upIt));
const drape = mul(vec3(0.3, 0.31, 0.27), add(0.5, mul(0.5, valueNoise(vec2(mul(run, 9), 0.5), seed("drape-folds")))));
// A fluorescent tube under the ceiling of a few rooms.
const tubeHeight = add(0.8, mul(0.1, hash(key, seed("tube-height"))));
const tubeLine = sub(1, smoothstep(0.012, 0.022, abs(sub(upIt, tubeHeight))));
const tubeRun = mul(smoothstep(0.2, 0.26, acrossIt), sub(1, smoothstep(0.7, 0.76, acrossIt)));
const tube = mul(mul(step(hash(key, seed("tube")), lamps), tubeLine), tubeRun);

// The room: darker toward its ceiling, uneven from window to window; the glass adds the sky's sheen toward its top.
const depth = mul(add(0.55, mul(0.9, hash(key, seed("room")))), sub(1, mul(0.5, upIt)));
const room = mul(tint, depth);
const behind = mix(mix(room, mul(drape, 0.16), mul(draped, interior)), mul(mul(cloth, folds), 0.2), mul(curtain, interior));
const lit = mix(behind, vec3(0.62, 0.68, 0.58), mul(tube, interior));
const sheen = mul(mul(sky, 0.14), mul(smoothstep(0.05, 1, upIt), add(0.4, mul(0.6, valueNoise(mul(vec2(run, world.z), 1.7), seed("sheen"))))));
const glass = add(lit, mul(vec3(0.55, 0.62, 0.66), sheen));

// Dirt: a film in streaks, and grime along each pane's edges and along the foot.
const rim = creaseDistance("boundary", 30, 0.16);
const streaks = smoothstep(0.35, 0.8, valueNoise(vec2(mul(run, 23), mul(world.z, 1.1)), seed("glass-streaks")));
const dirty = mul(dirt, add(0.45, mul(1.1, hash(key, seed("glass-dirty")))));
const film = mul(mul(dirty, 0.3), add(0.35, mul(0.65, streaks)));
const packed = mul(dirty, mul(sub(1, smoothstep(0.004, 0.11, rim)), add(0.45, mul(0.55, valueNoise(mul(vec2(run, world.z), 31), seed("glass-rim"))))));
const footing = mul(mul(dirty, 0.5), mul(sub(1, smoothstep(0, 0.22, upIt)), add(0.4, mul(0.6, streaks))));
// Dust washed down from the head of the window in runs.
const heads = mul(mul(dirty, 0.55), mul(smoothstep(0.55, 1, upIt), smoothstep(0.4, 0.75, valueNoise(vec2(mul(run, 37), mul(world.z, 0.7)), seed("glass-head")))));
const soiled = min(add(add(add(film, mul(packed, 0.8)), footing), heads), 0.9);

export default defineMaterial({
  name: "Kowloon window glass",
  base: {
    baseColor: mix(glass, vec3(0.2, 0.19, 0.165), soiled),
    roughness: mix(gloss, 0.85, soiled),
    metalness: 0,
    // A lit tube glows through the glass and its dirt.
    emissive: mul(vec3(1, 0.86, 0.62), mul(mul(tube, interior), mul(glow, sub(1, mul(soiled, 0.5))))),
    // Clear glass lets the room through where it is clean; dirt and sheen are on the glass.
    opacity: clamp(add(sub(1, transmission), add(mul(soiled, 1.2), mul(sheen, 2))), 0, 1),
  },
});
