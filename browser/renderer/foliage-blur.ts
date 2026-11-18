import * as THREE from "three";
import { followPaintedFinishing } from "./shade.ts";

/**
 * Foliage blur (prototype, `?leafblur=<meters>`): a plant's leaves and billboards are drawn offscreen and their colours
 * blurred among themselves, then composited where the leaves were, so neighbouring colours blend like wet paint while
 * the silhouette keeps its crisp leaf edges.
 *
 * 1. The scene draws without the leaves (they live on `BLUR_LAYER` alone).
 * 2. Offscreen: everything else draws depth only (so walls, grass and bark still hide leaves), then the leaves draw with
 *    their colour and alpha 1.
 * 3. A separable Gaussian over colour × coverage and coverage alone; their ratio is the leaves' colour blurred among
 *    leaves only, so no background bleeds in.
 * 4. The composite writes that colour where the leaves were (their own coverage, unblurred).
 * The radius is meters at the plant, turned to pixels by its distance, so the smudge stays the same on the plant.
 */
export const BLUR_LAYER = 9;

const QUAD_VERTEX = /* glsl */`void main() { gl_Position = vec4(position.xy, 0.0, 1.0); }`;

export class FoliageBlur {
  private target: THREE.WebGLRenderTarget | null = null;
  private passes: THREE.WebGLRenderTarget[] = [];
  private size = new THREE.Vector2();
  private readonly quad = new THREE.Mesh(new THREE.PlaneGeometry(2, 2));
  private readonly ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly depthOnly = new THREE.MeshBasicMaterial({ colorWrite: false });
  private readonly blur: THREE.ShaderMaterial;
  private readonly composite: THREE.ShaderMaterial;
  /** The last frame's blur, for inspection. */
  last: { radius: number; pixelsPerMeter: number; distance: number } | null = null;

  /**
   * `repaint` (`?leafrepaint=1`): blur and re-threshold, for the painted-mass prototype's parts (leaf-normals.ts and
   * the cards, fuzz and strips that share its bands). The coverage is blurred too and cut again at a half, so small
   * gaps fill, specks drop and leaf edges merge into rounder paint shapes; the colours are blurred among the leaves
   * and each pixel snapped back to its nearest paint band (by brightness, the bands rising in it), flat, so band
   * edges are smooth shapes rather than leaf-by-leaf noise. Gaps fill only where nearly closed, so a twig or the bark
   * in front of the leaves is not painted over. Those shaders do not tone-map,
   * so neither does this composite.
   */
  /**
   * `options.layer`: the layer its meshes are drawn on (default BLUR_LAYER), so two blurs can run in one frame.
   * `options.pixels`: the radius in screen pixels instead of meters at the plant. `options.soften` (far grass): the
   * coverage is blurred and cut again at a half as in `repaint`, so a few broad strokes run together into dabs, but the
   * colours are only blurred among themselves, not snapped to bands, and finished as painted materials are.
   */
  constructor(private readonly renderer: THREE.WebGLRenderer, readonly meters: number, strength = 0.6, readonly repaint = false,
    readonly options: { layer?: number; pixels?: number; soften?: boolean } = {}) {
    this.quad.frustumCulled = false;
    this.blur = new THREE.ShaderMaterial({
      uniforms: { source: { value: null }, direction: { value: new THREE.Vector2(1, 0) }, radius: { value: 4 }, first: { value: 1 } },
      vertexShader: QUAD_VERTEX, depthTest: false, depthWrite: false,
      fragmentShader: /* glsl */`
uniform sampler2D source;
uniform vec2 direction;
uniform float radius;
uniform float first;
void main() {
  ivec2 at = ivec2(gl_FragCoord.xy), size = textureSize(source, 0);
  // Taps a pixel apart up to 3 sigma (radius is sigma), at most 48 a side.
  int reach = int(min(ceil(radius * 3.0), 48.0));
  vec4 sum = vec4(0.0);
  float weights = 0.0;
  for (int k = -48; k <= 48; k++) {
    if (k < -reach || k > reach) continue;
    ivec2 q = clamp(at + ivec2(direction * float(k)), ivec2(0), size - 1);
    vec4 c = texelFetch(source, q, 0);
    // The first pass premultiplies colour by coverage; the second reads premultiplied values.
    if (first > 0.5) c = vec4(c.rgb * c.a, c.a);
    float w = exp(-0.5 * float(k * k) / max(radius * radius, 1e-3));
    sum += c * w;
    weights += w;
  }
  gl_FragColor = sum / weights;
}`,
    });
    this.composite = new THREE.ShaderMaterial({
      uniforms: { original: { value: null }, blurred: { value: null }, strength: { value: strength }, repaint: { value: repaint ? 1 : 0 },
        soften: { value: options.soften ? 1 : 0 } },
      vertexShader: QUAD_VERTEX, depthTest: false, depthWrite: false,
      fragmentShader: /* glsl */`
uniform sampler2D original;
uniform sampler2D blurred;
uniform float strength;
uniform float repaint;
uniform float soften;
// The paint bands of leaf-normals.ts (BAND_GLSL) and its highlight (msLeafPaint), linear, darkest first.
const vec3 BANDS[8] = vec3[8](vec3(0.0232, 0.1022, 0.0844), vec3(0.0331, 0.1441, 0.0823), vec3(0.0452, 0.1779, 0.0782),
  vec3(0.0844, 0.2384, 0.0802), vec3(0.15, 0.3277, 0.0844), vec3(0.2747, 0.4793, 0.1022), vec3(0.40, 0.64, 0.12),
  vec3(0.55, 0.95, 0.06));
float luma(vec3 c) { return dot(c, vec3(0.2126, 0.7152, 0.0722)); }
void main() {
  ivec2 at = ivec2(gl_FragCoord.xy);
  vec4 here = texelFetch(original, at, 0);
  vec4 soft = texelFetch(blurred, at, 0);
  if (soften > 0.5) {
    // Coverage blurred and cut again (strokes run together, specks drop); the colours blurred among the strokes.
    if (soft.a < 0.5) discard;
    gl_FragColor = vec4(soft.rgb / soft.a, 1.0);
    #include <tonemapping_fragment>
    #include <colorspace_fragment>
    return;
  }
  if (repaint > 0.5) {
    // Re-threshold the blurred coverage at a half where there were leaves (specks and thin tips drop), but fill a gap
    // only where it is nearly closed (0.8), so a twig or the bark in front of the leaves stays. Then snap the blurred
    // colour to its nearest band by log brightness, flat, as one stroke of paint.
    if (soft.a < (here.a > 0.5 ? 0.5 : 0.8)) discard;
    vec3 c = soft.rgb / soft.a;
    float l = log(max(luma(c), 1e-4)), best = 1e9;
    vec3 paint = BANDS[0];
    for (int i = 0; i < 8; i++) {
      float d = abs(l - log(luma(BANDS[i])));
      if (d < best) { best = d; paint = BANDS[i]; }
    }
    gl_FragColor = vec4(paint, 1.0);
    #include <colorspace_fragment>
    return;
  }
  if (here.a < 0.5) discard;
  // Part blurred, part the leaf's own colour, so neighbouring colours blend while each leaf still reads.
  vec3 blended = soft.a > 1e-4 ? soft.rgb / soft.a : here.rgb;
  gl_FragColor = vec4(mix(here.rgb, blended, strength), 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}`,
    });
    // The leaves' colours are the painted materials' before finishing; the composite finishes them as they would be.
    if (repaint && !options.soften) this.composite.toneMapped = false;
    else followPaintedFinishing(this.composite);
  }

