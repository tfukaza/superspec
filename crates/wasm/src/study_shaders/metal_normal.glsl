
      #include <normal_fragment_maps>
      float facing = abs(dot(normal, normalize(vViewPosition)));
      float silhouetteRim = studySilhouetteHighlight * (1.0 - coverage)
        * (1.0 - smoothstep(0.06, 0.30, facing));
      vec3 alongTube = vec3(0.0);
      if (studyDimensions.w > 2.5) {
        // Recover the direction of increasing tube U on the local surface.
        vec3 localDx = dFdx(studyLocal.xyz), localDy = dFdy(studyLocal.xyz);
        vec3 tangentX = cross(localDy, normalize(studyNormal));
        vec3 tangentY = cross(normalize(studyNormal), localDx);
        vec3 gradient = dFdx(studyLocal.w) * tangentX + dFdy(studyLocal.w) * tangentY;
        alongTube = sign(dot(localDx, tangentX)) * gradient / max(length(gradient), 1e-12);
      }
      if (polishedRim > 0.0) {
        normal = normalize(normal + exposedMetal * studyEdgeToView * studyRoundOver(roundWidth, alongTube));
      }
