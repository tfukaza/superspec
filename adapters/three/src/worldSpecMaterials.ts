import type { MetalFinish } from "./reflectionField";
import * as THREE from "three";
import { paintedMetalSky, radialSkyShader, radialSkyVertex } from "./paintedMetalSky";
import { paintedWorktop, paintedWorktopShader } from "./paintedWorktop";
import { legacyRenderDiagnostics, type RenderDiagnostics } from "./renderProfile";

type LiveSetting = { id: string; arguments: Record<string, { value?: number }> };

/** Published fixture finishes use uniform arguments. Texture masks are resolved by the Material Studio renderer. */
function uniformOperation(material: THREE.Material, id: string, fields: string[]): LiveSetting["arguments"] | undefined {
  const operations: LiveSetting[] = material.userData.materialSpec?.live ?? [];
  const args = operations.find(operation => operation.id === id)?.arguments;
  return args && Object.entries(args).every(([field, arg]) => fields.includes(field) && Number.isFinite(arg.value))
    ? args : undefined;
}

const stockGlaze = /* glsl */ `
  // The catalog properties atlas distinguishes glazed glass from paper, caps and plastic.
  float glass = (1.0 - smoothstep(.43, .54, roughnessFactor))
    * (1.0 - smoothstep(.12, .42, metalnessFactor));
  float paper = smoothstep(.72, .91, roughnessFactor)
    * smoothstep(.063, .20, pigment) * (1.0 - smoothstep(.15, .40, metalnessFactor));
  float amber = smoothstep(.01, .13, (diffuseColor.r - diffuseColor.g) / max(pigment, .045));
  vec3 glaze = mix(vec3(.70, 1.11, .43), vec3(1.46, .90, .40), amber);
  vec3 glassColor = (vec3(.032) + diffuseColor.rgb) * glaze;
  diffuseColor.rgb = mix(diffuseColor.rgb, glassColor, .63 * glass);
  diffuseColor.rgb *= mix(vec3(1.0), vec3(1.15, 1.04, .85), .68 * paper);
  // Bottle shoulders facing the doorway catch a narrow broken daylight reflection.
  float shoulder = smoothstep(.16, .48, paintedNormal.y) * (1.0 - smoothstep(.73, .98, paintedNormal.y));
  float pane = smoothstep(.50, .87, dot(paintedNormal, normalize(vec3(.65, .40, -.55))));
  roughnessFactor = mix(roughnessFactor, .21, shoulder * pane * glass);
`;
const paintedSteel = /* glsl */ `
  // Long patches follow the length of the counter. Low-frequency bands remain
  // legible in shadow and leave the smaller brushed-metal normal map intact.
  float wipe = sin(paintedPosition.z * 7.3 + .80 * sin(paintedPosition.x * 14.0 + paintedPosition.y * 10.3));
  float band = smoothstep(-.31, .48, wipe + .28 * sin(paintedPosition.z * 15.1 + paintedPosition.y * 3.4));
  float plate = smoothstep(.25, .76, metalnessFactor);
  vec3 steel = mix(vec3(.77, .94, 1.15), vec3(1.21, 1.02, .78), band);
  diffuseColor.rgb *= mix(vec3(1.0), steel, .68 * plate);
  roughnessFactor = mix(roughnessFactor, mix(.49, .30, band), .34 * plate);
`;

function styleDisplayGlass(material: THREE.MeshStandardMaterial): void {
  if (material.name !== "builtin:display-glass") return;
  const previous = material.onBeforeCompile;
  const key = material.customProgramCacheKey.bind(material);
  material.onBeforeCompile = function (shader, renderer) {
    previous.call(this, shader, renderer);
    // The interior spots illuminate the stock. The pane retains the
    // surrounding environment and sun contribution.
    const include = "#include <lights_fragment_begin>";
    const source = shader.fragmentShader.replace(include, THREE.ShaderChunk.lights_fragment_begin);
    shader.fragmentShader = source.replace(
      "getSpotLightInfo( spotLight, geometryPosition, directLight );",
      "getSpotLightInfo( spotLight, geometryPosition, directLight ); directLight.color = vec3(0.);",
    );
  };
  material.customProgramCacheKey = () => key() + "|display-glass-reflection";
}

