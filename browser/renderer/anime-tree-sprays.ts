import * as THREE from "three";
import { ANIME_SHADE_GLSL, ANIME_VERTEX_GLSL, SPRAY_FANS, SPRAY_GRID, SPRAY_SPRIGS, SPRAY_WHORLS, coverageAlpha, generator, sprayAtlas, type Crown } from "./anime-tree-crown.ts";

/**
 * Illustrated tree, look `sprays`: many small cards at the stations of the crown (anime-tree-crown.ts), each textured
 * with a painted spray of distinct pointed leaves (`sprayAtlas`: fans from a stem, whorls, and sprigs of one to three
 * leaves). A card stands along its twig (leaning out of its cluster, each its own way) and turns about that axis to
 * face the camera; seen down its axis it lies toward the camera instead of thinning to a line. Its texels carry each
 * leaf's own id, so the ramp steps fall on whole leaves, and the light comes from the cluster's puffy normal at the
 * station, tipped a little along the card. Tiny sprig cards on the clusters' open tops are highlights: lighter than
 * the ramp gives where they are in the light. A card never draws smaller than `atMinimum.y` pixels, so its leaves stay
 * shapes at a distance. Edges are the texture's coverage through alpha to coverage: antialiased where the target is
 * multisampled, with no sorting.
 */
export interface SprayCard { position: THREE.Vector3; axis: THREE.Vector3; normal: THREE.Vector3; occlusion: number; size: number;
  kind?: "fan" | "whorl" | "sprig";
  /** A highlight card's extra light where it is lit, 0 for an ordinary card. */
  boost?: number }

const VERTEX = /* glsl */`
attribute vec4 iBase;
attribute vec4 iAxis;
attribute vec4 iShade;
attribute vec3 iSide;
attribute vec3 iTone;
varying vec2 vUv;
varying float vCell;
varying vec3 vNormal;
varying vec3 vUp;
varying float vOcclusion;
varying vec2 vJitter;
varying vec3 vToCamera;
${ANIME_SHADE_GLSL}
${ANIME_VERTEX_GLSL}
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
  float size = max(iBase.w, atPixel(base) * atMinimum.y);
  vec3 sway = atSway(base, iTone.y + 0.37, 0.8 * position.y);
  vec3 world = base + right * (position.x * size) + up * ((position.y - 0.06) * size) + sway;
  vUv = vec2(position.x + 0.5, position.y);
  vCell = iAxis.w;
  vNormal = normalize(mat3(modelMatrix) * iShade.xyz);
  vUp = up;
  vOcclusion = iShade.w;
  vJitter = iTone.xz;
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
varying vec2 vJitter;
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
  float light = atSeen(atLightValue(n, vOcclusion + (1.0 - leaf.r) * 0.4, leaf.g * 0.75 + vJitter.x * 0.25), normalize(vNormal), vToCamera);
  vec3 colour = atRampColour(atGlint(light, vJitter.y));
  gl_FragColor = vec4(atBackLight(colour, n, vToCamera, vOcclusion), alpha);
  #include <colorspace_fragment>
#endif
}`;

/**
 * Cards for the stations of `crown`, about `size` meters long (each its own size and direction), and, on `glints` of
 * the stations open to the sky, a tiny highlight sprig.
 */
export function stationCards(crown: Crown, size: number, glints = 0, seed = 31): SprayCard[] {
  const random = generator(seed), up = new THREE.Vector3(0, 0, 1), cards: SprayCard[] = [];
  const stray = () => new THREE.Vector3(random() - 0.5, random() - 0.5, random() - 0.5);
  for (const station of crown.stations) {
    cards.push({
      position: station.position,
      // Along the twig and out of the cluster, with a turn of its own; now and then drooping.
      axis: station.direction.clone().multiplyScalar(0.6).addScaledVector(station.normal, 0.55).addScaledVector(up, random() < 0.2 ? -0.25 : 0.2).addScaledVector(stray(), 0.9).normalize(),
      normal: station.normal, occlusion: station.occlusion, size: size * crown.scale * (0.65 + 0.7 * random()),
      kind: random() < 0.25 ? "whorl" : "fan",
    });
    if (random() < glints && station.occlusion < 0.4 && station.normal.z > 0.25) cards.push(glintCard(station.position, station.direction, station.normal, station.occlusion, size * crown.scale, random));
  }
  return cards;
}

