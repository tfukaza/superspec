import * as THREE from "three";
import { ANIME_SHADE_GLSL, crownShade, generator, type Crown } from "./anime-tree-crown.ts";

/**
 * Illustrated tree, look `leaves`: every leaf is geometry. At each station of the crown (anime-tree-crown.ts) stands a
 * whorl of five to eight pointed leaves spreading from the twig like the ribs of an umbrella, plus one at its tip; a
 * leaf is one instance of a six-cornered outline (four triangles) that turns about its own length to show its width to
 * the camera, so it never thins to a line, with no texture and no cutout: its edges are the mesh's, antialiased as
 * any mesh is. A whole leaf takes one step of the ramp from the light at its middle (the cluster's puffy normal and
 * occlusion there) and its own jitter, with a slightly lighter tip, a darker half and a midrib.
 */
export interface LeafOptions {
  /** A leaf's length in meters and its width over its length. */
  length?: number; width?: number;
  /** Leaves in a whorl: the fewest, and how many more at random. */
  whorl?: number; whorlMore?: number;
  seed?: number;
  /** `bake`: what the fragments write instead of a colour, for an impostor atlas (anime-tree-impostors.ts). */
  bake?: THREE.IUniform<number>;
}

const VERTEX = /* glsl */`
attribute vec4 iOrigin;
attribute vec4 iAlong;
attribute vec4 iShade;
attribute vec2 iTone;
varying vec2 vUv;
varying float vLight;
varying vec3 vNormal;
varying float vOcclusion;
varying vec3 vToCamera;
varying float vJitter;
${ANIME_SHADE_GLSL}
void main() {
  vec3 origin = (modelMatrix * vec4(iOrigin.xyz, 1.0)).xyz;
  origin += atSway(origin, iTone.y, 1.0);
  vec3 along = normalize(mat3(modelMatrix) * iAlong.xyz);
  vec3 toCamera = isOrthographic ? normalize(vec3(viewMatrix[0][2], viewMatrix[1][2], viewMatrix[2][2])) : normalize(cameraPosition - origin);
  // The leaf turns about its length to face the camera: its width is always seen whole.
  vec3 across = cross(along, toCamera);
  float seen = length(across);
  across = seen > 1e-4 ? across / seen : vec3(1.0, 0.0, 0.0);
  // A slight droop toward its tip, and a flutter there in the wind.
  vec3 tip = vec3(0.0, 0.0, -0.1 * position.y * position.y * iOrigin.w) + atSway(origin, iTone.y + 0.37, 0.6 * position.y);
  vec3 world = origin + along * (position.y * iOrigin.w) + across * (position.x * iAlong.w) + tip;
  vUv = vec2(position.x + 0.5, position.y);
  vNormal = normalize(mat3(modelMatrix) * iShade.xyz);
  vOcclusion = iShade.w;
  vJitter = iTone.x;
  vLight = atSeen(atLightValue(vNormal, iShade.w, iTone.x), vNormal, toCamera);
  vToCamera = toCamera;
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}`;

const FRAGMENT = /* glsl */`
uniform float atBake;
varying vec2 vUv;
varying float vLight;
varying vec3 vNormal;
varying float vOcclusion;
varying vec3 vToCamera;
varying float vJitter;
${ANIME_SHADE_GLSL}
void main() {
#ifdef AT_DEPTH
  gl_FragColor = vec4(1.0);
#else
  // Within the leaf: a lighter tip, a darker half, a fine midrib.
  float inside = (vUv.y - 0.5) * 0.07 - step(0.5, vUv.x) * 0.035 - (1.0 - smoothstep(0.02, 0.07, abs(vUv.x - 0.5))) * 0.075;
  if (atBake > 0.5) {
    // An impostor's texels: the light's normal in the view's frame and the leaf's own shift of light, or its occlusion.
    vec3 n = normalize(mat3(viewMatrix) * vNormal);
    gl_FragColor = atBake < 1.5 ? vec4(n.xy * 0.5 + 0.5, clamp(vJitter + inside * 2.0, 0.0, 1.0), 1.0) : vec4(vOcclusion, n.z * 0.5 + 0.5, 0.0, 1.0);
    return;
  }
  vec3 colour = atRampColour(clamp(vLight + inside, 0.0, 1.0));
  gl_FragColor = vec4(atBackLight(colour, vNormal, vToCamera, vOcclusion), 1.0);
  #include <colorspace_fragment>
#endif
}`;

