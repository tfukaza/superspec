varying vec2 vConcreteFaceHalfSize;
varying float vConcreteTopDistance;
varying vec2 vConcreteGroundContact;
uniform float concreteGroundWeathering;
uniform float concreteEdgeWidth;
varying vec3 vConcreteLocalPosition;
varying vec4 vConcreteEdges[4];
uniform sampler2D concreteGrimeMask;
uniform float concreteGrime;
uniform float concreteGround;
uniform float concreteVariation;
uniform float concreteCutouts;
uniform float concreteHoles;
uniform vec3 concreteMean;
uniform sampler2D concreteMapB;
uniform vec3 concreteMeanB;
uniform float darknessAmount;
uniform float darknessAmountB;
uniform float blendScale;
uniform float blendContrast;
uniform float edgeJaggedness;
uniform float hueAmount;
uniform float hueAngle;
uniform vec2 concreteChipPattern;
uniform vec3 concreteChipFinish;

vec3 concreteHash(vec2 cell) {
  return fract(sin(vec3(dot(cell, vec2(127.1, 311.7)),
    dot(cell, vec2(269.5, 183.3)), dot(cell, vec2(419.2, 371.9)))) * 43758.5453);
}

float concreteNoise(vec2 p) {
  vec2 cell = floor(p), f = fract(p);
  f = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
  return mix(mix(concreteHash(cell).x, concreteHash(cell + vec2(1, 0)).x, f.x),
    mix(concreteHash(cell + vec2(0, 1)).x, concreteHash(cell + vec2(1, 1)).x, f.x), f.y);
}

float concreteFractal(vec2 p) {
  float value = 0.0, amplitude = 0.5, total = 0.0;
  mat2 turn = mat2(0.8, -0.6, 0.6, 0.8);
  for (int octave = 0; octave < 3; octave++) {
    value += concreteNoise(p) * amplitude;
    total += amplitude;
    p = turn * p * 2.03 + vec2(13.7, 9.2);
    amplitude *= 0.28;
  }
  return value / total;
}

// The same chipped shoulder profile is used at construction joints and exposed edges.
float concreteChipShoulder(float distance, float width) {
  return 1.0 - smoothstep(width * 0.4, width, distance);
}

// Blends toward neighboring faces and returns recessed height and cavity darkness.
float concreteEdgeRelief(inout vec3 rounded, out float darkness) {
  darkness = 0.0;
  if (concreteEdgeWidth <= 0.0) return 0.0;
  float nearest = 1e6;
  for (int i = 0; i < 4; i++) {
    vec3 adjacent = vConcreteEdges[i].xyz;
    if (dot(adjacent, adjacent) < 0.5) continue;
    float distance = abs(vConcreteEdges[i].w);
    float weight = 1.0 - smoothstep(0.0, concreteEdgeWidth, distance);
    rounded += normalize(adjacent) * weight;
    nearest = min(nearest, distance);
  }
  if (nearest > concreteEdgeWidth * 2.5) return 0.0;
  vec3 p = vConcreteLocalPosition;
  float pockets = concreteFractal(p.xy * 13.0 + p.z * vec2(7.9, 15.3) + 37.2);
  float grit = concreteNoise(p.xy * 170.0 + p.z * vec2(99.0, 211.0));
  float breaks = smoothstep(0.40, 0.66, pockets + (grit - 0.5) * 0.08);
  float chips = smoothstep(0.54, 0.75, pockets + (grit - 0.5) * 0.08);
  float chipWidth = concreteEdgeWidth * mix(0.60, 2.5, chips);
  float shoulder = concreteChipShoulder(nearest, chipWidth) * chips;
  float groove = concreteChipShoulder(nearest, concreteEdgeWidth * 0.34) * breaks;
  darkness = 0.16 * groove + 0.20 * shoulder;
  return -0.006 * shoulder * (0.5 + 0.5 * grit);
}

// Returns stain coverage, moss coverage, and moss height in meters.
vec3 concreteBaseWeathering(vec2 uv) {
  if (concreteGroundWeathering < 0.5) return vec3(0.0);
  float height = vConcreteGroundContact.x;
  float damp = smoothstep(0.50, 1.0, vConcreteGroundContact.y);
  vec2 along = vec2(uv.x * 2.0, 0.0);
  float clumps = concreteFractal(along * 3.8 + vec2(9.2, 22.0));
  float grain = concreteNoise(uv * 280.0 + 7.1);
  float limit = 0.06 + 0.30 * clumps * clumps + 0.12 * damp + 0.025 * grain;
  float stain = (1.0 - smoothstep(limit * 0.18, limit, height))
    * smoothstep(0.20, 0.64, clumps + damp * 0.15);
  float islands = concreteFractal(vec2(along.x, height * 2.2) * 8.0 + vec2(31.1, 14.6));
  float moss = smoothstep(0.64, 0.79, islands + damp * 0.12 + grain * 0.09)
    * (1.0 - smoothstep(0.015, 0.11 + damp * 0.09, height))
    * smoothstep(0.08, 0.40, stain);
  return vec3(stain, moss, moss * (0.0005 + grain * 0.0016));
}

