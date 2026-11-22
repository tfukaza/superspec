import * as THREE from "three";

export type ConcreteDetail = { version: 1; texture: number; metersPerTile: number; mean: number; strength: number };

/** The base atlas carries authored wear. This single tiled residual restores the aggregate
 * finer than an atlas texel; its last mips converge to the exported neutral mean. */
export function applyConcreteDetail(material: THREE.MeshStandardMaterial, map: THREE.Texture, detail: ConcreteDetail): void {
  const original = material.onBeforeCompile, previousKey = material.customProgramCacheKey.bind(material);
  material.onBeforeCompile = function(shader, renderer) {
    original.call(this, shader, renderer);
    shader.uniforms.wsConcreteGrain = { value: map };
    shader.uniforms.wsConcreteParameters = { value: new THREE.Vector3(detail.metersPerTile, detail.mean, detail.strength) };
    shader.vertexShader = "varying vec2 wsConcreteMeters;\n" + shader.vertexShader.replace(
      "#include <project_vertex>", `#include <project_vertex>
      vec3 wsN = normalize(transformedNormal * mat3(viewMatrix));
      vec3 wsU = normalize(cross(abs(wsN.y) < 0.95 ? vec3(0.,1.,0.) : vec3(0.,0.,1.), wsN));
      vec3 wsP = cameraPosition + mvPosition.xyz * mat3(viewMatrix);
      wsConcreteMeters = vec2(dot(wsP, wsU), dot(wsP, cross(wsN, wsU)));`,
    );
    shader.fragmentShader = "uniform sampler2D wsConcreteGrain;\nuniform vec3 wsConcreteParameters;\nvarying vec2 wsConcreteMeters;\n" + shader.fragmentShader.replace(
      "#include <map_fragment>", `#include <map_fragment>
      float wsResidual = texture2D(wsConcreteGrain, wsConcreteMeters / wsConcreteParameters.x).r - wsConcreteParameters.y;
      diffuseColor.rgb *= 1.0 + wsConcreteParameters.z * wsResidual;`,
    );
  };
  material.customProgramCacheKey = () => previousKey() + "|worldspec-concrete-detail-v1";
  // The map owner discovers and disposes textures attached to its materials, including this shared sampler.
  Object.assign(material, { worldSpecConcreteTexture: map });
}
