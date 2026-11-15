
      #include <common>
      varying vec3 studyRustPosition;
      varying vec4 studyLocal;
      varying vec4 studyDimensions;
      varying vec3 studyNormal;
      varying mat3 studyEdgeToView;
      uniform sampler2D studyRustMap;
      uniform float studyEdgeRust, studySurfaceRust, studyRainRust;
      uniform float studyPolishedEdges;
      uniform float studyBandedGloss;
      uniform float studyPaintedValues;
      uniform float studySilhouetteHighlight;
      float studyGlossWeight;
      uniform float studyRustRepeat;
      uniform float studyRustRoughness;
      uniform vec3 studyOxideDark;
      uniform vec3 studyOxideLight;

      float studyNoiseHash(vec3 p) {
        p = fract(p * 0.1031);
        p += dot(p, p.yzx + 33.33);
        return fract((p.x + p.y) * p.z);
      }

      float studyNoise(vec3 p) {
        vec3 cell = floor(p);
        vec3 f = fract(p);
        f = f * f * (3.0 - 2.0 * f);
        float nearFace = mix(
          mix(studyNoiseHash(cell), studyNoiseHash(cell + vec3(1, 0, 0)), f.x),
          mix(studyNoiseHash(cell + vec3(0, 1, 0)), studyNoiseHash(cell + vec3(1, 1, 0)), f.x), f.y);
        float farFace = mix(
          mix(studyNoiseHash(cell + vec3(0, 0, 1)), studyNoiseHash(cell + vec3(1, 0, 1)), f.x),
          mix(studyNoiseHash(cell + vec3(0, 1, 1)), studyNoiseHash(cell + vec3(1, 1, 1)), f.x), f.y);
        return mix(nearFace, farFace, f.z);
      }

      float studyEdgeDistance() {
        vec3 size = studyDimensions.xyz;
        if (studyDimensions.w < 1.5) {
          vec3 clearance = size - abs(studyLocal.xyz) + abs(studyNormal) * 10.0;
          return min(clearance.x, min(clearance.y, clearance.z));
        }
        if (studyDimensions.w < 2.5) {
          return abs(studyNormal.y) > 0.5
            ? size.x - length(studyLocal.xz) : size.y - abs(studyLocal.y);
        }
        return min(studyLocal.w, 1.0 - studyLocal.w) * size.x;
      }

      vec3 studyRoundOver(float width, vec3 alongTube) {
        if (studyDimensions.w < 1.5) {
          vec3 clearance = studyDimensions.xyz - abs(studyLocal.xyz);
          return sign(studyLocal.xyz) * (1.0 - abs(studyNormal))
            * (1.0 - smoothstep(vec3(0.0), vec3(width), clearance));
        }
        float weight = 1.0 - smoothstep(0.0, width, studyEdgeDistance());
        if (studyDimensions.w < 2.5) {
          if (abs(studyNormal.y) > 0.5) return normalize(vec3(studyLocal.x, 0.0, studyLocal.z)) * weight;
          return vec3(0.0, sign(studyLocal.y), 0.0) * weight;
        }
        return alongTube * sign(studyLocal.w - 0.5) * weight;
      }

      float studyRaindrops(vec3 meters, float grain) {
        // Tilt the sampling lattice so rows don't line up with the pipes and cage rails.
        mat3 rotation = mat3(0.36, 0.80, -0.48, -0.48, 0.60, 0.64, 0.80, 0.0, 0.60);
        vec3 p = rotation * meters * 38.0;
        vec3 cell = floor(p);
        float chance = studyNoiseHash(cell + 33.5);
        vec3 center = 0.40 + 0.20 * vec3(studyNoiseHash(cell),
          studyNoiseHash(cell + 11.0), studyNoiseHash(cell + 62.0));
        float radius = mix(0.14, 0.34, chance);
        float distanceToDrop = length(fract(p) - center) + (grain - 0.5) * 0.12;
        return step(0.49, chance) * (1.0 - smoothstep(radius * 0.65, radius, distanceToDrop));
      }

      vec3 studyOxidePatch(vec2 uv, vec2 cell, vec2 dx, vec2 dy) {
        vec3 key = vec3(cell, 27.4);
        float angle = studyNoiseHash(key) * 6.283185;
        float c = cos(angle), s = sin(angle);
        mat2 rotation = mat2(c, -s, s, c);
        vec2 offset = vec2(studyNoiseHash(key + 13.0), studyNoiseHash(key + 19.0));
        return textureGrad(studyRustMap, rotation * uv + offset, rotation * dx, rotation * dy).rgb;
      }

      vec3 studyOxideSample(vec2 uv, vec2 dx, vec2 dy) {
        vec2 skewed = vec2(uv.x - uv.y / sqrt(3.0), uv.y * 2.0 / sqrt(3.0));
        vec2 cell = floor(skewed), f = fract(skewed);
        vec2 a = cell;
        vec3 weights = vec3(1.0 - f.x - f.y, f.x, f.y);
        if (f.x + f.y > 1.0) {
          a += 1.0;
          weights = vec3(f.x + f.y - 1.0, 1.0 - f.y, 1.0 - f.x);
        }
        weights = pow(max(weights, vec3(0.0)), vec3(3.0));
        weights /= weights.x + weights.y + weights.z;
        return studyOxidePatch(uv, a, dx, dy) * weights.x
          + studyOxidePatch(uv, cell + vec2(1, 0), dx, dy) * weights.y
          + studyOxidePatch(uv, cell + vec2(0, 1), dx, dy) * weights.z;
      }
