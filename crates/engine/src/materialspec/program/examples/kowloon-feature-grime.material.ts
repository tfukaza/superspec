// Grime gathered along a feature the slot names (a sill, a wall's base, a joint), as a weathering
// layer: a band from `near` to `far` meters that mixes a dirt color and roughness over the surface
// beneath. The slot says which feature it follows: `{ "follow": "sill" }`.
import { defineLayer, param, featureDistance, smoothstep, sub, mul, vec3 } from "@superspec/material/v1";

const amount = param.scalar({ default: 0.8, range: [0, 1], label: "Grime" });
const near = param.scalar({ default: 0.02, range: [0, 1], unit: "m", label: "Full grime within" });
const far = param.scalar({ default: 0.22, range: [0.03, 2], unit: "m", label: "No grime beyond" });
const color = param.vec3({ default: vec3(0.03, 0.03, 0.028), range: [0, 1], label: "Grime color" });
const roughness = param.scalar({ default: 0.92, range: [0, 1], label: "Grime roughness" });

export default defineLayer({
  name: "Feature grime",
  coverage: mul(amount, sub(1, smoothstep(near, far, featureDistance("follow", 2)))),
  baseColor: color,
  roughness,
});
