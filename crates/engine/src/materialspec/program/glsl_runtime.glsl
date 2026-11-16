// MaterialSpec preview runtime (GLSL ES 3.00): the evaluator's lattice fields, image sampling and
// receiver feature queries. Integer hashing matches the baker bit for bit; filtering uses screen
// derivatives where the baker propagates a footprint, so a preview is close, not identical.
uniform sampler2D ms_features;
uniform int ms_feature_count;
uniform vec3 ms_gravity;
uniform vec3 ms_bounds_min;
uniform vec3 ms_bounds_max;
// Always zero: no host sets it. Loops over noise slots add it to their bounds so that a compiler cannot count their
// iterations and unroll them into one copy of the noise per slot, which is what makes large programs slow to compile.
uniform int ms_loop;
vec3 ms_world;
vec3 ms_object;
vec3 ms_normal;
vec2 ms_uv;
vec2 ms_surface;
float ms_footprint;
float ms_uv_footprint;
// The texel of the bake grid being shaded (rows from the upper edge), which indexes fields the CPU baked.
// Per-fragment shading has no grid and never reads it.
ivec2 ms_texel;

const vec3 MS_GRADIENTS[12] = vec3[12](
  vec3(1.0, 1.0, 0.0), vec3(-1.0, 1.0, 0.0), vec3(1.0, -1.0, 0.0), vec3(-1.0, -1.0, 0.0),
  vec3(1.0, 0.0, 1.0), vec3(-1.0, 0.0, 1.0), vec3(1.0, 0.0, -1.0), vec3(-1.0, 0.0, -1.0),
  vec3(0.0, 1.0, 1.0), vec3(0.0, -1.0, 1.0), vec3(0.0, 1.0, -1.0), vec3(0.0, -1.0, -1.0));

uint ms_bits(int x, int y, uint seed) {
  uint h = (uint(x) * 374761393u) ^ (uint(y) * 668265263u) ^ seed;
  h = (h ^ (h >> 13)) * 1274126177u;
  return h ^ (h >> 16);
}
float ms_lattice(int x, int y, uint seed) { return float(ms_bits(x, y, seed)) * (1.0 / 4294967296.0); }
uint ms_seed_of(float value) { return uint(int(value)); }
float ms_spread(float v) { return fwidth(v); }
float ms_spread(vec2 v) { vec2 w = fwidth(v); return max(w.x, w.y); }
float ms_spread(vec3 v) { vec3 w = fwidth(v); return max(w.x, max(w.y, w.z)); }

float ms_value2(vec2 p, uint seed) {
  vec2 cell = floor(p);
  vec2 f = p - cell;
  f = f * f * (3.0 - 2.0 * f);
  ivec2 c = ivec2(cell);
  float low = mix(ms_lattice(c.x, c.y, seed), ms_lattice(c.x + 1, c.y, seed), f.x);
  float high = mix(ms_lattice(c.x, c.y + 1, seed), ms_lattice(c.x + 1, c.y + 1, seed), f.x);
  return mix(low, high, f.y);
}
float ms_value3(vec3 p, uint seed) {
  float layer = floor(p.z);
  float w = p.z - layer;
  w = w * w * (3.0 - 2.0 * w);
  int z = int(layer);
  return mix(ms_value2(p.xy, seed + uint(z * 7919)), ms_value2(p.xy, seed + uint((z + 1) * 7919)), w);
}
float ms_fade_value(float raw, float spread) { return mix(raw, 0.5, clamp((spread - 0.25) / 0.75, 0.0, 1.0)); }
float ms_value_noise(vec2 p, uint seed) { return ms_fade_value(ms_value2(p, seed), ms_spread(p)); }
float ms_value_noise(vec3 p, uint seed) { return ms_fade_value(ms_value3(p, seed), ms_spread(p)); }

float ms_hash(vec3 p, uint seed) {
  ivec3 c = ivec3(floor(p));
  return ms_lattice(c.x + c.z * 151, c.y + c.z * 269, seed);
}
float ms_hash(vec2 p, uint seed) { return ms_hash(vec3(p, 0.0), seed); }
float ms_hash(float p, uint seed) { return ms_hash(vec3(p, 0.0, 0.0), seed); }

