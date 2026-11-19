import * as THREE from "three";
import { EffectComposer } from "three/examples/jsm/postprocessing/EffectComposer.js";
import { GTAOPass } from "three/examples/jsm/postprocessing/GTAOPass.js";
import { RenderPass } from "three/examples/jsm/postprocessing/RenderPass.js";
import { ShaderPass } from "three/examples/jsm/postprocessing/ShaderPass.js";
import { SimplexNoise } from "three/examples/jsm/math/SimplexNoise.js";
import type { AmbientOcclusionSettings } from "./lighting.ts";

/** The lighting record's ambient occlusion values; whether the effect is on (`enabled`) is its host's decision. */
type AmbientOcclusionParameters = Pick<AmbientOcclusionSettings, "radius" | "intensity" | "thickness">;

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
    // The denoise turns its sample pattern by a noise texture the pass draws from Math.random() when it is made, so
    // two runs of one scene differed in a fifth or more of their pixels. A fixed texture makes pictures comparable.
    const denoise = this.pass as unknown as { pdNoiseTexture: THREE.Texture; pdMaterial: THREE.ShaderMaterial };
    denoise.pdNoiseTexture.dispose();
    denoise.pdNoiseTexture = fixedNoise();
    denoise.pdMaterial.uniforms.tNoise.value = denoise.pdNoiseTexture;
    this.pass.normalMaterial.side = THREE.DoubleSide;
    faceNormals(this.pass.normalMaterial);
    const internals = this.pass as unknown as { _overrideVisibility(): void; _restoreVisibility(): void; normalRenderTarget: THREE.WebGLRenderTarget; blendMaterial: THREE.ShaderMaterial };
    const hide = internals._overrideVisibility.bind(this.pass), restore = internals._restoreVisibility.bind(this.pass);
    let hidden: THREE.Object3D[] = [], leaves: THREE.Object3D[] = [];
    // Foliage is drawn by shaders of its own (leaf cards stood up and bent in their vertex shaders), which the pass's one
    // override material does not run: drawn with it, a canopy's cards lie where their raw vertices are and darken the
    // building through the tree. So foliage is left out of the depth and normals (it casts no occlusion), and drawn
    // instead, with its own materials, into a mask against that depth: where a leaf is in front, the image is not
    // darkened (it receives none either).
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
      this.drawFoliageMask(leaves, camera, internals.normalRenderTarget);
      hidden = []; leaves = [];
      restore();
    };
    // The blend leaves the image as it is where the mask holds a leaf.
    const blend = internals.blendMaterial;
    blend.uniforms.tFoliage = { value: this.foliageMask.texture };
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
    const resize = this.pass.setSize.bind(this.pass);
    this.pass.setSize = (width: number, height: number) => { resize(width, height); this.foliageMask.setSize(width, height); };
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

  /** Where foliage is the nearest surface, in its alpha (see the constructor). */
  private readonly foliageMask = new THREE.WebGLRenderTarget(1, 1, { type: THREE.UnsignedByteType, depthBuffer: false });

  /**
   * Draws `leaves` with their own materials into the foliage mask, depth-tested (not written) against the depth the
   * occlusion pass drew without them, so only leaves in front of what the occlusion sees mark it. The shadow maps are
   * not redrawn for this draw (it shows only the leaves); nothing else of the renderer's state is changed.
   */
  private drawFoliageMask(leaves: THREE.Object3D[], camera: THREE.Camera, depth: THREE.WebGLRenderTarget): void {
    const blend = (this.pass as unknown as { blendMaterial: THREE.ShaderMaterial }).blendMaterial;
    blend.uniforms.hasFoliage.value = leaves.length ? 1 : 0;
    if (!leaves.length || !depth.depthTexture) return;
    const mask = this.foliageMask;
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
    this.foliageMask.dispose();
    this.composer.dispose();
  }
}
