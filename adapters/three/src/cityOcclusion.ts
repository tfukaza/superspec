import * as THREE from "three";
import { overlapOutlineShader } from "./overlapOutline.ts";
import { renderColorAndExclusions, renderExcludedDepth, outlineDepthLimit, warmExcludedDepth } from "./outlineExclusions.ts";
import { FullScreenQuad } from "three/addons/postprocessing/Pass.js";
import { CityEdgeLight, edgeLightDeclarations, edgeLightFragment } from "./cityEdgeLight.ts";
import { ShopPaint, shopPaintShader } from "./shopPaint.ts";
import { HORIZON_OCCLUSION_GLSL } from "./horizonOcclusionShader.ts";
import { legacyRenderDiagnostics, type RenderDiagnostics } from "./renderProfile.ts";

/** The lighting record's `ambientOcclusion`. Only the `horizon` method is drawn here; its
 * `tint` is the share of the sun shadow's hue the occlusion takes. */
type Occlusion = { enabled: boolean; radius: number; intensity: number; thickness: number; method?: string; tint?: number };
type ShadowHue = { tint: [number, number, number]; tintStrength: number };

/** Reuse the world's resolved depth. Occlusion darkens the tone-mapped image, so contact
 * shading under small props survives on bright sunlit ground, and takes the shadow's hue. */
export class CityOcclusion {
  private target?: THREE.WebGLRenderTarget;
  private highlights?: THREE.WebGLRenderTarget;
  private excluded?: THREE.WebGLRenderTarget;
  private extract?: FullScreenQuad;
  private outlines = false;
  setOutlines(outlinesByDefault: boolean): void {
    const mode = this.diagnostics.effects.outline;
    const enabled = mode === "on" || mode === "debug" || (outlinesByDefault && mode !== "off");
    if (enabled === this.outlines) return;
    this.outlines = enabled;
    this.pass?.material.dispose();
    this.pass?.dispose();
    this.pass = undefined;
  }
  private pass?: FullScreenQuad;
  private readonly size = new THREE.Vector2();
  private readonly edges: CityEdgeLight;
  private readonly shopPaint: ShopPaint;
  private readonly excludedMaterials = new Map<THREE.Material, THREE.MeshDepthMaterial>();
  private readonly uniforms;

  constructor(private readonly diagnostics: RenderDiagnostics =
    legacyRenderDiagnostics()) {
    this.edges = new CityEdgeLight(diagnostics.effects.edgeBoost);
    this.shopPaint = new ShopPaint(diagnostics.effects.shopArt);
    this.uniforms = {
      ...this.edges.uniforms,
      ...this.shopPaint.uniforms,
      highlightMap: { value: null as THREE.Texture | null },
      bloomEnabled: { value: false },
      specularOutlineMask: { value: false },
      excludedDepth: { value: null as THREE.DepthTexture | null }, hasExclusions: { value: false },
      frame: { value: null as THREE.Texture | null },
      aoDepth: { value: null as THREE.DepthTexture | null }, aoInvProjection: { value: new THREE.Matrix4() },
      aoTexel: { value: new THREE.Vector2() }, aoFocal: { value: new THREE.Vector2() },
      aoRadius: { value: 1 }, aoThickness: { value: 0.3 }, aoIntensity: { value: 0 }, aoTint: { value: 0 },
      aoHue: { value: new THREE.Vector3(1, 1, 1) },
    };
  }

  /** The world-color destination selected on the most recent perspective draw. */
  get colorTarget(): THREE.WebGLRenderTarget | undefined { return this.target; }

  private bloomEnabled(byDefault: boolean) {
    const mode = this.diagnostics.effects.bloom;
    return mode === "on" || (byDefault && mode !== "off");
  }

  private aoIntensity(ao: Occlusion) {
    const mode = this.diagnostics.lighting.ambientOcclusion;
    return (mode === "on" || (mode === "auto" && ao.enabled)) && ao.intensity > 0 ? ao.intensity : 0;
  }

