import * as THREE from "three";
import { FoliageVisibility } from "./foliage-visibility.ts";
import { createSplatDepthOrder } from "./splat-depth-order.mjs";

const levels = [
  { start: 0, keep: 1, scale: 1 },
  { start: 9, keep: 0.42, scale: 1.26 },
  { start: 17, keep: 0.14, scale: 1.62 },
  { start: 31, keep: 0.045, scale: 1.95 },
];

const basePaint = ["#20384a", "#5e8047", "#a79d5e"].map(color => new THREE.Color(color));
const luminance = ({ r, g, b }) => r * 0.2126 + g * 0.7152 + b * 0.0722;
function hueRamp(degrees) {
  return basePaint.map(color => {
    const { h, s, l } = color.getHSL({ h: 0, s: 0, l: 0 }, THREE.SRGBColorSpace);
    const tint = new THREE.Color().setHSL(h + degrees / 360, s, l, THREE.SRGBColorSpace);
    return tint.multiplyScalar(luminance(color) / luminance(tint));
  });
}
const paintRamp = [...basePaint, ...hueRamp(-18), ...hueRamp(18)];
const paintRampShader = /* glsl */ `
  uniform vec3 canopyPaint[9];
  uniform float canopyHueStrength;
  vec3 tintStop(vec3 neutral, vec3 warm, vec3 cool, float hue) {
    return mix(neutral, hue < 0.0 ? warm : cool, abs(hue));
  }
  vec3 canopyInk(float light, float hue) {
    vec3 shade = tintStop(canopyPaint[0], canopyPaint[3], canopyPaint[6], hue);
    vec3 middle = tintStop(canopyPaint[1], canopyPaint[4], canopyPaint[7], hue);
    vec3 sun = tintStop(canopyPaint[2], canopyPaint[5], canopyPaint[8], hue);
    return mix(mix(shade, middle, smoothstep(0.05, 0.53, light)), sun, smoothstep(0.37, 0.82, light));
  }
`;

/** Give the opaque far shell the same pigments as the leaf dabs it replaces. */
export function createGaussianCanopyMaterial(hueStrength) {
  const material = new THREE.MeshStandardMaterial({ color: "#ffffff", roughness: 0.98, toneMapped: false });
  material.onBeforeCompile = shader => {
    shader.uniforms.canopyPaint = { value: paintRamp };
    shader.uniforms.canopyHueStrength = hueStrength;
    shader.vertexShader = "attribute float canopyHue;\nvarying float leafHue;\n"
      + shader.vertexShader.replace("#include <begin_vertex>", "#include <begin_vertex>\nleafHue = canopyHue;");
    shader.fragmentShader = "varying float leafHue;\n" + paintRampShader + shader.fragmentShader.replace("#include <opaque_fragment>", `
      float shellLight = dot(outgoingLight, vec3(0.2126, 0.7152, 0.0722));
      outgoingLight = canopyInk(clamp((shellLight - 0.23) * 1.53 + 0.025, 0.0, 1.0), leafHue * canopyHueStrength);
      #include <opaque_fragment>
    `);
  };
  return material;
}

