import * as THREE from "three";
import { EffectComposer } from "three/examples/jsm/postprocessing/EffectComposer.js";
import { GTAOPass } from "three/examples/jsm/postprocessing/GTAOPass.js";
import { RenderPass } from "three/examples/jsm/postprocessing/RenderPass.js";
import { ShaderPass } from "three/examples/jsm/postprocessing/ShaderPass.js";
import type { AmbientOcclusionParameters } from "./rendering.ts";

/**
 * The last pass: the renderer's own tone mapping, exposure and output color space (three.js applies them to a material
 * drawn to the canvas through these two chunks, exactly as it does to a scene material), then the backdrop behind
 * whatever the scene did not cover. The scene target is premultiplied by coverage after its multisample resolve.
 */
const OUTPUT = {
  uniforms: { tDiffuse: { value: null }, backdrop: { value: new THREE.Vector3() } },
  vertexShader: `varying vec2 vUv; void main() { vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`,
  fragmentShader: `
uniform sampler2D tDiffuse;
uniform vec3 backdrop;
varying vec2 vUv;
void main() {
  vec4 texel = texture2D(tDiffuse, vUv);
  float coverage = clamp(texel.a, 0.0, 1.0);
  gl_FragColor = vec4(coverage > 0.0 ? texel.rgb / coverage : vec3(0.0), 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
  gl_FragColor.rgb = mix(backdrop, gl_FragColor.rgb, coverage);
}`,
};

/**
 * Screen-space ambient occlusion for a scene view: soft darkening in creases and where objects meet, from the depth and
 * geometric normals the camera sees (three.js's GTAO pass with its denoise). A global rendering setting of the scene,
 * never part of a material; the planned baked `occlusion()` material field is a different thing.
 *
 * The scene is drawn into a multisampled linear half-float target (so edges stay antialiased), the occlusion multiplies
 * it, and an output pass applies the renderer's tone mapping, exposure and output color space, as drawing straight to
 * the canvas would. Materials therefore compile for this target, not for the canvas: anything that compiles a scene
 * material ahead of a frame must do it inside `compiling`.
 *
 * It draws only when `render` is called, so an on-demand viewer stays idle between frames. A relief normal in a
 * material does not affect the occlusion.
 */
export class AmbientOcclusion {
  private readonly composer: EffectComposer;
  private readonly pass: GTAOPass;

  /** Why this renderer cannot composite, or null: the scene target needs renderable half-float color. */
  static unavailable(renderer: THREE.WebGLRenderer): string | null {
    if (!renderer.capabilities.isWebGL2) return "WebGL2 is not available";
    if (!renderer.extensions.has("EXT_color_buffer_float") && !renderer.extensions.has("EXT_color_buffer_half_float")) return "half-float render targets are not available";
    return null;
  }

  /**
   * `excluded` names what the occlusion must not see (inspection overlays): it is hidden while depth and normals are
   * drawn, and drawn in the image as usual.
   */
  constructor(private readonly renderer: THREE.WebGLRenderer, private readonly scene: THREE.Scene, camera: THREE.Camera,
    parameters: AmbientOcclusionParameters, excluded: () => THREE.Object3D[] = () => []) {
    const size = renderer.getSize(new THREE.Vector2());
    const target = new THREE.WebGLRenderTarget(Math.max(1, size.x), Math.max(1, size.y), { type: THREE.HalfFloatType, samples: 4 });
    this.composer = new EffectComposer(renderer, target);
    this.composer.addPass(new RenderPass(scene, camera));
    this.pass = new GTAOPass(scene, camera, size.x, size.y);
    // Both faces of a thin part write depth and a normal, as the scene's materials draw them.
    this.pass.normalMaterial.side = THREE.DoubleSide;
    const internals = this.pass as unknown as { _overrideVisibility(): void; _restoreVisibility(): void };
    const hide = internals._overrideVisibility.bind(this.pass), restore = internals._restoreVisibility.bind(this.pass);
    let hidden: THREE.Object3D[] = [];
    internals._overrideVisibility = () => { hide(); hidden = excluded().filter(object => object.visible); for (const object of hidden) object.visible = false; };
    internals._restoreVisibility = () => { for (const object of hidden) object.visible = true; hidden = []; restore(); };
    this.composer.addPass(this.pass);
    this.output = new ShaderPass(OUTPUT);
    this.composer.addPass(this.output);
    this.set(parameters);
  }

  /**
   * Draws one frame to the canvas. The scene's background color is not lit, so it is not tone mapped when drawn
   * straight to the canvas; here it is left out of the scene target (cleared transparent) and put behind the image
   * by the output pass, in display space, which is also where antialiased edges blend with it.
   */
  render(): void {
    const background = this.scene.background;
    const color = (background as THREE.Color | null)?.isColor ? background as THREE.Color : null;
    if (color) {
      color.getRGB(this.backdrop, this.renderer.outputColorSpace);
      (this.output.uniforms.backdrop.value as THREE.Vector3).set(this.backdrop.r, this.backdrop.g, this.backdrop.b);
      this.scene.background = null;
    }
    const clear = this.renderer.getClearColor(this.clearColor), alpha = this.renderer.getClearAlpha();
    this.renderer.setClearColor(0x000000, 0);
    try { this.composer.render(); } finally {
      this.renderer.setClearColor(clear, alpha);
      if (color) this.scene.background = color;
    }
  }

  private readonly output: ShaderPass;
  private readonly backdrop = { r: 0, g: 0, b: 0 };
  private readonly clearColor = new THREE.Color();

  /** Changes the look in place. */
  set(parameters: AmbientOcclusionParameters): void {
    // Lengths are scene meters (`screenSpaceRadius` off). Every sample within the radius raises the horizon in full
    // (no distance falloff): with the pass's default falloff only the nearest centimeters of a corner darken.
    this.pass.updateGtaoMaterial({ radius: parameters.radius, thickness: parameters.thickness, distanceExponent: 1, distanceFallOff: 0,
      scale: Math.max(1, parameters.intensity), samples: 16, screenSpaceRadius: false });
    // The denoise removes the sampling pattern, so nothing crawls while orbiting; it stops at depth and normal edges.
    this.pass.updatePdMaterial({ lumaPhi: 10, depthPhi: 2, normalPhi: 3, radius: 8, radiusExponent: 2, rings: 2, samples: 16 });
    // Up to 1 the intensity is how much of the occlusion is blended in; above it, the occlusion itself is deepened
    // (raised to that power), which darkens creases and leaves open surfaces, where it is 1, as they are.
    this.pass.blendIntensity = Math.min(1, parameters.intensity);
  }

  setSize(width: number, height: number, pixelRatio: number): void {
    this.composer.setPixelRatio(pixelRatio);
    this.composer.setSize(width, height);
  }

  /** Runs `compile` with the scene target bound, so programs are made for the target the scene is drawn into. */
  compiling<T>(compile: () => T): T {
    const before = this.renderer.getRenderTarget();
    this.renderer.setRenderTarget(this.composer.readBuffer);
    try { return compile(); } finally { this.renderer.setRenderTarget(before); }
  }

  dispose(): void {
    this.pass.dispose();
    this.composer.dispose();
  }
}
