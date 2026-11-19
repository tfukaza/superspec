import * as THREE from "three";
import type { LightShadow, Rgb } from "./lighting.ts";

/**
 * Stylized directional shadows for standard materials (an illustration look): the shadowed region is darker than a
 * plain shadow and colored, its edge is a gradient that widens with the distance to whatever casts it, and a
 * large-scale noise makes the edge wobble and varies darkness and hue inside the shadow. Renderer-side only: it
 * patches a material's shader where three.js applies the directional shadow and where the shaded color is final,
 * and every parameter is a uniform, so changing the style never recompiles a program; only turning the style on or
 * off does. Lit areas are untouched: everything acts where the shadow term is below one.
 *
 * The color is a multiply on the final shaded color, so it also colors the ambient and environment light in the
 * shadow. Faces turned away from the light are unlit without being in the shadow map; a soft facing term joins them
 * to the cast shadows, so both read as one shade.
 *
 * Form light (`form`, experimental, off at 0): inside a cast shadow a share of the light's diffuse shading is kept, from
 * the light's compass direction level with the ground, so a prop standing in shadow shows a lit and a dark side while the
 * ground keeps its shadow; `formFill` adds a weaker one from the opposite side. Both are tinted and darkened like the
 * shadow they are in, and compiled in only while one of them is above 0. Painted materials shade themselves and take
 * neither.
 *
 * The gradient is contact-hardening. three.js binds the shadow map as a comparison sampler, which cannot return a
 * depth, so the distance to the occluder is estimated by comparing against a few depths nearer the light and
 * integrating how much of a search disk is still occluded at each (the mean occluder distance, without a raw depth
 * read or a second texture unit).
 *
 * The noise lives in the light's shadow-map plane, in meters, like a gobo in front of the sun: it is stable in the
 * world and does not move with the camera. It is procedural, so the style uses no texture unit.
 *
 * The look is the `shadow` of a light in the lighting record (`lighting.ts`); its `illustration` default is a tight
 * edge that still widens a little with distance and a nearly neutral shade with a slight cool cast. With the record's
 * `plain` style the patch stays out of the shader and three.js's own shadow is drawn.
 *
 * The same module is meant for other three.js hosts (a game runtime): construct one `ShadowStyle` from the sun's
 * `shadow`, call `apply` on each standard material and `fit` whenever the light's shadow camera changes.
 */

