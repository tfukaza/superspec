/** Horizon contact occlusion for resolved depth. Apply occlusionTone after tone mapping. */
export const HORIZON_OCCLUSION_GLSL = /* glsl */`
uniform sampler2D aoDepth;
uniform mat4 aoInvProjection;
uniform vec2 aoTexel, aoFocal;
uniform float aoRadius, aoThickness, aoIntensity, aoTint;
uniform vec3 aoHue;
vec3 aoAt(vec2 uv) {
  vec4 p = aoInvProjection * vec4(uv * 2. - 1., texture2D(aoDepth, uv).r * 2. - 1., 1.);
  return p.xyz / p.w;
}
// One sample's occlusion: how far above the surface plane it rises, weighted by a quadratic falloff over the radius plus
// thickness. Near geometry dominates, so a wall's foot darkens toward the floor while the open wall above stays lit.
float aoContact(vec3 p, vec3 n, vec2 uv) {
  if (any(lessThan(uv, vec2(0.))) || any(greaterThan(uv, vec2(1.)))) return 0.;
  vec3 delta = aoAt(uv) - p;
  float distanceToSample = max(length(delta), .0001);
  float above = dot(n, delta);
  float falloff = clamp(1. - distanceToSample / (aoRadius + aoThickness), 0., 1.);
  return max(above / distanceToSample - .1, 0.) * smoothstep(.002, .012, above) * falloff * falloff;
}
// The darkening at uv, 0 to 0.8.
float horizonOcclusion(vec2 uv) {
  if (aoIntensity <= 0. || texture2D(aoDepth, uv).r >= 1.) return 0.;
  vec3 p = aoAt(uv);
  vec3 left = p - aoAt(uv - vec2(aoTexel.x, 0.)), right = aoAt(uv + vec2(aoTexel.x, 0.)) - p;
  vec3 down = p - aoAt(uv - vec2(0., aoTexel.y)), up = aoAt(uv + vec2(0., aoTexel.y)) - p;
  vec3 n = cross(abs(left.z) < abs(right.z) ? left : right, abs(down.z) < abs(up.z) ? down : up);
  n *= inversesqrt(max(dot(n, n), 1e-16));
  // Interleaved gradient noise turns the directions and staggers the steps per pixel, so thin occluders spread into
  // a soft falloff instead of stamping shifted copies of their silhouette at a fixed offset.
  float jitter = fract(52.9829189 * fract(dot(floor(uv / aoTexel), vec2(.06711056, .00583715))));
  vec2 span = clamp(aoFocal * aoRadius / max(-2. * p.z, 0.01), 4. * aoTexel, vec2(.18));
  float amount = 0.;
  for (int i = 0; i < 8; i++) {
    float a = (float(i) + jitter) * .78539816;
    vec2 d = vec2(cos(a), sin(a));
    float horizon = 0.;
    for (int j = 0; j < 4; j++) {
      float t = (float(j) + fract(jitter + float(i) * .618)) * .25;
      t *= t; // more steps near the pixel, where contact shading lives
      // Nearer steps weigh more, so crevices stay darker than open floor.
      horizon = max(horizon, aoContact(p, n, uv + d * max(span * t, aoTexel)) * (1. - .55 * t));
    }
    amount += horizon;
  }
  return clamp(aoIntensity * amount * .125, 0., .8);
}
// The tone-mapped colour's multiplier for a darkening: darker, and toward the shadow's hue as it deepens.
vec3 occlusionTone(float darkening) {
  return (1. - darkening) * mix(vec3(1.), aoHue, aoTint * min(1., darkening * 2.5));
}
`;