function styleShopFinish(material: THREE.MeshStandardMaterial): void {
  if (!material.map || material.transparent) return;
  const id = material.name;
  const stock = id === "builtin:grocer-props";
  const hero = id === "builtin:market-hero-props";
  const steel = id === "builtin:kitchen-metal" || id === "builtin:kitchen-worktop" || id === "builtin:restaurant-surfaces";
  if (!stock && !hero && !steel) return;
  const previous = material.onBeforeCompile;
  const key = material.customProgramCacheKey.bind(material);
  material.onBeforeCompile = function (shader, renderer) {
    previous.call(this, shader, renderer);
    shader.fragmentShader = shader.fragmentShader.replace("#include <lights_physical_fragment>", `
      // vViewPosition points from the surface to the camera. Undo the
      // view rotation without normalizing it to recover world coordinates.
      vec3 paintedPosition = cameraPosition - vViewPosition * mat3(viewMatrix);
      bool liquor = paintedPosition.x > 12.0 && paintedPosition.x < 21.5
        && paintedPosition.z > 6.0 && paintedPosition.z < 14.5;
      bool roast = paintedPosition.x > 15.0 && paintedPosition.x < 19.3
        && paintedPosition.z > -6.55 && paintedPosition.z < -2.45;
      if (paintedPosition.y < 2.95 && (${steel ? "roast" : hero ? "liquor || roast" : "liquor"})) {
        vec3 paintedNormal = nonPerturbedNormal * mat3(viewMatrix);
        float pigment = dot(diffuseColor.rgb, vec3(.2126, .7152, .0722));
        ${stock ? stockGlaze : steel ? paintedSteel : `
          // Quiet the saturated plastic so the bottle shelves lead the eye.
          float teal = smoothstep(.005, .036, min(diffuseColor.g, diffuseColor.b) - diffuseColor.r);
          vec3 faded = mix(diffuseColor.rgb, vec3(pigment) * vec3(1.03, 1.02, .89), .43);
          diffuseColor.rgb = mix(diffuseColor.rgb, faded, teal);
          roughnessFactor = mix(roughnessFactor, .55, teal * .32);
        `}
      }
      #include <lights_physical_fragment>`);
  };
  material.customProgramCacheKey = () => key() + "|shop-finish-" + id;
}

/** Retain the receiver's derivative normal frame after packing its chart into a larger atlas.
 * The red and green mask channels carry MaterialSpec's grazingFalloff and scaleSpecular
 * arguments. Rust, unpolished paint and unrelated surfaces retain their own response. */
