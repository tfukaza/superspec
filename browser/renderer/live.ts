import * as THREE from "three";
import type { LiveOperation } from "../model/packet.ts";
import { shadowStyleKey } from "./shadow-style.ts";

/** Operations this renderer executes; others are ignored, like any renderer without support. */
export const SUPPORTED_LIVE_OPERATIONS = ["modulateDiffuse", "grazingFalloff", "filterEnvironment", "bandLuminance", "scaleSpecular", "lightFloor"] as const;

/** Texture identities a material's live operations sample. */
export function liveTextures(operations: LiveOperation[] | undefined): string[] {
  return (operations ?? []).flatMap(operation => Object.values(operation.arguments))
    .flatMap(argument => "texture" in argument ? [argument.texture] : []);
}

/**
 * Executes live operations in a standard material's shader:
 * - modulateDiffuse scales direct and indirect diffuse light by mix(1, factor, mask);
 * - grazingFalloff raises specular F90 toward 1.65 by a mask from facing `from` (0) to `to` (1), times `mask`;
 * - filterEnvironment reflects nine environment taps spread `spread` around the reflection at `roughness`,
 *   keeping the lowest-variance quadrant's mean;
 * - bandLuminance steps reflected luminance into bands `spacing` apart in square-root luminance, by `mask`,
 *   softening bright bands toward `brightSoftness` and lifting dark ones by `lift*`;
 * - scaleSpecular multiplies the blended specular color by mix(1, factor, mask), capped at 1;
 * - lightFloor adds (red, green, blue) * mask, ramped by view facing from `from` to `to` when they differ.
 * Texture arguments are sampled on the material's base color UVs. Returns the executed operation identities.
 */
