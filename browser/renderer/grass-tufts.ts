import * as THREE from "three";
import { followPaintedFinishing } from "./shade.ts";
import { grassTuftSamples, tuftAtlas, type GrassTuftOptions } from "./grass-tuft-source.ts";
export { tuftAtlas, type GrassTuftOptions } from "./grass-tuft-source.ts";
import { PAINT_GLSL, RELIEF_GLSL } from "./grass-relief.ts";

/**
 * Grass tufts over the grass relief (an experiment): camera-facing billboards of a few painted grass blades, pre-blurred
 * into an atlas, scattered densely over the lawn and kept most where the picture changes:
 *
 * - **edges**: where the relief's normal turns away from the view (the outline of a clump, a crest, the bank's lip);
 * - **light borders**: where the shade band is between lit and shaded, from the relief's own normal or from a cast
 *   shadow (read with a wide kernel, so its edge is a band, not a line);
 * - **colour changes**: where the ground's own colour changes around the root.
 *
 * Elsewhere a thin share remains. A tuft stands on the relief (raised by it when the relief is displaced), upright along
 * the relief's normal and turned about it to face the camera, and takes the relief's paint at its root (ground colour,
 * clump tone, crown lift, shade band and cast shadow), each blade lighter or darker by its own tone in the atlas and its
 * lit tip a little lighter. Tufts share the relief's uniforms (`relief`), so both always agree, and blend over it; with
 * the scene's `?clumpblur=` they are blurred with it.
 */
