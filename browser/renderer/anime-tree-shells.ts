import * as THREE from "three";
import { ANIME_SHADE_GLSL, crownShade, generator, leafPattern, type Crown } from "./anime-tree-crown.ts";
import type { SprayCard } from "./anime-tree-sprays.ts";

/**
 * Illustrated tree, look `shells`: every cluster of the crown (anime-tree-crown.ts) is a lumpy low-polygon ellipsoid,
 * flatter beneath, painted with a repeating pattern of leaves laid on it from the three axes (`leafPattern`): the
 * ramp's steps are taken per leaf of the pattern, so the surface reads as leaves, not as a smooth ball. Spray cards
 * (anime-tree-sprays.ts) stand out of each shell, so its outline is made of leaves; the caller draws them with
 * `shellCards`.
 */
const VERTEX = /* glsl */`
attribute vec4 aShade;
attribute float aPhase;
varying vec3 vWorld;
varying vec3 vNormal;
varying vec3 vFace;
varying float vOcclusion;
varying vec3 vToCamera;
${ANIME_SHADE_GLSL}
void main() {
  vec4 world = modelMatrix * vec4(position, 1.0);
  world.xyz += atSway(world.xyz, aPhase, 1.0);
  vWorld = world.xyz;
  vNormal = normalize(mat3(modelMatrix) * aShade.xyz);
  vFace = normalize(mat3(modelMatrix) * normal);
  vOcclusion = aShade.w;
  vToCamera = isOrthographic ? normalize(vec3(viewMatrix[0][2], viewMatrix[1][2], viewMatrix[2][2])) : normalize(cameraPosition - world.xyz);
  gl_Position = projectionMatrix * viewMatrix * world;
}`;

const FRAGMENT = /* glsl */`
uniform sampler2D atPattern;
uniform float atTile;
varying vec3 vWorld;
varying vec3 vNormal;
varying vec3 vFace;
varying float vOcclusion;
varying vec3 vToCamera;
${ANIME_SHADE_GLSL}
void main() {
#ifdef AT_DEPTH
  gl_FragColor = vec4(1.0);
#else
  // The pattern from the axis the surface faces most (leaves are whole on each side; the seams fall between leaves).
  vec3 f = abs(normalize(vFace));
  vec2 uv = f.z >= f.x && f.z >= f.y ? vWorld.xy : f.x >= f.y ? vWorld.yz + 0.37 : vWorld.xz + 0.61;
  vec4 leaf = texture2D(atPattern, uv / atTile);
  float light = atLightValue(normalize(vNormal), vOcclusion + (1.0 - leaf.r) * 0.3, leaf.g);
  vec3 colour = atRampColour(light);
  gl_FragColor = vec4(atBackLight(colour, normalize(vNormal), vToCamera, vOcclusion), 1.0);
  #include <colorspace_fragment>
#endif
}`;

/** How far a shell stands in from its cluster's radii (the sprays reach the rest). */
const INSET = 0.78;

/** The shells of `crown` as one mesh (named `anime/shells`); `tile` is the leaf pattern's repeat in meters. */
export function animeShells(crown: Crown, uniforms: Record<string, THREE.IUniform>, tile = 1.9): THREE.Mesh {
  const ball = new THREE.IcosahedronGeometry(1, 2), directions = ball.getAttribute("position");
  const positions: number[] = [], normals: number[] = [], shades: number[] = [], phases: number[] = [];
  const random = generator(41), p = new THREE.Vector3(), d = new THREE.Vector3();
  for (const cluster of crown.clusters) {
    if (cluster.stations < 3) continue;
    const s1 = random() * 6.28, s2 = random() * 6.28, s3 = random() * 6.28, phase = random();
    for (let i = 0; i < directions.count; i++) {
      d.fromBufferAttribute(directions, i);
      // Lumps, and a flatter underside.
      const lump = 1 + 0.16 * Math.sin(d.x * 4.3 + s1) * Math.sin(d.y * 4.9 + s2) + 0.1 * Math.sin(d.z * 6.1 + s3);
      p.copy(d).multiplyScalar(lump * INSET).multiply(cluster.radii);
      if (p.z < 0) p.z *= 0.6;
      p.add(cluster.centre);
      const shade = crownShade(crown, cluster, p);
      positions.push(p.x, p.y, p.z);
      // The pattern's axis is chosen by the ball's own direction, which is smooth across its faces.
      normals.push(d.x, d.y, d.z);
      shades.push(shade.normal.x, shade.normal.y, shade.normal.z, shade.occlusion);
      phases.push(phase);
    }
  }
  ball.dispose();
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("aShade", new THREE.Float32BufferAttribute(shades, 4));
  geometry.setAttribute("aPhase", new THREE.Float32BufferAttribute(phases, 1));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute(normals, 3));
  const all = { ...uniforms, atPattern: { value: leafPattern() }, atTile: { value: tile * crown.scale } };
  const mesh = new THREE.Mesh(geometry, new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT }));
  mesh.name = "anime/shells";
  mesh.customDepthMaterial = new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT, defines: { AT_DEPTH: 1 } });
  mesh.userData.animeTree = { instances: positions.length / 3 / directions.count, triangles: positions.length / 9 };
  return mesh;
}

/** Spray cards standing out of every shell of `crown`: `each` a shell (more on a larger one), `size` meters long. */
export function shellCards(crown: Crown, each = 16, size = 0.62, seed = 43): SprayCard[] {
  const random = generator(seed), cards: SprayCard[] = [], up = new THREE.Vector3(0, 0, 1);
  const widest = Math.max(...crown.clusters.map(cluster => cluster.radii.x * cluster.radii.y), 1e-6);
  for (const cluster of crown.clusters) {
    if (cluster.stations < 3) continue;
    const count = Math.max(6, Math.round(each * cluster.radii.x * cluster.radii.y / widest * 1.6));
    for (let k = 0; k < count; k++) {
      // Anywhere on the shell but its underside's middle.
      const z = -0.35 + 1.35 * random(), turn = random() * Math.PI * 2, flat = Math.sqrt(Math.max(0, 1 - Math.min(1, z * z)));
      const d = new THREE.Vector3(Math.cos(turn) * flat, Math.sin(turn) * flat, z).normalize();
      const length = size * crown.scale * (0.8 + 0.4 * random());
      const position = d.clone().multiplyScalar(INSET * 0.92).multiply(cluster.radii);
      if (position.z < 0) position.z *= 0.6;
      position.add(cluster.centre);
      const shade = crownShade(crown, cluster, position.clone().addScaledVector(d, 0.35 * length));
      cards.push({ position, axis: d.clone().multiplyScalar(0.8).addScaledVector(up, 0.25).add(new THREE.Vector3(random() - 0.5, random() - 0.5, random() - 0.5).multiplyScalar(0.5)).normalize(),
        normal: shade.normal, occlusion: shade.occlusion, size: length, whorl: random() < 0.2 });
    }
  }
  return cards;
}