float ms_gradient(vec3 p, int dimensions, uint seed) {
  vec3 base = floor(p);
  vec3 t = p - base;
  vec3 fade = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
  ivec3 b = ivec3(base);
  float sum = 0.0;
  for (int corner = 0; corner < (1 << dimensions); corner++) {
    ivec3 high = ivec3(corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
    ivec3 c = b + high;
    vec3 weights = mix(1.0 - fade, fade, vec3(high));
    float weight = weights.x * weights.y * (dimensions == 3 ? weights.z : 1.0);
    uint h = ms_bits(c.x + c.z * 151, c.y + c.z * 269, seed);
    uint index = (((h >> 16) * 12u) + (((h & 65535u) * 12u) >> 16)) >> 16;
    sum += weight * dot(t - vec3(high), MS_GRADIENTS[index]);
  }
  return clamp(0.5 + sum * 0.5, 0.0, 1.0);
}
float ms_cellular(vec3 p, int dimensions, uint seed) {
  ivec3 b = ivec3(floor(p));
  float nearest = 1e30;
  int reach = dimensions == 3 ? 1 : 0;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) for (int z = -reach; z <= reach; z++) {
    ivec3 c = b + ivec3(x, y, z);
    float gap = 0.0;
    for (int axis = 0; axis < dimensions; axis++) {
      float point = float(c[axis]) + ms_lattice(c.x + c.z * 151, c.y + c.z * 269, seed + uint(axis * 379));
      gap += (point - p[axis]) * (point - p[axis]);
    }
    nearest = min(nearest, gap);
  }
  return clamp(sqrt(nearest) / sqrt(float(dimensions)), 0.0, 1.0);
}
float ms_octaves(vec3 v, int dimensions, uint seed, int count, bool cells, float spread) {
  float sum = 0.0, total = 0.0, frequency = 1.0, amplitude = 1.0;
  for (int octave = 0; octave < count; octave++) {
    uint key = seed + uint(octave * 131);
    float raw = cells ? ms_cellular(v * frequency, dimensions, key) : ms_gradient(v * frequency, dimensions, key);
    sum += amplitude * mix(raw, 0.5, clamp((spread * frequency - 0.25) / 0.75, 0.0, 1.0));
    total += amplitude;
    amplitude *= 0.5;
    frequency *= 2.0;
  }
  return sum / total;
}
float ms_noise(vec2 p, uint seed, float count, bool cells) { return ms_octaves(vec3(p, 0.0), 2, seed, int(count), cells, ms_spread(p)); }
float ms_noise(vec3 p, uint seed, float count, bool cells) { return ms_octaves(p, 3, seed, int(count), cells, ms_spread(p)); }

float ms_soft_threshold(float v, float threshold, float softness) {
  return smoothstep(0.0, 1.0, (v - threshold) / (softness + fwidth(v)) + 0.5);
}
vec2 ms_soft_threshold(vec2 v, float threshold, float softness) {
  return smoothstep(0.0, 1.0, (v - threshold) / (softness + fwidth(v)) + 0.5);
}
vec3 ms_soft_threshold(vec3 v, float threshold, float softness) {
  return smoothstep(0.0, 1.0, (v - threshold) / (softness + fwidth(v)) + 0.5);
}
float ms_distance_band(float d, float inner, float outer) {
  float w = fwidth(d) * 0.5;
  return 1.0 - smoothstep(inner - w, outer + w, d);
}
float ms_rem(float x, float y) { return x - y * trunc(x / y); }
vec2 ms_rem(vec2 x, vec2 y) { return x - y * trunc(x / y); }
vec3 ms_rem(vec3 x, vec3 y) { return x - y * trunc(x / y); }
vec2 ms_rotate(vec2 v, float angle) { float s = sin(angle), c = cos(angle); return vec2(v.x * c - v.y * s, v.x * s + v.y * c); }
vec3 ms_triplanar(vec3 normal, float sharpness) { vec3 w = pow(abs(normal), vec3(sharpness)); return w / max(w.x + w.y + w.z, 1e-12); }
vec3 ms_adjust_normal(vec3 from, vec3 to, float weight) {
  vec3 a = normalize(from), b = normalize(to);
  float cosine = clamp(dot(a, b), -1.0, 1.0);
  if (cosine < -1.0 + 1e-6) return a;
  float angle = acos(cosine);
  if (angle < 1e-5) return a;
  float t = clamp(weight, 0.0, 1.0);
  return normalize((a * sin((1.0 - t) * angle) + b * sin(t * angle)) / sin(angle));
}