const VERTEX = /* glsl */`
attribute vec3 tuftRoot;
attribute vec3 tuftUp;
// x: height (m), y: width (m), z: atlas cell, w: the tuft's stable random (kept while below its share).
attribute vec4 tuftData;
uniform float lift;
uniform vec3 tuftKeep; // x: base share, y: far (m), z: mirror seed
uniform vec2 tuftNear; // close tufts: full to x (m), gone at y
uniform float tuftFace;
varying float vGround; // tall blades: how light the ground under them is (0 dark, 1 light)
uniform vec2 tuftLight; // tall blades: ground lightness (luma) from which they thin out (x) to where all are kept (y) // the least a tuft's painted normal faces the camera
varying float vKind;
varying float vHue;
varying vec2 vUv;
varying vec3 vColor;
varying float vLit;
${RELIEF_GLSL}
${PAINT_GLSL}
float luma(vec3 c) { return dot(c, vec3(0.2126, 0.7152, 0.0722)); }
// The cast shadow read over a wider kernel (four times the relief's), so a shadow's edge is a band tufts can gather in.
float castWide(vec3 world, vec3 normal) {
  if (msFoliageShadowParams.x < 0.5) return 1.0;
  vec4 c = msFoliageShadowMatrix * vec4(world + normal * msFoliageShadowParams.z, 1.0);
  c.xyz /= c.w;
  if (c.x < 0.0 || c.x > 1.0 || c.y < 0.0 || c.y > 1.0 || c.z > 1.0) return 1.0;
  float depth = c.z + msFoliageShadowParams.y, lit = 0.0;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) lit += texture(msFoliageShadow, vec3(c.xy + vec2(float(x), float(y)) * msFoliageShadowParams.w * 6.0, depth));
  return lit / 9.0;
}
void main() {
  // Close tufts (their random stored plus one): only near the camera, fading out over tuftNear (m), and dropped here,
  // before any relief work, everywhere else.
  bool close = tuftData.w >= 1.0;
  float rank = close ? tuftData.w - 1.0 : tuftData.w;
  if (close && rank > 1.0 - smoothstep(tuftNear.x, tuftNear.y, length(cameraPosition - tuftRoot))) { gl_Position = vec4(0.0, 0.0, 2.0, 1.0); return; }
  vec3 n = normalize(tuftUp);
  float h = reliefHeight(tuftRoot, n);
  float tone = reliefTone;
  vec3 rn = reliefNormal(tuftRoot, n);
  vec3 root = tuftRoot + n * h * lift;
  vec3 toCamera = cameraPosition - root;
  float away = length(toCamera);
  vec3 v = toCamera / away;
  float band = smoothstep(-shadeBand.y, shadeBand.y, dot(rn, normalize(msSunDirection)) - shadeBand.x);
  vec3 t1, t2;
  reliefFrame(n, t1, t2);
#ifdef TUFT_EVERYWHERE
  float keep = 1.0 - smoothstep(0.6 * tuftKeep.y, tuftKeep.y, away);
#else
  // Where the picture changes: the outline, a light border (shade band or cast shadow), a change of ground colour.
  float edge = 1.0 - smoothstep(0.12, 0.45, abs(dot(rn, v)));
  float light = min(band, castWide(root, rn));
  float border = 1.0 - abs(2.0 * light - 1.0);
  vec2 span = groundRect.zw - groundRect.xy;
  float g0 = luma(texture(groundColor, clamp((root.xy - groundRect.xy) / span, 0.0, 1.0)).rgb);
  float g1 = luma(texture(groundColor, clamp((root.xy + t1.xy * 0.05 - groundRect.xy) / span, 0.0, 1.0)).rgb);
  float g2 = luma(texture(groundColor, clamp((root.xy + t2.xy * 0.05 - groundRect.xy) / span, 0.0, 1.0)).rgb);
  float colour = smoothstep(0.02, 0.08, (abs(g1 - g0) + abs(g2 - g0)) / max(g0, 1e-3));
  float score = max(edge, max(border, colour));
  float keep = mix(tuftKeep.x, 1.0, score) * (1.0 - smoothstep(0.6 * tuftKeep.y, tuftKeep.y, away));
#endif
  // Tall blades grow mostly on the lighter patches of the ground's colour (a tenth on the darkest).
  vGround = 1.0;
  if (tuftData.z > 15.5 && tuftData.z < 19.5) {
    float lightness = dot(texture(groundColor, clamp((root.xy - groundRect.xy) / (groundRect.zw - groundRect.xy), 0.0, 1.0)).rgb, vec3(0.2126, 0.7152, 0.0722));
    // and only where lit (shade band and cast shadow): a blade reads as it catches the sun, as a glint.
    float sun = min(band, reliefCast(root, rn));
    vGround = smoothstep(tuftLight.x, tuftLight.y, lightness);
    keep *= mix(0.45, 1.0, smoothstep(tuftLight.x, tuftLight.y, lightness)) * mix(0.45, 1.0, smoothstep(0.3, 0.8, sun));
  }
  if (!close && rank > keep) { gl_Position = vec4(0.0, 0.0, 2.0, 1.0); return; }
  // Grass grows mostly upward: the tuft stands between the relief's normal and straight up, turned to face the camera.
  vec3 grow = normalize(mix(rn, vec3(0.0, 0.0, 1.0), 0.6));
  vec3 side = cross(grow, v);
  side = length(side) < 1e-4 ? t1 : normalize(side);
  // Each tuft tilted its own way across the view (the tall blades most), so they do not all stand straight up.
  float tilt = (fract(rank * 17.31 + 0.5) - 0.5) * (tuftData.z > 19.5 ? 1.6 : tuftData.z > 15.5 ? 0.9 : 0.35);
  grow = normalize(grow + side * tilt);
  float mirror = fract(rank * 91.7 + tuftKeep.z) < 0.5 ? -1.0 : 1.0;
  vec3 world = root - grow * 0.08 * tuftData.x + side * position.x * tuftData.y * mirror + grow * position.y * tuftData.x;
  float cell = tuftData.z;
  vUv = (vec2(mod(cell, 4.0), floor(cell / 4.0)) + uv) / vec2(4.0, 6.0);
  vKind = cell < 15.5 ? 0.0 : cell < 19.5 ? 1.0 : 2.0;
  vHue = fract(rank * 53.7 + 0.21);
  // Painted as seen: a tuft rooted on ground turned away from the view (the far side of a crest, the outline of a clump)
  // takes its normal turned toward the camera until it faces it a little, so the fuzz over an outline matches the slope it
  // is seen against, not the hidden side it grows on. Tufts on ground facing the view keep their own normal.
  float facing = dot(rn, v);
  vec3 pn = facing < tuftFace ? normalize(rn + v * (tuftFace - facing)) : rn;
  vColor = reliefPaint(root, pn, h, tone);
  vLit = min(smoothstep(-shadeBand.y, shadeBand.y, dot(pn, normalize(msSunDirection)) - shadeBand.x), reliefCast(root, pn));
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}`;

