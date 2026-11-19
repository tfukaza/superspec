import * as THREE from "three";
import { ANIME_SHADE_GLSL, crownShade, generator, type Crown, type CrownStation } from "./anime-tree-crown.ts";
import { animeLeaves } from "./anime-tree-leaves.ts";

/**
 * Illustrated tree, look `impostors`: one camera-facing card for each cluster of the crown (anime-tree-crown.ts),
 * showing a detailed cluster of geometry leaves (anime-tree-leaves.ts) drawn once, in the page, from 8 directions
 * round it at 3 heights into an atlas that keeps, per texel, the light's normal, the leaf's own shift of light and its
 * occlusion rather than a colour. A card picks the view nearest the camera's (about the cluster's own turn, so no two
 * clusters look alike) and lights its texels like any leaf: the stored normal turned back into the scene and mixed
 * with the crown's broad form there, through the same ramp. The atlas is drawn on the first frame that draws a card.
 */
const AROUND = 8, HEIGHTS = [-15, 20, 55], CELL = 192, EXTENT = 1.3;

interface Atlas { normals: THREE.WebGLRenderTarget; depths: THREE.WebGLRenderTarget; baked: boolean }
let atlas: Atlas | undefined;
function impostorAtlas(): Atlas {
  if (atlas) return atlas;
  const target = () => {
    const made = new THREE.WebGLRenderTarget(AROUND * CELL, HEIGHTS.length * CELL, { samples: 4, depthBuffer: true, generateMipmaps: true,
      minFilter: THREE.LinearMipmapLinearFilter, magFilter: THREE.LinearFilter });
    made.texture.colorSpace = THREE.NoColorSpace;
    return made;
  };
  atlas = { normals: target(), depths: target(), baked: false };
  return atlas;
}

/** Draws the model cluster from every view into the atlas (`uniforms` are `animeUniforms`; its ramp is not used). */
function bake(renderer: THREE.WebGLRenderer, made: Atlas, uniforms: Record<string, THREE.IUniform>): void {
  // The model cluster: stations over a dome one meter in radius and 0.65 high, an inner layer under an outer one.
  const random = generator(53), stations: CrownStation[] = [];
  const radii = new THREE.Vector3(1, 1, 0.65), centre = new THREE.Vector3();
  for (const [count, reach] of [[34, 0.72], [14, 0.4]] as const) {
    for (let k = 0; k < count; k++) {
      const z = 1 - (k + 0.5) / count * 1.25, turn = k * 2.39996 + random() * 0.5, flat = Math.sqrt(Math.max(0, 1 - z * z));
      const direction = new THREE.Vector3(Math.cos(turn) * flat, Math.sin(turn) * flat, z).normalize();
      stations.push({ position: direction.clone().multiply(radii).multiplyScalar(reach * (0.9 + 0.2 * random())), direction, along: 1, cluster: 0, normal: direction.clone(), occlusion: 0 });
    }
  }
  const crown: Crown = { stations, clusters: [{ centre, radii, covered: 0, stations: stations.length }], centre, radii, scale: 1 };
  for (const station of stations) {
    // The model's light is its own form alone: the crown's is added where a card stands.
    const d = station.position.clone().divide(radii);
    station.normal = d.clone().divide(radii).normalize().multiplyScalar(0.8).add(new THREE.Vector3(0, 0, 0.3)).normalize();
    station.occlusion = Math.min(1, 0.45 * (1 - Math.min(1, d.length())) + 0.45 * Math.max(0, -d.z));
  }
  const mode = { value: 1 };
  const leaves = animeLeaves(crown, { ...uniforms, atTime: { value: 0 } }, { length: 0.36, bake: mode, seed: 59 });
  // The leaves' own light in the model: by their place in the dome, not a crown's.
  const shades = leaves.geometry.getAttribute("iShade") as THREE.BufferAttribute, origins = leaves.geometry.getAttribute("iOrigin"), alongs = leaves.geometry.getAttribute("iAlong");
  const p = new THREE.Vector3(), d = new THREE.Vector3();
  for (let i = 0; i < shades.count; i++) {
    d.fromBufferAttribute(alongs, i);
    p.fromBufferAttribute(origins, i).addScaledVector(d, 0.5 * origins.getW(i)).divide(radii);
    const depth = p.length();
    const n = p.clone().divide(radii).normalize().multiplyScalar(0.8).add(new THREE.Vector3(0, 0, 0.3)).addScaledVector(d, 0.3).normalize();
    shades.setXYZW(i, n.x, n.y, n.z, Math.min(1, 0.5 * (1 - Math.min(1, depth)) + 0.45 * Math.max(0, -p.z)));
  }
  shades.needsUpdate = true;
  const scene = new THREE.Scene();
  scene.add(leaves);
  leaves.frustumCulled = false;
  const camera = new THREE.OrthographicCamera(-EXTENT, EXTENT, EXTENT, -EXTENT, 0.1, 20);
  camera.up.set(0, 0, 1);
  const before = { target: renderer.getRenderTarget(), clear: renderer.getClearColor(new THREE.Color()), alpha: renderer.getClearAlpha(), auto: renderer.autoClear,
    viewport: renderer.getViewport(new THREE.Vector4()), scissor: renderer.getScissor(new THREE.Vector4()), scissorTest: renderer.getScissorTest(),
    shadows: renderer.shadowMap.autoUpdate, tone: renderer.toneMapping };
  renderer.shadowMap.autoUpdate = false;
  renderer.toneMapping = THREE.NoToneMapping;
  renderer.autoClear = false;
  renderer.setClearColor(0x000000, 0);
  for (const [target, value] of [[made.normals, 1], [made.depths, 2]] as const) {
    mode.value = value;
    renderer.setRenderTarget(target);
    renderer.setScissorTest(false);
    target.viewport.set(0, 0, target.width, target.height);
    renderer.clear(true, true, true);
    for (let row = 0; row < HEIGHTS.length; row++) for (let column = 0; column < AROUND; column++) {
      const around = column / AROUND * Math.PI * 2, height = HEIGHTS[row] * Math.PI / 180;
      camera.position.set(Math.cos(height) * Math.cos(around), Math.cos(height) * Math.sin(around), Math.sin(height)).multiplyScalar(6);
      camera.lookAt(0, 0, 0);
      camera.updateMatrixWorld();
      target.viewport.set(column * CELL, row * CELL, CELL, CELL);
      target.scissor.set(column * CELL, row * CELL, CELL, CELL);
      target.scissorTest = true;
      renderer.setRenderTarget(target);
      renderer.render(scene, camera);
    }
    target.scissorTest = false;
    target.viewport.set(0, 0, target.width, target.height);
  }
  renderer.setRenderTarget(before.target);
  renderer.setViewport(before.viewport); renderer.setScissor(before.scissor); renderer.setScissorTest(before.scissorTest);
  renderer.setClearColor(before.clear, before.alpha);
  renderer.autoClear = before.auto;
  renderer.shadowMap.autoUpdate = before.shadows;
  renderer.toneMapping = before.tone;
  leaves.geometry.dispose();
  (leaves.material as THREE.Material).dispose();
  made.baked = true;
}

