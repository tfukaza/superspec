// The dark band where a Kowloon concrete wall meets damp ground, as a weathering layer: the
// shipped concrete's ground stain, following the feature the slot names `base`. It darkens the
// surface beneath by multiplication. An item that stands off the ground has no `base` to follow.
import { defineLayer, param, smoothstep, vec3 } from "@superspec/material/v1";
import { aboveBase, groundStain } from "./kowloon-ground.material";

const dampness = param.scalar({ default: 0.5, range: [0, 1], label: "Ground dampness" });

export default defineLayer({
  name: "Kowloon ground stain",
  coverage: groundStain(smoothstep(0.5, 1, dampness), aboveBase(0.6)),
  baseColor: vec3(0.19, 0.16, 0.13),
  blend: { baseColor: "multiply" },
});