const FRAGMENT = /* glsl */`
#include <common>
uniform sampler2D tuftAtlas;
varying vec2 vUv;
varying vec3 vColor;
varying float vLit;
varying float vKind;
varying float vHue;
uniform vec3 tuftLime; // the tall blades' tip colour, a bright lime
varying float vGround;
uniform vec3 tuftTip; // tall tips: share only a little lighter (x), the rest from y to z times the ground
uniform vec2 tuftVary; // chubby tufts: x brightness range (share), y hue shift (0 to 1)
uniform vec2 tuftDetail; // x: brightness of the thin tufts, y: their hue variation
uniform vec3 tuftSpark;
void main() {
  vec4 t = texture(tuftAtlas, vUv);
  if (t.a < 0.02) discard;
  vec3 c;
  float a;
  // The thin tufts' hues: warm straw, yellow-green, lime and cool teal, each tuft its own place among them.
  float hs = vHue * 3.0;
  vec3 hue = hs < 1.0 ? mix(vec3(1.3, 1.12, 0.5), vec3(1.12, 1.2, 0.55), hs) : hs < 2.0 ? mix(vec3(1.12, 1.2, 0.55), vec3(0.95, 1.25, 0.45), hs - 1.0) : mix(vec3(0.95, 1.25, 0.45), vec3(0.78, 1.08, 1.3), hs - 2.0);
  vec3 bright = vColor * tuftDetail.x * mix(vec3(1.0), hue, tuftDetail.y) * mix(0.88, 1.12, t.r);
  if (vKind < 0.5) {
    // Chubby: the ground's own paint, each tuft its own brightness and a slight warm or cool shift, its strokes a little
    // apart in tone; soft edged.
    vec3 shift = mix(vec3(1.07, 1.03, 0.82), vec3(0.93, 1.0, 1.15), vHue);
    c = vColor * mix(1.0 - tuftVary.x, 1.0 + tuftVary.x, fract(vHue * 7.13 + 0.3)) * mix(vec3(1.0), shift, tuftVary.y) * mix(0.92, 1.08, t.r);
    a = smoothstep(0.0, 0.85, t.a);
  } else if (vKind < 1.5) {
    // Tall thin blades: the ground's paint at the base, turning brighter and to their own hue (warm yellow-green to
    // cool blue-green) up the blade; crisp edged.
    // Toward the tip it gleams: brighter with the light it catches, toward a pale sunlit yellow-green.
    // On dark ground the blade stays dark; on light ground its tip gleams neon lime, brighter where lit.
    // The ground's own colour at the base, lighter up the blade: most tips a little (1.1 to 1.35 times), some much more
    // (1.7 to 2.3 times), each tuft its own; more where lit.
    float pick = fract(vHue * 7.13 + 0.3);
    float lift = pick < tuftTip.x ? mix(1.1, 1.35, pick / tuftTip.x) : mix(tuftTip.y, tuftTip.z, (pick - tuftTip.x) / (1.0 - tuftTip.x));
    c = vColor * mix(0.94, 1.06, t.r) * mix(1.0, (lift - 1.0) * (0.7 + 0.3 * vLit) + 1.0, smoothstep(0.25, 1.0, t.g));
    a = smoothstep(0.3, 0.6, t.a);
  } else {
    // Clusters: short grass in the sun, bright and brighter where lit; soft edged like the chubby tufts.
    // The patch's own colour pushed bright, much brighter where lit, so each dash matches the grass it lies on.
    c = vColor * mix(0.92, 1.08, t.r) * mix(vec3(1.8), tuftSpark, vLit);
    a = smoothstep(0.02, 0.6, t.a);
  }
  gl_FragColor = vec4(c, a);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}`;

