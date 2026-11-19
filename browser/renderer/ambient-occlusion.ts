import * as THREE from "three";
import { EffectComposer } from "three/examples/jsm/postprocessing/EffectComposer.js";
import { GTAOPass } from "three/examples/jsm/postprocessing/GTAOPass.js";
import { RenderPass } from "three/examples/jsm/postprocessing/RenderPass.js";
import { ShaderPass } from "three/examples/jsm/postprocessing/ShaderPass.js";
import { SimplexNoise } from "three/examples/jsm/math/SimplexNoise.js";
import { ContourPass, type ContourSource } from "./contours.ts";
import { LitEdges } from "./lit-edges.ts";
import type { AmbientOcclusionSettings, Contours } from "./lighting.ts";

/** The lighting record's ambient occlusion values; whether the effect is on (`enabled`) is its host's decision. */
type AmbientOcclusionParameters = Pick<AmbientOcclusionSettings, "radius" | "intensity" | "thickness">;

/**
 * The last pass: the renderer's own tone mapping, exposure and output color space (three.js applies them to a material
 * drawn to the canvas through these two chunks, exactly as it does to a scene material), then the backdrop behind
 * whatever the scene did not cover. The scene target is premultiplied by coverage after its multisample resolve.
 * The optional edge filter measures contrast in the displayed image, including its backdrop.
 */
const OUTPUT = {
  uniforms: { tDiffuse: { value: null }, backdrop: { value: new THREE.Vector3() }, edgeTexel: { value: new THREE.Vector2() }, edgeThreshold: { value: 0.12 } },
  vertexShader: `varying vec2 vUv; void main() { vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`,
  fragmentShader: `
uniform sampler2D tDiffuse;
uniform vec3 backdrop;
varying vec2 vUv;
#ifdef EDGE_BLUR
uniform vec2 edgeTexel;
uniform float edgeThreshold;
#endif
vec3 displayColor(vec2 uv) {
  vec4 texel = texture2D(tDiffuse, uv);
  float coverage = clamp(texel.a, 0.0, 1.0);
  vec3 color = coverage > 0.0 ? texel.rgb / coverage : vec3(0.0);
  #ifdef TONE_MAPPING
    color = toneMapping(color);
  #endif
  return mix(backdrop, linearToOutputTexel(vec4(color, 1.0)).rgb, coverage);
}
#ifdef EDGE_BLUR
float contrast(vec3 a, vec3 b) {
  vec3 delta = abs(a - b);
  return max(delta.r, max(delta.g, delta.b));
}
#endif
void main() {
  vec3 color = displayColor(vUv);
  #ifdef EDGE_BLUR
    // Soften across the stronger gradient using taps one device pixel away.
    vec2 dx = vec2(edgeTexel.x, 0.0), dy = vec2(0.0, edgeTexel.y);
    vec3 left = displayColor(vUv - dx), right = displayColor(vUv + dx);
    vec3 down = displayColor(vUv - dy), up = displayColor(vUv + dy);
    float horizontal = max(contrast(color, left), contrast(color, right));
    float vertical = max(contrast(color, down), contrast(color, up));
    float wx = horizontal*horizontal, wy = vertical*vertical;
    vec3 neighbors = mix(down + up, left + right, wx / max(wx + wy, 1e-8));
    float amount = 0.4 * smoothstep(edgeThreshold, edgeThreshold + 0.08, max(horizontal, vertical));
    color = mix(color, 0.5 * neighbors, amount);
  #endif
  gl_FragColor = vec4(color, 1.0);
}`,
};

/**
 * Materials that are not tone mapped (`toneMapped` false: painted programs and painted foliage follow the painted
 * finishing this way, shade.ts) come out of the scene target through the output pass's tone mapping all the same, which
 * drawn straight to the canvas they skip: their paint read paler and yellower with ambient occlusion on. So while this
 * pass draws, such a material's colour is put through the inverse of the renderer's tone mapping, at the renderer's
 * exposure, before it is written: the output pass then gives back exactly the colour it paints on the canvas (up to the
 * half-float target and colours past what the curve reaches). Elsewhere (`mode` 0) the colour is left as it is. Exact
 * for linear, Reinhard and ACES filmic; neutral is inverted by iteration; other curves are left as they are.
 */