export function applyWorldSpecMaterial(material: THREE.MeshStandardMaterial, shadow?: { form?: number; formFill?: number }, maxAnisotropy = 1,
  finish?: MetalFinish, hasProbe = false, settings: Pick<RenderDiagnostics["materials"], "shopFinish" | "worktopReflection"> =
    legacyRenderDiagnostics().materials): void {
  if (settings.shopFinish) {
    styleDisplayGlass(material);
    styleShopFinish(material);
  }
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
  if (material.userData.worldSpecConcrete && !material.transparent) {
    // The HDR city resolve reads alpha above one as opaque masonry coverage.
    const previous = material.onBeforeCompile, key = material.customProgramCacheKey.bind(material);
    material.onBeforeCompile = function(shader, renderer) {
      previous.call(this, shader, renderer);
      shader.fragmentShader = shader.fragmentShader.replace("#include <opaque_fragment>",
        "#include <opaque_fragment>\ngl_FragColor.a = 1.0625;");
    };
    material.customProgramCacheKey = () => key() + "|concrete-edge-coverage";
  }
  const chart = !!material.userData.materialSpecNormalFrame;
  const metal = (!material.userData.worldSpecSurface || !!finish)
    && (material.metalness > 0 || (finish?.coatingResponse ?? 0) > 0)
    && !material.transparent && !material.userData.worldSpecConcrete;
  const worktop = material.name === "builtin:kitchen-worktop";
  const paintedBench = worktop && settings.worktopReflection !== "strip" && !hasProbe;
  const sky = metal && uniformOperation(material, "virtualSky", ["intensity", "radial", "red", "green", "blue", "verticals"]);
  const radial = (sky && material.map && sky.radial?.value) || 0;
  const brushEdges = metal && material.userData.worldSpecPaintedReflection === true;
  const specularFinish = material.userData.worldSpecSpecularBands;
  const specularBandStrength = brushEdges || specularFinish === true ? 1
    : Number.isFinite(specularFinish) ? THREE.MathUtils.clamp(specularFinish, 0, 1) : 0;
  const specularBands = specularBandStrength > 0;
  const satin = material.userData.worldSpecSatin;
  const hasSatin = Number.isFinite(satin) && satin > 0;
  const rim = metal && uniformOperation(material, "lightFloor", ["red", "green", "blue", "mask", "from", "to"]);
  if (!chart && !metal && !specularBands && !hasSatin) return;
  const edge = material.userData.materialSpecEdge as { from: number; to: number; gain?: number; polish?: number; width?: number; creases?: boolean } | undefined;
  const hasEdge = !!material.aoMap && !!edge;
  const wide = hasEdge && (edge?.width ?? 0) > 0;
  const creases = wide && edge?.creases === true;
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
    if (hasSatin) {
      shader.uniforms.msSatin = { value: Math.min(satin, 1) };
      shader.fragmentShader = "uniform float msSatin;\n" + shader.fragmentShader.replace(
        "#include <metalnessmap_fragment>",
        `#include <metalnessmap_fragment>
        // Use the weathering recipe's metalness to leave oxidized areas matte.
        roughnessFactor *= 1.0 - msSatin * smoothstep(0.10, 0.32, metalnessFactor);`,
      );
    }
    if (chart) {
      shader.vertexShader = "attribute float _ms_aspect; varying float msChartAspect;\n" + shader.vertexShader
        .replace("#include <uv_vertex>", "#include <uv_vertex>\nmsChartAspect = _ms_aspect;");
      shader.fragmentShader = "varying float msChartAspect;\n" + shader.fragmentShader
        .replace("#include <normal_fragment_begin>", THREE.ShaderChunk.normal_fragment_begin
          .replace("vNormalMapUv", "(vNormalMapUv * vec2(1.0, msChartAspect))"));
    }
    if (hasEdge && edge) {
      if (wide) {
        if (creases) {
          shader.vertexShader = "attribute vec4 _ms_crease; varying vec4 msCreases;\n" + shader.vertexShader
            .replace("#include <begin_vertex>", "#include <begin_vertex>\nmsCreases = _ms_crease;");
          shader.fragmentShader = "varying vec4 msCreases;\n" + shader.fragmentShader;
        }
        shader.uniforms.msWearWidth = { value: edge.width };
        shader.vertexShader = "varying vec3 msWearAxis, msWearPoint;\n" + shader.vertexShader
          .replace("#include <begin_vertex>", "#include <begin_vertex>\nmsWearAxis = normalize(modelViewMatrix[0].xyz); msWearPoint = position;");
        shader.fragmentShader = `
          varying vec3 msWearAxis, msWearPoint;
          uniform float msWearWidth;
          float msWearNoise(vec3 p) {
            vec3 i = floor(p), f = fract(p);
            f = f*f*(3.0-2.0*f);
            vec2 a = vec2(dot(i, vec3(127.1,311.7,74.7)), 0.0);
            a.y = a.x + 74.7;
            vec2 x = mix(fract(sin(a)*43758.5453), fract(sin(a+127.1)*43758.5453), f.x);
            vec2 y = mix(fract(sin(a+311.7)*43758.5453), fract(sin(a+438.8)*43758.5453), f.x);
            vec2 z = mix(x,y,f.y);
            return mix(z.x,z.y,f.z);
          }
        ` + shader.fragmentShader.replace("#include <normal_fragment_maps>", `#include <normal_fragment_maps>
          float msWear = 0.56*msWearNoise(msWearPoint*14.0)
                       + 0.29*msWearNoise(msWearPoint*37.0+4.7)
                       + 0.15*msWearNoise(msWearPoint*93.0+9.3);
          vec2 msEnds = vAoMapUv;
          float msWidth = msWearWidth * mix(0.55, 1.45, msWear);
          float msBand = 1.0-smoothstep(0.18*msWidth, msWidth, min(msEnds.x,msEnds.y));
          vec3 msToward = normalize(msWearAxis) * (msEnds.x < msEnds.y ? -1.0 : 1.0);
          ${creases ? `vec4 ds = abs(msCreases);
          float dist = min(min(ds.x,ds.y),min(ds.z,ds.w));
          float fleck = msWearNoise(msWearPoint*180.0);
          float width = msWearWidth * mix(0.60,0.88,0.75*msWear+0.25*fleck);
          float aa = max(0.0003,0.6*fwidth(dist));
          msBand = (1.0-smoothstep(max(0.0,width*0.5-aa),width+aa,dist))
                 * mix(0.30,0.52,msWear);
          vec3 px = dFdx(-vViewPosition), py = dFdy(-vViewPosition);
          vec3 cx = cross(py,normal), cy = cross(normal,px);
          float det = dot(px,cx);
          vec3 grad = (cx*dFdx(dist)+cy*dFdy(dist))*sign(det)/max(abs(det),1e-10);
          msToward = -grad/max(1.0,length(grad));` : ""}
          normal = normalize(normal + msToward*msBand*mix(0.24,0.85,msWear));
          diffuseColor.rgb *= 1.0+0.15*(msWear-0.5);
        `);
      }
      shader.uniforms.msEdgeFacing = { value: new THREE.Vector2(edge.from, edge.to) };
      shader.uniforms.msEdgeFinish = { value: new THREE.Vector2(edge.gain ?? 0.12, edge.polish ?? 0) };
      shader.fragmentShader = "uniform vec2 msEdgeFacing, msEdgeFinish;\n" + shader.fragmentShader
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
          vec2 msEdge = ${wide ? "vec2(msBand, msBand*mix(0.27,1.0,msWear))" : "texture2D(aoMap, vAoMapUv).rg"};
          float span = msEdgeFacing.y - msEdgeFacing.x;
          float facing = abs(dot(normal, normalize(vViewPosition)));
          float t = clamp((facing - msEdgeFacing.x) / (abs(span) < 1e-6 ? -1e-6 : span), 0.0, 1.0);
          material.specularF90 = mix(material.specularF90, 1.08, msSunEdge * t * t * (3.0 - 2.0 * t) * msEdge.r);
          material.specularColorBlended = min(vec3(1.0), material.specularColorBlended * (1.0 + msEdgeFinish.x * msSunEdge * msEdge.g));
          material.roughness = mix(material.roughness, min(material.roughness, msEdgeFinish.y), msEdge.g * step(0.001, msEdgeFinish.y));
        `);
      if (creases) shader.fragmentShader = shader.fragmentShader.replace("#include <lights_fragment_end>",
        `#include <lights_fragment_end>
        #if NUM_DIR_LIGHTS > 0
        float creaseGlint = pow(max(0.0,dot(normal,normalize(directionalLights[0].direction+normalize(vViewPosition)))),9.0);
        reflectedLight.directSpecular += directionalLights[0].color * msSunEdge * msBand * creaseGlint * msEdgeFinish.x;
        #endif`);
    }
    if (metal) {
      shader.uniforms.msSkyStrength = { value: (finish?.reflection ?? 1) * (brushEdges ? 1.55 : 1) };
      shader.uniforms.msCoatingResponse = { value: finish?.coatingResponse ?? 0 };
      shader.uniforms.msPaintedSky = { value: paintedMetalSky(material.userData.materialSpec?.broadReflectionBands === true, brushEdges) };
      shader.uniforms.msSkyMipScale = { value: brushEdges ? 1 : 3 };
      shader.uniforms.msSkyTint = {value: new THREE.Vector3((sky ? sky.red?.value ?? 1 : 1)*(finish?.skyRed ?? 1), (sky ? sky.green?.value ?? 1 : 1)*(finish?.skyGreen ?? 1), (sky ? sky.blue?.value ?? 1 : 1)*(finish?.skyBlue ?? 1))};
      if (paintedBench) {
        shader.uniforms.msWorktopPanorama = { value: paintedWorktop(settings.worktopReflection) };
        shader.fragmentShader = shader.fragmentShader.replace("#include <common>",
          `#include <common>\n${paintedWorktopShader}`);
      }
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
      shader.fragmentShader = "uniform float msSkyStrength, msCoatingResponse, msSkyMipScale; uniform vec3 msSkyTint; uniform sampler2D msPaintedSky;\n"
        + (sky ? "uniform float msSkyIntensity;\n" : "")
        + (rim ? "uniform vec3 msRimColor, msRimRamp;\n" : "") + shader.fragmentShader
        .replace("#include <aomap_fragment>", `#include <aomap_fragment>
          vec3 msView = isOrthographic ? vec3(0.0, 0.0, 1.0) : normalize(vViewPosition);
          vec3 msReflected = transformDirectionByInverseViewMatrix(reflect(-msView, normal), viewMatrix);
          vec3 msPaint = msSkyTint * textureLod(msPaintedSky, equirectUv(msReflected), msSkyMipScale * material.roughness * material.roughness).rgb;
          float msGrazing = 1.0 - smoothstep(0.045, 0.52, abs(dot(normal, msView)));
          float msShine = 1.0 - material.roughness;
          float msSkyWeight = max(msCoatingResponse, smoothstep(0.09, 0.48, material.metalness))
            * (0.055 * msShine + msGrazing * (0.12 + 0.52 * msShine));
          ${worktop && !hasProbe ? `// Stretch the reflected fixture along the bench, with a brushed falloff across it.
          float msCeilingStrip = 1.0 - smoothstep(0.045, 0.29, abs(msReflected.y - 0.33));
          float msUpward = smoothstep(0.45, 0.95, dot(normal, viewMatrix[1].xyz));
          float msBenchReflection = msUpward * msCeilingStrip * smoothstep(0.12, 0.7, abs(msReflected.x));
          vec3 msBenchTint = vec3(1.0, 0.93, 0.78) * (0.055 + 0.51 * msBenchReflection);
          ${paintedBench ? `
          vec3 msBenchPosition = cameraPosition - vViewPosition * mat3(viewMatrix);
          vec2 msBayIn = smoothstep(vec2(15.75, -6.56), vec2(16.10, -6.31), msBenchPosition.xz);
          vec2 msBayOut = 1.0 - smoothstep(vec2(18.98, -2.55), vec2(19.24, -2.29), msBenchPosition.xz);
          float msBay = msBayIn.x * msBayIn.y * msBayOut.x * msBayOut.y;
          if (msBay * msUpward > .001) {
            vec3 paint = msWorktopPaint(msBenchPosition, msReflected, material.roughness);
            vec3 reflection = .035 + paint * (.77 + .18 * msBenchReflection);
            float proxyValue = dot(paint, vec3(.2126, .7152, .0722));
            reflectedLight.directSpecular *= mix(1.0, mix(.65, 1.0, smoothstep(.08, .51, proxyValue)), msBay * msUpward);
            msBenchTint = mix(msBenchTint, reflection, msBay * msUpward);
          }` : ""}
          reflectedLight.indirectSpecular += msBenchTint * material.specularColorBlended * msShine;` : ""}
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
    if (specularBands) {
      shader.uniforms.msSpecularBandStrength = { value: specularBandStrength };
      shader.fragmentShader = "uniform float msSpecularBandStrength;\n" + shader.fragmentShader.replace("vec3 totalSpecular =", `
        // Step the combined specular grade while preserving its RGB proportions.
        vec3 specularLuma = vec3(.2126,.7152,.0722);
        float specularValue = max(0., dot(reflectedLight.directSpecular + reflectedLight.indirectSpecular, specularLuma));
        float brushOffset = ${metal ? ".36 * (dot(msPaint, specularLuma) - .45)" : "0."};
        float grade = sqrt(specularValue) * 3.6 + brushOffset;
        float bandAA = clamp(fwidth(grade) * .65, .105, .24);
        float band = max(0., floor(grade) + smoothstep(.50-bandAA,.50+bandAA,fract(grade)) - brushOffset);
        float bandValue = band / 3.6;
        float specularGain = mix(1., bandValue * bandValue / max(specularValue, .0001), .43 * msSpecularBandStrength);
        reflectedLight.directSpecular *= specularGain;
        reflectedLight.indirectSpecular *= specularGain;
        vec3 totalSpecular =`);
    }
  };
  material.customProgramCacheKey = () => previousKey() + "|worldspec-chart-v12-" + [chart, hasEdge, wide, creases, metal, serviceForm, !!sky, !!rim, !!radial, worktop, paintedBench, !!finish, hasProbe, brushEdges, specularBands, hasSatin].map(Number).join("-");
}
