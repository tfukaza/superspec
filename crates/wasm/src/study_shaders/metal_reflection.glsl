
    vec3 center = textureCubeUV(envMap, envMapRotation * reflectVec, roughness).rgb;
    if (studyGlossWeight <= 0.0) return center * envMapIntensity;
    // Orient the filter around the reflection direction in environment space.
    vec3 up = vec3(0, 1, 0);
    vec3 tangent = cross(up, reflectVec);
    if (dot(tangent, tangent) < 0.0001) tangent = cross(vec3(1, 0, 0), reflectVec);
    tangent = normalize(tangent) * 0.22;
    vec3 across = normalize(cross(reflectVec, tangent)) * 0.16;
    vec3 samples[9];
    samples[4] = center;
    for (int i = 0; i < 9; ++i) {
      if (i == 4) continue;
      vec3 direction = normalize(reflectVec + float(i % 3 - 1) * tangent + float(i / 3 - 1) * across);
      samples[i] = textureCubeUV(envMap, envMapRotation * direction, roughness).rgb;
    }
    float lowestVariance = 1e20;
    vec3 filtered = center;
    for (int region = 0; region < 4; ++region) {
      int first = region % 2 + 3 * (region / 2);
      vec3 a = samples[first], b = samples[first + 1];
      vec3 c = samples[first + 3], d = samples[first + 4];
      vec3 mean = (a + b + c + d) * 0.25;
      vec3 variance = (a * a + b * b + c * c + d * d) * 0.25 - mean * mean;
      float score = dot(variance, vec3(0.2126, 0.7152, 0.0722));
      if (score < lowestVariance) {
        lowestVariance = score;
        filtered = mean;
      }
    }
    // Space the bands in square-root luminance so dark reflections also show distinct steps.
    float luminance = dot(filtered, vec3(0.2126, 0.7152, 0.0722));
    float level = sqrt(max(luminance, 0.0)) * 5.5;
    float bandEdge = (floor(level) + 0.5) / 5.5;
    float brightEdge = smoothstep(0.60, 0.95, bandEdge);
    float softness = mix(0.04, mix(0.48, 0.035, brightEdge), studyPaintedValues);
    float boundary = smoothstep(0.5 - softness, 0.5 + softness, fract(level));
    float band = (floor(level) + boundary) / 5.5;
    vec3 banded = filtered * (band * band / max(luminance, 1e-5));
    // Open up the broad dark shapes while preserving the brightest reflection.
    float shadowWeight = 1.0 - smoothstep(0.42, 0.87, band);
    banded += vec3(0.105, 0.135, 0.16) * studyPaintedValues * shadowWeight;
    vec4 envMapColor = vec4(mix(center, banded, studyGlossWeight), 1.0);
