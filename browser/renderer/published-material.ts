import type { MetalFinish } from "./reflection-field.ts";
import * as THREE from "three";
import { paintedMetalSky, radialSkyShader, radialSkyVertex } from "./painted-metal-sky.ts";

type LiveSetting = { id: string; arguments: Record<string, { value?: number }> };

/** Published fixture finishes use uniform arguments. Texture masks are resolved by the Material Studio renderer. */
function uniformOperation(material: THREE.Material, id: string, fields: string[]): LiveSetting["arguments"] | undefined {
  const operations: LiveSetting[] = material.userData.materialSpec?.live ?? [];
  const args = operations.find(operation => operation.id === id)?.arguments;
  return args && Object.entries(args).every(([field, arg]) => fields.includes(field) && Number.isFinite(arg.value))
    ? args : undefined;
}

/** Retain the receiver's derivative normal frame after packing its chart into a larger atlas.
 * The red and green mask channels carry MaterialSpec's grazingFalloff and scaleSpecular
 * arguments. Rust, unpolished paint and unrelated surfaces retain their own response. */
export function applyWorldSpecMaterial(material: THREE.MeshStandardMaterial, shadow?: { form?: number; formFill?: number }, maxAnisotropy = 1, finish?: MetalFinish): void {
  if (material.userData.foliageClusterNormals) {
    const painted = material.userData.foliagePaintCoverage === true && !!material.map && material.alphaTest > 0;
    const useCoverage = { value: false };
    if (painted) {
      material.alphaToCoverage = true;
      // Fractional alpha selects MSAA samples; it must not punch transparency
      // back into an opaque sky or facade behind the selected leaf samples.
      material.blending = THREE.CustomBlending;
      material.blendSrc = THREE.OneFactor;
      material.blendDst = THREE.ZeroFactor;
      material.blendSrcAlpha = THREE.OneFactor;
      material.blendDstAlpha = THREE.OneMinusSrcAlphaFactor;
      const previousRender = material.onBeforeRender;
      const defaultSamples = new WeakMap<THREE.WebGLRenderer, boolean>();
      material.onBeforeRender = function (renderer, ...args) {
        previousRender.call(this, renderer, ...args);
        const target = renderer.getRenderTarget();
        if (!target && !defaultSamples.has(renderer)) defaultSamples.set(renderer, !!renderer.getContextAttributes().antialias);
        useCoverage.value = target ? target.samples > 1 : defaultSamples.get(renderer)!;
      };
    }
    // Each two-sided leaf carries its crown cluster's outward normal. Keep it
    // facing out of the crown on either side of the leaf sheet.
    const previous = material.onBeforeCompile, previousKey = material.customProgramCacheKey.bind(material);
    material.onBeforeCompile = function (shader, renderer) {
      previous.call(this, shader, renderer);
      shader.fragmentShader = shader.fragmentShader.replace("#include <normal_fragment_begin>",
        THREE.ShaderChunk.normal_fragment_begin.replace("normal *= faceDirection;", ""));
      // The alpha stamp already contains a blurred coverage integral. Feeding
      // that integral directly to MSAA preserves the soft edge and depth writes
      // through crossing leaves; the usual cutout sharpens it back to one pixel.
      if (painted) {
        shader.uniforms.msPaintCoverage = useCoverage;
        shader.fragmentShader = "uniform bool msPaintCoverage;\n" + shader.fragmentShader.replace("#include <alphatest_fragment>", `
          if (diffuseColor.a < (msPaintCoverage ? 0.02 : alphaTest)) discard;
          if (!msPaintCoverage) diffuseColor.a = 1.0;
          else if (diffuseColor.a < 0.98) {
            // Distribute the four-sample coverage levels across the brush
            // transition; otherwise a broad wash resolves into contour bands.
            float noise = fract(52.9829189 * fract(dot(floor(gl_FragCoord.xy), vec2(0.06711056, 0.00583715))));
            diffuseColor.a = clamp(diffuseColor.a + 0.24 * (noise - 0.5), 0.0, 1.0);
          }
        `);
      }
    };
    material.customProgramCacheKey = () => previousKey() + "|worldspec-crown-normal-v2-" + Number(painted);
    return;
  }
  const chart = !!material.userData.materialSpecNormalFrame;
  const metal = (!material.userData.worldSpecSurface || !!finish) && material.metalness > 0 && !material.transparent && !material.userData.worldSpecConcrete;
  const sky = metal && uniformOperation(material, "virtualSky", ["intensity", "radial", "red", "green", "blue", "verticals"]);
  const radial = (sky && material.map && sky.radial?.value) || 0;
  const rim = metal && uniformOperation(material, "lightFloor", ["red", "green", "blue", "mask", "from", "to"]);
  if (!chart && !metal) return;
  const edge = material.userData.materialSpecEdge as { from: number; to: number } | undefined;
  const hasEdge = !!material.aoMap && !!edge;
  const serviceForm = metal && (material.userData.worldSpecDuct === true || material.userData.worldSpecPipe === true)
    && !!(shadow?.form || shadow?.formFill);
  if (material.userData.worldSpecDuct && chart) {
    // Long soffit views compress the grain in only one texture direction.
    for (const map of [material.map, material.normalMap, material.roughnessMap, material.metalnessMap, material.aoMap]) {
      if (map) map.anisotropy = Math.max(map.anisotropy, Math.min(8, maxAnisotropy));
    }
  }
  const previous = material.onBeforeCompile, previousKey = material.customProgramCacheKey.bind(material);
  material.onBeforeCompile = function (shader, renderer) {
    previous.call(this, shader, renderer);
    if (chart) {
      shader.vertexShader = "attribute float _ms_aspect; varying float msChartAspect;\n" + shader.vertexShader
        .replace("#include <uv_vertex>", "#include <uv_vertex>\nmsChartAspect = _ms_aspect;");
      shader.fragmentShader = "varying float msChartAspect;\n" + shader.fragmentShader
        .replace("#include <normal_fragment_begin>", THREE.ShaderChunk.normal_fragment_begin
          .replace("vNormalMapUv", "(vNormalMapUv * vec2(1.0, msChartAspect))"));
    }
    if (hasEdge && edge) {
      shader.uniforms.msEdgeFacing = { value: new THREE.Vector2(edge.from, edge.to) };
      shader.fragmentShader = "uniform vec2 msEdgeFacing;\n" + shader.fragmentShader
        .replace("#include <lights_physical_fragment>", `#include <lights_physical_fragment>
float msSunEdge = 0.0;
#if defined(USE_SHADOWMAP) && NUM_DIR_LIGHT_SHADOWS > 0
  vec4 msCoord = vDirectionalShadowCoord[0];
  vec3 msProjected = msCoord.xyz / msCoord.w;
  float msFacing = dot(normal, directionalLights[0].direction);
  if (receiveShadow && ${serviceForm ? "" : "msFacing > 0.0 &&"} all(greaterThanEqual(msProjected, vec3(0.0))) &&
      all(lessThanEqual(msProjected, vec3(1.0))) && dot(directionalLights[0].color, vec3(1.0)) > 0.0) {
    DirectionalLightShadow msShadow = directionalLightShadows[0];
    float msVisible = getShadow(directionalShadowMap[0], msShadow.shadowMapSize, 1.0,
      msShadow.shadowBias, msShadow.shadowRadius, msCoord);
    msSunEdge = smoothstep(0.8, 1.0, msVisible) * smoothstep(0.0, 0.35, msFacing);
    ${serviceForm ? `vec3 msAcross = directionalLights[0].direction - dot(directionalLights[0].direction, viewMatrix[1].xyz) * viewMatrix[1].xyz;
    float msEdgeToward = dot(normal, msAcross / max(length(msAcross), 1e-4));
    msSunEdge += (1.0 - msVisible) * dot(msServiceForm, vec2(max(msEdgeToward, 0.0), max(-msEdgeToward, 0.0)));` : ""}
  }
#endif
          vec2 msEdge = texture2D(aoMap, vAoMapUv).rg;
          float span = msEdgeFacing.y - msEdgeFacing.x;
          float facing = abs(dot(normal, normalize(vViewPosition)));
          float t = clamp((facing - msEdgeFacing.x) / (abs(span) < 1e-6 ? -1e-6 : span), 0.0, 1.0);
          material.specularF90 = mix(material.specularF90, 1.08, msSunEdge * t * t * (3.0 - 2.0 * t) * msEdge.r);
          material.specularColorBlended = min(vec3(1.0), material.specularColorBlended * (1.0 + 0.12 * msSunEdge * msEdge.g));
        `);
    }
    if (metal) {
      shader.uniforms.msSkyTint = {value:new THREE.Vector3((sky ? sky.red?.value ?? 1 : 1)*(finish?.skyRed ?? 1),(sky ? sky.green?.value ?? 1 : 1)*(finish?.skyGreen ?? 1),(sky ? sky.blue?.value ?? 1 : 1)*(finish?.skyBlue ?? 1))};
      shader.uniforms.msSkyStrength={value:finish?.reflection??1};
      shader.uniforms.msPaintedSky = { value: paintedMetalSky() };
      if (sky) shader.uniforms.msSkyIntensity = { value: sky.intensity?.value ?? 1 };
      if (radial) {
        shader.vertexShader = radialSkyVertex(shader.vertexShader);
        shader.uniforms.msRadialSkyStrength = { value: radial };
        shader.fragmentShader = shader.fragmentShader.replace("#include <normal_pars_fragment>",
          `#include <normal_pars_fragment>\nuniform float msRadialSkyStrength;\n${radialSkyShader}`);
      }
      if (rim) {
        shader.uniforms.msRimColor = { value: new THREE.Vector3(rim.red?.value ?? 0, rim.green?.value ?? 0, rim.blue?.value ?? 0) };
        shader.uniforms.msRimRamp = { value: new THREE.Vector3(rim.from?.value ?? 0, rim.to?.value ?? 0, rim.mask?.value ?? 1) };
      }
      shader.fragmentShader = "uniform float msSkyStrength; uniform vec3 msSkyTint; uniform sampler2D msPaintedSky;\n"
        + (sky ? "uniform float msSkyIntensity;\n" : "")
        + (rim ? "uniform vec3 msRimColor, msRimRamp;\n" : "") + shader.fragmentShader
        .replace("#include <aomap_fragment>", `#include <aomap_fragment>
          vec3 msView = isOrthographic ? vec3(0.0, 0.0, 1.0) : normalize(vViewPosition);
          vec3 msReflected = transformDirectionByInverseViewMatrix(reflect(-msView, normal), viewMatrix);
          vec3 msPaint = msSkyTint * textureLod(msPaintedSky, equirectUv(msReflected), 3.0 * material.roughness * material.roughness).rgb;
          float msGrazing = 1.0 - smoothstep(0.045, 0.52, abs(dot(normal, msView)));
          float msShine = 1.0 - material.roughness;
          float msSkyWeight = smoothstep(0.09, 0.48, material.metalness)
            * (0.055 * msShine + msGrazing * (0.12 + 0.52 * msShine));
          ${sky ? `// The enamel clearcoat sees the same painted panorama as exposed metal.
          msSkyWeight += msSkyIntensity * (0.26 + 0.74 * msGrazing) * (0.5 + 0.5 * msShine);` : ""}
          #ifdef USE_AOMAP
            msSkyWeight *= ambientOcclusion;
          #endif
          reflectedLight.indirectSpecular += msSkyWeight * msSkyStrength * msPaint * mix(material.specularColorBlended, vec3(1.0), msGrazing * 0.55);
          ${radial ? `reflectedLight.indirectSpecular += msSkyTint * msRadialSkyStrength * msSkyIntensity * material.metalness
            * msRadialReflection(msPaintedSky, -vViewPosition, msView, normal, material.roughness, false);` : ""}
          ${rim ? `// Carry the grazing highlight into the HDR resolve, where its brightest edges bloom.
          float msSpan = msRimRamp.y - msRimRamp.x;
          float msT = abs(msSpan) < 1e-6 ? 1.0 : clamp((abs(dot(normal, msView)) - msRimRamp.x) / msSpan, 0.0, 1.0);
          totalEmissiveRadiance += msRimColor * (msT * msT * (3.0 - 2.0 * msT)) * clamp(msRimRamp.z, 0.0, 1.0);` : ""}
        `);
      if (serviceForm) {
        shader.uniforms.msServiceForm = { value: new THREE.Vector2(shadow?.form ?? 0, shadow?.formFill ?? 0).multiplyScalar(0.32) };
        shader.fragmentShader = "uniform vec2 msServiceForm;\n" + shader.fragmentShader;
      }
      shader.fragmentShader = shader.fragmentShader.replace("#include <lights_fragment_end>", `#include <lights_fragment_end>
        #if NUM_DIR_LIGHTS > 0
          // Atlas metalness selects exposed flecks amongst rust and paint.
          float msMetal = smoothstep(0.025, 0.55, material.metalness);
          vec3 msSun = directionalLights[0].direction;
          float msLit = saturate(dot(normal, msSun));
          float msVisibility = 1.0;
          #if defined(USE_SHADOWMAP) && NUM_DIR_LIGHT_SHADOWS > 0
            if (receiveShadow && msMetal ${serviceForm ? "" : "* msLit"} > 0.0) {
              DirectionalLightShadow sh = directionalLightShadows[0];
              msVisibility = getShadow(directionalShadowMap[0], sh.shadowMapSize, 1.0,
                sh.shadowBias, sh.shadowRadius, vDirectionalShadowCoord[0]);
            }
          #endif
          vec3 msEye = normalize(vViewPosition);
          float msViewFacing = abs(dot(normal, msEye));
          float msRim = 1.0 - smoothstep(0.04, 0.55, msViewFacing);
          float msSunRim = 1.0 - smoothstep(0.06, 0.58, msViewFacing);
          float msFlash = pow(saturate(dot(reflect(-msSun, normal), msEye)), 12.0);
          float msFinish = msMetal * (1.0 - 0.75 * material.roughness);
          ${serviceForm ? `
          // A weak, broad lobe follows the sun across shaded service metal.
          vec3 msLevel = msSun - dot(msSun, viewMatrix[1].xyz) * viewMatrix[1].xyz;
          msLevel /= max(length(msLevel), 1e-4);
          float msServiceFacing = dot(normal, msLevel);
          vec3 msKey = msServiceFacing >= 0.0 ? msLevel : -msLevel;
          float msFormWeight = msServiceFacing >= 0.0 ? msServiceForm.x : msServiceForm.y;
          float msSheen = pow(saturate(dot(reflect(-msKey, normal), msEye)), 4.0);
          // Smooth folded lips catch a tighter grazing reflection than the rough plate.
          float msPolish = 1.0 - smoothstep(0.24, 0.58, material.roughness);
          float msLip = (1.0 - smoothstep(0.06, 0.42, msViewFacing)) * msPolish;
          vec3 msResponse = material.specularColorBlended * (0.42 + 0.22 * msRim + 0.22 * msSheen + 1.6 * msLip)
            + 0.22 * material.diffuseContribution;
          // The AO map attenuates this simulated reflected light at pipe collars.
          reflectedLight.indirectDiffuse += directionalLights[0].color * (1.0 - msVisibility)
            * msFormWeight * abs(msServiceFacing) * msResponse;
          ` : ""}
          // Give thin rails enough highlight width and intensity to read at gameplay distance.
          reflectedLight.directSpecular += directionalLights[0].color * msFinish * msVisibility
            * smoothstep(0.0, 0.55, msLit) * (${finish ? "0.0" : "3.2 * msSunRim + 3.0 * msFlash"});
          // Open sky gives upward-facing worn edges a quieter reflection in the shade.
          float msSky = smoothstep(-0.15, 0.7, dot(normal, viewMatrix[1].xyz));
          reflectedLight.indirectSpecular += getAmbientLightIrradiance(ambientLightColor)
            * msFinish * msSky * msRim;
          #if NUM_HEMI_LIGHTS > 0
            reflectedLight.indirectSpecular += hemisphereLights[0].skyColor * msFinish * msSky * msRim;
          #endif
        #endif`);
    }
  };
  material.customProgramCacheKey = () => previousKey() + "|worldspec-chart-v11-" + [chart, hasEdge, metal, serviceForm, !!sky, !!rim, !!radial,!!finish].map(Number).join("-");
}
