// Grime washed down from a wall's top, as a weathering layer: the shipped Kowloon concrete's top
// grime (the study's periodic drip noise, a metre deep), measured from the feature the slot names
// `top` instead of the receiver's bounds. It darkens the surface beneath by multiplication.
import { defineLayer, param, featureDistance, smoothstep, pow, sub, mul, div, surface, vec3 } from "@superspec/material/v1";
import { topGrime } from "./kowloon-noise.material";

const amount = param.scalar({ default: 1, range: [0, 1], label: "Top grime" });

// Meters below the top, and along the wall on the pattern anchor.
const topDistance = featureDistance("top", 1);
const grime = mul(pow(topGrime(div(surface.x, 8), topDistance), 0.75), sub(1, smoothstep(0.85, 1, topDistance)));

export default defineLayer({
  name: "Kowloon top grime",
  coverage: mul(amount, grime),
  baseColor: vec3(0.32, 0.28, 0.24),
  blend: { baseColor: "multiply" },
});
