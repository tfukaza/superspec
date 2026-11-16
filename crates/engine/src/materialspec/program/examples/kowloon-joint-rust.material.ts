// Rust gathered at a steel route's joints (seams, flanges, couplings), as a weathering layer over
// Kowloon rusted steel: the steel's own oxide color (the catalog rust image, triplanar, toned
// between two browns) and oxide roughness, in a band around the feature the slot names `joint`.
// Give it the base's oxide parameters so the two rusts match.
import {
  defineLayer, param, vec2, vec3, add, sub, mul, mix, smoothstep, sqrt, dot, clamp,
  variedColor, triplanarWeights, seed, featureDistance, world, normal,
} from "@superspec/material/v1";

const amount = param.scalar({ default: 0.9, range: [0, 1], label: "Joint rust" });
const near = param.scalar({ default: 0.03, range: [0, 0.5], unit: "m", label: "Full rust within" });
const far = param.scalar({ default: 0.12, range: [0.01, 1], unit: "m", label: "No rust beyond" });
const rustRepeat = param.scalar({ default: 2, range: [0.1, 20], label: "Oxide texture repeat per meter" });
const rustRoughness = param.scalar({ default: 0.92, range: [0, 1], label: "Oxide roughness" });
const oxideDark = param.vec3({ default: vec3(0.02122, 0.009134, 0.005605), range: [0, 1], label: "Oxide dark" });
const oxideLight = param.vec3({ default: vec3(0.215861, 0.08438, 0.0319), range: [0, 1], label: "Oxide light" });

// The steel's oxide color.
const rustP = mul(world, rustRepeat);
const axis = triplanarWeights(normal, 4);
const oxideSample = add(add(
  mul(variedColor("superspec-material:rust.png", vec2(rustP.z, rustP.y), seed("oxide-x"), 6.283185, 0), axis.x),
  mul(variedColor("superspec-material:rust.png", vec2(rustP.x, rustP.z), seed("oxide-y"), 6.283185, 0), axis.y)),
  mul(variedColor("superspec-material:rust.png", vec2(rustP.x, rustP.y), seed("oxide-z"), 6.283185, 0), axis.z));
const tone = clamp(mul(sqrt(dot(oxideSample, vec3(0.2126, 0.7152, 0.0722))), 1.7), 0, 1);

export default defineLayer({
  name: "Kowloon joint rust",
  coverage: mul(amount, sub(1, smoothstep(near, far, featureDistance("joint", 1)))),
  baseColor: mix(oxideDark, oxideLight, tone),
  roughness: rustRoughness,
});
