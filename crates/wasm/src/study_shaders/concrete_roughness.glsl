#include <roughnessmap_fragment>
      if (concreteChipFinishEnabled > 0.5) {
        float intact = mix(concreteChipFinish.x, roughnessFactor, plasterEnabled * (1.0 - plasterExposed.y));
        roughnessFactor = mix(intact, concreteChipFinish.y, chipCoverage.y);
      }
      roughnessFactor = mix(roughnessFactor, 0.95, baseWeathering.y);
