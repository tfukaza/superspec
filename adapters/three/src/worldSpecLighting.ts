import * as THREE from "three";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";
import { paintedCastShadow } from "./paintedCastShadow.ts";

type Rgb = [number, number, number];
type Shadow = {
  style: string; darkness: number; tint: Rgb; tintStrength: number;
  wrap: number; form?: number; formFill?: number;
};

/** WorldSpec's portable scene lighting. Directions use the authored Z-up frame. */
export type WorldSpecLighting = {
  version: 1; exposure: number; toneMapping: "aces" | "neutral" | "none";
  environment: { source: string; intensity: number; rotation: number };
  ambient: { sky: Rgb; ground: Rgb; intensity: number };
  lights: { role: string; kind: string; direction: Rgb; color: Rgb; intensity: number; castsShadow: boolean; shadow?: Shadow }[];
};

export function studioReflections(renderer: THREE.WebGLRenderer): THREE.WebGLRenderTarget {
  const studio = new RoomEnvironment(), pmrem = new THREE.PMREMGenerator(renderer);
  try { return pmrem.fromScene(studio); }
  finally { studio.dispose(); pmrem.dispose(); }
}

/** Shade occluded standard materials with the record's directional form light. */
export function styleWorldShadows(material: THREE.MeshStandardMaterial, style: Shadow, paintCamera?: THREE.OrthographicCamera): void {
  if (paintCamera && material.userData.worldSpecConcrete && !material.transparent && material.side === THREE.FrontSide) {
    // A concrete pier's illuminated front face blocks light all the way to
    // its foot. Back-face casting leaves a bias-sized gap at the floor.
    material.shadowSide = THREE.FrontSide;
  }
  const tone = new THREE.Color(...style.tint);
  tone.multiplyScalar(1 / Math.max(tone.r, tone.g, tone.b, 0.0001))
    .lerp(new THREE.Color(1, 1, 1), 1 - style.tintStrength).multiplyScalar(1 - style.darkness);
  // The relief and the tuft roots carry the same growth normal. Evaluate
  // TreeSpec's painted sun band on both, using the existing shadow sample.
  const grassField = material.userData.grassField === true;
  const shadeColor = material.userData.worldSpecShadeColor;
  const tintedShade = Array.isArray(shadeColor) && shadeColor.length === 4 && shadeColor.every(Number.isFinite);
  const diffuseFinish = material.userData.worldSpecPaintedDiffuse;
  const diffuseStrokeStrength = diffuseFinish === true ? 1
    : Number.isFinite(diffuseFinish) ? THREE.MathUtils.clamp(diffuseFinish, 0, 1) : 0;
  const paintedDiffuse = diffuseStrokeStrength > 0;
  const softTufts = grassField && material.transparent;
  if (softTufts) {
    // Composite a bounded material mask independently of RGB coverage.
    // Ordinary opaque pixels carry 1, soft paint 0, and sharp grass 0.5.
    material.blending = THREE.CustomBlending;
    material.blendSrc = THREE.SrcAlphaFactor;
    material.blendDst = THREE.OneMinusSrcAlphaFactor;
    material.blendAlpha = material.userData.paintedStroke === 2 ? 0.5 : 0;
    material.blendSrcAlpha = THREE.ConstantAlphaFactor;
    material.blendDstAlpha = THREE.OneMinusSrcAlphaFactor;
  }
  const turf = grassField && !material.map;
  const turfAlpha = { value: 1 };
  if (turf) {
    const previous = material.onBeforeRender;
    material.onBeforeRender = function(renderer,...args) {
      previous.call(this, renderer, ...args);
      turfAlpha.value = renderer.getRenderTarget()?.texture.type === THREE.HalfFloatType ? 0 : 1;
    };
  }
  const lawnBand = grassField
    ? "cityLawnBand = min(cityCast, smoothstep(0.46, 1.06, dot(geometryNormal, directLight.direction)));"
    : "";
  // Three sets visible=false when a local light contributes zero irradiance.
  // Keep projector sampling outside this branch to preserve mip derivatives.
  const lights = THREE.ShaderChunk.lights_fragment_begin.replaceAll(
    "RE_Direct( directLight,", "if (directLight.visible) RE_Direct( directLight,",
  ).replace(
    /directLight\.color \*= \( directLight\.visible && receiveShadow \) \? (getShadow\( directionalShadowMap\[ i \][^;]+) : 1\.0;/,
    `float cityCast = (directLight.visible && receiveShadow) ? $1 : 1.0;
    ${paintCamera ? `#if defined(SHADOWMAP_TYPE_PCF)
      vec3 key = directLight.color * vec3(.963, 1.006, 1.084);
      vec3 luminance = vec3(.2126, .7152, .0722);
      directLight.color = key * (dot(directLight.color, luminance) / max(dot(key, luminance), 1e-6));
      float strength = directionalLightShadow.shadowIntensity;
      if (directLight.visible && receiveShadow && strength > 0.) {
        float visibility = clamp(1. - (1. - cityCast) / max(strength, 1e-6), 0., 1.);
        vec3 gamePosition = cameraPosition - vViewPosition * mat3(viewMatrix);
        float paint = cityPaintCast(directionalShadowMap[ i ], vDirectionalShadowCoord[ i ], directionalLightShadow.shadowBias, visibility, gamePosition);
        cityCast = mix(1., paint, strength);
        cityCastCoverage = max(cityCastCoverage, 1. - cityCast);
      }
    #endif` : ""}
    vec3 cityUp = viewMatrix[1].xyz;
    vec3 cityLevel = directLight.direction - dot(directLight.direction, cityUp) * cityUp;
    cityLevel /= max(length(cityLevel), 0.0001);
    float cityFacing = dot(geometryNormal, cityLevel);
    cityShadowTerm = min(cityShadowTerm, cityCast * smoothstep(0.0, max(cityWrap, 0.001), dot(geometryNormal, directLight.direction)));
    ${paintedDiffuse ? `
      float sunRamp = dot(geometryNormal, directLight.direction) / max(cityWrap, .001);
      float rampAA = clamp(fwidth(sunRamp), .018, .10);
      float strokePosition = dot(cityDiffusePoint, vec3(10., 2.1, 6.2));
      float strokeAA = clamp(fwidth(strokePosition), .015, .18);
      float diffuseTerm = smoothstep(0., 1., sunRamp);
      if (sunRamp > 0. && sunRamp < 1.) {
        // Each stroke has a straight shoulder and a short, antialiased end.
        // Adjacent strokes shift the boundary without changing the interiors.
        float strokeIndex = floor(strokePosition);
        float alongStroke = fract(strokePosition);
        vec2 seed = fract((strokeIndex + vec2(0., 1.)) * .3183099);
        vec2 offsets = .70 * (fract(seed * (seed + 19.19) * 17.1) - .5)
                     + .30 * (mod(strokeIndex + vec2(0., 1.), 2.) - .5);
        float stroke = mix(offsets.x, offsets.y, smoothstep(.5-strokeAA,.5+strokeAA,alongStroke));
        float border = .48 + .58 * stroke;
        float firstBand = smoothstep(border-.025-rampAA, border+.025+rampAA, sunRamp);
        float secondBand = smoothstep(border+.105-rampAA, border+.155+rampAA, sunRamp);
        diffuseTerm = mix(diffuseTerm, .68 * firstBand + .32 * secondBand, .96);
      }
      cityDiffuseShadow = min(cityDiffuseShadow, cityCast * diffuseTerm);
    ` : ""}
    reflectedLight.directDiffuse += directLight.color * BRDF_Lambert(material.diffuseColor) *
      (cityForm.x * (1.0 - cityCast) * saturate(cityFacing) + cityForm.y * (1.0 - cityShadowTerm) * saturate(-cityFacing));
    ${lawnBand}
    directLight.color *= cityCast;`,
  );
  material.onBeforeCompile = shader => {
    if (paintedDiffuse) {
      const varying = "varying vec3 cityDiffusePoint;\n";
      shader.uniforms.cityDiffuseStrokeStrength = { value: diffuseStrokeStrength };
      shader.vertexShader = varying + shader.vertexShader.replace("#include <begin_vertex>",
        "#include <begin_vertex>\ncityDiffusePoint = position;");
      shader.fragmentShader = "uniform float cityDiffuseStrokeStrength;\n" + varying + shader.fragmentShader;
    }
    if (paintCamera) {
      shader.uniforms.cityShadowSpan = { value: new THREE.Vector3(
        paintCamera.right - paintCamera.left,
        paintCamera.top - paintCamera.bottom,
        paintCamera.far - paintCamera.near,
      ) };
      shader.fragmentShader = paintedCastShadow + shader.fragmentShader;
    }
    if (softTufts) {
      // Reject the empty part of each painted stamp before shadows and probes.
      shader.fragmentShader = shader.fragmentShader.replace("#include <alphatest_fragment>",
        "#include <alphatest_fragment>\nif (diffuseColor.a < 0.02) discard;");
    }
    if (grassField) {
      // As in reliefPaint, lift dark pigment at a steep viewing angle. Use
      // the local growth normal and apply this before lights and cast shadows.
      shader.fragmentShader = shader.fragmentShader.replace("#include <lights_physical_fragment>", `
        float lawnDown = smoothstep(0.3, 0.85, abs(dot(normal, normalize(vViewPosition))));
        // Bring the field's olive pigment into the demo's warm yellow-green range.
        diffuseColor.rgb *= vec3(1.22, 1.53, 0.64);
        float lawnLuma = dot(diffuseColor.rgb, vec3(0.2126, 0.7152, 0.0722));
        diffuseColor.rgb *= mix(1.0, max(0.30 / max(lawnLuma, 1e-4), 1.0), lawnDown);
        #include <lights_physical_fragment>`);
    }
    if (turf) {
      shader.uniforms.turfAlpha = turfAlpha;
      shader.fragmentShader = "uniform float turfAlpha;\n" + shader.fragmentShader.replace("#include <dithering_fragment>",
        "#include <dithering_fragment>\ngl_FragColor.a = turfAlpha;");
    }
    shader.uniforms.cityShadowTone = { value: tone };
    shader.uniforms.cityWrap = { value: style.wrap };
    shader.uniforms.cityForm = { value: new THREE.Vector2(style.form ?? 0, style.formFill ?? 0) };
    if (tintedShade) {
      shader.uniforms.cityShadeColor = { value: new THREE.Vector4().fromArray(shadeColor) };
      shader.fragmentShader = "uniform vec4 cityShadeColor;\n" + shader.fragmentShader;
    }
    shader.fragmentShader = "uniform vec3 cityShadowTone; uniform float cityWrap; uniform vec2 cityForm;\n" + shader.fragmentShader
      .replace("#include <lights_fragment_begin>", "float cityShadowTerm = 1.0;\n"
        + (paintedDiffuse ? "float cityDiffuseShadow = 1.0;\n" : "")
        + (paintCamera ? "float cityCastCoverage = 0.0;\n" : "")
        + (grassField ? "float cityLawnBand = 1.0;\n" : "") + lights)
      .replace("#include <opaque_fragment>", (grassField
        ? "outgoingLight *= mix(vec3(0.30, 0.44, 0.66), vec3(1.0), cityLawnBand);"
        : "outgoingLight = totalEmissiveRadiance + (outgoingLight - totalEmissiveRadiance) * mix(cityShadowTone, vec3(1.0), cityShadowTerm);")
        + (paintedDiffuse ? `
          outgoingLight += totalDiffuse * (1.0 - cityShadowTone) * (cityDiffuseShadow - cityShadowTerm)
            * cityDiffuseStrokeStrength;
        ` : "") + (paintCamera ? `
          vec3 castPigment = mix(vec3(1.), vec3(.88, .97, 1.12), cityCastCoverage);
          outgoingLight = totalEmissiveRadiance + (outgoingLight - totalEmissiveRadiance) * castPigment;
        ` : "") + (tintedShade ? `
          vec3 shadeLight = max(outgoingLight - totalEmissiveRadiance, vec3(0.0));
          float shadeLuma = dot(shadeLight, vec3(0.2126, 0.7152, 0.0722));
          // Let a little reflected color show through even on saturated pigment.
          vec3 shadeFill = mix(shadeLight, vec3(shadeLuma), 0.20) * cityShadeColor.rgb;
          float shadeWeight = (1.0 - cityShadowTerm) * cityShadeColor.a * (1.0 - smoothstep(0.18, 0.75, shadeLuma));
          outgoingLight += (shadeFill - shadeLight) * shadeWeight;
        ` : "") + "\n#include <opaque_fragment>");
  };
  const key = turf ? "worldspec-shadow-form-turf-v2" : grassField ? "worldspec-shadow-form-lawn-v2" : "worldspec-shadow-form-v1";
  material.customProgramCacheKey = () => key + (paintCamera ? "-painted-cast-v1" : "") + (tintedShade ? "-shade-color" : "")
    + (paintedDiffuse ? "-painted-diffuse" : "");
}

