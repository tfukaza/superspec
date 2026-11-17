import * as THREE from "three";
import { followPaintedFinishing } from "./shade.ts";

/**
 * Stroke edges (prototype, `?strokes=1`): a plant's canopy masses (the `hull` family: TreeSpec's clumps and sprigs, or
 * its one hull) are painted as 2D strokes whose edges a leaf brush redraws, as a painter's foliage brush stamps leaf
 * clumps along a stroke's edge.
 *
 * 1. The scene draws without the strokes (they live on `STROKE_LAYER` alone).
 * 2. The strokes are drawn into an offscreen target over a prepass of everything else's depth (drawn as occluders), with
 *    their banded colour, and in alpha an id per stroke (`0` nothing, `255` an occluder).
 * 3. Every pixel where the id changes is a seed; a jump flood gives each pixel its nearest seed.
 * 4. A composite over the canvas: near an edge, the front stroke (the nearer of the two sides) takes the pixel where its
 *    signed distance in pixels over `band`, plus the leaf brush about its middle times `amount`, is positive. So the
 *    stroke bites into itself and stands out past its outline in the shapes of the brush's leaf clumps, across a band
 *    as wide as asked, not the few pixels a 3D surface turns edge-on over. The other side's colour is read across the
 *    edge (mirrored through the nearest seed); background shows where neither stroke holds the pixel. Edges against
 *    occluders stay hard. The brush is read in screen pixels from the plant's projected base, scaled by its distance,
 *    so it stays on the plant as the camera moves rather than on the screen.
 */
export const STROKE_LAYER = 8;

const QUAD_VERTEX = /* glsl */`void main() { gl_Position = vec4(position.xy, 0.0, 1.0); }`;

export class StrokeEdges {
  private target: THREE.WebGLRenderTarget | null = null;
  private seeds: THREE.WebGLRenderTarget[] = [];
  private size = new THREE.Vector2();
  private readonly quad = new THREE.Mesh(new THREE.PlaneGeometry(2, 2));
  private readonly ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly occluder = new THREE.MeshBasicMaterial({ color: 0x000000 });
  private readonly seed: THREE.ShaderMaterial;
  private readonly jump: THREE.ShaderMaterial;
  readonly composite: THREE.ShaderMaterial;
  lastSeen = 0;

