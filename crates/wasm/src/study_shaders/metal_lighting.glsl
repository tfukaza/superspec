
      #include <lights_physical_fragment>
      material.specularColorBlended = min(vec3(1.0), material.specularColorBlended * (1.0 + 0.45 * polishedRim));
      material.specularF90 = mix(material.specularF90, 1.65, silhouetteRim);