const DECLARATIONS = /* glsl */`
#if defined( USE_SHADOWMAP ) && defined( SHADOWMAP_TYPE_PCF ) && NUM_DIR_LIGHT_SHADOWS > 0
#define MS_SHADOW_STYLE
uniform float msShadowSoftness;
uniform float msShadowSpread;
uniform float msShadowSoftnessMax;
uniform float msShadowExtent;
uniform float msShadowDepthRange;
uniform float msShadowNoiseScale;
uniform float msShadowEdgeNoise;
uniform float msShadowNoiseAmount;
uniform vec3 msShadowColor;
uniform vec3 msShadowColorAlt;
uniform float msShadowWrap;
uniform float msShadowSeed;
uniform float msShadowForm;
uniform float msShadowFormFill;
uniform vec3 msShadowUp;
// The styled shadow term of the pixel: 1 lit, 0 in full shadow (cast or facing away). Later code may read it as a mask.
float msShadowTerm = 1.0;
// What the shaded color is multiplied by in full shadow, with the interior noise applied.
vec3 msShadowShade = vec3( 1.0 );
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
// Large shapes with a smaller octave breaking them up.
float msShadowFbm( vec2 p, uint seed ) {
	return ( msShadowNoise( p, seed ) * 2.0 + msShadowNoise( p * 2.9 + 17.3, seed + 131u ) ) / 3.0;
}
// Meters across the shadow map's plane over the noise size, as a gobo in front of the light would be laid out.
vec2 msShadowField( vec4 shadowCoord ) {
	return shadowCoord.xy / shadowCoord.w * msShadowExtent / max( msShadowNoiseScale, 1e-4 );
}
// Darker and differently colored patches inside the shadow. The shade never exceeds the base color, so noise cannot lift a shadow.
void msShadowInterior( vec4 shadowCoord ) {
	vec2 field = msShadowField( shadowCoord );
	uint seed = uint( msShadowSeed );
	float dark = smoothstep( 0.35, 0.65, msShadowFbm( field * 0.83 + 9.2, seed + 4409u ) );
	float hue = smoothstep( 0.35, 0.65, msShadowFbm( field * 0.57 + 31.4, seed + 7723u ) );
	msShadowShade = mix( msShadowColor, msShadowColorAlt, hue * min( 1.0, 2.0 * msShadowNoiseAmount ) ) * ( 1.0 - msShadowNoiseAmount * dark );
}
float msStyledShadow( sampler2DShadow shadowMap, vec2 shadowMapSize, float shadowIntensity, float shadowBias, vec4 shadowCoord ) {
	vec2 field = msShadowField( shadowCoord );
	shadowCoord.xyz /= shadowCoord.w;
	// How the receiver's depth changes across the shadow map. Lookups this far from the pixel compare against
	// the receiver's own plane, or a receiver slanted to the light would shadow itself.
	vec2 slope;
	{
		vec3 across = dFdx( shadowCoord.xyz ), down = dFdy( shadowCoord.xyz );
		float det = across.x * down.y - across.y * down.x;
		slope = abs( det ) > 1e-20 ? vec2( down.y * across.z - across.y * down.z, across.x * down.z - down.x * across.z ) / det : vec2( 0.0 );
		// Nearly edge-on to the light the plane is too steep to follow.
		float steepest = 4.0 * msShadowExtent / msShadowDepthRange;
		slope *= min( 1.0, steepest / max( length( slope ), 1e-9 ) );
	}
	shadowCoord.z += shadowBias;
	bool inFrustum = shadowCoord.x >= 0.0 && shadowCoord.x <= 1.0 && shadowCoord.y >= 0.0 && shadowCoord.y <= 1.0;
	if ( ! ( inFrustum && shadowCoord.z <= 1.0 ) ) return 1.0;
	uint seed = uint( msShadowSeed );
	// The edge noise moves the lookup, so the outline wobbles instead of the whole shadow fading.
	vec2 wobble = vec2( msShadowFbm( field, seed ), msShadowFbm( field + 41.7, seed + 977u ) ) - 0.5;
	vec2 moved = wobble * ( 2.0 * msShadowEdgeNoise / msShadowExtent );
	vec2 at = shadowCoord.xy + moved;
	shadowCoord.z += dot( slope, moved );
	// A fixed rotation per shadow texel: the sampling pattern is stable in the world, unlike a screen-space dither.
	float phi = msShadowLattice( ivec2( floor( at * shadowMapSize ) ), 12345u ) * PI2;
	float texel = 1.0 / shadowMapSize.x;
	// Mean distance to the occluders around this point. The comparison sampler only answers "is anything nearer
	// the light than this depth", so it is asked at four depths in front of the receiver; the share of the search
	// disk still occluded at each, integrated over distance, is the occluders' mean distance times their share.
	float search = max( msShadowSoftnessMax / msShadowExtent, texel );
	vec4 occluded = vec4( 0.0 );
	for ( int tap = 0; tap < 8; tap ++ ) {
		vec2 offset = vogelDiskSample( tap, 8, phi ) * search;
		vec2 uv = at + offset;
		float depth = shadowCoord.z + dot( slope, offset );
		occluded += 1.0 - vec4(
			texture( shadowMap, vec3( uv, depth - 0.03 / msShadowDepthRange ) ),
			texture( shadowMap, vec3( uv, depth - 0.15 / msShadowDepthRange ) ),
			texture( shadowMap, vec3( uv, depth - 0.5 / msShadowDepthRange ) ),
			texture( shadowMap, vec3( uv, depth - 1.4 / msShadowDepthRange ) ) );
	}
	float distance = occluded.x > 0.0 ? 0.03 + dot( occluded, vec4( 0.12, 0.35, 0.9, 1.6 ) ) / occluded.x : 0.0;
	float radius = clamp( msShadowSoftness + msShadowSpread * distance, 0.0, max( msShadowSoftnessMax, msShadowSoftness ) );
	radius = max( radius / msShadowExtent, texel );
	float lit = 0.0;
	for ( int tap = 0; tap < 32; tap ++ ) {
		vec2 offset = vogelDiskSample( tap, 32, phi ) * radius;
		lit += texture( shadowMap, vec3( at + offset, shadowCoord.z + dot( slope, offset ) ) );
	}
	lit *= 1.0 / 32.0;
	// An eased falloff reads as a painted gradient instead of a linear ramp.
	lit = smoothstep( 0.0, 1.0, lit );
	return mix( 1.0, lit, shadowIntensity );
}
#endif
`;

