import * as THREE from "three";
import { ANIME_SHADE_GLSL, SPRAY_GRID, generator, keepTargetAlpha, sprayAtlas, type Crown } from "./anime-tree-crown.ts";

/**
 * Illustrated tree, look `sprays`: a card at each station of the crown (anime-tree-crown.ts), textured with a painted
 * spray of distinct pointed leaves (`sprayAtlas`: fans from a stem, and whorls). A card stands along its twig (leaning
 * out of its cluster) and turns about that axis to face the camera; seen down its axis it lies toward the camera
 * instead of thinning to a line. Its texels carry each leaf's own id, so the ramp steps fall on whole leaves, and the
 * light comes from the cluster's puffy normal at the station, tipped a little along the card. Edges are the texture's
 * coverage through alpha to coverage: antialiased where the target is multisampled, with no sorting.
 */
export interface SprayCard { position: THREE.Vector3; axis: THREE.Vector3; normal: THREE.Vector3; occlusion: number; size: number; whorl?: boolean }

const VERTEX = /* glsl */`
attribute vec4 iBase;
attribute vec4 iAxis;
attribute vec4 iShade;
attribute vec3 iSide;
attribute vec2 iTone;
varying vec2 vUv;
varying float vCell;
varying vec3 vNormal;
varying vec3 vUp;
varying float vOcclusion;
varying float vJitter;
varying vec3 vToCamera;
${ANIME_SHADE_GLSL}
void main() {
  vec3 base = (modelMatrix * vec4(iBase.xyz, 1.0)).xyz;
  base += atSway(base, iTone.y, 1.0);
  vec3 axis = normalize(mat3(modelMatrix) * iAxis.xyz);
  vec3 toCamera = isOrthographic ? normalize(vec3(viewMatrix[0][2], viewMatrix[1][2], viewMatrix[2][2])) : normalize(cameraPosition - base);
  // Up the card: its axis with most of what points at the camera taken out, and a fixed sideways lean, so a spray
  // seen down its axis lies across the view rather than vanishing.
  vec3 up = axis - toCamera * dot(axis, toCamera) * 0.85 + normalize(mat3(modelMatrix) * iSide) * 0.12;
  up = normalize(up - toCamera * dot(up, toCamera) * 0.6);
  vec3 right = normalize(cross(up, toCamera));
  float size = iBase.w;
  vec3 sway = atSway(base, iTone.y + 0.37, 0.8 * position.y);
  vec3 world = base + right * (position.x * size) + up * ((position.y - 0.06) * size) + sway;
  vUv = vec2(position.x + 0.5, position.y);
  vCell = iAxis.w;
  vNormal = normalize(mat3(modelMatrix) * iShade.xyz);
  vUp = up;
  vOcclusion = iShade.w;
  vJitter = iTone.x;
  vToCamera = toCamera;
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}`;

const FRAGMENT = /* glsl */`
uniform sampler2D atSprays;
varying vec2 vUv;
varying float vCell;
varying vec3 vNormal;
varying vec3 vUp;
varying float vOcclusion;
varying float vJitter;
varying vec3 vToCamera;
${ANIME_SHADE_GLSL}
void main() {
  float cell = floor(vCell + 0.5);
  vec2 uv = (vec2(mod(cell, ${SPRAY_GRID}.0), floor(cell / ${SPRAY_GRID}.0)) + clamp(vec2(vUv.x, 1.0 - vUv.y), 0.004, 0.996)) / ${SPRAY_GRID}.0;
  vec4 leaf = texture2D(atSprays, uv);
#ifdef AT_DEPTH
  if (leaf.a < 0.5) discard;
  gl_FragColor = vec4(1.0);
#else
  // The coverage's edge sharpened to about a pixel: alpha to coverage antialiases it.
  float alpha = clamp((leaf.a - 0.5) / max(fwidth(leaf.a), 1e-4) + 0.5, 0.0, 1.0);
  if (alpha < 0.02) discard;
  // The spray's tip leans into the light of where it points; leaves behind others sit a little deeper.
  vec3 n = normalize(vNormal + vUp * (vUv.y - 0.4) * 0.3);
  float light = atSeen(atLightValue(n, vOcclusion + (1.0 - leaf.r) * 0.16, leaf.g * 0.75 + vJitter * 0.25), normalize(vNormal), vToCamera);
  vec3 colour = atRampColour(light);
  gl_FragColor = vec4(atBackLight(colour, n, vToCamera, vOcclusion), alpha);
  #include <colorspace_fragment>
#endif
}`;

/** A card for every station of `crown`, `size` meters long. */
export function stationCards(crown: Crown, size: number, seed = 31): SprayCard[] {
  const random = generator(seed), up = new THREE.Vector3(0, 0, 1);
  return crown.stations.map(station => ({
    position: station.position,
    axis: station.direction.clone().multiplyScalar(0.6).addScaledVector(station.normal, 0.55).addScaledVector(up, 0.2).normalize(),
    normal: station.normal, occlusion: station.occlusion, size: size * crown.scale * (0.8 + 0.4 * random()),
    // Stations pointing nearly straight up or down show their whorl from the side views: mostly fans.
    whorl: random() < 0.25,
  }));
}

/** `cards` as one instanced mesh (named `anime/sprays`); `uniforms` are `animeUniforms`. */
export function animeSprays(cards: SprayCard[], uniforms: Record<string, THREE.IUniform>, seed = 37): THREE.Mesh {
  const random = generator(seed);
  const bases = new Float32Array(cards.length * 4), axes = new Float32Array(cards.length * 4), shades = new Float32Array(cards.length * 4),
    sides = new Float32Array(cards.length * 3), tones = new Float32Array(cards.length * 2);
  const side = new THREE.Vector3(), up = new THREE.Vector3(0, 0, 1);
  cards.forEach((card, i) => {
    card.position.toArray(bases, i * 4); bases[i * 4 + 3] = card.size;
    card.axis.toArray(axes, i * 4);
    // Ten fans, then six whorls.
    axes[i * 4 + 3] = card.whorl ? 10 + Math.floor(random() * 6) : Math.floor(random() * 10);
    card.normal.toArray(shades, i * 4); shades[i * 4 + 3] = card.occlusion;
    side.crossVectors(card.axis, Math.abs(card.axis.z) < 0.9 ? up : new THREE.Vector3(1, 0, 0)).normalize().toArray(sides, i * 3);
    tones[i * 2] = random(); tones[i * 2 + 1] = random();
  });
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([-0.5, 0, 0, 0.5, 0, 0, 0.5, 1, 0, -0.5, 1, 0], 3));
  geometry.setIndex([0, 1, 2, 0, 2, 3]);
  geometry.setAttribute("iBase", new THREE.InstancedBufferAttribute(bases, 4));
  geometry.setAttribute("iAxis", new THREE.InstancedBufferAttribute(axes, 4));
  geometry.setAttribute("iShade", new THREE.InstancedBufferAttribute(shades, 4));
  geometry.setAttribute("iSide", new THREE.InstancedBufferAttribute(sides, 3));
  geometry.setAttribute("iTone", new THREE.InstancedBufferAttribute(tones, 2));
  geometry.instanceCount = cards.length;
  const all = { ...uniforms, atSprays: { value: sprayAtlas() } };
  const material = new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT, side: THREE.DoubleSide, alphaToCoverage: true });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = "anime/sprays";
  keepTargetAlpha(mesh);
  mesh.customDepthMaterial = new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT, side: THREE.DoubleSide, defines: { AT_DEPTH: 1 } });
  mesh.userData.animeTree = { instances: cards.length, triangles: cards.length * 2 };
  return mesh;
}
