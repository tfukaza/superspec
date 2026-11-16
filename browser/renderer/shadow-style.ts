import * as THREE from "three";

/**
 * Stylized directional shadows for standard materials: a wide soft penumbra, a large-scale noise that breaks the
 * shadow's edge and mottles its interior, and a tint, so shadowed direct light leans toward a color instead of
 * vanishing (an illustration look). Renderer-side only: it patches a material's shader where three.js applies the
 * directional shadow, and every parameter is a uniform, so changing the style never recompiles a program; only
 * turning the style on or off does. Lit areas are untouched: everything acts where the shadow term is below one.
 *
 * The noise lives in the light's shadow-map plane, in meters, like a gobo in front of the sun: it is stable in the
 * world and does not move with the camera. It is procedural, so the style uses no texture unit.
 *
 * The same module is meant for other three.js hosts (a game runtime): construct one `ShadowStyle`, call `apply` on
 * each standard material and `fit` whenever the light's shadow camera changes.
 */
export interface ShadowStyleParameters {
  /** Penumbra radius on a receiver facing the light, meters. */
  softness: number;
  /** Size of the noise features, meters. */
  noiseScale: number;
  /** How far the noise displaces the shadow's edge, meters. */
  edgeNoise: number;
  /** How much the noise lifts the shadow's interior, 0 to 1. */
  noiseAmount: number;
  /** Color the shadowed direct light leans to (linear RGB when given as a `THREE.Color`). */
  tint: THREE.ColorRepresentation;
  /** Share of the direct light that remains, tinted, in full shadow, 0 to 1. */
  tintStrength: number;
  seed: number;
}

/** Named looks. `illustration` is the prototype's default look; the viewer's plain shadows use no style at all. */
export const SHADOW_STYLES: Record<string, ShadowStyleParameters> = {
  // The tint strength is small because the viewer's sun is bright (intensity 3): 6% of it already reads as a blue shadow.
  illustration: { softness: 0.02, noiseScale: 0.6, edgeNoise: 0.02, noiseAmount: 0.3, tint: "#3f6cff", tintStrength: 0.06, seed: 7 },
};

/** A style from page parameters: `shadowstyle=<name>` with optional overrides, or null for plain shadows. */
export function shadowStyleFromQuery(query: URLSearchParams): ShadowStyleParameters | null {
  const preset = SHADOW_STYLES[query.get("shadowstyle") ?? ""];
  if (!preset) return null;
  const number = (name: string, fallback: number, min: number, max: number): number => {
    const value = Number(query.get(name) ?? NaN);
    return Number.isFinite(value) ? Math.min(Math.max(value, min), max) : fallback;
  };
  const tint = query.get("shadowtint");
  return {
    softness: number("shadowblur", preset.softness, 0, 1),
    noiseScale: number("shadownoisescale", preset.noiseScale, 0.01, 100),
    edgeNoise: number("shadowedge", preset.edgeNoise, 0, 1),
    noiseAmount: number("shadownoise", preset.noiseAmount, 0, 1),
    tint: tint && /^[0-9a-f]{6}$/i.test(tint) ? `#${tint}` : preset.tint,
    tintStrength: number("shadowtintstrength", preset.tintStrength, 0, 1),
    seed: number("shadowseed", preset.seed, 0, 4294967295),
  };
}