const PLAIN = "directLight.color *= ( directLight.visible && receiveShadow ) ? getShadow( directionalShadowMap[ i ], directionalLightShadow.shadowMapSize, directionalLightShadow.shadowIntensity, directionalLightShadow.shadowBias, directionalLightShadow.shadowRadius, vDirectionalShadowCoord[ i ] ) : 1.0;";
const STYLED = /* glsl */`
		#if defined( MS_SHADOW_STYLE )
		if ( directLight.visible ) {
			float msCast = receiveShadow ? msStyledShadow( directionalShadowMap[ i ], directionalLightShadow.shadowMapSize, directionalLightShadow.shadowIntensity, directionalLightShadow.shadowBias, vDirectionalShadowCoord[ i ] ) : 1.0;
			#if defined( MS_SHADOW_FORM ) && ! defined( MS_PAINTED )
			vec3 msSunColor = directLight.color;
			#endif
			directLight.color *= msCast;
			// Faces turned away from the light join the cast shadows, over a soft band instead of at the terminator.
			msShadowTerm = min( msShadowTerm, msCast * smoothstep( 0.0, msShadowWrap, dot( geometryNormal, directLight.direction ) ) );
			msShadowInterior( vDirectionalShadowCoord[ i ] );
			#if defined( MS_SHADOW_FORM ) && ! defined( MS_PAINTED )
			// Form light: where something else shades the surface, a share of the light's diffuse shading is kept, so a
			// side turned to the light stays brighter than one turned away. It comes from the light's compass direction,
			// level with the ground: the ground, a roof or an awning's top gets none and keeps its cast shadow in full,
			// while the sides of what stands on it are told apart. It is added before the shadow's shade multiplies the
			// color (FINAL), so it is darkened and tinted like the rest of the shadow.
			vec3 msUp = normalize( ( viewMatrix * vec4( msShadowUp, 0.0 ) ).xyz );
			vec3 msLevel = directLight.direction - dot( directLight.direction, msUp ) * msUp;
			// A light nearly overhead has no compass direction, and gives no form light.
			float msAcross = length( msLevel );
			msLevel *= smoothstep( 0.05, 0.25, msAcross ) / max( msAcross, 1e-4 );
			float msToward = dot( geometryNormal, msLevel );
			vec3 msFormLight = msSunColor * BRDF_Lambert( material.diffuseColor );
			reflectedLight.directDiffuse += ( msShadowForm * ( 1.0 - msCast ) * saturate( msToward ) ) * msFormLight;
			// Its fill, from the opposite side, on what is in shadow or turned from the light: the far sides are graded too.
			float msShaded = 1.0 - msCast * smoothstep( 0.0, msShadowWrap, dot( geometryNormal, directLight.direction ) );
			reflectedLight.directDiffuse += ( msShadowFormFill * msShaded * saturate( - msToward ) ) * msFormLight;
			#endif
		}
		#else
		${PLAIN}
		#endif
`;
/**
 * The sun's cast shadow at the drawn point, 1 lit and 0 shadowed, in the scene's shadow style: the styled term where
 * the style is on, three.js's own where it is plain. A painted material (shade.ts) folds it into its shade factor
 * (`shadeBand`), so painted and lit surfaces share one shadow shape: the same softness, spread and edge noise. Only the
 * cast part: a painted material's own band decides where a surface turns from the sun. Insert it after
 * `#include <shadowmap_pars_fragment>` before `apply` patches the material, so the style's declarations precede it.
 */