/** Apply the exported environment for a scene and return its unload callback. */
export class WorldSpecEnvironment {
  private reflections?: THREE.WebGLRenderTarget;

  apply(scene: THREE.Scene, renderer: THREE.WebGLRenderer | undefined, sun: THREE.DirectionalLight,
    lighting: WorldSpecLighting, castsShadow: boolean): () => void {
    const previous = { environment: scene.environment, intensity: scene.environmentIntensity, rotation: scene.environmentRotation.clone(),
      tone: renderer?.toneMapping, exposure: renderer?.toneMappingExposure, shadow: sun.shadow.intensity };
    const restore = () => {
      scene.environment = previous.environment;
      scene.environmentIntensity = previous.intensity;
      scene.environmentRotation.copy(previous.rotation);
      sun.shadow.intensity = previous.shadow;
      if (renderer) { renderer.toneMapping = previous.tone!; renderer.toneMappingExposure = previous.exposure!; }
    };
    if (renderer) {
      if (lighting.environment.source === "studio") this.reflections ??= studioReflections(renderer);
      scene.environment = lighting.environment.source === "studio" ? this.reflections!.texture : null;
      scene.environmentIntensity = lighting.environment.intensity;
      scene.environmentRotation.set(0, lighting.environment.rotation, 0);
      renderer.toneMapping = { aces: THREE.ACESFilmicToneMapping, neutral: THREE.NeutralToneMapping, none: THREE.LinearToneMapping }[lighting.toneMapping];
      renderer.toneMappingExposure = lighting.exposure;
    }
    sun.shadow.intensity = castsShadow ? previous.shadow : 0;
    return restore;
  }

  dispose(): void {
    this.reflections?.dispose();
    this.reflections = undefined;
  }
}