const DECLARATIONS = /* glsl */`
#if defined( USE_SHADOWMAP ) && defined( SHADOWMAP_TYPE_PCF ) && NUM_DIR_LIGHT_SHADOWS > 0
uniform float msShadowSoftness;
uniform float msShadowExtent;
uniform float msShadowNoiseScale;
uniform float msShadowEdgeNoise;
uniform float msShadowNoiseAmount;
uniform vec3 msShadowTint;
uniform float msShadowSeed;
// The styled shadow term of the last directional light shaded: 1 lit, 0 in full shadow. Later code may read it as a mask.
float msShadowTerm = 1.0;
float msShadowLattice( ivec2 cell, uint seed ) {
	uint h = ( uint( cell.x ) * 374761393u ) ^ ( uint( cell.y ) * 668265263u ) ^ seed;
	h = ( h ^ ( h >> 13 ) ) * 1274126177u;
	return float( h ^ ( h >> 16 ) ) * ( 1.0 / 4294967296.0 );
}
float msShadowNoise( vec2 p, uint seed ) {
	vec2 cell = floor( p ), f = p - cell;
	f = f * f * ( 3.0 - 2.0 * f );
	ivec2 c = ivec2( cell );
	return mix( mix( msShadowLattice( c, seed ), msShadowLattice( c + ivec2( 1, 0 ), seed ), f.x ),
		mix( msShadowLattice( c + ivec2( 0, 1 ), seed ), msShadowLattice( c + ivec2( 1, 1 ), seed ), f.x ), f.y );
}
// Two octaves, so the pattern has large shapes with some finer breakup.
float msShadowFbm( vec2 p, uint seed ) {
	return ( msShadowNoise( p, seed ) * 2.0 + msShadowNoise( p * 2.03 + 17.3, seed + 131u ) ) / 3.0;
}
vec3 msStyledShadow( sampler2DShadow shadowMap, vec2 shadowMapSize, float shadowIntensity, float shadowBias, vec4 shadowCoord ) {
	shadowCoord.xyz /= shadowCoord.w;
	shadowCoord.z += shadowBias;
	bool inFrustum = shadowCoord.x >= 0.0 && shadowCoord.x <= 1.0 && shadowCoord.y >= 0.0 && shadowCoord.y <= 1.0;
	if ( ! ( inFrustum && shadowCoord.z <= 1.0 ) ) { msShadowTerm = 1.0; return vec3( 1.0 ); }
	// Meters across the shadow map's plane, as a gobo in front of the light would be laid out.
	vec2 meters = shadowCoord.xy * msShadowExtent;
	uint seed = uint( msShadowSeed );
	vec2 field = meters / max( msShadowNoiseScale, 1e-4 );
	// The edge noise moves the lookup, so the outline wobbles instead of the whole shadow fading.
	vec2 wobble = vec2( msShadowFbm( field, seed ), msShadowFbm( field + 41.7, seed + 977u ) ) - 0.5;
	vec2 at = shadowCoord.xy + wobble * ( 2.0 * msShadowEdgeNoise / msShadowExtent );
	// A fixed rotation per shadow texel: the sampling pattern is stable in the world, unlike a screen-space dither.
	float phi = msShadowLattice( ivec2( floor( at * shadowMapSize ) ), 12345u ) * PI2;
	float radius = max( msShadowSoftness / msShadowExtent, 1.0 / shadowMapSize.x );
	float lit = 0.0;
	for ( int tap = 0; tap < 16; tap ++ ) lit += texture( shadowMap, vec3( at + vogelDiskSample( tap, 16, phi ) * radius, shadowCoord.z ) );
	lit *= 1.0 / 16.0;
	// The interior mottle only lifts shadow, in large patches; it cannot darken lit ground.
	float mottle = msShadowFbm( field * 0.61 + 9.2, seed + 4409u );
	lit = clamp( lit + ( 1.0 - lit ) * msShadowNoiseAmount * smoothstep( 0.35, 0.8, mottle ), 0.0, 1.0 );
	lit = mix( 1.0, lit, shadowIntensity );
	msShadowTerm = lit;
	return mix( msShadowTint, vec3( 1.0 ), lit );
}
#endif
`;