/** Tufts over `ground` (the lawn, before relief), painted and placed by the relief mesh `relief` (grass-relief.ts). */
export function grassTufts(ground: THREE.Mesh, relief: THREE.Mesh, options: GrassTuftOptions = {}): THREE.Mesh[] {
  const { roots, ups, data } = grassTuftSamples(ground, options), count = data.length / 4;
  // Two meshes: the chubby tufts, blurred with the relief, and the thin ones, drawn after it unblurred.
  const atlas = tuftAtlas();
  const shared = (relief.material as THREE.ShaderMaterial);
  // Three meshes: chubby tufts and the sunlit clusters (drawn after them, so they are not buried), both blurred with the
  // relief, and the tall blades, drawn after it unblurred.
  const build = (part: "chubby" | "cluster" | "tall"): THREE.Mesh => {
  const detail = part === "tall";
  const picked: number[] = [];
  const partOf = (cell: number) => cell < 16 ? "chubby" : cell < 20 ? "tall" : "cluster";
  for (let k = 0; k < count; k++) if (partOf(data[k * 4 + 2]) === part) picked.push(k);
  const pick = (source: Float32Array, size: number) => { const out = new Float32Array(picked.length * size); picked.forEach((k, i) => out.set(source.subarray(k * size, k * size + size), i * size)); return out; };
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([-0.5, 0, 0, 0.5, 0, 0, 0.5, 1, 0, -0.5, 1, 0], 3));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute([0, 0, 1, 0, 1, 1, 0, 1], 2));
  geometry.setIndex([0, 1, 2, 0, 2, 3]);
  geometry.setAttribute("tuftRoot", new THREE.InstancedBufferAttribute(pick(roots, 3), 3));
  geometry.setAttribute("tuftUp", new THREE.InstancedBufferAttribute(pick(ups, 3), 3));
  geometry.setAttribute("tuftData", new THREE.InstancedBufferAttribute(pick(data, 4), 4));
  geometry.instanceCount = picked.length;
  const material = new THREE.ShaderMaterial({
    vertexShader: VERTEX, fragmentShader: FRAGMENT, transparent: true, depthWrite: false,
    // The chubby tufts gather where the picture changes (over a base share); the tall blades are kept everywhere.
    defines: { ...shared.defines, ...(detail || (options.everywhere ?? false) ? { TUFT_EVERYWHERE: "" } : {}) },
    uniforms: {
      ...shared.uniforms,
      tuftAtlas: { value: atlas },
      tuftVary: { value: new THREE.Vector2(...(options.vary ?? [0, 0])) },
      tuftLight: { value: new THREE.Vector2(...(options.light ?? [0.33, 0.45])) },
      tuftLime: { value: new THREE.Vector3(...(options.lime ?? [0.42, 1.0, 0.02])) },
      tuftTip: { value: new THREE.Vector3(...(options.tip ?? [0.7, 1.7, 2.3])) },
      tuftFace: { value: options.face ?? 0.35 },
      tuftNear: { value: new THREE.Vector2(...(options.near ?? [3, 6])) },
      tuftKeep: { value: new THREE.Vector3(options.base ?? 0.5, options.far ?? 30, 0.37) },
      tuftDetail: { value: new THREE.Vector2(options.brightness ?? 1.4, options.hue ?? 1.0) },
      tuftSpark: { value: new THREE.Vector3(...(options.sparkle ?? [3.1, 3.0, 1.3])) },
    },
  });
  followPaintedFinishing(material);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = `grass/tufts-${part}`;
  mesh.frustumCulled = false;
  mesh.renderOrder = part === "cluster" ? 4 : 3;
  if (detail) mesh.userData.clumpDetail = true; else mesh.userData.clumpBlur = true;
  mesh.userData.grassTufts = { part, candidates: picked.length };
  return mesh;
  };
  return [build("chubby"), build("cluster"), build("tall")];
}
