// A stain washed down from a fixing, as a weathering layer: rust from a bracket, a bolt, a cage's foot or a sign's
// fixing plate, in the zones the slot follows as `zone` (each a short edge where the water leaves the fixing, its owner
// says how far it runs). The run is as wide as its zone's edge at the top, breaks into threads and thins as it falls,
// and fades out where its zone ends. It darkens the surface beneath by multiplication, so paint and grime show
// through; a receiver without such a zone shows nothing. Give it any stain's colour: rust by default, verdigris, tar,
// lime.
import { defineLayer, param, zoneCoord, zoneExtent, smoothstep, valueNoise, add, mul, sub, div, min, max, vec2, vec3, seed, select, gt } from "@superspec/material/v1";

const color = param.vec3({ default: vec3(0.43, 0.23, 0.095), range: [0, 1], label: "Stain colour" });
const strength = param.scalar({ default: 0.95, range: [0, 1], label: "Strength" });
const breakup = param.scalar({ default: 0.6, range: [0, 1], label: "Threads" });

const zone = zoneCoord("zone");
const extent = zoneExtent("zone");
const fall = div(zone.y, max(extent.y, 0.05));
// Threads across the run, each wandering a little as it falls; fewer of them survive further down.
const at = vec2(add(mul(zone.x, 44), mul(zone.z, 53)), mul(zone.y, 2.5));
const threads = valueNoise(at, seed("run-threads"));
const kept = smoothstep(sub(0.25, mul(0.5, mul(breakup, sub(1, fall)))), add(0.5, mul(breakup, 0.25)), add(threads, mul(0.35, sub(1, fall))));
const weight = add(0.65, mul(0.35, valueNoise(vec2(mul(zone.z, 91), mul(zone.y, 1.5)), seed("run-weight"))));
const fade = sub(1, smoothstep(0.55, 1, fall));
// Darkest right under the fixing.
const head = sub(1, smoothstep(0, 0.06, zone.y));

export default defineLayer({
  name: "Kowloon rust run",
  coverage: select(gt(extent.x, 0), mul(mul(strength, extent.x), min(add(mul(mul(kept, weight), fade), mul(head, 0.5)), 1)), 0),
  baseColor: color,
  blend: { baseColor: "multiply" },
});