const untoned = { mode: { value: 0 }, exposure: { value: 1 } };
const UNTONED_MODES = new Map<THREE.ToneMapping, number>([
  [THREE.LinearToneMapping, 1], [THREE.ReinhardToneMapping, 2], [THREE.ACESFilmicToneMapping, 3], [THREE.NeutralToneMapping, 4],
]);
const UNTONED_GLSL = /* glsl */`
uniform float msUntonedMode;
uniform float msUntonedExposure;
vec3 msNeutralCurve(vec3 color) {
  const float start = 0.8 - 0.04;
  const float desaturation = 0.15;
  color *= msUntonedExposure;
  float x = min(color.r, min(color.g, color.b));
  float offset = x < 0.08 ? x - 6.25 * x * x : 0.04;
  color -= offset;
  float peak = max(color.r, max(color.g, color.b));
  if (peak < start) return color;
  float d = 1. - start;
  float newPeak = 1. - d * d / (peak + d - start);
  color *= newPeak / peak;
  float g = 1. - 1. / (desaturation * (peak - newPeak) + 1.);
  return mix(color, vec3(newPeak), g);
}
vec3 msUntonedInverse(vec3 color) {
  if (msUntonedMode < 0.5) return color;
  float exposure = max(msUntonedExposure, 1e-4);
  vec3 y = clamp(color, 0.0, 0.985);
  if (msUntonedMode < 1.5) return y / exposure;
  if (msUntonedMode < 2.5) return y / (1.0 - y) / exposure;
  if (msUntonedMode < 3.5) {
    // three.js's ACES filmic: out * fit(in * colour * exposure / 0.6), its fit a ratio of quadratics solved per channel.
    const mat3 inputMat = mat3(vec3(0.59719, 0.07600, 0.02840), vec3(0.35458, 0.90834, 0.13383), vec3(0.04823, 0.01566, 0.83777));
    const mat3 outputMat = mat3(vec3(1.60475, -0.10208, -0.00327), vec3(-0.53108, 1.10813, -0.07276), vec3(-0.07367, -0.00605, 1.07602));
    vec3 fit = clamp(inverse(outputMat) * y, 0.0, 0.985);
    vec3 a = 1.0 - 0.983729 * fit, b = 0.0245786 - 0.4329510 * fit, c = -(0.000090537 + 0.238081 * fit);
    vec3 v = (-b + sqrt(max(b * b - 4.0 * a * c, 0.0))) / (2.0 * a);
    return max(inverse(inputMat) * v, 0.0) * 0.6 / exposure;
  }
  // Neutral: its curve is the identity below its shoulder and flattens above, so stepping by what is missing converges.
  vec3 x = y;
  for (int i = 0; i < 16; i++) x = max(x + (y - msNeutralCurve(x)), 0.0);
  return x;
}
`;

/** Makes a material put its colour through `msUntonedInverse` while it is not tone mapped (see `untoned`). */
function prepareUntoned(material: THREE.Material): void {
  if (material.userData.untonedOutput) return;
  material.userData.untonedOutput = true;
  const previous = material.onBeforeCompile, key = material.customProgramCacheKey;
  // The default key is the hook's source: keep the original hook's, so two materials with different hooks stay apart.
  const previousKey = key === THREE.Material.prototype.customProgramCacheKey ? previous.toString() : null;
  material.onBeforeCompile = function (shader, renderer) {
    previous.call(this, shader, renderer);
    if (material.toneMapped || !shader.fragmentShader.includes("#include <colorspace_fragment>")) return;
    shader.uniforms.msUntonedMode = untoned.mode;
    shader.uniforms.msUntonedExposure = untoned.exposure;
    shader.fragmentShader = UNTONED_GLSL + shader.fragmentShader.replaceAll("#include <colorspace_fragment>",
      "gl_FragColor.rgb = msUntonedInverse(gl_FragColor.rgb);\n#include <colorspace_fragment>");
  };
  material.customProgramCacheKey = () => `${previousKey ?? key.call(material)}|untoned:${material.toneMapped}`;
  material.needsUpdate = true;
}

