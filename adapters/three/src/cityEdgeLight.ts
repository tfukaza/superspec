import * as THREE from "three";
import { legacyRenderDiagnostics, type RenderDiagnostics } from "./renderProfile";

/** The near side of a geometric edge catches the sun. Depth differences along
 * a plane (lettering, texture changes and cast-shadow boundaries) give zero. */
export const edgeLightDeclarations = `
  uniform mat4 viewToShadow;
  uniform vec3 sunDirection, sunTint, viewUp;
  uniform float shadowBias, boost;
  #if CITY_SHADOW == 2
    uniform highp sampler2DShadow sunDepth;
  #elif CITY_SHADOW == 1
    uniform sampler2D sunDepth;
  #endif
  float sunVisibility(vec3 p) {
    #if CITY_SHADOW > 0
      vec4 h = viewToShadow * vec4(p, 1.);
      vec3 s = h.xyz / h.w;
      s.z += shadowBias;
      if (any(lessThan(s,vec3(0.))) || any(greaterThan(s,vec3(1.)))) return 0.;
      #if CITY_SHADOW == 2
        return texture(sunDepth,s);
      #else
        return step(s.z, texture2D(sunDepth,s.xy).r);
      #endif
    #else
      return 0.;
    #endif
  }`;

export const edgeLightFragment = `
  if (boost > 0.) {
    float behind = max(max(dot(n,left),dot(n,-right)),max(dot(n,down),dot(n,-up)));
    float footprint = max(.001,-2. * p.z * texel.y / focal.y);
    vec4 gaps = vec4(dot(n,left),dot(n,-right),dot(n,down),dot(n,-up)) / footprint;
    vec4 nearEdges = smoothstep(vec4(.20),vec4(1.25),gaps);
    float coverage = min(1., dot(nearEdges, vec4(.48)));
    // A concrete arris under an overhang picks up the alley's reflected light.
    // Use the shaded face's own colour and the near side of the depth break.
    float underhang = 1. - smoothstep(-.45, .35, dot(n,viewUp));
    color.rgb += boost * 3.8 * concreteCoverage * coverage * underhang
      * sqrt(clamp(color.rgb,vec3(0.),vec3(1.)));
    float edge = mix(smoothstep(.20,1.25,behind / footprint), 3.4 * coverage, concreteCoverage);
    float facing = smoothstep(.18,.8,dot(n,sunDirection));
    if (edge * facing > 0.) {
      float light = edge * facing * smoothstep(.8,1.,sunVisibility(p));
      color.rgb += boost * light * sunTint * min(color.rgb,vec3(1.));
    }
  }`;

export class CityEdgeLight {
  readonly enabled: boolean;
  private readonly target = new THREE.Vector3();
  readonly uniforms = {
    sunDepth: { value: null as THREE.DepthTexture | null }, viewToShadow: { value: new THREE.Matrix4() },
    sunDirection: { value: new THREE.Vector3() }, sunTint: { value: new THREE.Color() },
    viewUp: { value: new THREE.Vector3() },
    shadowBias: { value: 0 }, boost: { value: 0 },
  };

  constructor(mode: RenderDiagnostics["effects"]["edgeBoost"] =
    legacyRenderDiagnostics().effects.edgeBoost) {
    this.enabled = mode !== "off";
  }

  update(renderer: THREE.WebGLRenderer, camera: THREE.Camera, sun: THREE.DirectionalLight, material: THREE.ShaderMaterial): void {
    const shadow = sun.shadow;
    const depth = renderer.shadowMap.enabled && sun.castShadow ? shadow.map?.depthTexture : null;
    const mode = depth ? (depth.compareFunction == null ? 1 : 2) : 0;
    if (material.defines.CITY_SHADOW !== mode) { material.defines.CITY_SHADOW = mode; material.needsUpdate = true; }
    const u = this.uniforms;
    u.sunDepth.value = depth ?? null;
    u.viewToShadow.value.multiplyMatrices(shadow.matrix, camera.matrixWorld);
    u.shadowBias.value = shadow.bias;
    sun.getWorldPosition(u.sunDirection.value).sub(sun.target.getWorldPosition(this.target)).transformDirection(camera.matrixWorldInverse);
    u.sunTint.value.copy(sun.color);
    u.viewUp.value.set(0,1,0).transformDirection(camera.matrixWorldInverse);
    u.boost.value = this.enabled && sun.visible && mode > 0 ? .055 * Math.min(1, sun.intensity) : 0;
  }
}