export const CAST_SHADOW = /* glsl */`
#if defined( USE_SHADOWMAP ) && NUM_DIR_LIGHT_SHADOWS > 0
float msCastShadow() {
	if ( ! receiveShadow ) return 1.0;
	DirectionalLightShadow msSun = directionalLightShadows[ 0 ];
	#if defined( MS_SHADOW_STYLE )
	return msStyledShadow( directionalShadowMap[ 0 ], msSun.shadowMapSize, msSun.shadowIntensity, msSun.shadowBias, vDirectionalShadowCoord[ 0 ] );
	#else
	return getShadow( directionalShadowMap[ 0 ], msSun.shadowMapSize, msSun.shadowIntensity, msSun.shadowBias, msSun.shadowRadius, vDirectionalShadowCoord[ 0 ] );
	#endif
}
#else
float msCastShadow() { return 1.0; }
#endif
`;

/**
 * Where the shaded color is final: the shadow multiplies everything but emission. A painted material (`MS_PAINTED`)
 * has folded the cast shadow into its own shade factor and keeps its authored color.
 */
const FINAL = /* glsl */`
	#if defined( MS_SHADOW_STYLE ) && ! defined( MS_PAINTED )
	outgoingLight = totalEmissiveRadiance + ( outgoingLight - totalEmissiveRadiance ) * mix( msShadowShade, vec3( 1.0 ), msShadowTerm );
	#endif
	#include <opaque_fragment>`;

/** The style each patched material belongs to. */
const styles = new WeakMap<THREE.Material, ShadowStyle>();

/** Appended to a material's program cache key by every patch that sets its own key (live.ts, shade.ts). */
export function shadowStyleKey(material: THREE.Material): string {
  return styles.get(material)?.key ?? "";
}

/** What the shaded color is multiplied by in full shadow: the tint at full brightness, desaturated by `1 - strength`, then darkened. */
function shade(target: THREE.Color, tint: Rgb, shadow: LightShadow): THREE.Color {
  target.setRGB(tint[0], tint[1], tint[2], THREE.LinearSRGBColorSpace);
  const peak = Math.max(target.r, target.g, target.b, 1e-6);
  return target.multiplyScalar(1 / peak).lerp(new THREE.Color(1, 1, 1), 1 - shadow.tintStrength).multiplyScalar(1 - shadow.darkness);
}

export class ShadowStyle {
  /** Whether patched materials draw the styled shadow (the record's `illustration`) or three.js's plain one. */
  private styled = true;
  /**
   * Whether the form light is compiled in (`form` or `formFill` above 0). It is left out of the program otherwise, so
   * a scene without it draws exactly the pixels it drew before the form light existed.
   */
  private formed = false;
  /** Every material patched so far, to recompile them when the style is turned on or off. */
  private readonly materials = new Set<WeakRef<THREE.Material>>();

  /** Shared by every styled material: assigning a value restyles all of them with no recompile. */
  readonly uniforms: Record<string, THREE.IUniform> = {
    msShadowSoftness: { value: 0 }, msShadowSpread: { value: 0 }, msShadowSoftnessMax: { value: 0 },
    msShadowExtent: { value: 1 }, msShadowDepthRange: { value: 1 },
    msShadowNoiseScale: { value: 1 }, msShadowEdgeNoise: { value: 0 }, msShadowNoiseAmount: { value: 0 },
    msShadowColor: { value: new THREE.Color() }, msShadowColorAlt: { value: new THREE.Color() },
    msShadowWrap: { value: 0.3 }, msShadowSeed: { value: 0 },
    // Form light in shadow and its fill (0 is none), and the world's up axis, which the form light is level to.
    msShadowForm: { value: 0 }, msShadowFormFill: { value: 0 }, msShadowUp: { value: new THREE.Vector3(0, 0, 1) },
  };

