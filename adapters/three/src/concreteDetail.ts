import * as THREE from "three";

export type ConcreteDetail = {
  version: 1;
  texture: number;
  metersPerTile: number;
  mean: number;
  strength: number;
  micro?: { metersPerTile: number; strength: number };
};

/** The base atlas carries authored wear. This single tiled residual restores the aggregate
 * finer than an atlas texel; its last mips converge to the exported neutral mean. */
export function applyConcreteDetail(material: THREE.MeshStandardMaterial, map: THREE.Texture, detail: ConcreteDetail): void {
  const original = material.onBeforeCompile, previousKey = material.customProgramCacheKey.bind(material);
  const micro = detail.micro;
  const polishedEdges = material.userData.worldSpecConcreteEdge === true && !!material.roughnessMap;
  const hasMicro = micro && Number.isFinite(micro.metersPerTile) && micro.metersPerTile > 0 &&
    Number.isFinite(micro.strength) && micro.strength > 0;
  material.onBeforeCompile = function(shader, renderer) {
    original.call(this, shader, renderer);
    shader.uniforms.wsConcreteGrain = { value: map };
    shader.uniforms.wsConcreteParameters = { value: new THREE.Vector3(detail.metersPerTile, detail.mean, detail.strength) };
    if (hasMicro) shader.uniforms.wsConcreteMicro = { value: new THREE.Vector2(micro.metersPerTile, micro.strength) };
    shader.vertexShader = "varying vec2 wsConcreteMeters;\n" + shader.vertexShader.replace(
      "#include <project_vertex>", `#include <project_vertex>
      vec3 wsN = normalize(transformedNormal * mat3(viewMatrix));
      vec3 wsU = normalize(cross(abs(wsN.y) < 0.95 ? vec3(0.,1.,0.) : vec3(0.,0.,1.), wsN));
      vec3 wsP = cameraPosition + mvPosition.xyz * mat3(viewMatrix);
      wsConcreteMeters = vec2(dot(wsP, wsU), dot(wsP, cross(wsN, wsU)));`,
    );
    shader.fragmentShader = (hasMicro ? "uniform vec2 wsConcreteMicro;\n" : "") +
      "uniform sampler2D wsConcreteGrain;\nuniform vec3 wsConcreteParameters;\nvarying vec2 wsConcreteMeters;\n" + shader.fragmentShader.replace(
      "#include <map_fragment>", `#include <map_fragment>
      float wsResidual = texture2D(wsConcreteGrain, wsConcreteMeters / wsConcreteParameters.x).r - wsConcreteParameters.y;
      diffuseColor.rgb *= 1.0 + wsConcreteParameters.z * wsResidual;
      ${hasMicro ? `
      // A rotated second scale gives inspection-distance mineral detail using the
      // same resident mip chain. Evaluate gradients before the conditional sample.
      vec2 wsMicroUV = mat2(0.8, 0.6, -0.6, 0.8) * wsConcreteMeters / wsConcreteMicro.x + vec2(.31, .57);
      vec2 wsDx = dFdx(wsMicroUV), wsDy = dFdy(wsMicroUV);
      float wsFootprint = max(length(wsDx), length(wsDy)) * float(textureSize(wsConcreteGrain, 0).x);
      float wsCoverage = 1.0 - smoothstep(2.0, 12.0, wsFootprint);
      if (wsCoverage > 0.0) {
        float wsMicro = textureGrad(wsConcreteGrain, wsMicroUV, wsDx, wsDy).r - wsConcreteParameters.y;
        diffuseColor.rgb *= 1.0 + wsCoverage * wsConcreteMicro.y * wsMicro;
      }` : ""}`,
    );
    if (polishedEdges) {
      shader.fragmentShader = shader.fragmentShader.replace("#include <lights_fragment_end>", `#include <lights_fragment_end>
        // These charts encode dry paste at ~0.79 roughness and the burnished,
        // chipped corners below ~0.65. Catch local light only on those fragments.
        float wsPolish = 1.0 - smoothstep(.57, .77, roughnessFactor);
        float wsGrazing = 1.0 - abs(dot(normal, normalize(vViewPosition)));
        vec3 wsEdgeLight = sqrt(max(reflectedLight.directDiffuse + reflectedLight.indirectDiffuse, vec3(0.0)));
        reflectedLight.directSpecular += wsEdgeLight * wsPolish * (.105 + .34 * wsGrazing * wsGrazing);`);
    }
  };
  material.customProgramCacheKey = () => previousKey() + (hasMicro ? "|worldspec-concrete-micro-v1" : "|worldspec-concrete-detail-v1") +
    (polishedEdges ? "|polished-corners" : "");
  // The map owner discovers and disposes textures attached to its materials, including this shared sampler.
  Object.assign(material, { worldSpecConcreteTexture: map });
}