/**
 * Cosines of the angle between a vertex normal and its face's own normal: at `NORMAL_KEPT` and above the vertex normal
 * is what the occlusion reads, at `NORMAL_REPLACED` and below the face's, with a blend between. A smoothed normal of a
 * solid stays well above (a hexagonal tube's is at 0.87).
 */
const NORMAL_KEPT = 0.8, NORMAL_REPLACED = 0.5;

/**
 * Makes the normal the occlusion reads the normal of the surface THE CAMERA SEES, whatever the mesh stores: turned to
 * the camera's side of the face, and the face's own normal (from the depth's slope) where the vertex normal is far
 * from it. Without this a mesh's stored normals decide the occlusion in two ways that are not about its shape:
 *
 * - A two-sided part delivered with both faces (foliage: each triangle twice, wound both ways, sharing vertices, so no
 *   renderer turns its shade normals over) has two coincident faces under this two-sided material. The one wound away
 *   from the camera is drawn with its normal turned over, and being drawn last it is the one kept: a normal pointing
 *   away from the camera, which the occlusion reads as a surface that sees nothing. Such foliage came out nearly black.
 * - Vertex normals that are shade normals (a leaf cluster lit as part of a canopy) say nothing of the leaf's own
 *   facing. The same foliage flattened into one mesh (shade normals in its vertices) and as an instanced batch (the
 *   cluster's geometric normals) then got different occlusion.
 *
 * A solid's smoothed normals are on the camera's side of their faces and near them, so they are read as they are.
 */
function faceNormals(material: THREE.Material): void {
  material.onBeforeCompile = shader => {
    const vertex = "#include <project_vertex>", fragment = "#include <normal_fragment_begin>";
    // If three.js changes what this patches, the pass keeps its own normals.
    if (!shader.vertexShader.includes(vertex) || !shader.fragmentShader.includes(fragment)) return;
    shader.vertexShader = `varying vec3 msViewPoint;\n${shader.vertexShader.replace(vertex, `${vertex}\n  msViewPoint = mvPosition.xyz;`)}`;
    shader.fragmentShader = `varying vec3 msViewPoint;\n${shader.fragmentShader.replace(fragment, `${fragment}
  {
    // The face's own normal in view space: it is toward the camera whichever way the triangle is wound.
    vec3 msFace = cross( dFdx( msViewPoint ), dFdy( msViewPoint ) );
    float msArea = dot( msFace, msFace );
    if ( msArea > 0.0 ) {
      msFace *= inversesqrt( msArea );
      float msAlong = dot( normal, msFace );
      normal = normalize( mix( msFace, sign( msAlong ) * normal, smoothstep( ${NORMAL_REPLACED.toFixed(2)}, ${NORMAL_KEPT.toFixed(2)}, abs( msAlong ) ) ) );
    }
  }`)}`;
  };
  material.customProgramCacheKey = () => "ms-face-normals";
}

/**
 * The denoise's rotation noise as the pass makes it (four channels of simplex noise over a repeating 64 by 64 tile),
 * from a fixed sequence instead of Math.random(): the same texture in every session.
 */
