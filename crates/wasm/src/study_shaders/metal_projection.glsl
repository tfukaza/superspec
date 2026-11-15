
      #include <project_vertex>
      vec4 rustPosition = vec4(transformed, 1.0);
      studyLocal = vec4(transformed, uv.x);
      studyDimensions = rustDimensions;
      studyNormal = normal;
      // Wire instances scale along the box axes without rotating them.
      studyEdgeToView = mat3(normalize(modelViewMatrix[0].xyz),
        normalize(modelViewMatrix[1].xyz), normalize(modelViewMatrix[2].xyz));
      #ifdef USE_INSTANCING
        rustPosition = instanceMatrix * rustPosition;
        vec3 instanceScale = vec3(length(instanceMatrix[0].xyz), length(instanceMatrix[1].xyz), length(instanceMatrix[2].xyz));
        studyLocal.xyz *= instanceScale;
        studyDimensions.xyz *= instanceScale;
      #endif
      studyRustPosition = (modelMatrix * rustPosition).xyz;