  prepareColorTarget(renderer: THREE.WebGLRenderer, ao: Occlusion | undefined, bloomByDefault: boolean) {
    const enabled = ao && (this.aoIntensity(ao) > 0 || this.edges.enabled || this.bloomEnabled(bloomByDefault) || this.outlines);
    if (!enabled || !renderer.extensions.has("EXT_color_buffer_float")) return;
    renderer.getDrawingBufferSize(this.size);
    this.target ??= new THREE.WebGLRenderTarget(this.size.x, this.size.y, {
      type: THREE.HalfFloatType, samples: Math.min(4, renderer.capabilities.maxSamples),
      depthTexture: new THREE.DepthTexture(this.size.x, this.size.y, THREE.UnsignedIntType),
    });
    this.target.setSize(this.size.x, this.size.y);
    return this.target;
  }

  async warmExclusions(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, map: THREE.Object3D) {
    if (this.excluded) await warmExcludedDepth(renderer, scene, camera, map, this.excluded,
      this.diagnostics.effects.exclusionDepth === "depth" ? this.excludedMaterials : undefined);
  }

  dispose(): void {
    for (const material of this.excludedMaterials.values()) material.dispose();
    this.excludedMaterials.clear();
    this.target?.dispose();
    this.highlights?.dispose();
    this.excluded?.dispose();
    for (const quad of [this.pass, this.extract]) {
      quad?.material.dispose();
      quad?.dispose();
    }
  }