  /** `band`: meters on each side of an edge the brush redraws; `amount`: how far its leaf clumps reach across that band. */
  constructor(private readonly renderer: THREE.WebGLRenderer, brush: THREE.Texture, readonly options: { band: number; amount: number; tile: number; bite: number; bands: number }) {
    this.quad.frustumCulled = false;
    this.seed = new THREE.ShaderMaterial({
      uniforms: { strokes: { value: null } }, vertexShader: QUAD_VERTEX, depthTest: false, depthWrite: false,
      fragmentShader: /* glsl */`
uniform sampler2D strokes;
float id(ivec2 q) { return floor(texelFetch(strokes, q, 0).a * 255.0 + 0.5); }
void main() {
  ivec2 at = ivec2(gl_FragCoord.xy), size = textureSize(strokes, 0);
  float here = id(at);
  bool edge = false;
  for (int i = 0; i < 4; i++) {
    ivec2 q = at + (i == 0 ? ivec2(1, 0) : i == 1 ? ivec2(-1, 0) : i == 2 ? ivec2(0, 1) : ivec2(0, -1));
    if (any(lessThan(q, ivec2(0))) || any(greaterThanEqual(q, size))) continue;
    if (id(q) != here) edge = true;
  }
  gl_FragColor = edge ? vec4(gl_FragCoord.xy, 0.0, 1.0) : vec4(-1.0);
}`,
    });
    this.jump = new THREE.ShaderMaterial({
      uniforms: { seeds: { value: null }, step: { value: 1 } }, vertexShader: QUAD_VERTEX, depthTest: false, depthWrite: false,
      fragmentShader: /* glsl */`
uniform highp sampler2D seeds;
uniform int step;
void main() {
  ivec2 at = ivec2(gl_FragCoord.xy), size = textureSize(seeds, 0);
  vec4 best = texelFetch(seeds, at, 0);
  float nearest = best.a > 0.5 ? distance(best.xy, gl_FragCoord.xy) : 1e20;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) {
    ivec2 q = at + ivec2(x, y) * step;
    if (any(lessThan(q, ivec2(0))) || any(greaterThanEqual(q, size))) continue;
    vec4 seed = texelFetch(seeds, q, 0);
    if (seed.a < 0.5) continue;
    float d = distance(seed.xy, gl_FragCoord.xy);
    if (d < nearest) { nearest = d; best = seed; }
  }
  gl_FragColor = best;
}`,
    });
    this.composite = new THREE.ShaderMaterial({
      uniforms: {
        strokes: { value: null }, depths: { value: null }, seeds: { value: null }, brush: { value: brush },
        // x, y: the plant's base on screen (pixels); z: pixels per brush tile.
        anchor: { value: new THREE.Vector3(0, 0, 512) },
        band: { value: options.band }, amount: { value: options.amount }, bite: { value: options.bite }, bands: { value: options.bands },
        debug: { value: typeof location !== "undefined" && new URLSearchParams(location.search).get("strokedebug") === "1" ? 1 : 0 },
      },
      vertexShader: QUAD_VERTEX, depthTest: false, depthWrite: false, transparent: false,
      fragmentShader: /* glsl */`
uniform sampler2D strokes;
uniform highp sampler2D depths;
uniform highp sampler2D seeds;
uniform sampler2D brush;
uniform vec3 anchor;
uniform float band;
uniform float amount;
uniform float debug;
uniform float bite;
uniform float bands;
float idOf(vec4 c) { return floor(c.a * 255.0 + 0.5); }
void main() {
  ivec2 at = ivec2(gl_FragCoord.xy), size = textureSize(strokes, 0);
  vec4 here = texelFetch(strokes, at, 0);
  float idHere = idOf(here);
  bool strokeHere = idHere > 0.5 && idHere < 254.5;
  vec4 seed = texelFetch(seeds, at, 0);
  float d = seed.a > 0.5 ? distance(seed.xy, gl_FragCoord.xy) : 1e20;
  vec4 color = here;
  if (d < band * (1.0 + amount)) {
    // The other side: mirrored through the nearest edge pixel, one pixel past it.
    vec2 away = d > 0.5 ? (seed.xy - gl_FragCoord.xy) / d : vec2(0.0);
    ivec2 across = clamp(ivec2(seed.xy + away * (d + 1.0)), ivec2(0), size - 1);
    if (d < 0.5) {
      // An edge pixel is its own nearest seed: the other side is a neighbour with another id, a stroke first.
      bool found = false;
      for (int i = 0; i < 4; i++) {
        ivec2 q = clamp(at + (i == 0 ? ivec2(1, 0) : i == 1 ? ivec2(-1, 0) : i == 2 ? ivec2(0, 1) : ivec2(0, -1)), ivec2(0), size - 1);
        float other = idOf(texelFetch(strokes, q, 0));
        if (other != idHere && (!found || (other > 0.5 && other < 254.5))) { across = q; found = true; }
      }
    }
    vec4 there = texelFetch(strokes, across, 0);
    float idThere = idOf(there);
    bool strokeThere = idThere > 0.5 && idThere < 254.5;
    if (idHere < 254.5 && idThere < 254.5 && (strokeHere || strokeThere)) {
      // Two bands of one stroke: the lighter is painted over the darker, as a painter lays light over shade. Two
      // strokes: the nearer is in front.
      bool sameStroke = strokeHere && strokeThere && floor((idHere - 1.0) / bands) == floor((idThere - 1.0) / bands);
      bool hereFront = strokeHere && (!strokeThere || (sameStroke
        ? mod(idHere - 1.0, bands) > mod(idThere - 1.0, bands)
        : texelFetch(depths, at, 0).r <= texelFetch(depths, across, 0).r));
      float inside = (hereFront ? d : -d) / band;
      float leaves = texture(brush, (gl_FragCoord.xy - anchor.xy) / anchor.z).r;
      // Leaf clumps stand out past the front stroke's edge across the whole band, but bite into it only a share of the
      // band deep (bite), so a narrow stroke (a sprig a few pixels wide) is notched at its edge, not holed through.
      bool front = hereFront && d >= bite * band ? true : inside + (leaves - 0.5) * 2.0 * amount > 0.0;
      // A background crack a pixel or two wide between strokes is closed, not drawn as a line of sky.
      if (!strokeHere && d < 1.5) front = true;
      color = front == hereFront ? here : there;
    }
  }
  float id = idOf(color);
  if (debug > 0.5) {
    // Ids as colours; red where a stroke pixel ends as background, white seeds.
    vec3 hue = fract(vec3(0.13, 0.37, 0.71) * idHere * 7.0);
    gl_FragColor = vec4(seed.a > 0.5 && d < 0.6 ? vec3(1.0) : strokeHere && id < 0.5 ? vec3(1.0, 0.0, 0.0) : idHere < 0.5 ? vec3(0.0) : idHere > 254.5 ? vec3(0.3) : hue, 1.0);
    return;
  }
  if (id < 0.5 || id > 254.5) discard;
  gl_FragColor = vec4(color.rgb, 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}`,
    });
    // The strokes' colours are the painted materials' before finishing; the composite finishes them as they would be.
    followPaintedFinishing(this.composite);
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
    for (const t of this.seeds) t.dispose();
    const depthTexture = new THREE.DepthTexture(size.x, size.y);
    depthTexture.type = THREE.FloatType;
    this.target = new THREE.WebGLRenderTarget(size.x, size.y, { type: THREE.HalfFloatType, depthBuffer: true, depthTexture });
    this.target.texture.colorSpace = THREE.LinearSRGBColorSpace;
    this.target.texture.minFilter = this.target.texture.magFilter = THREE.NearestFilter;
    this.target.texture.generateMipmaps = false;
    const type = this.renderer.extensions.has("EXT_color_buffer_float") ? THREE.FloatType : THREE.HalfFloatType;
    this.seeds = [0, 1].map(() => {
      const t = new THREE.WebGLRenderTarget(size.x, size.y, { type, depthBuffer: false });
      t.texture.minFilter = t.texture.magFilter = THREE.NearestFilter;
      t.texture.generateMipmaps = false;
      return t;
    });
  }