function fixedNoise(size = 64): THREE.DataTexture {
  let state = 0x2f6e2b1;
  // mulberry32: a small generator with a fixed seed.
  const random = (): number => {
    state = (state + 0x6d2b79f5) | 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const simplex = new SimplexNoise({ random });
  const data = new Uint8Array(size * size * 4);
  for (let i = 0; i < size; i++) for (let j = 0; j < size; j++) {
    const at = (i * size + j) * 4;
    data[at] = (simplex.noise(i, j) * 0.5 + 0.5) * 255;
    data[at + 1] = (simplex.noise(i + size, j) * 0.5 + 0.5) * 255;
    data[at + 2] = (simplex.noise(i, j + size) * 0.5 + 0.5) * 255;
    data[at + 3] = (simplex.noise(i + size, j + size) * 0.5 + 0.5) * 255;
  }
  const texture = new THREE.DataTexture(data, size, size, THREE.RGBAFormat, THREE.UnsignedByteType);
  texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  texture.needsUpdate = true;
  return texture;
}

/**
 * Screen-space ambient occlusion for a scene view: soft darkening in creases and where objects meet, from the depth and
 * geometric normals the camera sees (three.js's GTAO pass with its denoise; `faceNormals` makes its normals those). A global rendering setting of the scene
 * (`ambientOcclusion` in the lighting record), never part of a material; the planned baked `occlusion()` material
 * field is a different thing.
 *
 * The scene is drawn into a multisampled linear half-float target (so edges stay antialiased), the occlusion multiplies
 * it, and an output pass applies the renderer's tone mapping, exposure and output color space, as drawing straight to
 * the canvas would. Materials therefore compile for this target, not for the canvas: anything that compiles a scene
 * material ahead of a frame must do it inside `compiling`.
 *
 * The same composited frame carries the scene's contour lines (`setContours`), with the occlusion or, its passes left
 * out, without it.
 *
 * It draws only when `render` is called, so an on-demand viewer stays idle between frames. A relief normal in a
 * material does not affect the occlusion.
 */
export class AmbientOcclusion {
  private readonly composer: EffectComposer;
  private readonly pass: GTAOPass;
  private readonly contact: GTAOPass;
  /** Whether materials that are not tone mapped are kept so (see `untoned`); off only to compare pictures with it. */
  static invertUntoned = true;

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
    parameters: AmbientOcclusionParameters | null, excluded: () => THREE.Object3D[] = () => [], contours: Contours | null = null,
    edgeThreshold: number | null = null) {
    const size = renderer.getSize(new THREE.Vector2());
    const target = new THREE.WebGLRenderTarget(Math.max(1, size.x), Math.max(1, size.y), { type: THREE.HalfFloatType, samples: 4 });
    this.composer = new EffectComposer(renderer, target);
    this.composer.addPass(new RenderPass(scene, camera));
    this.pass = new GTAOPass(scene, camera, size.x, size.y);
    this.foliageMask = this.prepare(this.pass, camera, excluded);
    // A second, short-range pass for contact: where a duct, a pipe or a sill meets a wall the broad pass (its radius
    // tuned for the large forms) spreads its few samples too widely to darken the narrow strip between them. Its radius
    // and thickness are a fraction of the record's, so it only darkens creases and contact lines a few centimetres wide
    // and leaves no halo round a small prop.
    this.contact = new GTAOPass(scene, camera, size.x, size.y);
    this.contactMask = this.prepare(this.contact, camera, excluded);
    // It reads the depth, normals and foliage mask the broad pass drew just before it, the same view drawn the same way,
    // rather than drawing the scene and its foliage again (a third of the frame's draw calls in the building scene). Its
    // own targets are never drawn into, so they take no memory. (`?aoshare=off` draws them again, to compare.)
    if (typeof location === "undefined" || new URLSearchParams(location.search).get("aoshare") !== "off") {
      this.contact.setGBuffer(this.pass.depthTexture, this.pass.normalTexture);
      const broad = (this.pass as unknown as { blendMaterial: THREE.ShaderMaterial }).blendMaterial.uniforms;
      const close = (this.contact as unknown as { blendMaterial: THREE.ShaderMaterial }).blendMaterial.uniforms;
      close.tFoliage = broad.tFoliage;
      close.hasFoliage = broad.hasFoliage;
    }
    this.composer.addPass(this.pass);
    this.composer.addPass(this.contact);
    this.camera = camera;
    this.setContours(contours);
    this.output = new ShaderPass(OUTPUT);
    if (edgeThreshold !== null) {
      this.output.material.defines.EDGE_BLUR = 1;
      this.output.uniforms.edgeThreshold.value = edgeThreshold;
    }
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
    // Materials that are not tone mapped write the colour the output pass's tone mapping gives back (see `untoned`).
    this.scene.traverse(object => {
      const material = (object as THREE.Mesh).isMesh ? (object as THREE.Mesh).material : undefined;
      for (const one of Array.isArray(material) ? material : material ? [material] : []) if (!one.toneMapped) prepareUntoned(one);
    });
    untoned.mode.value = AmbientOcclusion.invertUntoned ? UNTONED_MODES.get(this.renderer.toneMapping) ?? 0 : 0;
    untoned.exposure.value = this.renderer.toneMappingExposure;
    this.geometryDrawn = false;
    try { this.composer.render(); } finally {
      untoned.mode.value = 0;
      this.renderer.setClearColor(clear, alpha);
      if (color) this.scene.background = color;
    }
  }

  /** Where foliage is the nearest surface, in its alpha, for each pass (see `prepare`). */
  private readonly foliageMask: THREE.WebGLRenderTarget;
  private readonly contactMask: THREE.WebGLRenderTarget;

  /**
   * Sets an occlusion pass up for this view, and returns its foliage mask:
   * - The denoise turns its sample pattern by a noise texture the pass draws from Math.random() when it is made, so two
   *   runs of one scene differed in a fifth or more of their pixels. A fixed texture makes pictures comparable.
   * - Both faces of a thin part write depth and a normal, as the scene's materials draw them, and the normals are the
   *   faces the camera sees (`faceNormals`).
   * - Foliage is drawn by shaders of its own (leaf cards stood up and bent in their vertex shaders), which the pass's one
   *   override material does not run: drawn with it, a canopy's cards lie where their raw vertices are and darken the
   *   building through the tree. So foliage is left out of the depth and normals (it casts no occlusion), and drawn
   *   instead, with its own materials, into a mask against that depth: where a leaf is in front, the image is not
   *   darkened (it receives none either).
   */
  private prepare(pass: GTAOPass, camera: THREE.Camera, excluded: () => THREE.Object3D[]): THREE.WebGLRenderTarget {
    const mask = new THREE.WebGLRenderTarget(1, 1, { type: THREE.UnsignedByteType, depthBuffer: false });
    const denoise = pass as unknown as { pdNoiseTexture: THREE.Texture; pdMaterial: THREE.ShaderMaterial };
    denoise.pdNoiseTexture.dispose();
    denoise.pdNoiseTexture = fixedNoise();
    denoise.pdMaterial.uniforms.tNoise.value = denoise.pdNoiseTexture;
    pass.normalMaterial.side = THREE.DoubleSide;
    faceNormals(pass.normalMaterial);
    const internals = pass as unknown as { _overrideVisibility(): void; _restoreVisibility(): void; normalRenderTarget: THREE.WebGLRenderTarget; blendMaterial: THREE.ShaderMaterial };
    const hide = internals._overrideVisibility.bind(pass), restore = internals._restoreVisibility.bind(pass);
    let hidden: THREE.Object3D[] = [], leaves: THREE.Object3D[] = [];
    const scene = this.scene;
    internals._overrideVisibility = () => {
      hide();
      hidden = excluded().filter(object => object.visible);
      leaves = [];
      scene.traverse(object => { if (object.visible && (object as THREE.Mesh).isMesh && object.userData.foliage) leaves.push(object); });
      for (const object of [...hidden, ...leaves]) object.visible = false;
    };
    internals._restoreVisibility = () => {
      for (const object of hidden) object.visible = true;
      for (const object of leaves) object.visible = true;
      this.drawFoliageMask(pass, mask, leaves, camera, internals.normalRenderTarget);
      hidden = []; leaves = [];
      restore();
    };
    // The blend leaves the image as it is where the mask holds a leaf.
    const blend = internals.blendMaterial;
    blend.uniforms.tFoliage = { value: mask.texture };
    blend.uniforms.hasFoliage = { value: 0 };
    blend.fragmentShader = `
uniform float intensity;
uniform sampler2D tDiffuse;
uniform sampler2D tFoliage;
uniform float hasFoliage;
varying vec2 vUv;
void main() {
  vec4 texel = texture2D( tDiffuse, vUv );
  float leaf = hasFoliage * texture2D( tFoliage, vUv ).a;
  gl_FragColor = vec4( mix( mix( vec3( 1. ), texel.rgb, intensity ), vec3( 1. ), leaf ), texel.a );
}`;
    blend.needsUpdate = true;
    const resize = pass.setSize.bind(pass);
    pass.setSize = (width: number, height: number) => { resize(width, height); mask.setSize(width, height); };
    return mask;
  }

  /**
   * Draws `leaves` with their own materials into a pass's foliage mask, depth-tested (not written) against the depth the
   * pass drew without them, so only leaves in front of what the occlusion sees mark it. The shadow maps are not redrawn
   * for this draw (it shows only the leaves); nothing else of the renderer's state is changed.
   */
  private drawFoliageMask(pass: GTAOPass, mask: THREE.WebGLRenderTarget, leaves: THREE.Object3D[], camera: THREE.Camera, depth: THREE.WebGLRenderTarget): void {
    const blend = (pass as unknown as { blendMaterial: THREE.ShaderMaterial }).blendMaterial;
    blend.uniforms.hasFoliage.value = leaves.length ? 1 : 0;
    if (!leaves.length || !depth.depthTexture) return;
    if (mask.depthTexture !== depth.depthTexture) {
      mask.dispose();
      mask.depthBuffer = true;
      mask.depthTexture = depth.depthTexture;
    }
    if (mask.width !== depth.width || mask.height !== depth.height) mask.setSize(depth.width, depth.height);
    const renderer = this.renderer, scene = this.scene;
    const shown: THREE.Object3D[] = [];
    const wanted = new Set(leaves);
    scene.traverse(object => { if (object.visible && (object as THREE.Mesh).isMesh && !wanted.has(object)) { shown.push(object); object.visible = false; } });
    const background = scene.background, before = renderer.getRenderTarget(), clear = renderer.getClearColor(this.clearColor), alpha = renderer.getClearAlpha();
    const autoClear = renderer.autoClear, shadows = renderer.shadowMap.autoUpdate, needs = renderer.shadowMap.needsUpdate;
    try {
      scene.background = null;
      renderer.shadowMap.autoUpdate = false; renderer.shadowMap.needsUpdate = false;
      renderer.setRenderTarget(mask);
      renderer.setClearColor(0x000000, 0);
      renderer.clear(true, false, false);
      renderer.autoClear = false;
      // Only colour is cleared: the depth is the occlusion pass's, which the leaves are tested against.
      renderer.state.buffers.depth.setLocked(false);
      renderer.state.buffers.depth.setMask(false);
      renderer.state.buffers.depth.setLocked(true);
      renderer.render(scene, camera);
    } finally {
      renderer.state.buffers.depth.setLocked(false);
      renderer.state.buffers.depth.setMask(true);
      scene.background = background;
      renderer.shadowMap.autoUpdate = shadows; renderer.shadowMap.needsUpdate = needs;
      renderer.autoClear = autoClear;
      renderer.setClearColor(clear, alpha);
      renderer.setRenderTarget(before);
      for (const object of shown) object.visible = true;
    }
  }

  private readonly output: ShaderPass;
  private readonly backdrop = { r: 0, g: 0, b: 0 };
  private readonly clearColor = new THREE.Color();

  /** Contour lines over the frame (`contours.ts`), after the occlusion and before the output pass; null draws none. */
  setContours(contours: Contours | null): void {
    if (!contours) { if (this.contours) this.contours.enabled = false; return; }
    if (this.contours) { this.contours.set(contours); this.contours.enabled = true; return; }
    this.contours = new ContourPass(this.camera, () => this.contourSource(), contours);
    this.contours.setPixelRatio(this.pixelRatio);
    // Before the output pass, wherever that is by now.
    const output = this.composer.passes.indexOf(this.output);
    if (output < 0) this.composer.addPass(this.contours);
    else this.composer.insertPass(this.contours, output);
  }

  /**
   * What the contour lines read: the first occlusion pass's depth, normals and foliage mask. With the occlusion off the
   * pass does not draw them, so they are drawn here, the same way (foliage left out and masked, overlays hidden).
   */
  private contourSource(): ContourSource {
    const internals = this.pass as unknown as { _overrideVisibility(): void; _restoreVisibility(): void; normalRenderTarget: THREE.WebGLRenderTarget;
      _renderOverride(renderer: THREE.WebGLRenderer, material: THREE.Material, target: THREE.WebGLRenderTarget, color: number, alpha: number): void;
      blendMaterial: THREE.ShaderMaterial };
    if (!this.pass.enabled && !this.geometryDrawn) {
      internals._overrideVisibility();
      internals._renderOverride(this.renderer, this.pass.normalMaterial, internals.normalRenderTarget, 0x7777ff, 1.0);
      internals._restoreVisibility();
      this.geometryDrawn = true;
    }
    return { depth: internals.normalRenderTarget.depthTexture!, normal: internals.normalRenderTarget.texture, foliage: this.foliageMask.texture,
      hasFoliage: internals.blendMaterial.uniforms.hasFoliage.value > 0 };
  }

  private contours?: ContourPass;
  private geometryDrawn = false;
  private litEdges?: LitEdges;

  setLitEdges(sun: THREE.DirectionalLight | null): void {
    if (this.litEdges) { this.litEdges.enabled = sun !== null; return; }
    if (!sun) return;
    this.litEdges = new LitEdges(this.camera, () => this.contourSource(), sun);
    this.composer.insertPass(this.litEdges, this.composer.passes.indexOf(this.output));
  }
  private readonly camera: THREE.Camera;
  private pixelRatio = 1;

  /** Changes the occlusion's look in place; null leaves the occlusion out of the frame (contour lines alone are then drawn). */
  set(parameters: AmbientOcclusionParameters | null): void {
    this.pass.enabled = this.contact.enabled = parameters !== null;
    if (!parameters) return;
    // Lengths are scene meters (`screenSpaceRadius` off). Every sample within the radius raises the horizon in full
    // (no distance falloff): with the pass's default falloff only the nearest centimeters of a corner darken.
    this.pass.updateGtaoMaterial({ radius: parameters.radius, thickness: parameters.thickness, distanceExponent: 1, distanceFallOff: 0,
      scale: Math.max(1, parameters.intensity), samples: 16, screenSpaceRadius: false });
    // The denoise removes the sampling pattern, so nothing crawls while orbiting; it stops at depth and normal edges.
    this.pass.updatePdMaterial({ lumaPhi: 10, depthPhi: 2, normalPhi: 3, radius: 8, radiusExponent: 2, rings: 2, samples: 16 });
    // Up to 1 the intensity is how much of the occlusion is blended in; above it, the occlusion itself is deepened
    // (raised to that power), which darkens creases and leaves open surfaces, where it is 1, as they are.
    this.pass.blendIntensity = Math.min(1, parameters.intensity);
    // The contact term: a fifth of the radius (at most 15 cm) and of the thickness, blended a little lighter.
    this.contact.updateGtaoMaterial({ radius: Math.min(0.15, parameters.radius * 0.2), thickness: Math.min(0.08, parameters.thickness * 0.25),
      distanceExponent: 1, distanceFallOff: 0, scale: Math.max(1, parameters.intensity), samples: 16, screenSpaceRadius: false });
    this.contact.updatePdMaterial({ lumaPhi: 10, depthPhi: 2, normalPhi: 3, radius: 4, radiusExponent: 2, rings: 2, samples: 16 });
    this.contact.blendIntensity = Math.min(1, parameters.intensity) * 0.75;
  }

  setSize(width: number, height: number, pixelRatio: number): void {
    this.pixelRatio = pixelRatio;
    this.contours?.setPixelRatio(pixelRatio);
    this.composer.setPixelRatio(pixelRatio);
    this.composer.setSize(width, height);
    const size = this.composer.readBuffer;
    this.output.uniforms.edgeTexel.value.set(1 / Math.max(1, size.width), 1 / Math.max(1, size.height));
  }

  /** Runs `compile` with the scene target bound, so programs are made for the target the scene is drawn into. */
  compiling<T>(compile: () => T): T {
    const before = this.renderer.getRenderTarget();
    this.renderer.setRenderTarget(this.composer.readBuffer);
    try { return compile(); } finally { this.renderer.setRenderTarget(before); }
  }

  dispose(): void {
    this.pass.dispose();
    this.contact.dispose();
    this.contours?.dispose();
    this.litEdges?.dispose();
    this.output.dispose();
    this.foliageMask.dispose();
    this.contactMask.dispose();
    this.composer.dispose();
  }
}
