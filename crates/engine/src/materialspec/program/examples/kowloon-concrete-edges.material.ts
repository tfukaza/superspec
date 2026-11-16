// Worn convex edges of Kowloon concrete, as a weathering layer over a base: the shipped recipe's
// 14 mm crease treatment (broken grooves and chipped shoulders, darker and lower) and its shading
// round-over toward the adjacent face. It follows the receiver's own creases, so it lies wherever
// a face turns, whatever the receiver's size.
import {
  defineLayer, param, vec2, vec3, add, sub, mul, mix, smoothstep, dot, valueNoise, seed,
  creaseDistance, creaseNormal, object, normal, footprint,
} from "@superspec/material/v1";
import { fractal, shoulder } from "./kowloon-noise.material";

const edgeWidth = param.scalar({ default: 0.014, range: [0.002, 0.05], unit: "m", label: "Edge width" });
const edgeAngle = param.scalar({ default: 35, range: [1, 179], unit: "deg", label: "Sharpest flat angle" });

const creaseLimit = add(mul(edgeWidth, 2.5), footprint);
const creaseNearest = creaseDistance("convex", edgeAngle, creaseLimit);
const edgePockets = fractal(add(add(mul(vec2(object.x, object.y), 13), mul(vec2(7.9, 15.3), object.z)), 37.2), seed("edge-pockets"));
const edgeGrit = valueNoise(add(mul(vec2(object.x, object.y), 170), mul(vec2(99, 211), object.z)), seed("edge-grit"));
const edgeField = add(edgePockets, mul(sub(edgeGrit, 0.5), 0.08));
const edgeChips = smoothstep(0.54, 0.75, edgeField);
const edgeShoulder = mul(shoulder(creaseNearest, mul(edgeWidth, mix(0.6, 2.5, edgeChips))), edgeChips);
const edgeGroove = mul(shoulder(creaseNearest, mul(edgeWidth, 0.34)), smoothstep(0.4, 0.66, edgeField));
const edgeDarkness = add(mul(0.16, edgeGroove), mul(0.2, edgeShoulder));
const edgeHeight = mul(mul(-0.006, edgeShoulder), add(0.5, mul(0.5, edgeGrit)));
const roundOver = sub(1, smoothstep(0, edgeWidth, creaseNearest));
const creaseMiddle = creaseNormal("convex", edgeAngle, add(edgeWidth, footprint));
// The adjacent face's normal: the receiver normal reflected across the crease's middle normal.
const creaseAdjacent = sub(mul(creaseMiddle, mul(2, dot(creaseMiddle, normal))), normal);

export default defineLayer({
  name: "Kowloon concrete edges",
  coverage: 1,
  baseColor: vec3(sub(1, edgeDarkness)),
  height: edgeHeight,
  normalOffset: mul(creaseAdjacent, roundOver),
  blend: { baseColor: "multiply", height: "add" },
});
