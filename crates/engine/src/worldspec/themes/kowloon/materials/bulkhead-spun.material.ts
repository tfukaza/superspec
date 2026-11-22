import {
  defineMaterial,
  vec2,
  vec3,
  world,
  add,
  sub,
  mul,
  div,
  mix,
  max,
  dot,
  sqrt,
  sin,
  cos,
  smoothstep,
  valueNoise,
  seed,
} from "@superspec/material/v1";

// The spun-profile tool unwraps each oval face into a centred disc. Radius
// increases across the machined section. Direction rotates around the lamp.
const p = vec2(sub(world.x, 0.5), sub(world.z, 0.5));
const radius = sqrt(dot(p, p));
const radial = div(p, max(radius, 0.0005));
const quadrant = sub(mul(radial.x, radial.x), mul(radial.y, radial.y));
const diagonal = mul(2, mul(radial.x, radial.y));
const wash = sub(valueNoise(mul(p, 13), seed("silver-wash")), 0.5);
const satin = sub(valueNoise(mul(p, 310), seed("satin-pore")), 0.5);

// Broad, low-contrast warm and cool areas are intrinsic surface variation.
// The specular lobe follows the view and light through the normal and MR maps.
const mood = smoothstep(
  -0.75,
  0.72,
  add(mul(diagonal, 0.57), add(mul(radial.x, 0.37), mul(wash, 0.36))),
);
const silver = mix(vec3(0.5, 0.56, 0.6), vec3(0.66, 0.62, 0.52), mood);
const band = sin(add(mul(radius, 142), add(mul(quadrant, 1.3), mul(wash, 0.65))));
const tooling = sin(add(mul(radius, 870), mul(wash, 0.95)));
const finish = smoothstep(-0.65, 0.8, add(mul(band, 0.54), mul(diagonal, 0.46)));
const slope = add(mul(band, 0.092), add(mul(tooling, 0.027), mul(satin, 0.022)));
const crossSlope = mul(cos(add(mul(radius, 48), mul(diagonal, 1.8))), 0.025);

export default defineMaterial({
  name: "Bulkhead spun metal",
  base: {
    baseColor: mul(silver, add(0.98, add(mul(band, 0.035), mul(wash, 0.075)))),
    roughness: add(mix(0.46, 0.28, finish), mul(satin, 0.045)),
    metalness: mix(0.66, 0.81, finish),
    normalOffset: vec3(
      sub(mul(radial.x, slope), mul(radial.y, crossSlope)),
      0,
      add(mul(radial.y, slope), mul(radial.x, crossSlope)),
    ),
  },
});
