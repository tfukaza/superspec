
      #include <metalnessmap_fragment>
      vec3 positionMeters = studyRustPosition;
      float fineWeathering = studyNoise(positionMeters * 85.0);
      float edgeNoise = studyNoise(positionMeters * 34.0);
      float edgeBand = mix(0.001, 0.022, edgeNoise * edgeNoise);
      if (studyDimensions.w < 1.5) {
        edgeBand = min(edgeBand, 0.4 * min(studyDimensions.x, min(studyDimensions.y, studyDimensions.z)));
      }
      float edge = (1.0 - smoothstep(edgeBand * 0.25, edgeBand + 0.0004, studyEdgeDistance()))
        * smoothstep(0.32, 0.62, edgeNoise + (fineWeathering - 0.5) * 0.19);
      float patchField = studyNoise(positionMeters * 3.1) * 0.68
        + studyNoise(positionMeters * 6.2) * 0.20 + studyNoise(positionMeters * 12.4) * 0.08
        + fineWeathering * 0.04;
      float surface = smoothstep(0.61, 0.67, patchField);
      float rain = studyRaindrops(positionMeters, fineWeathering);
      float coverage = 1.0 - (1.0 - studyEdgeRust * edge) * (1.0 - studySurfaceRust * surface)
        * (1.0 - studyRainRust * rain);
      studyGlossWeight = studyBandedGloss * (1.0 - coverage);
      float roundWidth = 0.008;
      if (studyDimensions.w < 1.5) {
        roundWidth = min(roundWidth, 0.45 * min(studyDimensions.x,
          min(studyDimensions.y, studyDimensions.z)));
      }
      float exposedMetal = studyPolishedEdges * (1.0 - coverage);
      float polishedRim = exposedMetal * (1.0 - smoothstep(0.0, roundWidth, studyEdgeDistance()));
      roughnessFactor = mix(roughnessFactor, 0.13, polishedRim);
      vec3 p = positionMeters * studyRustRepeat;
      vec3 dx = dFdx(p), dy = dFdy(p);
      if (coverage > 0.0) {
        vec3 axisWeight = pow(abs(normalize(cross(dx, dy))), vec3(4.0));
        axisWeight /= axisWeight.x + axisWeight.y + axisWeight.z;
        vec3 textureColor = studyOxideSample(p.zy, dx.zy, dy.zy) * axisWeight.x
          + studyOxideSample(p.xz, dx.xz, dy.xz) * axisWeight.y
          + studyOxideSample(p.xy, dx.xy, dy.xy) * axisWeight.z;
        float tone = clamp(sqrt(dot(textureColor, vec3(0.2126, 0.7152, 0.0722))) * 1.7, 0.0, 1.0);
        vec3 oxide = mix(studyOxideDark, studyOxideLight, tone);
        diffuseColor.rgb = mix(diffuseColor.rgb, oxide, coverage);
        roughnessFactor = mix(roughnessFactor, studyRustRoughness, coverage);
        metalnessFactor = mix(metalnessFactor, 0.0, coverage);
      }
