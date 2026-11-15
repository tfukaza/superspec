vec3 concreteRelief = vec3(0.0);
      if (concreteGround < 0.5) {
        concreteRelief = concreteSeam(vMapUv * 2.0) + concreteHole(vMapUv * 2.0);
      } else {
        concreteRelief.x = concreteFractal(vMapUv * 65.0) * 0.0006
          + concreteNoise(vMapUv * 420.0) * 0.00018;
      }
      vec2 plasterExposed = plasterExposure(vMapUv);
      vec2 chipCoverage = concreteChipCoverage(vMapUv)
        * mix(vec2(1.0), plasterExposed, plasterEnabled);
      float surfaceHeight = concreteRelief.x;
      if (plasterEnabled > 0.5) {
        surfaceHeight = concreteRelief.x * plasterExposed.x + plasterHeight(vMapUv, plasterExposed.x);
      }
      diffuseColor *= layeredConcrete(vMapUv, concreteRelief, plasterExposed);
      if (concreteChipFinishEnabled > 0.5 && concreteChipPattern.y > 0.0) {
        float grain = concreteNoise(vMapUv * 420.0) * 0.45
          + concreteNoise(vMapUv * 127.0) * 0.55;
        float chipHeight = -chipCoverage.x * concreteChipFinish.z * (1.35 - grain * 0.70);
        surfaceHeight += chipHeight;
        diffuseColor.rgb *= mix(1.0, 0.59 + grain * 0.29, chipCoverage.y);
      }
      vec3 baseWeathering = concreteBaseWeathering(vMapUv);
      diffuseColor.rgb *= mix(vec3(1.0), vec3(0.19, 0.16, 0.13), baseWeathering.x);
      diffuseColor.rgb = mix(diffuseColor.rgb, vec3(0.073, 0.091, 0.022), baseWeathering.y);
      surfaceHeight += baseWeathering.z;
      vec3 roundedEdge = vec3(0.0);
      float edgeDarkness;
      surfaceHeight += concreteEdgeRelief(roundedEdge, edgeDarkness);
      diffuseColor.rgb *= 1.0 - edgeDarkness;