const VERTEX = /* glsl */`
attribute vec4 iCentre;
attribute vec4 iBroad;
attribute vec4 iTurn;
varying vec2 vUv;
varying vec3 vBroad;
varying float vCovered;
varying vec3 vRight;
varying vec3 vUp;
varying vec3 vOut;
varying vec3 vToCamera;
varying float vJitter;
${ANIME_SHADE_GLSL}
void main() {
  vec3 centre = (modelMatrix * vec4(iCentre.xyz, 1.0)).xyz;
  centre += atSway(centre, iTurn.w, 1.0);
  vec3 toCamera = isOrthographic ? normalize(vec3(viewMatrix[0][2], viewMatrix[1][2], viewMatrix[2][2])) : normalize(cameraPosition - centre);
  // The view nearest the camera's, about the cluster's own turn (iTurn.xy: its cosine and sine).
  vec3 local = vec3(iTurn.x * toCamera.x + iTurn.y * toCamera.y, -iTurn.y * toCamera.x + iTurn.x * toCamera.y, toCamera.z);
  float around = floor(atan(local.y, local.x) / 6.2831853 * ${AROUND}.0 + 0.5);
  float height = degrees(asin(clamp(local.z, -1.0, 1.0)));
  float row = height < 2.5 ? 0.0 : height < 37.5 ? 1.0 : 2.0;
  float tilt = radians(row < 0.5 ? ${HEIGHTS[0]}.0 : row < 1.5 ? ${HEIGHTS[1]}.0 : ${HEIGHTS[2]}.0), turn = around / ${AROUND}.0 * 6.2831853;
  // That view's frame in the scene: toward its camera, to its right and up, turned back by the cluster's turn.
  vec3 outL = vec3(cos(tilt) * cos(turn), cos(tilt) * sin(turn), sin(tilt));
  vec3 rightL = normalize(cross(vec3(0.0, 0.0, 1.0), outL)), upL = cross(outL, rightL);
  mat3 back = mat3(iTurn.x, iTurn.y, 0.0, -iTurn.y, iTurn.x, 0.0, 0.0, 0.0, 1.0);
  vOut = back * outL; vRight = back * rightL; vUp = back * upL;
  // The card faces the camera, upright.
  vec3 right = normalize(cross(vec3(0.0, 0.0, 1.0), toCamera) + vec3(1e-5, 0.0, 0.0)), up = cross(toCamera, right);
  vec3 world = centre + (right * position.x + up * position.y) * (iCentre.w * ${EXTENT.toFixed(2)});
  vUv = (vec2(mod(around + ${AROUND}.0, ${AROUND}.0), row) + position.xy * 0.5 + 0.5) / vec2(${AROUND}.0, ${HEIGHTS.length}.0);
  vBroad = iBroad.xyz; vCovered = iBroad.w; vToCamera = toCamera; vJitter = iTurn.z;
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}`;