const vertexShader = /* glsl */ `
  #include <common>
  #include <shadowmap_pars_vertex>
  attribute vec3 center;
  attribute vec3 major;
  attribute vec3 minor;
  attribute vec4 pigment;
  attribute float thickness;
  attribute vec3 shadeNormal;
  attribute float packedCrownOffset;
  uniform vec2 viewport;
  uniform bool billboard;
  uniform float leafNormalTilt;
  varying vec2 dab;
  varying vec4 paint;
  varying float coverage;
  varying vec3 lightNormal;
  varying vec3 crownDirection;
  varying float strokeShape;
  varying float leafHue;

  // Derivative of the perspective divide, in pixels per world-space sigma.
  vec2 projectAxis(vec3 axis, vec4 clip) {
    vec4 delta = projectionMatrix * vec4(mat3(modelViewMatrix) * axis, 0.0);
    return (delta.xy - clip.xy / clip.w * delta.w) / clip.w * viewport * 0.5;
  }

  // Seed in scene space so changing the view or the sun keeps each leaf's tilt.
  vec3 tiltedLeafNormal(vec3 normal, vec3 point) {
    vec3 seed = fract(point * vec3(1.039, 1.093, 0.871) + vec3(0.17, 0.37, 0.53));
    seed += dot(seed, seed.yzx + 19.19);
    vec3 variation = fract((seed.xxy + seed.yzz) * seed.zyx) * 2.0 - 1.0;
    vec3 tangent = variation - normal * dot(normal, variation);
    tangent *= inversesqrt(max(1.0, dot(tangent, tangent)));
    return normalize(normal + leafNormalTilt * tangent);
  }

  void main() {
    vec4 mvPosition = modelViewMatrix * vec4(center, 1.0);
    vec4 clip = projectionMatrix * mvPosition;
    if (clip.z <= -clip.w || clip.w <= 0.0) {
      gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
      return;
    }
    vec2 u = projectAxis(major, clip);
    vec2 v = projectAxis(minor, clip);
    vec2 w = vec2(0.0);
    if (billboard) {
      float pixelsPerMeter = projectionMatrix[1][1] * viewport.y * 0.5 / clip.w;
      vec2 direction = dot(u, u) > 1e-6 ? normalize(u) : vec2(0.0, 1.0);
      u = direction * length(major) * pixelsPerMeter;
      v = vec2(-direction.y, direction.x) * length(minor) * pixelsPerMeter;
    } else {
      // The three projected axes give J * covariance * transpose(J).
      w = projectAxis(normalize(cross(major, minor)) * thickness, clip);
    }
    float xx = dot(vec3(u.x, v.x, w.x), vec3(u.x, v.x, w.x));
    float yy = dot(vec3(u.y, v.y, w.y), vec3(u.y, v.y, w.y));
    float xy = dot(vec3(u.x, v.x, w.x), vec3(u.y, v.y, w.y));
    float spread = length(vec2(xx - yy, 2.0 * xy));
    vec2 eigenvector = xx >= yy ? vec2(spread + xx - yy, 2.0 * xy) : vec2(2.0 * xy, spread + yy - xx);
    vec2 direction = spread > 1e-6 ? normalize(eigenvector) : vec2(1.0, 0.0);
    // An ellipse is symmetric, but an asymmetric stroke needs a consistent tip.
    if (dot(direction, u) < 0.0) direction = -direction;
    vec2 variance = max(vec2(0.0), 0.5 * vec2(xx + yy + spread, xx + yy - spread));
    vec2 filtered = variance + vec2(0.16); // Subpixel low-pass filter.
    dab = position.xy * 3.0;
    vec2 offset = direction * sqrt(filtered.x) * dab.x
                + vec2(-direction.y, direction.x) * sqrt(filtered.y) * dab.y;
    gl_Position = clip + vec4(offset * 2.0 / viewport * clip.w, 0.0, 0.0);
    float paintCode = floor(pigment.a);
    strokeShape = floor(paintCode / 256.0);
    leafHue = (mod(paintCode, 256.0) - 128.0) / 127.0;
    paint = vec4(pigment.rgb, fract(pigment.a) * 2.0);
    coverage = sqrt((variance.x * variance.y) / (filtered.x * filtered.y));
    vec3 surfaceNormal = normalize(shadeNormal);
    vec3 leafNormal = surfaceNormal;
    if (leafNormalTilt > 0.0 && strokeShape > 0.5 && strokeShape < 6.5) {
      leafNormal = tiltedLeafNormal(surfaceNormal, center);
    }
    lightNormal = normalize(normalMatrix * leafNormal) * length(shadeNormal);
    vec3 crownOffset = vec3(mod(packedCrownOffset, 256.0), mod(floor(packedCrownOffset / 256.0), 256.0),
      floor(packedCrownOffset / 65536.0)) * (2.5 / 255.0) - 1.25;
    crownDirection = mat3(modelViewMatrix) * crownOffset;
    vec4 worldPosition = modelMatrix * vec4(center + surfaceNormal * 0.012, 1.0);
    #include <shadowmap_vertex>
  }
`;

