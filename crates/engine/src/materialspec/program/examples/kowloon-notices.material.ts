// Paper notices pasted on a wall, as a weathering layer: bills, rental slips and advertisements from the catalog's
// poster sheet, each a little askew, faded, rain-stained and torn at its corners, with only a ghost of paste and paper
// left where one has been pulled off.
//
// The owner of the geometry says where a notice hangs with three features of its frame, `size` wide and
// `size * aspect` tall: `left` and `right`, its upright edges, and `bottom`, its lower edge. A texel is inside a frame
// where its distances to the nearest left and right edges add up to the frame's width, so any number of frames can
// hang on one wall (keep them apart by anything but exactly one width). Which notice a frame shows, its tilt, tears
// and fading come from where the frame is, so a wall keeps its notices when it is rebaked. Nothing is read but the
// three distances and one image.
import { defineLayer, param, featureDistance, sampleColor, smoothstep, step, hash, valueNoise, rotate, cross, dot, floor, abs, rem, neg, add, mul, sub, div, min, max, mix, vec2, vec3, seed, world, normal, gravity } from "@superspec/material/v1";

const size = param.scalar({ default: 0.3, range: [0.05, 2], unit: "m", label: "Notice width" });
const aspect = param.scalar({ default: 1.5, range: [0.5, 3], label: "Height over width" });
const kept = param.scalar({ default: 0.8, range: [0, 1], label: "Share still hanging" });
const wear = param.scalar({ default: 0.45, range: [0, 1], label: "Tears" });
const fading = param.scalar({ default: 0.4, range: [0, 1], label: "Fading" });
const grime = param.scalar({ default: 0.5, range: [0, 1], label: "Grime" });

const fromLeft = featureDistance("left", 4);
const fromRight = featureDistance("right", 4);
const fromBottom = featureDistance("bottom", 4);
const tall = mul(size, aspect);
const framed = sub(1, smoothstep(0.002, 0.004, abs(sub(add(fromLeft, fromRight), size))));

// Where the frame is: its middle along the wall and its foot, the same for every texel inside it.
const up = neg(gravity);
const across = cross(normal, up);
const middle = add(dot(world, across), mul(sub(fromLeft, fromRight), 0.5));
const foot = sub(dot(world, up), fromBottom);
const frame = vec2(mul(middle, 37.3), mul(foot, 37.3));
const pick = hash(frame, seed("notice-pick"));
const tilt = mul(sub(hash(frame, seed("notice-tilt")), 0.5), 0.12);
const corner = vec2(sub(mul(step(0.5, hash(frame, seed("notice-corner-x"))), 2), 1), sub(mul(step(0.5, hash(frame, seed("notice-corner-y"))), 2), 1));
const hanging = step(hash(frame, seed("notice-kept")), kept);

// The sheet inside the frame: a little smaller, turned about its middle.
const centered = vec2(sub(fromLeft, mul(size, 0.5)), sub(fromBottom, mul(tall, 0.5)));
const turned = rotate(centered, tilt);
const half = vec2(mul(size, 0.44), mul(tall, 0.44));
const inset = sub(half, abs(turned));
const ragged = mul(mul(wear, 0.008), valueNoise(mul(turned, 140), seed("notice-edge")));
const cut = mul(add(0.006, mul(wear, 0.035)), add(1, mul(mul(mul(step(0, mul(turned.x, corner.x)), step(0, mul(turned.y, corner.y))), 4), wear)));
const sheet = mul(mul(
  smoothstep(ragged, add(ragged, 0.002), min(inset.x, inset.y)),
  smoothstep(cut, add(cut, 0.002), add(inset.x, inset.y))),
  mul(framed, step(fromBottom, tall)));

// Its print: one of the sixteen 512 by 768 cells of the 2048 by 3072 sheet, inside the cell's margins.
const index = floor(mul(pick, 16));
const column = rem(index, 4);
const row = floor(div(index, 4));
const across01 = add(div(turned.x, mul(half.x, 2)), 0.5);
const upward01 = add(div(turned.y, mul(half.y, 2)), 0.5);
const print = sampleColor("superspec-material:posters-atlas.jpg", vec2(
  div(add(add(mul(column, 512), 16.5), mul(across01, 479)), 2048),
  div(add(add(mul(row, 768), 24.5), mul(sub(1, upward01), 719)), 3072)));

// Sun and rain: bleached toward old paper in clouds, and streaked darker from its top edge.
const cloud = valueNoise(add(mul(turned, 9), mul(frame, 0.37)), seed("notice-fade"));
const bleached = mix(print, vec3(0.62, 0.58, 0.47), mul(fading, add(0.35, mul(0.65, cloud))));
const runs = mul(smoothstep(0.45, 0.8, valueNoise(vec2(mul(turned.x, 55), mul(turned.y, 3)), seed("notice-runs"))), upward01);
const soiled = mul(bleached, sub(1, mul(grime, add(mul(0.3, runs), mul(0.25, sub(1, cloud))))));

// A notice pulled off leaves paste and scraps: a pale patch with torn remains.
const scraps = smoothstep(0.55, 0.7, valueNoise(add(mul(turned, 23), frame), seed("notice-scraps")));
const ghost = mul(add(0.22, mul(0.5, scraps)), sub(1, hanging));

export default defineLayer({
  name: "Kowloon notices",
  coverage: mul(sheet, max(hanging, ghost)),
  baseColor: mix(vec3(0.5, 0.47, 0.4), soiled, hanging),
  roughness: 0.9,
});