export function applyLiveOperations(material: THREE.MeshStandardMaterial, operations: LiveOperation[],
  texture: (id: string) => THREE.Texture | null | undefined): string[] {
  const usable = operations.filter(operation => (SUPPORTED_LIVE_OPERATIONS as readonly string[]).includes(operation.id)
    && Object.values(operation.arguments).every(argument => !("texture" in argument) || (material.map && texture(argument.texture))));
  if (!usable.length) return [];
  const uniforms: Record<string, THREE.IUniform> = {};
  const declarations: string[] = [];
  const reads: string[] = [];
  const fetches: string[] = [];
  const slots = new Map<string, number>();
  /** A GLSL expression for an argument, or its default when the operation omits it. */
  const symbols = usable.map((operation, index) => {
    const named: Record<string, string> = {};
    for (const [name, argument] of Object.entries(operation.arguments)) {
      const symbol = `msLive${index}_${name}`;
      named[name] = symbol;
      declarations.push(`float ${symbol};`);
      if ("glsl" in argument) {
        // A shaded material computes the argument itself (see shade.ts).
        reads.push(`${symbol} = ${(argument as { glsl: string }).glsl};`);
      } else if ("value" in argument) {
        uniforms[`${symbol}Value`] = { value: argument.value };
        declarations.push(`uniform float ${symbol}Value;`);
        reads.push(`${symbol} = ${symbol}Value;`);
      } else {
        // Arguments packed in one texture share its sampler and its one fetch: a program has a few dozen arguments, and
        // a fragment shader has a fixed number of texture units.
        let slot = slots.get(argument.texture);
        if (slot === undefined) {
          slot = slots.size;
          slots.set(argument.texture, slot);
          uniforms[`msLiveTex${slot}Map`] = { value: texture(argument.texture) };
          declarations.push(`uniform sampler2D msLiveTex${slot}Map;`, `vec4 msLiveTex${slot};`);
          fetches.push(`msLiveTex${slot} = texture2D(msLiveTex${slot}Map, vMapUv);`);
        }
        uniforms[`${symbol}Range`] = { value: new THREE.Vector2(argument.scale, argument.offset) };
        declarations.push(`uniform vec2 ${symbol}Range;`);
        // A swizzle, not an index: indexing a vec4 with a literal misreads on some drivers (ANGLE on SwiftShader returned
        // values that left every live effect with no visible effect).
        // The argument is one channel of the packed texel. (It used to be a dot product with a THREE.Vector4 whose default
        // w of 1 also added the texture's alpha to every argument, which saturated masks packed in opaque textures.)
        reads.push(`${symbol} = msLiveTex${slot}.${"xyzw"[argument.channel]} * ${symbol}Range.x + ${symbol}Range.y;`);
      }
    }
    return (name: string, fallback: string) => named[name] ?? fallback;
  });
  const lighting: string[] = [], diffuse: string[] = [];
  let filter: { roughness: string; spread: string } | undefined,
    band: { spacing: string; softness: string; mask: string; bright: string; lift: string } | undefined;
  usable.forEach((operation, index) => {
    const arg = symbols[index];
    if (operation.id === "modulateDiffuse") {
      diffuse.push(`{ float k = mix(1.0, ${arg("factor", "1.0")}, clamp(${arg("mask", "1.0")}, 0.0, 1.0));
        reflectedLight.directDiffuse *= k; reflectedLight.indirectDiffuse *= k; }`);
    } else if (operation.id === "grazingFalloff") {
      lighting.push(`{ float span = ${arg("to", "0.0")} - ${arg("from", "1.0")};
        float facing = abs(dot(normal, normalize(vViewPosition)));
        float t = clamp((facing - ${arg("from", "1.0")}) / (abs(span) < 1e-6 ? -1e-6 : span), 0.0, 1.0);
        material.specularF90 = mix(material.specularF90, 1.65, t * t * (3.0 - 2.0 * t) * clamp(${arg("mask", "1.0")}, 0.0, 1.0)); }`);
    } else if (operation.id === "scaleSpecular") {
      lighting.push(`material.specularColorBlended = min(vec3(1.0), material.specularColorBlended
        * mix(1.0, max(${arg("factor", "1.0")}, 0.0), clamp(${arg("mask", "1.0")}, 0.0, 1.0)));`);
    } else if (operation.id === "lightFloor") {
      lighting.push(`{ float start = ${arg("from", "0.0")}, span = ${arg("to", "0.0")} - start;
        float facing = abs(dot(normal, normalize(vViewPosition)));
        float t = abs(span) < 1e-6 ? 1.0 : clamp((facing - start) / span, 0.0, 1.0);
        totalEmissiveRadiance += vec3(${arg("red", "0.0")}, ${arg("green", "0.0")}, ${arg("blue", "0.0")})
          * (t * t * (3.0 - 2.0 * t)) * clamp(${arg("mask", "1.0")}, 0.0, 1.0); }`);
    } else if (operation.id === "filterEnvironment") {
      filter = { roughness: arg("roughness", "-1.0"), spread: arg("spread", "0.0") };
    } else {
      band = { spacing: arg("spacing", "0.25"), softness: arg("softness", "0.05"), mask: arg("mask", "1.0"),
        bright: arg("brightSoftness", "-1.0"), lift: `vec3(${arg("liftRed", "0.0")}, ${arg("liftGreen", "0.0")}, ${arg("liftBlue", "0.0")})` };
    }
  });
  const radiance = filter || band ? `
#if defined( USE_ENVMAP ) && defined( ENVMAP_TYPE_CUBE_UV )
vec3 msLiveTap( vec3 reflectVec, float roughness ) {
  return textureCubeUV( envMap, envMapRotation * transformDirectionByInverseViewMatrix( normalize( reflectVec ), viewMatrix ), roughness ).rgb * envMapIntensity;
}
#endif
vec3 msLiveRadiance( const in vec3 viewDir, const in vec3 normal, const in float roughness ) {
#if defined( USE_ENVMAP ) && defined( ENVMAP_TYPE_CUBE_UV )
  float r = ${filter ? `${filter.roughness} < 0.0 ? roughness : clamp(${filter.roughness}, 0.0, 1.0)` : "roughness"};
  vec3 reflectVec = normalize( mix( reflect( - viewDir, normal ), normal, pow4( r ) ) );
  vec3 filtered = msLiveTap( reflectVec, r );
${filter ? `  if ( ${filter.spread} > 0.0 ) {
    vec3 tangent = cross( vec3( 0.0, 1.0, 0.0 ), reflectVec );
    if ( dot( tangent, tangent ) < 1e-4 ) tangent = cross( vec3( 1.0, 0.0, 0.0 ), reflectVec );
    tangent = normalize( tangent ) * ${filter.spread};
    vec3 across = normalize( cross( reflectVec, tangent ) ) * ${filter.spread} * 0.727;
    vec3 taps[ 9 ];
    for ( int i = 0; i < 9; i ++ ) taps[ i ] = msLiveTap( reflectVec + float( i % 3 - 1 ) * tangent + float( i / 3 - 1 ) * across, r );
    float lowest = 1e20;
    for ( int region = 0; region < 4; region ++ ) {
      int first = region % 2 + 3 * ( region / 2 );
      vec3 mean = ( taps[ first ] + taps[ first + 1 ] + taps[ first + 3 ] + taps[ first + 4 ] ) * 0.25;
      vec3 variance = ( taps[ first ] * taps[ first ] + taps[ first + 1 ] * taps[ first + 1 ]
        + taps[ first + 3 ] * taps[ first + 3 ] + taps[ first + 4 ] * taps[ first + 4 ] ) * 0.25 - mean * mean;
      float score = dot( variance, vec3( 0.2126, 0.7152, 0.0722 ) );
      if ( score < lowest ) { lowest = score; filtered = mean; }
    }
  }` : ""}
${band ? `  {
    float luminance = dot( filtered, vec3( 0.2126, 0.7152, 0.0722 ) );
    float spacing = max( ${band.spacing}, 1e-3 );
    float level = sqrt( max( luminance, 0.0 ) ) / spacing;
    float bright = smoothstep( 0.60, 0.95, ( floor( level ) + 0.5 ) * spacing );
    float softness = clamp( ${band.bright} < 0.0 ? ${band.softness} : mix( ${band.softness}, ${band.bright}, bright ), 0.0, 0.5 );
    float boundary = smoothstep( 0.5 - softness, 0.5 + softness, fract( level ) );
    float stepped = ( floor( level ) + boundary ) * spacing;
    vec3 banded = filtered * ( stepped * stepped / max( luminance, 1e-5 ) );
    banded += ${band.lift} * ( 1.0 - smoothstep( 0.42, 0.87, stepped ) );
    filtered = mix( filtered, banded, clamp( ${band.mask}, 0.0, 1.0 ) );
  }` : ""}
  return filtered;
#elif defined( USE_ENVMAP )
  return getIBLRadiance( viewDir, normal, roughness );
#else
  // No environment (the lighting record's \`environment.source: "none"\`): three.js defines no radiance lookup then,
  // and nothing calls this one.
  return vec3( 0.0 );
#endif
}
` : "";
  const previous = material.onBeforeCompile;
  material.onBeforeCompile = (shader, renderer) => {
    previous.call(material, shader, renderer);
    Object.assign(shader.uniforms, uniforms);
    let fragment = shader.fragmentShader
      .replace("#include <common>", `#include <common>\n${declarations.join("\n")}`)
      .replace("#include <clipping_planes_fragment>", `#include <clipping_planes_fragment>\n${fetches.concat(reads).join("\n")}`)
      .replace("#include <lights_physical_fragment>", `#include <lights_physical_fragment>\n${lighting.join("\n")}`)
      .replace("#include <aomap_fragment>", `#include <aomap_fragment>\n${diffuse.join("\n")}`);
    if (radiance) {
      fragment = fragment
        .replace("#include <envmap_physical_pars_fragment>", `#include <envmap_physical_pars_fragment>\n${radiance}`)
        .replace("#include <lights_fragment_maps>", THREE.ShaderChunk.lights_fragment_maps
          .replace("vec3 iblRadiance = getIBLRadiance( geometryViewDir, geometryNormal, material.roughness );",
            "vec3 iblRadiance = msLiveRadiance( geometryViewDir, geometryNormal, material.roughness );"));
    }
    shader.fragmentShader = fragment;
  };
  const layout = usable.map(operation => [operation.id, Object.entries(operation.arguments)
    .map(([name, argument]) => [name, "value" in argument ? "c" : `t${slots.get((argument as { texture: string }).texture)}.${(argument as { channel: number }).channel}`])]);
  material.customProgramCacheKey = () => `materialSpec-live:${JSON.stringify(layout)}${shadowStyleKey(material)}`;
  // A viewer that swaps the textures behind an argument (a GPU preview) updates these in place, with no recompile.
  material.userData.liveUniforms = uniforms;
  material.userData.liveDescriptors = usable;
  material.userData.liveTextureSlots = slots;
  material.needsUpdate = true;
  return usable.map(operation => operation.id);
}
