// Grime on the hidden sides of a fixture that stands off a wall (a duct, a pipe), as a weathering layer: its underside,
// where dirt and damp collect and the rain never washes, and the side that faces the wall, which no sun or rain reaches.
// The wall's side is told by the feature the slot follows as `fixture` (the line on the wall behind the fixture): the
// faces nearer to it than `facing` metres. It darkens the surface beneath by multiplication, in soft uneven patches.
import { defineLayer, param, featureDistance, smoothstep, valueNoise, add, mul, sub, min, max, neg, vec2, vec3, seed, surface, normal } from "@superspec/material/v1";

const color = param.vec3({ default: vec3(0.42, 0.39, 0.34), range: [0, 1], label: "Grime colour" });
const strength = param.scalar({ default: 0.75, range: [0, 1], label: "Strength" });
const facing = param.scalar({ default: 0.3, range: [0.02, 2], unit: "m", label: "Wall side within" });

const at = vec2(surface.x, surface.y);
const patchy = add(0.55, mul(0.45, valueNoise(mul(at, 4.5), seed("underside-patches"))));
const downward = smoothstep(0.2, 0.7, neg(normal.z));
const wallSide = sub(1, smoothstep(mul(facing, 0.7), facing, featureDistance("fixture", add(facing, 0.2))));

export default defineLayer({
  name: "Kowloon underside grime",
  coverage: mul(strength, mul(min(max(downward, mul(wallSide, 0.8)), 1), patchy)),
  baseColor: color,
  blend: { baseColor: "multiply" },
});