float concreteFracture(vec2 p) {
  vec2 warp = vec2(concreteFractal(p * 2.3 + vec2(7.1, 31.8)),
    concreteFractal(p * 2.3 + vec2(53.2, 9.4))) - 0.5;
  vec2 q = p + warp * 0.18 * edgeJaggedness;
  float detail = (concreteNoise(p * 13.0) - 0.5) * 0.20
    + (concreteNoise(p * 31.0 + 17.3) - 0.5) * 0.11
    + (concreteNoise(p * 73.0 + 39.1) - 0.5) * 0.06
    + (concreteNoise(p * 151.0 + 81.7) - 0.5) * 0.025;
  float broad = concreteFractal(q);
  // Fine erosion lives at the contour, leaving the clump interiors coherent.
  float border = 1.0 - smoothstep(0.045, 0.18, abs(broad - 0.5));
  return broad + detail * edgeJaggedness * border;
}

// Returns unfiltered coverage for bump and filtered coverage for shading.
vec2 concreteChipCoverage(vec2 uv) {
  if (concreteChipPattern.y <= 0.0) return vec2(0.0);
  vec2 p = uv * (2.0 / concreteChipPattern.x) + vec2(18.7, 42.3);
  float threshold = mix(0.93, 0.53, concreteChipPattern.y);
  float broad = concreteFracture(p);
  // Break up the chip perimeter at its own threshold without speckling the intact surface.
  float edgeBand = 1.0 - smoothstep(0.025, 0.11, abs(broad - threshold));
  float grit = (concreteNoise(p * 9.0 + 11.3) - 0.5) * 0.15
    + (concreteNoise(p * 24.0 + 37.1) - 0.5) * 0.11
    + (concreteNoise(p * 61.0 + 72.4) - 0.5) * 0.07;
  float field = broad + grit * edgeBand * edgeJaggedness;
  float filterWidth = max(fwidth(field) * 0.5, 0.022);
  return vec2(
    smoothstep(threshold - 0.012, threshold + 0.012, field),
    smoothstep(threshold - filterWidth, threshold + filterWidth, field)
  );
}

float concreteEdge(float field, float width) {
  float antialias = max(fwidth(field), 0.001);
  return smoothstep(0.5 - max(width, antialias), 0.5 + max(width, antialias), field);
}

// Filled repair rectangles stay inside construction panels; narrow box faces stay plain.
vec3 concreteCutout(vec2 meters) {
  vec2 panel = floor(meters / vec2(2.0, 1.6));
  vec3 random = concreteHash(panel + vec2(81.3, 42.7));
  if (concreteCutouts < 0.5 || min(vConcreteFaceHalfSize.x, vConcreteFaceHalfSize.y) < 0.5
      || random.x < 0.28) return vec3(100.0, 0.0, 0.0);
  vec2 center = (panel + 0.5) * vec2(2.0, 1.6) + (random.yz - 0.5) * vec2(0.35, 0.25);
  vec2 halfSize = vec2(0.30, 0.22) + random.zy * vec2(0.25, 0.19);
  vec2 q = abs(meters - center) - halfSize;
  float distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);
  return vec3(distance, random.y * 2.0 - 1.0, random.z * 2.0 - 1.0);
}