const fragmentShader = /* glsl */ `
  #include <common>
  #include <bsdfs>
  #include <lights_pars_begin>
  #include <shadowmap_pars_fragment>
  #include <shadowmask_pars_fragment>
  varying vec2 dab;
  varying vec4 paint;
  varying float coverage;
  varying vec3 lightNormal;
  varying vec3 crownDirection;
  varying float strokeShape;
  varying float leafHue;
  uniform float firmness;
  uniform bool relight;
  ${paintRampShader}

  float paintEdge(vec2 point, float shape) {
    shape = floor(shape + 0.5);
    // Three silhouettes, each with a reversed variant along the leaf's long axis.
    float style = mod(shape - 1.0, 3.0);
    float x = shape > 3.5 ? -point.x : point.x;
    float y = point.y;
    if (style < 0.5) {
      // A clipped, slightly skewed flat brush.
      float across = abs(y + 0.13 * x);
      return max(max(abs(x) - 1.02, across - 0.83), 0.68 * abs(x) + 0.72 * across - 0.99);
    }
    if (style < 1.5) {
      // A stroke that narrows to an angled tip.
      float across = abs(y - 0.14 * x - 0.06);
      float width = 0.78 - 0.37 * x;
      return max(max(abs(x + 0.02) - 1.10, across - width), -x + 0.88 * across - 1.27);
    }
    // A dry edge with three shallow bristle cuts and a clean rounded start.
    float across = y + 0.16 * x;
    float bristles = 1.0 - 2.0 * abs(fract((across + 0.95) * 1.9) - 0.5);
    float tip = 0.98 + 0.20 * across - 0.17 * bristles * bristles;
    float heel = length(vec2(min(x + 0.28, 0.0), across)) - 0.77;
    return max(x - tip, heel);
  }

  void main() {
    float radiusSquared = dot(dab, dab);
    float gaussian = exp(-0.5 * radiusSquared) * (1.0 - smoothstep(7.0, 9.0, radiusSquared));
    float brush = smoothstep(0.29, 0.64, gaussian);
    bool interior = strokeShape > 6.5;
    if (!interior && strokeShape > 0.5 && firmness > 0.0) {
      float edge = paintEdge(dab / 1.42, strokeShape);
      float feather = max(0.022, 0.68 * fwidth(edge));
      brush = 1.0 - smoothstep(-feather, feather, edge);
    }
    // A core dab has a round, soft edge and keeps its compact support at every firmness.
    float mask = interior ? 1.0 - smoothstep(0.58, 4.0, radiusSquared) : mix(gaussian, brush, firmness);
    float alpha = paint.a * coverage * mask;
    if (alpha < 0.0039) discard;
    vec3 pigment = paint.rgb;
    vec3 pigmentLight = vec3(1.0);
    float hue = leafHue * canopyHueStrength;
    if (relight) {
      pigmentLight = vec3(0.42, 0.50, 0.56);
      #if NUM_DIR_LIGHTS > 0
        vec3 toSun = directionalLights[0].direction;
        vec3 clumpNormal = normalize(lightNormal);
        float light = dot(clumpNormal, toSun);
        float shadow = getShadowMask();
        if (strokeShape > 0.5) {
          // A local clump can catch light, but the crown's far side belongs to a darker paint band.
          float crownSide = smoothstep(-0.56, 0.45, dot(crownDirection, toSun));
          float openness = clamp(2.0 * length(lightNormal) - 1.0, 0.0, 1.0);
          float skyFacing = dot(clumpNormal, mat3(viewMatrix) * vec3(0.0, 0.0, 1.0)) * 0.5 + 0.5;
          float clumpBand = 0.11 + 0.26 * smoothstep(-0.27, 0.28, light)
            + 0.16 * smoothstep(0.43, 0.78, light);
          float ambient = (0.06 + 0.11 * openness) * mix(0.69, 1.0, crownSide) * mix(0.84, 1.10, skyFacing);
          float sunStrength = clamp(directionalLights[0].color.r / 2.25, 0.0, 1.4);
          float direct = sunStrength * clumpBand * mix(0.13, 1.31, crownSide) * shadow * (interior ? 0.36 : 1.0);
          float leafTone = dot(paint.rgb, vec3(0.2126, 0.7152, 0.0722));
          pigment = canopyInk(ambient + direct, hue);
          pigmentLight = vec3(clamp(0.55 + 2.1 * leafTone, 0.74, 1.1));
        } else {
          float wrapped = clamp((light + 0.42) / 1.42, 0.0, 1.0);
          float transmission = max(0.0, -light) * 0.065;
          pigmentLight += directionalLights[0].color * (0.47 * wrapped + transmission) * shadow;
        }
      #endif
    } else if (!interior && strokeShape > 0.5 && canopyHueStrength > 0.0) {
      pigment *= canopyInk(0.52, hue) / canopyInk(0.52, 0.0);
    }
    gl_FragColor = vec4(pigment * pigmentLight, alpha);
    #include <colorspace_fragment>
  }
`;

