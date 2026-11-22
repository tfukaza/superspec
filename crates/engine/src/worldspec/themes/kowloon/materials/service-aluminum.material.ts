import {
  defineMaterial,
  param,
  vec2,
  vec3,
  add,
  sub,
  mul,
  mix,
  div,
  max,
  dot,
  smoothstep,
  valueNoise,
  seed,
  normal,
  world,
  footprint,
  creaseDistance,
  creaseNormal,
  grazingFalloff,
  scaleSpecular,
} from "@superspec/material/v1";

const across = vec3(mul(normal.y, -1), normal.x, 0);
const sheet = vec2(add(dot(across, world), dot(normal, world)), world.z);
// Overlapping broken patches at three metric scales follow the concrete study's
// warped mottling. The low-chroma cyan and warm gray recall the market hardware.
const wide = valueNoise(mul(sheet, 7.5), seed("broad-patch"));
const flecks = valueNoise(mul(sheet, 87), seed("worn-grain"));
const skew = vec2(wide, valueNoise(add(mul(sheet, 8), 11), seed("patch-shift")));
const broken = valueNoise(add(mul(sheet, 23), mul(skew, 1.8)), seed("broken-mottle"));
const field = add(mul(sub(wide, 0.5), 0.53), add(mul(sub(broken, 0.5), 0.35), mul(sub(flecks, 0.5), 0.12)));
const worn = smoothstep(0.035, 0.115, add(field, mul(sub(flecks, 0.5), 0.12)));
const pigment = mix(vec3(0.56, 0.594, 0.62), vec3(0.83, 0.795, 0.748), worn);
const grain = sub(valueNoise(mul(sheet, vec2(74, 17)), seed("soft-polish")), 0.5);

// The formed strip's U coordinate runs from the leading foot across the crown to
// the return. Keep color, roughness, normal and the live edge lobe in registration.
const formedWidth = param.scalar({ default: 0, range: [0, 0.08], label: "Formed strip width" });
const formed = smoothstep(0, 0.001, formedWidth);
const chart = div(sheet.x, max(formedWidth, 0.001));
const strap = smoothstep(0.03, 0.05, formedWidth);
const narrowLip = mul(smoothstep(0.72, 0.81, chart), sub(1, smoothstep(0.89, 0.99, chart)));
const crown = mul(
  formed,
  mix(mul(smoothstep(0.15, 0.36, chart), sub(1, smoothstep(0.65, 0.89, chart))), narrowLip, strap),
);
const toe = mul(formed, sub(1, smoothstep(0.02, 0.22, chart)));
const shoulder = mul(
  formed,
  mul(
    smoothstep(mix(0.61, 0.87, strap), mix(0.69, 0.9, strap), chart),
    sub(1, smoothstep(mix(0.73, 0.92, strap), mix(0.86, 0.99, strap), chart)),
  ),
);
const foldDistance = creaseDistance("exposed", 30, 0.016);
const rim = mul(sub(1, formed), sub(1, smoothstep(0, 0.006, foldDistance)));
const polished = add(rim, crown);
const middle = creaseNormal("convex", 30, add(0.006, footprint));
const bend = mul(sub(mul(middle, mul(2, dot(middle, normal))), normal), rim);
const local = add(mul(grain, 0.028), sub(mul(shoulder, 0.22), mul(toe, 0.12)));
const value = add(add(1, mul(field, 0.35)), sub(mul(polished, 0.135), mul(toe, 0.17)));
const rough = add(0.48, add(sub(mul(field, -0.18), mul(worn, 0.075)), mul(toe, 0.08)));

export default defineMaterial({
  name: "Rough-polished aluminum",
  base: {
    baseColor: mul(pigment, value),
    roughness: mix(rough, 0.27, polished),
    metalness: 0.92,
    normalOffset: add(bend, mul(across, local)),
  },
  live: [
    grazingFalloff({ from: 0.3, to: 0.06, mask: add(0.76, mul(polished, 0.24)) }),
    scaleSpecular({ factor: add(1, mul(0.45, polished)) }),
  ],
});
