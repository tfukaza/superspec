import * as THREE from "three";
import { ANIME_SHADE_GLSL, ANIME_VERTEX_GLSL } from "./anime-tree-crown.ts";

/**
 * Illustrated tree: its trunk and limbs, painted as an illustrator blocks them in. A warm grey-brown in four steps, a
 * clear light side and dark side by the sun (and a little from above), the steps' edges carried up and down the limb
 * by long soft vertical brush strokes rather than any fine grain; darker beneath limbs, and darker as the trunk rises
 * into its crown's shade. Every limb is drawn a little thicker than it is, by a fixed number of pixels
 * (`atMinimum.z`), so twigs are brush lines and never hairlines. Works on a plain mesh and on an instanced one.
 */
export const BARK_RAMP = ["#2f2a25", "#4b4036", "#6d5d4b", "#98866d"] as const;

const VERTEX = /* glsl */`
varying vec3 vWorld;
varying vec3 vNormal;
${ANIME_SHADE_GLSL}
${ANIME_VERTEX_GLSL}
void main() {
  mat4 placed = modelMatrix;
#ifdef USE_INSTANCING
  placed = modelMatrix * instanceMatrix;
#endif
  vec4 world = placed * vec4(position, 1.0);
  vNormal = normalize(mat3(placed) * normal);
  world.xyz += vNormal * atPixel(world.xyz) * atMinimum.z;
  vWorld = world.xyz;
  gl_Position = projectionMatrix * viewMatrix * world;
}`;

const FRAGMENT = /* glsl */`
uniform vec3 atBark[4];
uniform vec4 atBarkCrown;
varying vec3 vWorld;
varying vec3 vNormal;
${ANIME_SHADE_GLSL}
float atHash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float atNoise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(atHash(i), atHash(i + vec2(1.0, 0.0)), f.x), mix(atHash(i + vec2(0.0, 1.0)), atHash(i + 1.0), f.x), f.y);
}
void main() {
#ifdef AT_DEPTH
  gl_FragColor = vec4(1.0);
#else
  vec3 n = normalize(vNormal);
  // A bold light side and dark side: the sun across the limb, a little from above; the dark side is not black, it
  // keeps the sky's light, so the strokes still read in it.
  float lit = dot(sun, sun) > 0.0 ? dot(n, sun) * 0.42 + 0.55 : 0.5;
  float light = mix(lit, n.z * 0.5 + 0.5, 0.15);
  // Brush strokes: long up the limb and several round it (by its normal, so a thick trunk gets as many as a thin limb),
  // moving each step's edge in long vertical runs.
  float strokes = atNoise(vec2(n.x * 6.0 + n.y * 2.0, vWorld.z * 0.32)) * 0.6 + atNoise(vec2(n.y * 11.0 - n.x * 3.0 + 3.1, vWorld.z * 0.9)) * 0.4;
  light += (strokes - 0.5) * 0.42;
  // Reflected light on the edge of the dark side, as a painter lifts the shadow away from the outline.
  vec3 toCamera = normalize(cameraPosition - vWorld);
  light += 0.14 * smoothstep(0.55, 0.95, 1.0 - abs(dot(n, toCamera))) * (1.0 - smoothstep(0.35, 0.55, lit));
  // Darker beneath a limb, and as the trunk rises into the crown (atBarkCrown: where its shade starts and is full).
  light -= max(0.0, -n.z) * 0.3;
  light -= smoothstep(atBarkCrown.x, atBarkCrown.y, vWorld.z) * atBarkCrown.z;
  // Four steps, their edges a soft pixel or two wide rather than cut.
  float v = clamp(light, 0.0, 0.999) * 4.0, step0 = floor(v), edge = fwidth(v) * 1.5 + 0.04;
  float q = clamp(step0, 0.0, 3.0), f = v - step0;
  // Half way to the next step at the top of this one, and half way to the one below at its foot: continuous.
  float toward = f > 0.5 ? clamp(step0 + 1.0, 0.0, 3.0) : clamp(step0 - 1.0, 0.0, 3.0);
  float blend = f > 0.5 ? smoothstep(1.0 - edge, 1.0, f) * 0.5 : (1.0 - smoothstep(0.0, edge, f)) * 0.5;
  vec3 own = q < 0.5 ? atBark[0] : q < 1.5 ? atBark[1] : q < 2.5 ? atBark[2] : atBark[3];
  vec3 next = toward < 0.5 ? atBark[0] : toward < 1.5 ? atBark[1] : toward < 2.5 ? atBark[2] : atBark[3];
  vec3 colour = mix(own, next, blend);
  gl_FragColor = vec4(colour, 1.0);
  #include <colorspace_fragment>
#endif
}`;

/**
 * The bark's material and the one that casts its shadow. `uniforms` are `animeUniforms`; the crown's shade starts at
 * the height `shadeFrom` (meters, in the scene) and is full at `shadeFull`.
 */
export function animeBark(uniforms: Record<string, THREE.IUniform>, shadeFrom: number, shadeFull: number): { material: THREE.ShaderMaterial; depth: THREE.ShaderMaterial } {
  const all = { ...uniforms, atBark: { value: BARK_RAMP.map(hex => new THREE.Color(hex)) }, atBarkCrown: { value: new THREE.Vector4(shadeFrom, shadeFull, 0.2, 0) } };
  const material = new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT });
  material.toneMapped = false;
  return { material, depth: new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT, defines: { AT_DEPTH: 1 } }) };
}