export function createGaussianSplats(plants, patches = [new THREE.Vector3()]) {
  const stride = 18;
  const capacity = plants.length * patches.length;
  const source = new Float32Array(plants.length * stride);
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setIndex([0, 1, 2, 0, 2, 3]);
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([-1, -1, 0, 1, -1, 0, 1, 1, 0, -1, 1, 0], 3));
  let instances, stagedCapacity = 0;
  function reserve(required) {
    if (required <= stagedCapacity) return;
    if (instances) geometry.dispose();
    stagedCapacity = Math.min(capacity, Math.max(required, stagedCapacity * 2));
    instances = new THREE.InstancedInterleavedBuffer(new Float32Array(stagedCapacity * stride), stride);
    instances.setUsage(THREE.DynamicDrawUsage);
    for (const [name, size, offset] of [
      ["center", 3, 0], ["major", 3, 3], ["minor", 3, 6], ["pigment", 4, 9], ["thickness", 1, 13],
      ["shadeNormal", 3, 14], ["packedCrownOffset", 1, 17],
    ]) {
      geometry.setAttribute(name, new THREE.InterleavedBufferAttribute(instances, size, offset));
    }
  }
  const color = new THREE.Color();
  const patchColor = new THREE.Color();
  const fineColor = new THREE.Color();
  const individualPigments = new Float32Array(plants.length * 3);
  const patchPigments = new Float32Array(individualPigments.length);
  const bounds = new THREE.Box3();
  const center = new THREE.Vector3(), support = new THREE.Vector3();
  const ranks = new Float32Array(plants.length);
  const interiorDabs = new Uint8Array(plants.length);
  const byKind = { leaves: 0, grass: 0 };
  for (const [index, plant] of plants.entries()) {
    color.set(plant.color);
    color.toArray(individualPigments, index * 3);
    patchColor.set(plant.patchColor);
    if (plant.finePatchColor) patchColor.lerp(fineColor.set(plant.finePatchColor), 0.28);
    patchColor.lerp(color, 0.38).toArray(patchPigments, index * 3);
    interiorDabs[index] = plant.interior ? 1 : 0;
    const leafShape = 1 + Math.floor(((index + 1) * 0.41421356237 % 1) * 6);
    const shape = plant.interior ? 7 : plant.kind === "leaves" ? leafShape : 0;
    // The direction carries the clump normal and its length carries ambient openness.
    const exposedNormal = center.fromArray(plant.normal).multiplyScalar(0.5 + 0.5 * plant.openness);
    // One byte per axis is enough for the broad lighting ramp. All 24 bits fit exactly in one float.
    const [x, y, z] = plant.crownOffset.map(axis => Math.round(255 * THREE.MathUtils.clamp((axis + 1.25) / 2.5, 0, 1)));
    const packedCrownOffset = x | (y << 8) | (z << 16);
    // Shape and hue use the integer bits. Opacity uses half the fractional range.
    const hueCode = 128 + Math.round(127 * THREE.MathUtils.clamp(plant.hue ?? 0, -1, 1));
    const paintCode = 256 * shape + hueCode + 0.5 * plant.opacity;
    source.set([...plant.position, ...plant.major, ...plant.minor, color.r, color.g, color.b,
      paintCode, plant.thickness, ...exposedNormal, packedCrownOffset], index * stride);
    ranks[index] = (byKind[plant.kind]++ * 0.61803398875) % 1;
    center.fromArray(plant.position);
    support.setScalar(3 * levels.at(-1).scale * Math.max(Math.hypot(...plant.major), Math.hypot(...plant.minor), plant.thickness));
    bounds.expandByPoint(center.clone().sub(support));
    bounds.expandByPoint(center.add(support));
  }
  const material = new THREE.ShaderMaterial({
    vertexShader,
    fragmentShader,
    lights: true,
    uniforms: THREE.UniformsUtils.merge([THREE.UniformsLib.lights, {
      viewport: { value: new THREE.Vector2() }, billboard: { value: false }, firmness: { value: 0 }, relight: { value: true },
      leafNormalTilt: { value: 0 },
      canopyPaint: { value: paintRamp },
      canopyHueStrength: { value: 1 },
    }]),
    transparent: true,
    depthWrite: false,
    toneMapped: false,
  });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.frustumCulled = false;
  mesh.receiveShadow = true;
  const direction = new THREE.Vector3();
  const lastDirection = new THREE.Vector3();
  const depthOrder = createSplatDepthOrder(capacity);
  const patchDepths = new Float64Array(patches.length);
  const localDepths = new Float64Array(plants.length);
  const active = new Uint32Array(capacity);
  const patchLevels = new Int8Array(patches.length);
  const frustum = new THREE.Frustum(), clip = new THREE.Matrix4();
  const visibility = new FoliageVisibility(new Map(patches.map((at, index) => [index, bounds.clone().translate(at)])),
    levels.map(({ start }, index) => [index, start]), Infinity);
  let mode = "all";
  let variants = [];
  let activeCount = 0, visiblePatches = 0;
  let dirty = true;

  function setPigmentPatches(enabled) {
    const pigments = enabled ? patchPigments : individualPigments;
    for (let index = 0; index < plants.length; index++) {
      const from = index * 3, to = index * stride + 9;
      source[to] = pigments[from];
      source[to + 1] = pigments[from + 1];
      source[to + 2] = pigments[from + 2];
    }
    dirty = true;
  }

  function select({ leaves, grass, density, policy = "all", includes }) {
    mode = policy;
    const enabled = { leaves, grass };
    const limits = mode === "all" ? levels.slice(0, 1) : levels;
    variants = limits.map(({ keep }, level) => plants.flatMap((plant, index) =>
      enabled[plant.kind] && ranks[index] < keep * density && (level < 2 || !plant.interior)
        && (!includes || includes(plant)) ? [index] : []));
    dirty = true;
  }

  function prepare(camera) {
    const selectStart = performance.now();
    camera.updateMatrixWorld();
    let selectionChanged = dirty;
    if (mode !== "all") {
      frustum.setFromProjectionMatrix(clip.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
      if (visibility.update(frustum, camera.position, mode === "lod" ? 1 : false)) selectionChanged = true;
    }
    camera.getWorldDirection(direction);
    if (selectionChanged) {
      activeCount = 0;
      visiblePatches = 0;
      for (let patch = 0; patch < patches.length; patch++) {
        const level = mode === "all" ? 0 : visibility.chosen.get(patch);
        patchLevels[patch] = level;
        if (level < 0) continue;
        const base = patch * plants.length;
        for (const local of variants[level]) active[activeCount++] = base + local;
        visiblePatches++;
      }
      geometry.instanceCount = activeCount;
      mesh.visible = activeCount > 0;
    }
    const sortStart = performance.now();
    if (!selectionChanged && direction.distanceToSquared(lastDirection) < 1e-12) {
      return { selectMs: sortStart - selectStart, sortMs: 0, packMs: 0 };
    }
    for (let index = 0; index < plants.length; index++) {
      const offset = index * stride;
      localDepths[index] = direction.x * source[offset] + direction.y * source[offset + 1] + direction.z * source[offset + 2];
    }
    for (let patch = 0; patch < patches.length; patch++) patchDepths[patch] = direction.dot(patches[patch]);
    for (let i = 0; i < activeCount; i++) {
      const index = active[i];
      depthOrder.depths[index] = localDepths[index % plants.length] + patchDepths[Math.floor(index / plants.length)];
    }
    const drawn = depthOrder.sort(active, activeCount);
    const packStart = performance.now();
    reserve(activeCount);
    for (let row = 0; row < activeCount; row++) {
      const index = drawn[row];
      const patch = Math.floor(index / plants.length);
      const local = index % plants.length;
      const at = patches[patch], scale = interiorDabs[local] ? 1 : levels[patchLevels[patch]].scale;
      const offset = local * stride, target = row * stride;
      instances.array[target] = source[offset] + at.x;
      instances.array[target + 1] = source[offset + 1] + at.y;
      instances.array[target + 2] = source[offset + 2] + at.z;
      for (let field = 3; field < 9; field++) instances.array[target + field] = source[offset + field] * scale;
      for (let field = 9; field < stride; field++) instances.array[target + field] = source[offset + field];
      instances.array[target + 13] *= scale;
    }
    if (activeCount) {
      instances.clearUpdateRanges();
      instances.addUpdateRange(0, activeCount * stride);
      instances.needsUpdate = true;
    }
    lastDirection.copy(direction);
    dirty = false;
    return { selectMs: sortStart - selectStart, sortMs: packStart - sortStart, packMs: performance.now() - packStart };
  }

  return {
    mesh, material, select, setPigmentPatches, prepare, stats: () => ({
      activeCount, visiblePatches, capacity, stagedCapacity, instanceBytes: activeCount * stride * Float32Array.BYTES_PER_ELEMENT,
    }),
    dispose: () => { geometry.dispose(); material.dispose(); },
  };
}
