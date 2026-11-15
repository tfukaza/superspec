
      // Modulate only the accumulated diffuse lobe, after specular lighting is complete.
      float diffuseGrain = (studyNoise(studyRustPosition * 51.0) - 0.5) * 0.04;
      outgoingLight += totalDiffuse * diffuseGrain * (1.0 - coverage);
      // Add a small linear-light floor so a rim still reads against a dark reflection.
      outgoingLight += vec3(0.105, 0.111, 0.12) * polishedRim;
      outgoingLight += vec3(0.052, 0.057, 0.065) * silhouetteRim;
      #include <opaque_fragment>
