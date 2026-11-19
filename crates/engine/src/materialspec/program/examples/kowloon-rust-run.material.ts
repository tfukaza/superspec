// A stain that runs along a line, as a weathering layer: rust washed down a wall from a bracket, a bolt or a fixing,
// following the feature the slot names `run` (a polyline its owner draws from the source downward: where the water
// goes). The run is a few centimeters wide, wider and fainter in places, broken into threads, with a pale wash of the
// same colour spreading beside it. It darkens the surface beneath by multiplication, so paint and grime show through;
// a receiver without the feature shows nothing. Give it any stain's colour: rust by default, verdigris, tar, lime.
import { defineLayer, param, featureDistance, smoothstep, valueNoise, add, mul, sub, min, max, vec2, vec3, seed, surface } from "@superspec/material/v1";

const color = param.vec3({ default: vec3(0.43, 0.23, 0.095), range: [0, 1], label: "Stain colour" });
const strength = param.scalar({ default: 0.95, range: [0, 1], label: "Strength" });
const thickness = param.scalar({ default: 0.045, range: [0.005, 0.3], unit: "m", label: "Run width" });
const wash = param.scalar({ default: 0.45, range: [0, 1], label: "Wash beside the run" });
const washReach = param.scalar({ default: 0.18, range: [0.01, 1], unit: "m", label: "Wash reach" });

const away = featureDistance("run", 2);
const at = vec2(surface.x, surface.y);
// The run: its width and weight wander along its fall; fine threads break it across.
const swell = valueNoise(mul(at, vec2(6, 2.6)), seed("run-swell"));
const breadth = mul(thickness, add(0.6, mul(0.9, swell)));
const threads = add(0.75, mul(0.25, smoothstep(0.3, 0.75, valueNoise(mul(at, vec2(90, 5)), seed("run-threads")))));
const weight = add(0.7, mul(0.3, valueNoise(mul(at, vec2(3, 1.4)), seed("run-weight"))));
const core = mul(mul(sub(1, smoothstep(mul(breadth, 0.15), breadth, away)), threads), weight);
// The wash: a soft halo, patchy.
const halo = mul(mul(wash, sub(1, smoothstep(0, washReach, away))), add(0.3, mul(0.7, valueNoise(mul(at, vec2(11, 3)), seed("run-wash")))));

export default defineLayer({
  name: "Kowloon rust run",
  coverage: mul(strength, min(max(core, halo), 1)),
  baseColor: color,
  blend: { baseColor: "multiply" },
});