/** A tiny highlight sprig standing off `position`, for a place open to the sky; `size` is an ordinary card's. */
export function glintCard(position: THREE.Vector3, direction: THREE.Vector3, normal: THREE.Vector3, occlusion: number, size: number, random: () => number): SprayCard {
  const stray = () => new THREE.Vector3(random() - 0.5, random() - 0.5, random() - 0.5);
  return {
    position: position.clone().addScaledVector(normal, 0.3 * size).addScaledVector(stray(), 0.4 * size),
    axis: normal.clone().multiplyScalar(0.7).addScaledVector(direction, 0.4).addScaledVector(stray(), 0.8).normalize(),
    normal, occlusion: occlusion * 0.6, size: size * (0.36 + 0.2 * random()), kind: "sprig", boost: 0.2 + 0.14 * random(),
  };
}

/** `cards` as one instanced mesh (named `anime/sprays`); `uniforms` are `animeUniforms`. */
export function animeSprays(cards: SprayCard[], uniforms: Record<string, THREE.IUniform>, seed = 37): THREE.Mesh {
  const random = generator(seed);
  const bases = new Float32Array(cards.length * 4), axes = new Float32Array(cards.length * 4), shades = new Float32Array(cards.length * 4),
    sides = new Float32Array(cards.length * 3), tones = new Float32Array(cards.length * 3);
  const side = new THREE.Vector3(), up = new THREE.Vector3(0, 0, 1);
  cards.forEach((card, i) => {
    card.position.toArray(bases, i * 4); bases[i * 4 + 3] = card.size;
    card.axis.toArray(axes, i * 4);
    axes[i * 4 + 3] = card.kind === "whorl" ? SPRAY_FANS + Math.floor(random() * SPRAY_WHORLS)
      : card.kind === "sprig" ? SPRAY_FANS + SPRAY_WHORLS + Math.floor(random() * SPRAY_SPRIGS) : Math.floor(random() * SPRAY_FANS);
    card.normal.toArray(shades, i * 4); shades[i * 4 + 3] = card.occlusion;
    side.crossVectors(card.axis, Math.abs(card.axis.z) < 0.9 ? up : new THREE.Vector3(1, 0, 0)).normalize().toArray(sides, i * 3);
    tones[i * 3] = random(); tones[i * 3 + 1] = random(); tones[i * 3 + 2] = card.boost ?? 0;
  });
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([-0.5, 0, 0, 0.5, 0, 0, 0.5, 1, 0, -0.5, 1, 0], 3));
  geometry.setIndex([0, 1, 2, 0, 2, 3]);
  geometry.setAttribute("iBase", new THREE.InstancedBufferAttribute(bases, 4));
  geometry.setAttribute("iAxis", new THREE.InstancedBufferAttribute(axes, 4));
  geometry.setAttribute("iShade", new THREE.InstancedBufferAttribute(shades, 4));
  geometry.setAttribute("iSide", new THREE.InstancedBufferAttribute(sides, 3));
  geometry.setAttribute("iTone", new THREE.InstancedBufferAttribute(tones, 3));
  geometry.instanceCount = cards.length;
  const all = { ...uniforms, atSprays: { value: sprayAtlas() } };
  const material = new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT, side: THREE.DoubleSide, alphaToCoverage: true });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = "anime/sprays";
  coverageAlpha(material);
  mesh.customDepthMaterial = new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT, side: THREE.DoubleSide, defines: { AT_DEPTH: 1 } });
  mesh.userData.animeTree = { instances: cards.length, triangles: cards.length * 2 };
  return mesh;
}
