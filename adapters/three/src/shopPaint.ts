import * as THREE from "three";
import { legacyRenderDiagnostics, type RenderDiagnostics } from "./renderProfile";

/** A local look for the two playable market bays. The pose test skips the resolve work
 * across the rest of the map. The depth test below bounds it to the shop surfaces. */
export class ShopPaint {
  readonly uniforms = {
    shopViewToWorld: { value: new THREE.Matrix4() },
    shopPaintMode: { value: 0 },
  };

  constructor(private readonly mode: RenderDiagnostics["effects"]["shopArt"] =
    legacyRenderDiagnostics().effects.shopArt) {}

  update(camera: THREE.PerspectiveCamera, enabled: boolean): void {
    const { x, y, z } = camera.position;
    const close = x > 10 && x < 32 && y > -2 && y < 7 && z > -15 && z < 20;
    this.uniforms.shopPaintMode.value = !enabled || !close || this.mode === "off" ? 0
      : this.mode === "wash" ? 2 : this.mode === "brush" ? 3 : this.mode === "palette" ? 4 : 1;
    if (this.uniforms.shopPaintMode.value) {
      camera.updateMatrixWorld();
      this.uniforms.shopViewToWorld.value.copy(camera.matrixWorld);
    }
  }
}

export const shopPaintShader = /* glsl */ `
uniform mat4 shopViewToWorld;
uniform int shopPaintMode;

float shopWindow(vec3 p, vec3 lo, vec3 hi) {
  vec3 spread = vec3(.35,.28,.55);
  vec3 w = smoothstep(lo-spread,lo,p) * (1.-smoothstep(hi,hi+spread,p));
  return w.x*w.y*w.z;
}
float shopLuma(vec3 c) { return dot(c,vec3(.2126,.7152,.0722)); }

vec4 shopSample(vec2 uv, vec3 p, vec3 n, vec3 pigment) {
  vec3 sampleColor = texture2D(frame, uv).rgb;
  vec3 delta = aoAt(uv)-p;
  float acrossPlane = abs(dot(delta,n));
  float visible = 1.-smoothstep(.006,.025,acrossPlane);
  float similar = length(sqrt(max(sampleColor,0.))-sqrt(max(pigment,0.)));
  float weight = visible*(1.-smoothstep(.07,.25,similar));
  return vec4(sampleColor*weight,weight);
}

vec3 shopFinish(vec3 pigment,vec2 uv,vec3 p,vec3 n) {
  if (shopPaintMode == 0 || p.z < -26.) return pigment;
  vec3 world = (shopViewToWorld*vec4(p,1.)).xyz;
  float duck = shopWindow(world,vec3(15.7,.04,-6.5),vec3(19.2,2.94,-2.35));
  float liquor = shopWindow(world,vec3(14.7,.04,7.25),vec3(20.9,2.93,14.35));
  float coverage = max(duck,liquor);
  if (coverage < .01) return pigment;

  vec3 painted = pigment;
  if (shopPaintMode > 1) {
    vec2 texel = aoTexel*(2.6+0.8*smoothstep(.6,3.0,-p.z));
    if (shopPaintMode == 4) texel *= 1.6;
    float dx = shopLuma(texture2D(frame,uv+vec2(aoTexel.x,0.)).rgb)
             - shopLuma(texture2D(frame,uv-vec2(aoTexel.x,0.)).rgb);
    float dy = shopLuma(texture2D(frame,uv+vec2(0.,aoTexel.y)).rgb)
             - shopLuma(texture2D(frame,uv-vec2(0.,aoTexel.y)).rgb);
    float gradient = length(vec2(dx,dy));
    vec2 edge = vec2(-dy,dx)/max(gradient,.000001);
    edge *= dot(edge,vec2(.94,.34)) < 0. ? -1. : 1.;
    vec2 grain = mix(vec2(.94,.34),edge,smoothstep(.002,.04,gradient));
    vec2 tangent = normalize(grain+vec2(.00001))*texel;
    vec2 across = vec2(-tangent.y,tangent.x)*.55;
    vec4 sum = vec4(pigment*2.,2.);
    sum += shopSample(uv+tangent,p,n,pigment);
    sum += shopSample(uv-tangent,p,n,pigment);
    sum += shopSample(uv+across,p,n,pigment);
    sum += shopSample(uv-across,p,n,pigment);
    if (shopPaintMode >= 3) {
      sum += shopSample(uv+1.75*tangent+across,p,n,pigment);
      sum += shopSample(uv-1.75*tangent-across,p,n,pigment);
    }
    float value = shopLuma(pigment);
    float finish = smoothstep(.02,.12,value)*(1.-smoothstep(.55,1.3,value));
    float amount = shopPaintMode >= 3 ? .82 : .58;
    painted = mix(pigment,sum.rgb/sum.a,amount*finish);
  }
  float light = shopLuma(painted);
  float shade = (1.-smoothstep(.045,.28,light))*smoothstep(.004,.028,light);
  float middle = smoothstep(.035,.16,light)*(1.-smoothstep(.65,1.8,light));
  // Color the bounce and the diffuse shoulder in separate ranges.
  // Highlights retain enough headroom for the existing city bloom.
  painted *= mix(vec3(1.),vec3(.96,.99,1.065),shade);
  painted = mix(painted,painted*vec3(1.085,1.035,.935),middle*(.38+.28*duck));
  if (shopPaintMode == 4) {
    // Group nearby values in perceptual stops, with a broad transition within
    // each stop so a curved surface still has a gradient.
    float exposure = log2(max(light,.003)) * 2.0;
    float stepValue = floor(exposure) + smoothstep(.14,.86,fract(exposure));
    float gain = exp2((stepValue-exposure)*.5);
    painted *= mix(1.,gain,.68*middle+.20*shade);
  }
  return mix(pigment, painted, coverage);
}
`;