// Blends rotated, offset copies of an image over a triangular lattice (see images.rs `varied`).
vec3 ms_varied(sampler2D image, vec2 uv, uint seed, float rotation, float correction) {
  const float root3 = 1.7320508075688772;
  vec2 skewed = vec2(uv.x - uv.y / root3, 2.0 * uv.y / root3);
  vec2 cell = floor(skewed);
  vec2 f = skewed - cell;
  bool lower = f.x + f.y < 1.0;
  vec2 corners[3];
  corners[0] = lower ? cell : cell + vec2(1.0, 1.0);
  corners[1] = cell + vec2(1.0, 0.0);
  corners[2] = cell + vec2(0.0, 1.0);
  vec3 weights = lower ? vec3(1.0 - f.x - f.y, f.x, f.y) : vec3(f.x + f.y - 1.0, 1.0 - f.y, 1.0 - f.x);
  weights = max(weights, vec3(0.0));
  weights = weights * weights * weights;
  weights /= max(weights.x + weights.y + weights.z, 1e-12);
  vec2 dx = dFdx(uv), dy = dFdy(uv);
  vec3 blend = vec3(0.0);
  for (int k = 0; k < 3; k++) {
    ivec2 c = ivec2(corners[k]);
    float angle = (ms_lattice(c.x + 302, c.y + 538, seed) - 0.5) * rotation;
    vec2 offset = vec2(ms_lattice(c.x, c.y, seed), ms_lattice(c.x + 151, c.y + 269, seed));
    float s = sin(angle), co = cos(angle);
    mat2 turn = mat2(co, s, -s, co);
    blend += textureGrad(image, turn * uv + offset, turn * dx, turn * dy).rgb * weights[k];
  }
  vec3 mean = textureLod(image, vec2(0.5), 32.0).rgb;
  float gain = 1.0 + (1.0 / max(length(weights), 1e-6) - 1.0) * clamp(correction, 0.0, 1.0);
  return clamp(mean + (blend - mean) * gain, 0.0, 1.0);
}

// Receiver features: one row of three texels each (start + kind, end + angle, middle normal + has normal).
// Kinds: 0 convex, 1 concave, 2 boundary, 3 seam, 4 contact. Selectors: 0 convex, 1 concave,
// 2 boundary, 3 exposed, 4 any, 5 seam, 6 contact, 7 nothing.
bool ms_selected(float kind, float angle, int selector, float minimum) {
  bool convex = kind == 0.0 && angle >= minimum, concave = kind == 1.0 && angle >= minimum, boundary = kind == 2.0;
  if (selector == 0) return convex;
  if (selector == 1) return concave;
  if (selector == 2) return boundary;
  if (selector == 3) return convex || boundary;
  if (selector == 4) return convex || concave || boundary;
  if (selector == 5) return kind == 3.0;
  if (selector == 6) return kind == 4.0;
  return false;
}
float ms_segment(vec3 point, vec3 start, vec3 end) {
  vec3 along = end - start;
  float squared = dot(along, along);
  float t = squared > 0.0 ? clamp(dot(point - start, along) / squared, 0.0, 1.0) : 0.0;
  return length(point - start - along * t);
}
float ms_crease_distance(int selector, float minimum, float limit) {
  float nearest = limit;
  for (int i = 0; i < ms_feature_count; i++) {
    vec4 a = texelFetch(ms_features, ivec2(0, i), 0), b = texelFetch(ms_features, ivec2(1, i), 0);
    if (ms_selected(a.w, b.w, selector, minimum)) nearest = min(nearest, ms_segment(ms_world, a.xyz, b.xyz));
  }
  return nearest;
}
vec3 ms_crease_normal(int selector, float minimum, float limit) {
  float nearest = limit;
  vec3 chosen = ms_normal;
  for (int i = 0; i < ms_feature_count; i++) {
    vec4 a = texelFetch(ms_features, ivec2(0, i), 0), b = texelFetch(ms_features, ivec2(1, i), 0), n = texelFetch(ms_features, ivec2(2, i), 0);
    if (n.w < 0.5 || !ms_selected(a.w, b.w, selector, minimum) || dot(n.xyz, ms_normal) <= 0.0) continue;
    float gap = ms_segment(ms_world, a.xyz, b.xyz);
    if (gap < nearest) { nearest = gap; chosen = n.xyz; }
  }
  return chosen;
}
vec3 ms_crease_tangent(int selector, float minimum, float limit) {
  float nearest = limit;
  vec3 chosen = vec3(0.0);
  for (int i = 0; i < ms_feature_count; i++) {
    vec4 a = texelFetch(ms_features, ivec2(0, i), 0), b = texelFetch(ms_features, ivec2(1, i), 0);
    if (!ms_selected(a.w, b.w, selector, minimum)) continue;
    float gap = ms_segment(ms_world, a.xyz, b.xyz);
    vec3 along = b.xyz - a.xyz;
    if (gap < nearest && dot(along, along) > 1e-24) { nearest = gap; chosen = normalize(along); }
  }
  return chosen;
}
vec3 ms_crease_adjacent(int selector, float minimum, float limit) {
  vec3 middle = ms_crease_normal(selector, minimum, limit);
  vec3 across = middle * (2.0 * dot(middle, ms_normal)) - ms_normal;
  return dot(across, across) > 1e-24 ? normalize(across) : ms_normal;
}
