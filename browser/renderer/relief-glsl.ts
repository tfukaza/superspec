/**
 * GLSL shared by the normal passes of the GPU preview bakes (the swatch grid in bake.ts and the receiver atlas in
 * atlas.ts): the engine's `orient_relief` and `bend_shaded` on a tangent-space relief normal, and the byte dither.
 */

/**
 * `msAdjustRelief` turns a tangent-space relief normal by the program's `receiver_normal` output (`orient`) and bends
 * it by its `normal_offset` output (`bend`), in the tangent frame `frameX`, `frameY`, `frameZ` (world directions; z is
 * the receiver normal). `msFailed` is set when the receiver normal faces away from the receiver, which no tangent-space
 * normal can encode.
 */
export const RELIEF_ADJUST = `
bool msFailed = false;
vec3 msAdjustRelief(vec3 relief, vec3 receiverNormal, vec3 offset, vec3 frameX, vec3 frameY, vec3 frameZ, bool orient, bool bend) {
  vec3 n = frameZ;
  if (orient) {
    // orient_relief: the smallest rotation from the receiver normal to the receiver_normal output.
    vec3 target = normalize(receiverNormal);
    float cosine = dot(n, target);
    if (!(cosine > 1e-6)) msFailed = true;
    else if (cosine < 1.0 - 1e-6) {
      vec3 world = relief.x * frameX + relief.y * frameY + relief.z * n;
      vec3 axis = cross(n, target);
      vec3 kw = cross(axis, world);
      vec3 turned = world + kw + cross(axis, kw) / (1.0 + cosine);
      relief = normalize(vec3(dot(turned, frameX), dot(turned, frameY), dot(turned, n)));
    }
  }
  if (bend) {
    // bend_shaded: add the world-space offset, renormalize, and hold the result just above the horizon.
    if (any(notEqual(offset, vec3(0.0)))) {
      vec3 world = relief.x * frameX + relief.y * frameY + relief.z * n + offset;
      if (dot(world, world) > 1e-24) {
        vec3 bent = normalize(world);
        vec3 local = vec3(dot(bent, frameX), dot(bent, frameY), dot(bent, n));
        if (local.z > 1e-6) relief = local;
        else {
          float sideways = length(local.xy);
          if (sideways >= 1e-12) relief = vec3(local.xy * (sqrt(1.0 - 0.02 * 0.02) / sideways), 0.02);
        }
      }
    }
  }
  return relief;
}
`;

/**
 * `msEncodeNormal` writes a normal as bytes the way the CPU baker does: full dither on plateaus (under a step per
 * texel, judged against the normal of the texel before it in the row), none where the relief already varies.
 * `texel` counts rows from the atlas's upper edge.
 */
export const NORMAL_ENCODE = `
float dither(ivec2 texel, int channel, float amount) {
  uint h = uint(texel.x) * 0x9E3779B1u + uint(texel.y) * 0x85EBCA77u + uint(channel) * 0xC2B2AE3Du;
  h ^= h >> 15; h *= 0x2C1B3C6Du; h ^= h >> 12; h *= 0x297A2D39u; h ^= h >> 15;
  return (float(h) / 4294967296.0 - 0.5) * amount;
}
vec3 msEncodeNormal(vec3 n, vec3 before, ivec2 texel) {
  vec3 encoded = vec3(0.0);
  for (int channel = 0; channel < 3; channel++) {
    float amount = clamp(1.0 - abs(n[channel] - before[channel]) * 127.5, 0.0, 1.0);
    encoded[channel] = clamp(floor((n[channel] * 0.5 + 0.5) * 255.0 + dither(texel, channel, amount) + 0.5), 0.0, 255.0) / 255.0;
  }
  return encoded;
}
`;