const PLAIN = "directLight.color *= ( directLight.visible && receiveShadow ) ? getShadow( directionalShadowMap[ i ], directionalLightShadow.shadowMapSize, directionalLightShadow.shadowIntensity, directionalLightShadow.shadowBias, directionalLightShadow.shadowRadius, vDirectionalShadowCoord[ i ] ) : 1.0;";
const STYLED = /* glsl */`
		#if defined( SHADOWMAP_TYPE_PCF )
		if ( directLight.visible && receiveShadow ) directLight.color *= msStyledShadow( directionalShadowMap[ i ], directionalLightShadow.shadowMapSize, directionalLightShadow.shadowIntensity, directionalLightShadow.shadowBias, vDirectionalShadowCoord[ i ] );
		#else
		${PLAIN}
		#endif
`;

/** Appended to a material's program cache key by every patch that sets its own key (live.ts, shade.ts). */
export function shadowStyleKey(material: THREE.Material): string {
  return material.userData.shadowStyle ? "|shadow-style" : "";
}

export class ShadowStyle {
  /** Shared by every styled material: assigning a value restyles all of them with no recompile. */
  readonly uniforms: Record<string, THREE.IUniform>;
  private softness: number;
  private edgeNoise: number;

  constructor(parameters: ShadowStyleParameters) {
    this.softness = parameters.softness; this.edgeNoise = parameters.edgeNoise;
    this.uniforms = {
      msShadowSoftness: { value: parameters.softness }, msShadowExtent: { value: 1 },
      msShadowNoiseScale: { value: parameters.noiseScale }, msShadowEdgeNoise: { value: parameters.edgeNoise },
      msShadowNoiseAmount: { value: parameters.noiseAmount },
      msShadowTint: { value: new THREE.Color(parameters.tint).multiplyScalar(parameters.tintStrength) },
      msShadowSeed: { value: Math.floor(parameters.seed) },
    };
  }

  /** Changes the look in place. */
  set(parameters: ShadowStyleParameters): void {
    this.softness = parameters.softness; this.edgeNoise = parameters.edgeNoise;
    this.uniforms.msShadowSoftness.value = parameters.softness;
    this.uniforms.msShadowNoiseScale.value = parameters.noiseScale;
    this.uniforms.msShadowEdgeNoise.value = parameters.edgeNoise;
    this.uniforms.msShadowNoiseAmount.value = parameters.noiseAmount;
    (this.uniforms.msShadowTint.value as THREE.Color).set(parameters.tint).multiplyScalar(parameters.tintStrength);
    this.uniforms.msShadowSeed.value = Math.floor(parameters.seed);
  }

  /** Tells the style how many meters the light's (orthographic) shadow map spans, so softness and noise stay in meters. */
  fit(light: THREE.DirectionalLight): void {
    const camera = light.shadow.camera;
    this.uniforms.msShadowExtent.value = Math.max(camera.right - camera.left, 1e-3);
  }

  /**
   * Patches a standard material. It chains onto whatever `onBeforeCompile` the material has and only touches the
   * shadow-map declarations and the directional shadow line, so it composes with other patches in either order.
   * Patches that set `customProgramCacheKey` afterwards include `shadowStyleKey(material)`.
   */
  apply(material: THREE.Material): void {
    material.userData.shadowStyle = true;
    const previous = material.onBeforeCompile;
    const uniforms = this.uniforms;
    material.onBeforeCompile = (shader, renderer) => {
      previous.call(material, shader, renderer);
      const lights = THREE.ShaderChunk.lights_fragment_begin;
      // If three.js changes the line this replaces, the material keeps its plain shadows.
      if (!lights.includes(PLAIN) || !shader.fragmentShader.includes("#include <lights_fragment_begin>")
        || !shader.fragmentShader.includes("#include <shadowmap_pars_fragment>")) return;
      Object.assign(shader.uniforms, uniforms);
      shader.fragmentShader = shader.fragmentShader
        .replace("#include <shadowmap_pars_fragment>", `#include <shadowmap_pars_fragment>\n${DECLARATIONS}`)
        .replace("#include <lights_fragment_begin>", lights.replace(PLAIN, STYLED));
    };
    const key = material.customProgramCacheKey;
    material.customProgramCacheKey = () => `${key.call(material)}|shadow-style`;
    material.needsUpdate = true;
  }
}