// Returns recess height in meters, cavity darkness, and exposed chipped concrete.
vec3 concreteSeam(vec2 meters) {
  float verticalJoint = abs(mod(meters.x + 1.0, 2.0) - 1.0);
  vec2 rim = max(vConcreteFaceHalfSize - abs(meters), vec2(0.0));
  float distanceToSeam = min(verticalJoint, min(rim.x, rim.y));
  distanceToSeam = min(distanceToSeam, abs(concreteCutout(meters).x));
  float pixel = max(length(fwidth(meters)) * 0.65, 0.0006);
  float grain = concreteNoise(meters * 137.0 + vec2(3.1, 8.7));
  float pockets = smoothstep(0.40, 0.78, concreteFractal(meters * 22.0));
  float chippedWidth = 0.009 + pockets * 0.037 + (grain - 0.5) * 0.006;
  float groove = 1.0 - smoothstep(0.002, 0.006 + pixel, distanceToSeam);
  float chip = 1.0 - smoothstep(chippedWidth * 0.4, chippedWidth + pixel, distanceToSeam);
  // Height stays derivative-free; only color coverage uses screen-space widening.
  float heightGroove = 1.0 - smoothstep(0.002, 0.006, distanceToSeam);
  float heightChip = concreteChipShoulder(distanceToSeam, chippedWidth);
  float height = -0.006 * heightGroove - 0.004 * heightChip * (0.5 + 0.5 * grain);
  return vec3(height, groove * 0.24 + chip * 0.055, chip * (1.0 - groove));
}

// Form-tie holes affect normal relief only; their color channels remain zero.
vec3 concreteHole(vec2 meters) {
  vec2 center = floor(meters) + 0.5;
  vec2 clearance = vConcreteFaceHalfSize - abs(center);
  if (concreteHoles < 0.5 || min(clearance.x, clearance.y) < 0.04) return vec3(0.0);
  vec2 local = meters - center;
  float chip = (concreteNoise(meters * 190.0) - 0.5) * 0.003;
  float radius = length(local) + chip;
  float recess = 1.0 - smoothstep(0.017, 0.028, radius);
  return vec3(-0.012 * recess, 0.0, 0.0);
}

// Surface-gradient bump mapping keeps recess depth in meters on every box face.
vec3 concreteSeamNormal(vec3 position, vec3 surfaceNormal, float height) {
  vec3 dx = dFdx(position), dy = dFdy(position);
  vec3 acrossY = cross(dy, surfaceNormal), acrossX = cross(surfaceNormal, dx);
  float determinant = dot(dx, acrossY);
  vec3 gradient = sign(determinant) * (dFdx(height) * acrossY + dFdy(height) * acrossX);
  return normalize(max(abs(determinant), 1e-10) * surfaceNormal - gradient);
}

vec3 concretePigment(vec3 color, vec2 uv) {
  float hueNoise = concreteFractal(uv * 4.68 + vec2(41.2, 3.7)) * 2.0 - 0.7;
  vec3 direction = cos(vec3(hueAngle) + vec3(0.0, -2.0943951, 2.0943951));
  vec3 luminance = vec3(0.2126, 0.7152, 0.0722);
  direction -= vec3(dot(direction, luminance));
  return max(vec3(0.0), color + direction * hueNoise * hueAmount * dot(color, luminance));
}

vec3 concretePatch(sampler2D source, vec2 uv, vec2 cell, vec2 dx, vec2 dy) {
  vec3 random = concreteHash(cell + vec2(19.0, 47.0));
  float angle = (random.z - 0.5) * 0.32;
  float c = cos(angle), s = sin(angle);
  mat2 rotation = mat2(c, -s, s, c);
  return textureGrad(source, rotation * uv + random.xy, rotation * dx, rotation * dy).rgb;
}

vec4 concreteSample(sampler2D source, vec3 mean, vec2 uv) {
  vec2 dx = dFdx(uv), dy = dFdy(uv);
  if (concreteVariation < 0.5) return textureGrad(source, uv, dx, dy);
  vec2 skewed = vec2(uv.x - uv.y / sqrt(3.0), uv.y * 2.0 / sqrt(3.0));
  vec2 cell = floor(skewed), f = fract(skewed);
  vec2 a, b, c;
  vec3 weights;
  if (f.x + f.y < 1.0) {
    a = cell; b = cell + vec2(1.0, 0.0); c = cell + vec2(0.0, 1.0);
    weights = vec3(1.0 - f.x - f.y, f.x, f.y);
  } else {
    a = cell + vec2(1.0, 1.0); b = cell + vec2(0.0, 1.0); c = cell + vec2(1.0, 0.0);
    weights = vec3(f.x + f.y - 1.0, 1.0 - f.x, 1.0 - f.y);
  }
  weights = pow(max(weights, vec3(0.0)), vec3(3.0));
  weights /= weights.x + weights.y + weights.z;
  vec3 blended = concretePatch(source, uv, a, dx, dy) * weights.x
    + concretePatch(source, uv, b, dx, dy) * weights.y
    + concretePatch(source, uv, c, dx, dy) * weights.z;
  vec3 color = mean + (blended - mean) / sqrt(dot(weights, weights));
  return vec4(clamp(color, 0.0, 1.0), 1.0);
}