  /**
   * After the scene is drawn to the canvas without the strokes: draws the strokes' edges over it. `anchor` is a point on
   * the plant (its base) and `tile` the brush's meters per tile there. Returns false when no stroke is in view.
   */
  render(scene: THREE.Scene, camera: THREE.PerspectiveCamera, anchor: THREE.Vector3 | null, tile: number): boolean {
    if (!anchor) return false;
    this.resize();
    const target = this.target!;
    const renderer = this.renderer;
    const previous = renderer.getRenderTarget(), autoClear = renderer.autoClear, background = scene.background;
    const clear = renderer.getClearColor(new THREE.Color()), clearAlpha = renderer.getClearAlpha();
    const layers = camera.layers.mask, override = scene.overrideMaterial;
    try {
      // Occluders: everything but the strokes, black with id 255, for depth.
      scene.background = null;
      renderer.setClearColor(0x000000, 0);
      renderer.setRenderTarget(target);
      renderer.clear(true, true, false);
      renderer.autoClear = false;
      // A plant's own bark and leaves are not occluders: the strokes paint over them as a painter's would (and their
      // thin twigs, drawn whole here and antialiased on the canvas, would leave background lines through the strokes).
      const hidden: THREE.Object3D[] = [];
      // Leaves stay occluders: the leaf billboards at shoot tips and in sparse places are drawn over the strokes.
      scene.traverse(object => {
        const family = object.userData.foliage?.family;
        if (object.visible && (family === "bark" || family === "hull" || family === "shadow")) { hidden.push(object); object.visible = false; }
      });
      scene.overrideMaterial = this.occluder;
      renderer.render(scene, camera);
      scene.overrideMaterial = override;
      for (const object of hidden) object.visible = true;
      // The strokes, their colour and id.
      camera.layers.set(STROKE_LAYER);
      renderer.render(scene, camera);
      camera.layers.mask = layers;
      // Seeds where the id changes, then the jump flood.
      this.seed.uniforms.strokes.value = target.texture;
      this.draw(this.seed, this.seeds[0]);
      let [from, to] = this.seeds;
      for (let step = 2 ** Math.ceil(Math.log2(Math.max(this.size.x, this.size.y))) / 2; step >= 1; step /= 2) {
        this.jump.uniforms.seeds.value = from.texture;
        this.jump.uniforms.step.value = step;
        this.draw(this.jump, to);
        [from, to] = [to, from];
      }
      // The brush anchored at the plant's base on screen, its tile as many pixels as `tile` meters measure there.
      const projected = anchor.clone().project(camera);
      const distance = anchor.distanceTo(camera.position);
      const pixelsPerMeter = this.size.y / (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2) * Math.max(distance, 1e-3));
      this.composite.uniforms.anchor.value.set((projected.x * 0.5 + 0.5) * this.size.x, (projected.y * 0.5 + 0.5) * this.size.y, tile * pixelsPerMeter);
      this.composite.uniforms.band.value = Math.max(this.options.band * pixelsPerMeter, 1);
      this.composite.uniforms.amount.value = this.options.amount;
      this.composite.uniforms.bite.value = this.options.bite;
      this.composite.uniforms.strokes.value = target.texture;
      this.composite.uniforms.depths.value = target.depthTexture;
      this.composite.uniforms.seeds.value = from.texture;
      this.draw(this.composite, null);
    } finally {
      scene.overrideMaterial = override;
      camera.layers.mask = layers;
      scene.background = background;
      renderer.setClearColor(clear, clearAlpha);
      renderer.autoClear = autoClear;
      renderer.setRenderTarget(previous);
    }
    return true;
  }

  dispose(): void {
    this.target?.dispose();
    for (const t of this.seeds) t.dispose();
    this.seed.dispose(); this.jump.dispose(); this.composite.dispose(); this.occluder.dispose();
    this.quad.geometry.dispose();
  }
}
