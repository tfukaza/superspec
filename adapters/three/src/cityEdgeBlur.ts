import * as THREE from "three";
import { FullScreenQuad } from "three/addons/postprocessing/Pass.js";
import { FXAAShader } from "three/addons/shaders/FXAAShader.js";
import { legacyRenderDiagnostics, type RenderDiagnostics } from "./renderProfile.ts";

/** WorldSpec's one-pixel edge softening, applied to the displayed world before the viewmodel. */
export class CityEdgeBlur {
  private readonly size = new THREE.Vector2();
  private readonly uniforms = { frame: { value: null as THREE.FramebufferTexture | null }, texel: { value: new THREE.Vector2() } };
  private pass?: FullScreenQuad;

  constructor(private readonly settings: Pick<RenderDiagnostics["effects"], "edgeBlur" | "displayAA"> =
    legacyRenderDiagnostics().effects) {}

  dispose(): void {
    this.uniforms.frame.value?.dispose();
    this.pass?.material.dispose();
    this.pass?.dispose();
  }

  apply(renderer: THREE.WebGLRenderer, enableDisplayFilter: boolean): void {
    const { edgeBlur, displayAA } = this.settings;
    if ((!edgeBlur && !displayAA) || !enableDisplayFilter) {
      this.uniforms.frame.value?.dispose();
      this.uniforms.frame.value = null;
      return;
    }
    // Capture the resolved display bytes. This keeps each scene material's tone mapping,
    // the sky and the canvas MSAA exactly as the main render produced them.
    renderer.getDrawingBufferSize(this.size);
    const { x: width, y: height } = this.size;
    const frame = this.uniforms.frame.value;
    if (!frame || frame.image.width !== width || frame.image.height !== height) {
      frame?.dispose();
      this.uniforms.frame.value = new THREE.FramebufferTexture(width, height);
      this.uniforms.frame.value.minFilter = this.uniforms.frame.value.magFilter = THREE.LinearFilter;
      this.uniforms.texel.value.set(1 / width, 1 / height);
    }
    renderer.copyFramebufferToTexture(this.uniforms.frame.value!);
    this.pass ??= new FullScreenQuad(new THREE.ShaderMaterial(!edgeBlur ? {
      ...FXAAShader, uniforms: { tDiffuse: this.uniforms.frame, resolution: this.uniforms.texel },
      depthTest: false, depthWrite: false, toneMapped: false,
    } : {
      uniforms: this.uniforms, depthTest: false, depthWrite: false, toneMapped: false,
      vertexShader: "varying vec2 vUv; void main() { vUv = uv; gl_Position = vec4(position.xy, 0.0, 1.0); }",
      fragmentShader: `
        uniform sampler2D frame;
        uniform vec2 texel;
        varying vec2 vUv;
        float contrast(vec3 a, vec3 b) {
          vec3 delta = abs(a - b);
          return max(delta.r, max(delta.g, delta.b));
        }
        void main() {
          vec4 center = texture2D(frame, vUv);
          vec3 left = texture2D(frame, vUv - vec2(texel.x, 0.0)).rgb;
          vec3 right = texture2D(frame, vUv + vec2(texel.x, 0.0)).rgb;
          vec3 down = texture2D(frame, vUv - vec2(0.0, texel.y)).rgb;
          vec3 up = texture2D(frame, vUv + vec2(0.0, texel.y)).rgb;
          float x = max(contrast(center.rgb, left), contrast(center.rgb, right));
          float y = max(contrast(center.rgb, down), contrast(center.rgb, up));
          // Favor the stronger gradient continuously. A small lit-edge catch must
          // not abruptly flip the blur direction at a corner with equal gradients.
          float wx = x*x, wy = y*y;
          vec3 sum = mix(down + up, left + right, wx / max(wx + wy, 1e-8));
          float amount = 0.4 * smoothstep(0.12, 0.20, max(x, y));
          gl_FragColor = vec4(mix(center.rgb, 0.5 * sum, amount), center.a);
        }`,
    }));
    const clear = renderer.autoClear;
    const reset = renderer.info.autoReset;
    renderer.autoClear = false;
    renderer.info.autoReset = false;
    this.pass.render(renderer);
    renderer.autoClear = clear;
    renderer.info.autoReset = reset;
  }
}