  render(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.PerspectiveCamera, sun: THREE.DirectionalLight,
    ao?: Occlusion, shadow?: ShadowHue, hasExclusions = false, paintSurfaces = false, bloomByDefault = true,
    specularOutlineMask = false): void {
    const target = this.prepareColorTarget(renderer, ao, bloomByDefault);
    if (!target || !ao) {
      this.target?.dispose();
      this.target = undefined;
      this.excluded?.dispose();
      this.excluded = undefined;
      this.highlights?.dispose();
      this.highlights = undefined;
      this.uniforms.highlightMap.value = null;
      renderer.render(scene, camera);
      return;
    }
    this.shopPaint.update(camera, paintSurfaces);
    const u = this.uniforms;
    u.frame.value = target.texture;
    u.aoDepth.value = target.depthTexture;
    u.aoInvProjection.value.copy(camera.projectionMatrixInverse);
    u.aoFocal.value.set(camera.projectionMatrix.elements[0], camera.projectionMatrix.elements[5]);
    u.aoTexel.value.set(1 / this.size.x, 1 / this.size.y);
    u.aoRadius.value = ao.radius; u.aoThickness.value = ao.thickness;
    u.aoIntensity.value = this.aoIntensity(ao);
    u.aoTint.value = ao.tint ?? 0;
    // The shadow's tone before its darkness (SuperSpec's shadowHue): brightest channel 1.
    if (shadow) {
      const peak = Math.max(...shadow.tint, 1e-6), keep = 1 - shadow.tintStrength;
      u.aoHue.value.set(...shadow.tint.map(c => c / peak + (1 - c / peak) * keep) as [number, number, number]);
    } else u.aoHue.value.set(1, 1, 1);
    const vertexShader = "varying vec2 vUv; void main() { vUv = uv; gl_Position = vec4(position.xy, 0., 1.); }";
    const bloom = this.bloomEnabled(bloomByDefault);
    u.bloomEnabled.value = bloom;
    u.specularOutlineMask.value = specularOutlineMask;
    if (bloom) {
      const width = Math.max(1, Math.ceil(this.size.x / 2)), height = Math.max(1, Math.ceil(this.size.y / 2));
      this.highlights ??= new THREE.WebGLRenderTarget(width, height, {
        type: THREE.HalfFloatType, depthBuffer: false, generateMipmaps: true,
        minFilter: THREE.LinearMipmapLinearFilter,
      });
      this.highlights.setSize(width, height);
      u.highlightMap.value = this.highlights.texture;
      this.extract ??= new FullScreenQuad(new THREE.ShaderMaterial({
        uniforms: {frame: u.frame, aoTexel: u.aoTexel}, vertexShader, depthTest: false, depthWrite: false,
        fragmentShader: `
          varying vec2 vUv;
          uniform sampler2D frame;
          uniform vec2 aoTexel;
          void main() {
            vec3 sum = vec3(0.);
            for (int y = 0; y < 2; ++y) for (int x = 0; x < 2; ++x) {
              vec3 c = texture2D(frame, vUv + (vec2(float(x),float(y))-.5)*aoTexel).rgb;
              float l = dot(c, vec3(.2126,.7152,.0722));
              sum += min(c,vec3(8.))*smoothstep(.88,1.9,l);
            }
            gl_FragColor = vec4(.25*sum,1.);
          }`,
      }));
    } else if (this.highlights) {
      this.highlights.dispose();
      this.highlights = undefined;
      u.highlightMap.value = null;
    }
    this.pass ??= new FullScreenQuad(new THREE.ShaderMaterial({
      uniforms: u, depthWrite: false, depthTest: false,
      vertexShader,
      fragmentShader: `
        varying vec2 vUv;
        uniform sampler2D frame;
        uniform sampler2D highlightMap;
        uniform bool bloomEnabled;
        uniform bool specularOutlineMask;
        uniform sampler2D excludedDepth;
        uniform bool hasExclusions;
        ${HORIZON_OCCLUSION_GLSL}
        ${edgeLightDeclarations}
        ${shopPaintShader}
        ${this.outlines ? overlapOutlineShader : ""}
        void main() {
          vec4 color = texture2D(frame, vUv);
          // Two-pixel soft grass pass, fused with the existing resolve. Only
          // turf and the fuzzy tufts on it participate. Tall tips, masonry
          // and other opaque objects carry ordinary alpha and stay sharp.
          float paintedCoverage = clamp((1.0-color.a)*2.0, 0.0, 1.0);
          float lawn = clamp(1.0-color.a*2.0, 0.0, 1.0);
          if (lawn > 0.01) {
            vec3 sum = color.rgb * 2.0;
            float weights = 2.0;
            for (int j = 0; j < 8; ++j) {
              float angle = float(j) * 0.78539816;
              vec4 nb = texture2D(frame, vUv + 2.0 * aoTexel * vec2(cos(angle), sin(angle)));
              float weight = clamp(1.0-nb.a*2.0, 0.0, 1.0);
              sum += weight * nb.rgb;
              weights += weight;
            }
            color.rgb = mix(color.rgb, sum / weights, lawn);
          }
          float concreteCoverage = clamp((color.a - 1.0) * 16.0, 0.0, 1.0);
          float specularCoverage = 0.;
          if (specularOutlineMask) {
            float packedMask = max(0., color.a - 1.);
            float concreteSamples = floor(packedMask + .001);
            concreteCoverage = .25 * concreteSamples;
            specularCoverage = clamp(2. * (packedMask - concreteSamples), 0., 1.);
          }
          color.a = 1.0;
          // The edge light reads the surface the camera sees in these names.
          vec2 texel = aoTexel, focal = aoFocal;
          vec3 p = aoAt(vUv);
          vec3 left = p - aoAt(vUv - vec2(texel.x, 0.)), right = aoAt(vUv + vec2(texel.x, 0.)) - p;
          vec3 down = p - aoAt(vUv - vec2(0., texel.y)), up = aoAt(vUv + vec2(0., texel.y)) - p;
          vec3 n = cross(abs(left.z) < abs(right.z) ? left : right, abs(down.z) < abs(up.z) ? down : up);
          n *= inversesqrt(max(dot(n,n), 1e-16));
          color.rgb = shopFinish(color.rgb,vUv,p,n);
          if (paintedCoverage < 0.99) { ${edgeLightFragment} }
          // The opaque depth behind a translucent fringe belongs to its
          // backdrop. It must not stamp a masonry contact shadow into blades.
          float darkening = paintedCoverage >= 0.99 ? 0.0 : (1.0-paintedCoverage)*horizonOcclusion(vUv);
          if (bloomEnabled) {
            color.rgb += .33 * textureLod(highlightMap, vUv, 1.5).rgb
                       + .22 * textureLod(highlightMap, vUv, 3.2).rgb;
          }
          gl_FragColor = color;
          #include <tonemapping_fragment>
          gl_FragColor.rgb *= occlusionTone(darkening);
          ${this.outlines ? `float depthPerMeter = .5 / (abs(aoInvProjection[2][3]) * p.z * p.z);
          float footprint = -2. * p.z * texel.y / focal.y;
          float ink = overlapOutline(aoDepth, vUv, texel, depthPerMeter, footprint, specularOutlineMask);
          // At grazing angles the depth buffer undersamples folds such as awning ribs.
          vec3 eye = normalize(-p);
          float faceCoverage = smoothstep(.06,.25,abs(dot(n,eye)));
          float sizeFade = outlineSizeFade(-2. * p.z * texel / focal, n, eye);
          float outlineAmount = .66 * ink * faceCoverage * sizeFade;
          if (specularOutlineMask) {
            float silhouette = backgroundOutline(aoDepth,vUv,texel) * (1. - smoothstep(.04,.05,footprint));
            outlineAmount = max(outlineAmount, .55 * silhouette);
          }
          outlineAmount *= 1. - paintedCoverage;
          outlineAmount *= 1. - specularCoverage;
          if (hasExclusions && texture2D(excludedDepth,vUv).r <= texture2D(aoDepth,vUv).r)
            outlineAmount = 0.;
          ${this.diagnostics.effects.outline === "debug"
            ? "gl_FragColor = vec4(vec3(1. - outlineAmount), 1.); return;"
            : "gl_FragColor.rgb *= 1. - outlineAmount;"}` : ""}
          #include <colorspace_fragment>
        }`,
    }));
    const reset = renderer.info.autoReset;
    const previous = renderer.getRenderTarget();
    try {
      renderer.setRenderTarget(target);
      u.hasExclusions.value = this.outlines && hasExclusions;
      if (u.hasExclusions.value) {
        const samples = target.samples;
        renderColorAndExclusions(renderer, scene, camera, () => {
          this.excluded ??= new THREE.WebGLRenderTarget(this.size.x, this.size.y, {
            samples, depthTexture: new THREE.DepthTexture(this.size.x, this.size.y, THREE.UnsignedIntType),
          });
          this.excluded.setSize(this.size.x, this.size.y);
          const { exclusionDepth, exclusionCull } = this.diagnostics.effects;
          renderExcludedDepth(renderer, scene, camera, this.excluded, exclusionDepth === "depth" ? this.excludedMaterials : undefined,
            exclusionCull ? outlineDepthLimit(camera, this.size.x, this.size.y) : Infinity);
          u.excludedDepth.value = this.excluded.depthTexture;
        });
      } else {
        renderer.render(scene, camera);
        this.excluded?.dispose();
        this.excluded = undefined;
        u.excludedDepth.value = null;
      }
      if (this.highlights && this.extract) {
        renderer.setRenderTarget(this.highlights);
        this.extract.render(renderer);
      }
      renderer.setRenderTarget(previous);
      this.edges.update(renderer, camera, sun, this.pass.material as THREE.ShaderMaterial);
      renderer.info.autoReset = false;
      this.pass.render(renderer);
    } finally {
      renderer.setRenderTarget(previous);
      renderer.info.autoReset = reset;
    }
  }
}