uniform sampler2D plasterMap;
uniform float plasterEnabled;
uniform float plasterChips;

// x is derivative-free exposed coverage, y is filtered color coverage.
vec2 plasterExposure(vec2 uv) {
  vec2 p = uv * vec2(2.4, 2.0) + vec2(28.1, 4.7);
  float threshold = 0.60;
  vec2 warp = vec2(concreteFractal(p * 2.3 + vec2(7.1, 31.8)),
    concreteFractal(p * 2.3 + vec2(53.2, 9.4))) - 0.5;
  float broad = concreteFractal(p + warp * 0.18 * edgeJaggedness);
  // Fine, dense erosion roughens the outline while the broad field keeps patches clumped.
  float detail = (concreteNoise(p * 13.0) - 0.5) * 0.055
    + (concreteNoise(p * 31.0 + 17.3) - 0.5) * 0.015
    + (concreteNoise(p * 83.0 + 39.1) - 0.5) * 0.028
    + (concreteNoise(p * 173.0 + 81.7) - 0.5) * 0.012
    + (concreteNoise(p * 347.0 + 13.9) - 0.5) * 0.004;
  float border = 1.0 - smoothstep(0.025, 0.08, abs(broad - threshold));
  float field = broad + detail * edgeJaggedness * border;
  float coverage = smoothstep(threshold - 0.002, threshold + 0.002, field);
  // Keep the material break crisp; fuzziness comes from the eroded contour.
  float filterWidth = max(fwidth(field) * 0.5, 0.002);
  float filtered = smoothstep(threshold - filterWidth, threshold + filterWidth, field);
  return vec2(coverage, filtered) * plasterChips * plasterEnabled;
}

vec3 plasterColor(vec2 uv) {
  vec3 mean = vec3(0.7364711473, 0.6666688006, 0.5692678346);
  vec3 color = concreteSample(plasterMap, mean, uv).rgb;
  float mottling = concreteFractal(uv * vec2(3.7, 2.8) + 19.2);
  color *= 1.0 - darknessAmount * mottling;
  return concretePigment(color, uv * vec2(0.85, 0.68));
}

// Fine mineral grain and a thin chipped lip respond to the live lights.
float plasterHeight(vec2 uv, float exposed) {
  float grain = concreteNoise(uv * 480.0) * 0.00016
    + concreteFractal(uv * 65.0) * 0.00035;
  return (0.004 + grain) * (1.0 - exposed);
}

vec4 layeredConcrete(vec2 uv, vec3 relief, vec2 exposure) {
  vec3 a = concreteSample(map, concreteMean, uv).rgb;
  vec3 b = concreteSample(concreteMapB, concreteMeanB, uv * 1.13 + vec2(3.7, 11.2)).rgb;
  b *= mix(vec3(1.0), concreteMean / concreteMeanB, 0.9);
  // Broader noise stretches vertically without stretching the source texture grain.
  vec2 noiseUv = uv * vec2(0.85, 0.68);
  float darkA = concreteEdge(concreteFracture(noiseUv * 5.4 + vec2(5.3, 17.1)), 0.035);
  float darkB = concreteEdge(concreteFracture(noiseUv * 8.28 + vec2(73.4, 29.6)), 0.035);
  a *= 1.0 - darknessAmount * darkA;
  b *= 1.0 - darknessAmountB * darkB;
  float mask = concreteFracture(noiseUv * blendScale + vec2(113.2, 67.8));
  float width = 0.045 / blendContrast;
  float blend = concreteEdge(mask, width);
  vec3 color = concretePigment(mix(a, b, blend), noiseUv);
  vec3 cutout = concreteCutout(uv * 2.0);
  float cutoutCoverage = 1.0 - smoothstep(-0.003, 0.003, cutout.x);
  vec3 repairTint = vec3(0.045, 0.020, -0.025) * cutout.y;
  color *= vec3(1.0) + cutoutCoverage * (vec3(cutout.z * 0.14) + repairTint);
  color *= 1.0 - min(relief.y, 0.85);
  color += vec3(0.012, 0.011, 0.009) * relief.z;
  if (plasterEnabled > 0.5) {
    color = mix(plasterColor(uv), color, exposure.y);
  }
  float grime = pow(texture2D(concreteGrimeMask, vec2(uv.x * 0.25, vConcreteTopDistance)).r, 0.75)
    * concreteGrime * (1.0 - smoothstep(0.85, 1.0, vConcreteTopDistance));
  color *= vec3(1.0) - grime * vec3(0.68, 0.72, 0.76);
  return vec4(color, 1.0);
}