  constructor(shadow: LightShadow) {
    this.set(shadow);
  }

  get enabled(): boolean { return this.styled; }
  /** What a patched material's program depends on: whether the style is drawn, and whether with form light. */
  get key(): string { return this.styled ? (this.formed ? "|shadow-style|form" : "|shadow-style") : ""; }

  /**
   * Changes the look in place. Values are uniforms, so they cost nothing; only a change between `plain` and
   * `illustration`, or turning the form light on or off, recompiles the patched materials' programs.
   */
  set(shadow: LightShadow): void {
    const uniforms = this.uniforms;
    uniforms.msShadowSoftness.value = shadow.softness;
    uniforms.msShadowSpread.value = shadow.spread;
    uniforms.msShadowSoftnessMax.value = shadow.maxSoftness;
    uniforms.msShadowNoiseScale.value = shadow.noise.scale;
    uniforms.msShadowEdgeNoise.value = shadow.noise.edge;
    uniforms.msShadowNoiseAmount.value = shadow.noise.amount;
    shade(uniforms.msShadowColor.value as THREE.Color, shadow.tint, shadow);
    shade(uniforms.msShadowColorAlt.value as THREE.Color, shadow.tintAlt, shadow);
    uniforms.msShadowWrap.value = Math.max(shadow.wrap, 1e-3);
    uniforms.msShadowSeed.value = Math.floor(shadow.noise.seed);
    uniforms.msShadowForm.value = shadow.form ?? 0;
    uniforms.msShadowFormFill.value = shadow.formFill ?? 0;
    const styled = shadow.style === "illustration", formed = (shadow.form ?? 0) > 0 || (shadow.formFill ?? 0) > 0;
    if (styled === this.styled && formed === this.formed) return;
    this.styled = styled;
    this.formed = formed;
    for (const reference of this.materials) {
      const material = reference.deref();
      if (material) material.needsUpdate = true;
      else this.materials.delete(reference);
    }
  }

  /** Tells the style what the light's (orthographic) shadow map spans, across and in depth, so its lengths stay in meters. */
  fit(light: THREE.DirectionalLight): void {
    const camera = light.shadow.camera;
    this.uniforms.msShadowExtent.value = Math.max(camera.right - camera.left, 1e-3);
    this.uniforms.msShadowDepthRange.value = Math.max(camera.far - camera.near, 1e-3);
  }

  /**
   * Patches a standard material. It chains onto whatever `onBeforeCompile` the material has and only touches the
   * shadow-map declarations, the directional shadow line and the final color, so it composes with other patches in
   * either order. Patches that set `customProgramCacheKey` afterwards include `shadowStyleKey(material)`.
   */
  apply(material: THREE.Material): void {
    styles.set(material, this);
    this.materials.add(new WeakRef(material));
    const previous = material.onBeforeCompile;
    const uniforms = this.uniforms;
    material.onBeforeCompile = (shader, renderer) => {
      previous.call(material, shader, renderer);
      if (!this.styled) return;
      const lights = THREE.ShaderChunk.lights_fragment_begin;
      // If three.js changes what this replaces, the material keeps its plain shadows.
      if (!lights.includes(PLAIN) || !["#include <lights_fragment_begin>", "#include <shadowmap_pars_fragment>", "#include <opaque_fragment>"]
        .every(chunk => shader.fragmentShader.includes(chunk)) || !shader.fragmentShader.includes("totalEmissiveRadiance")) return;
      Object.assign(shader.uniforms, uniforms);
      shader.fragmentShader = shader.fragmentShader
        .replace("#include <shadowmap_pars_fragment>", `#include <shadowmap_pars_fragment>\n${this.formed ? "#define MS_SHADOW_FORM\n" : ""}${DECLARATIONS}`)
        .replace("#include <lights_fragment_begin>", lights.replace(PLAIN, STYLED))
        .replace("#include <opaque_fragment>", FINAL);
    };
    const key = material.customProgramCacheKey;
    material.customProgramCacheKey = () => `${key.call(material)}${this.key}`;
    material.needsUpdate = true;
  }
}