/** The leaves of `crown` as one instanced mesh (named `anime/leaves`); `uniforms` are `animeUniforms`. */
export function animeLeaves(crown: Crown, uniforms: Record<string, THREE.IUniform>, options: LeafOptions = {}): THREE.Mesh {
  const length = (options.length ?? 0.3) * crown.scale, widthShare = options.width ?? 0.4;
  const whorl = options.whorl ?? 5, whorlMore = options.whorlMore ?? 3;
  const random = generator(options.seed ?? 29);
  const between = (a: number, b: number) => a + (b - a) * random();
  const origins: number[] = [], alongs: number[] = [], shades: number[] = [], tones: number[] = [];
  const up = new THREE.Vector3(0, 0, 1), axis = new THREE.Vector3(), u = new THREE.Vector3(), w = new THREE.Vector3(), d = new THREE.Vector3(), middle = new THREE.Vector3();
  for (const station of crown.stations) {
    const cluster = crown.clusters[station.cluster];
    // The whorl opens along the twig, leaning out of its cluster and upward.
    axis.copy(station.direction).multiplyScalar(0.55).addScaledVector(station.normal, 0.6).addScaledVector(up, 0.25).normalize();
    u.crossVectors(axis, Math.abs(axis.z) < 0.9 ? up : new THREE.Vector3(1, 0, 0)).normalize();
    w.crossVectors(axis, u);
    const count = whorl + Math.floor(random() * (whorlMore + 1)), turn = random() * Math.PI * 2, phase = random();
    for (let k = 0; k <= count; k++) {
      // The last leaf stands at the whorl's tip, along its axis.
      const terminal = k === count;
      const azimuth = turn + (k + between(-0.25, 0.25)) / count * Math.PI * 2, tilt = terminal ? between(0, 0.3) : between(0.85, 1.4);
      d.copy(axis).multiplyScalar(Math.cos(tilt)).addScaledVector(u, Math.cos(azimuth) * Math.sin(tilt)).addScaledVector(w, Math.sin(azimuth) * Math.sin(tilt));
      d.z -= 0.12; d.normalize();
      const size = length * between(0.78, 1.2) * (terminal ? 0.9 : 1);
      middle.copy(station.position).addScaledVector(d, 0.5 * size);
      const shade = crownShade(crown, cluster, middle);
      // A leaf reaching toward the light is lit a little more than its place alone says.
      shade.normal.addScaledVector(d, 0.3).normalize();
      origins.push(station.position.x + d.x * 0.03 * size, station.position.y + d.y * 0.03 * size, station.position.z + d.z * 0.03 * size, size);
      alongs.push(d.x, d.y, d.z, size * widthShare * between(0.88, 1.12));
      shades.push(shade.normal.x, shade.normal.y, shade.normal.z, shade.occlusion);
      tones.push(random(), phase);
    }
  }
  // A pointed leaf from its foot along +y, one unit long and one wide: six corners, a fan of four triangles.
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([0, 0, 0, 0.44, 0.26, 0, 0.36, 0.62, 0, 0, 1, 0, -0.36, 0.62, 0, -0.44, 0.26, 0], 3));
  geometry.setIndex([0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5]);
  geometry.setAttribute("iOrigin", new THREE.InstancedBufferAttribute(new Float32Array(origins), 4));
  geometry.setAttribute("iAlong", new THREE.InstancedBufferAttribute(new Float32Array(alongs), 4));
  geometry.setAttribute("iShade", new THREE.InstancedBufferAttribute(new Float32Array(shades), 4));
  geometry.setAttribute("iTone", new THREE.InstancedBufferAttribute(new Float32Array(tones), 2));
  geometry.instanceCount = tones.length / 2;
  const all = { ...uniforms, atBake: options.bake ?? { value: 0 } };
  const material = new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT, side: THREE.DoubleSide });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = "anime/leaves";
  mesh.customDepthMaterial = new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT, side: THREE.DoubleSide, defines: { AT_DEPTH: 1 } });
  mesh.userData.animeTree = { instances: geometry.instanceCount, triangles: geometry.instanceCount * 4 };
  return mesh;
}