  private draw(material: THREE.Material, target: THREE.WebGLRenderTarget | null): void {
    this.quad.material = material;
    this.renderer.setRenderTarget(target);
    this.renderer.render(this.quad, this.ortho);
  }

  private resize(): void {
    const size = this.renderer.getDrawingBufferSize(new THREE.Vector2());
    if (this.target && size.equals(this.size)) return;
    this.size.copy(size);
    this.target?.dispose();
    for (const t of this.passes) t.dispose();
    const make = (depth: boolean) => {
      const t = new THREE.WebGLRenderTarget(size.x, size.y, { type: THREE.HalfFloatType, depthBuffer: depth });
      t.texture.colorSpace = THREE.LinearSRGBColorSpace;
      t.texture.minFilter = t.texture.magFilter = THREE.NearestFilter;
      t.texture.generateMipmaps = false;
      return t;
    };
    this.target = make(true);
    this.passes = [make(false), make(false)];
  }

  /** After the scene is drawn without the leaves: draws them over it, blurred among themselves. */
  render(scene: THREE.Scene, camera: THREE.PerspectiveCamera, anchor: THREE.Vector3 | null): void {
    if (!anchor) return;
    this.resize();
    const renderer = this.renderer, target = this.target!;
    const previous = renderer.getRenderTarget(), autoClear = renderer.autoClear, background = scene.background;
    const clear = renderer.getClearColor(new THREE.Color()), clearAlpha = renderer.getClearAlpha();
    const layers = camera.layers.mask, override = scene.overrideMaterial;
    try {
      scene.background = null;
      renderer.setClearColor(0x000000, 0);
      renderer.setRenderTarget(target);
      renderer.clear(true, true, false);
      renderer.autoClear = false;
      // Depth of everything else, then the leaves. Everything else draws with its own materials (grass and bark place
      // their vertices in their shaders) with colour writes locked off.
      const colorBuffer = renderer.state.buffers.color;
      colorBuffer.setMask(false);
      colorBuffer.setLocked(true);
      renderer.render(scene, camera);
      colorBuffer.setLocked(false);
      colorBuffer.setMask(true);
      camera.layers.set(this.options.layer ?? BLUR_LAYER);
      renderer.render(scene, camera);
      camera.layers.mask = layers;
      // The radius in pixels at the plant.
      const distance = anchor.distanceTo(camera.position);
      const pixelsPerMeter = this.size.y / (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2) * Math.max(distance, 1e-3));
      const radius = this.options.pixels ? this.options.pixels : Math.min(Math.max(this.meters * pixelsPerMeter, 0.5), 16);
      this.blur.uniforms.radius.value = radius;
      this.last = { radius, pixelsPerMeter, distance };
      this.blur.uniforms.source.value = target.texture;
      this.blur.uniforms.direction.value.set(1, 0);
      this.blur.uniforms.first.value = 1;
      this.draw(this.blur, this.passes[0]);
      this.blur.uniforms.source.value = this.passes[0].texture;
      this.blur.uniforms.direction.value.set(0, 1);
      this.blur.uniforms.first.value = 0;
      this.draw(this.blur, this.passes[1]);
      this.composite.uniforms.original.value = target.texture;
      this.composite.uniforms.blurred.value = this.passes[1].texture;
      this.draw(this.composite, null);
    } finally {
      scene.overrideMaterial = override;
      camera.layers.mask = layers;
      scene.background = background;
      renderer.setClearColor(clear, clearAlpha);
      renderer.autoClear = autoClear;
      renderer.setRenderTarget(previous);
    }
  }

  dispose(): void {
    this.target?.dispose();
    for (const t of this.passes) t.dispose();
    this.blur.dispose(); this.composite.dispose(); this.depthOnly.dispose();
    this.quad.geometry.dispose();
  }
}