const FRAGMENT = /* glsl */`
uniform sampler2D atNormals;
uniform sampler2D atDepths;
varying vec2 vUv;
varying vec3 vBroad;
varying float vCovered;
varying vec3 vRight;
varying vec3 vUp;
varying vec3 vOut;
varying vec3 vToCamera;
varying float vJitter;
${ANIME_SHADE_GLSL}
void main() {
  vec4 a = texture2D(atNormals, vUv);
#ifdef AT_DEPTH
  if (a.a < 0.5) discard;
  gl_FragColor = vec4(1.0);
#else
  float alpha = clamp((a.a - 0.5) / max(fwidth(a.a), 1e-4) + 0.5, 0.0, 1.0);
  if (alpha < 0.02) discard;
  // Texels at a leaf's edge hold its values times their coverage.
  vec4 b = texture2D(atDepths, vUv);
  float cover = max(a.a, 1e-3);
  vec3 stored = vec3(a.rg / cover * 2.0 - 1.0, b.g / cover * 2.0 - 1.0);
  vec3 own = normalize(vRight * stored.x + vUp * stored.y + vOut * stored.z);
  vec3 n = normalize(own * 0.62 + normalize(vBroad) * 0.38);
  float occlusion = min(1.0, b.r / cover + vCovered);
  float light = atSeen(atLightValue(n, occlusion, clamp(a.b / cover + (vJitter - 0.5) * 0.2, 0.0, 1.0)), own, vToCamera);
  gl_FragColor = vec4(atBackLight(atRampColour(light), n, vToCamera, occlusion), alpha);
  #include <colorspace_fragment>
#endif
}`;

/** A card for each cluster of `crown` as one instanced mesh (named `anime/impostors`). */
export function animeImpostors(crown: Crown, uniforms: Record<string, THREE.IUniform>): THREE.Mesh {
  const made = impostorAtlas(), random = generator(61);
  const clusters = crown.clusters.filter(cluster => cluster.stations >= 2);
  const centres = new Float32Array(clusters.length * 4), broads = new Float32Array(clusters.length * 4), turns = new Float32Array(clusters.length * 4);
  clusters.forEach((cluster, i) => {
    cluster.centre.toArray(centres, i * 4);
    centres[i * 4 + 3] = 0.5 * (cluster.radii.x + cluster.radii.y) * 1.08;
    // The crown's form and shade where the cluster stands: its top's, since the model brings its own underside.
    const shade = crownShade(crown, cluster, cluster.centre.clone().add(new THREE.Vector3(0, 0, 0.6 * cluster.radii.z)));
    const broad = cluster.centre.clone().sub(crown.centre).divide(crown.radii).divide(crown.radii).normalize();
    broad.toArray(broads, i * 4);
    broads[i * 4 + 3] = Math.max(0, shade.occlusion - 0.1);
    const turn = random() * Math.PI * 2;
    turns.set([Math.cos(turn), Math.sin(turn), random(), random()], i * 4);
  });
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([-1, -1, 0, 1, -1, 0, 1, 1, 0, -1, 1, 0], 3));
  geometry.setIndex([0, 1, 2, 0, 2, 3]);
  geometry.setAttribute("iCentre", new THREE.InstancedBufferAttribute(centres, 4));
  geometry.setAttribute("iBroad", new THREE.InstancedBufferAttribute(broads, 4));
  geometry.setAttribute("iTurn", new THREE.InstancedBufferAttribute(turns, 4));
  geometry.instanceCount = clusters.length;
  const all = { ...uniforms, atNormals: { value: made.normals.texture }, atDepths: { value: made.depths.texture } };
  const material = new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT, side: THREE.DoubleSide, alphaToCoverage: true });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = "anime/impostors";
  mesh.customDepthMaterial = new THREE.ShaderMaterial({ uniforms: all, vertexShader: VERTEX, fragmentShader: FRAGMENT, side: THREE.DoubleSide, defines: { AT_DEPTH: 1 } });
  // The atlas is drawn when a card is first drawn, unless its owner drew it before (`userData.animeTree.bake`): a
  // shadow pass before that casts nothing from the cards.
  const ensure = (renderer: THREE.WebGLRenderer) => { if (!made.baked) bake(renderer, made, uniforms); };
  mesh.onBeforeRender = ensure;
  mesh.userData.animeTree = { instances: clusters.length, triangles: clusters.length * 2, bake: (renderer: THREE.WebGLRenderer) => ensure(renderer) };
  return mesh;
}
