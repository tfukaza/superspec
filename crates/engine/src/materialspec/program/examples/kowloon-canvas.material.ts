// Awning and tarpaulin canvas that has hung over a street for years. Each receiver (one awning) takes one of three
// cloth colours, and a share of them are striped with a second colour along their fall; which one is chosen from where
// the receiver is, so a row of shops differs and keeps its awnings when rebaked. Over the dye lie the catalog's canvas
// weave (by the mesh's UVs), sun-bleaching in clouds, soot and rain soil running down the fall and gathering at the
// hem, and a few dark water stains.
import {
  defineMaterial, layer, param, vec2, vec3, add, sub, mul, div, mix, max, min, clamp, floor, rem, abs, step, smoothstep,
  dot, cross, normalize, hash, valueNoise, sampleColor, seed, uv, world, normal, boundsMin, boundsMax,
} from "@superspec/material/v1";

const clothA = param.vec3({ default: vec3(0.184, 0.242, 0.188), range: [0, 1], label: "Cloth A" });
const clothB = param.vec3({ default: vec3(0.238, 0.246, 0.191), range: [0, 1], label: "Cloth B" });
const clothC = param.vec3({ default: vec3(0.418, 0.198, 0.147), range: [0, 1], label: "Cloth C (striped)" });
const stripe = param.vec3({ default: vec3(0.644, 0.565, 0.429), range: [0, 1], label: "Stripe colour" });
const stripeWidth = param.scalar({ default: 0.22, range: [0.03, 2], unit: "m", label: "Stripe width" });
const striped = param.scalar({ default: 0.35, range: [0, 1], label: "Share striped" });
const bleaching = param.scalar({ default: 0.5, range: [0, 1], label: "Sun bleaching" });
const soil = param.scalar({ default: 0.6, range: [0, 1], label: "Soil" });

// Which cloth this receiver is.
const which = mul(boundsMin, 3.19);
const stripy = step(hash(which, seed("canvas-striped")), striped);
const plain = mix(clothA, clothB, step(0.5, hash(which, seed("canvas-cloth"))));

// Along the eave (horizontal, in the cloth) and how high in the receiver the texel is.
const eave = normalize(add(cross(vec3(0, 0, 1), normal), vec3(1e-5, 0, 0)));
const along = dot(world, eave);
const upIt = clamp(div(sub(world.z, boundsMin.z), max(sub(boundsMax.z, boundsMin.z), 0.01)), 0, 1);
const band = rem(abs(floor(div(along, stripeWidth))), 2);
const dyed = mix(plain, mix(clothC, stripe, band), stripy);

// The weave, about its own mean so it only textures the dye.
const weave = sampleColor("superspec-material:tarp.png", vec2(uv.x, sub(1, uv.y)));
const woven = mul(dyed, clamp(mul(div(dot(weave, vec3(0.3333)), 0.42), 1), 0.55, 1.35));

// Bleached in clouds toward old cloth; soiled down the fall and at the hem; stained in a few places.
const clouds = valueNoise(mul(vec2(along, world.z), 1.7), seed("canvas-bleach"));
const bleached = mix(woven, vec3(0.4, 0.35, 0.26), mul(bleaching, add(0.15, mul(0.45, smoothstep(0.35, 0.8, clouds)))));
const runs = smoothstep(0.4, 0.82, valueNoise(vec2(mul(along, 19), mul(world.z, 1.2)), seed("canvas-runs")));
const hem = sub(1, smoothstep(0, 0.3, upIt));
const stains = smoothstep(0.66, 0.82, valueNoise(add(mul(vec2(along, world.z), 3.1), vec2(7.3, 2.2)), seed("canvas-stains")));
// Soot gathers toward the wall, at the top of the fall.
const sooty = mul(smoothstep(0.55, 1, upIt), add(0.3, mul(0.3, runs)));
const soiled = mul(soil, min(add(add(add(mul(runs, 0.4), mul(hem, add(0.25, mul(0.35, runs)))), mul(stains, 0.45)), sooty), 0.85));

export default defineMaterial({
  name: "Kowloon canvas",
  base: { baseColor: bleached, roughness: 0.9, metalness: 0 },
  layers: [
    layer({ id: "soil", mask: soiled, baseColor: mul(bleached, vec3(0.3, 0.29, 0.26)), roughness: 0.95 }),
  ],
});
