import * as THREE from "three";

/**
 * Leaf normal map (prototype): a tileable texture painted with leaves, each leaf a small dome (its normal tips away
 * from its own middle) with a random tilt of its own, later leaves over earlier ones. Laid on a flat face and shaded in
 * bands against the sun, it breaks the face's light and shade into leaf-shaped patches: the banding of a painted bush
 * done per pixel on the GPU, on any surface.
 *
 * Texel layout (RGBA bytes): rgb = the tangent-space normal * 0.5 + 0.5 (z out of the surface); a = the leaf's tone.
 */
export interface LeafNormalOptions {
  size?: number;
  seed?: number;
  /** Leaf length in texels, the least and the most. */
  leafLength?: [number, number];
  /** Leaf width over length. */
  leafWidth?: number;
  /** Times the leaves cover the tile. */
  cover?: number;
  /** How far a leaf's dome tips its normal at its rim. */
  dome?: number;
  /** How far a whole leaf tilts at random. */
  tilt?: number;
}

function generator(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * The map painted in strokes, as a digital painter would paint a normal map with a leaf brush: each stroke is one
 * normal colour (one direction) laid along a curved path, stamping leaves that lean along it, with only a slight curve
 * across its width; later strokes over earlier ones. Within a stroke the light is one band; across strokes it steps,
 * and every step is a leafy stroke edge.
 */
export interface StrokeNormalOptions {
  size?: number;
  seed?: number;
  /** Strokes painted over the tile. */
  strokes?: number;
  /** Stroke length and width as shares of the tile. */
  length?: [number, number];
  width?: [number, number];
  /** A leaf's length as a share of the tile. */
  leaf?: [number, number];
  /** How far a stroke's normal tilts, at most. */
  tilt?: number;
  /** How far the normal curves across a stroke's width. */
  curve?: number;
  /** How far each leaf's own dome tips its normal at its rim (0: each leaf flat, one normal). */
  leafDome?: number;
  /** How far a leaf's normal strays from its stroke's. */
  jitter?: number;
  /** Steps along a stroke, as a share of its half-width, and leaves stamped per step: how loosely leaves sit. */
  spacing?: number;
  perStep?: number;
  /** Strokes of leaf-clump stamps on a jittered grid (true), or loose leaves along random strokes. */
  clumps?: boolean;
  /** Leaves in one clump stamp (0: 4 to 7); more makes them smaller so a clump keeps its size. */
  clumpLeaves?: number;
}

export function strokeNormalData(options: StrokeNormalOptions = {}): { size: number; data: Uint8Array; strokes: number } {
  // A tile is laid ~0.6 m across, so leaves of 0.06-0.09 of it are the contour fuzz's 4-5 cm.
  const { size = 512, seed = 7, strokes = 49, length = [0.2, 0.45], width = [0.08, 0.14], leaf = [0.06, 0.09], tilt = 0.75, curve = 0.25, leafDome = 0, jitter = 0.3, spacing = 0.55, perStep = 1, clumps = true, clumpLeaves = 0 } = options;
  const random = generator(seed);
  const between = (a: number, b: number) => a + (b - a) * random();
  // Texel layout: r, g = the normal's x, y (* 0.5 + 0.5; z = sqrt(1 - x² - y²)); b = the leaf's tone, its rim darker;
  // a = 0 in a gap, else the stroke's own random in (0, 1], so a renderer can keep or drop a stroke whole.
  const data = new Uint8Array(size * size * 4);
  // Gaps between leaves: facing straight out, in a dark tone, so loose leaves read against them.
  for (let i = 0; i < size * size; i++) data.set([128, 128, 70, 0], i * 4);
  // Alpha: 255 for a stroke an upper copy keeps, 128 for one it drops, 0 for a gap. Coverage, not an id, so filtering
  // blends it smoothly instead of producing in-between ids.
  let strokeId = 255;
  const stamp = (x: number, y: number, angle: number, len: number, wid: number, n: [number, number, number], tone: number) => {
    const cos = Math.cos(angle), sin = Math.sin(angle), box = len + wid, round = between(0.55, 0.85);
    for (let py = Math.floor(y - box); py <= Math.ceil(y + box); py++) for (let px = Math.floor(x - box); px <= Math.ceil(x + box); px++) {
      const dx = px + 0.5 - x, dy = py + 0.5 - y;
      const u = (dx * cos + dy * sin) / len;
      if (u <= 0 || u >= 1) continue;
      const v = -dx * sin + dy * cos;
      const half = 0.5 * wid * Math.pow(Math.sin(Math.PI * Math.pow(u, 0.75)), round);
      if (Math.abs(v) >= half) continue;
      // One flat normal per leaf (`n`, already the stroke's jittered for this leaf); `leafDome` adds an optional curve.
      const across = (v / half) * leafDome, along = (u - 0.45) * 1.4 * leafDome;
      let nx = n[0] + across * -sin + along * cos, ny = n[1] + across * cos + along * sin, nz = n[2];
      const l = Math.hypot(nx, ny, nz);
      nx /= l; ny /= l;
      const rim = Math.max(Math.abs(v) / half, u < 0.12 ? 1 - u / 0.12 : 0, u > 0.88 ? (u - 0.88) / 0.12 : 0);
      const shade = tone * (1 - 0.12 * Math.min(Math.max((rim - 0.5) / 0.5, 0), 1));
      const i = ((((py % size) + size) % size) * size + (((px % size) + size) % size)) * 4;
      data[i] = Math.round((nx * 0.5 + 0.5) * 255);
      data[i + 1] = Math.round((ny * 0.5 + 0.5) * 255);
      data[i + 2] = Math.round(Math.min(1, shade) * 255);
      data[i + 3] = strokeId;
    }
  };
  if (clumps) {
    // A leaf brush: a stamp of a small clump of leaves radiating from a point, dragged along strokes that start on a
    // jittered grid, so the tile is covered evenly by many small clumps, each in its stroke's colour.
    const grid = Math.max(1, Math.round(Math.sqrt(strokes)));
    const cell = size / grid;
    for (let gy = 0; gy < grid; gy++) for (let gx = 0; gx < grid; gx++) {
      strokeId = random() < 0.55 ? 255 : 128;
      const a = random() * Math.PI * 2, t = tilt * Math.sqrt(random());
      const base: [number, number] = [Math.cos(a) * t, Math.sin(a) * t];
      let x = (gx + random()) * cell, y = (gy + random()) * cell, heading = random() * Math.PI * 2;
      const bend = between(-0.4, 0.4), total = between(length[0], length[1]) * size, tone = between(0.65, 1);
      const leafLen = () => between(leaf[0], leaf[1]) * size;
      const gap = between(leaf[0], leaf[1]) * size * 1.35;
      for (let d = 0; d < total; d += gap) {
        // One clump: its normal near the stroke's, its leaves radiating at even turns, each a little off.
        const cn: [number, number] = [base[0] + between(-jitter, jitter), base[1] + between(-jitter, jitter)];
        // A clump of `clumpLeaves` (or 4 to 7) leaves; with more, smaller ones (scaled to keep the clump's size) in
        // two loose rings, the inner starting near the middle and the outer further out.
        const many = clumpLeaves > 0;
        const count = many ? clumpLeaves : 4 + Math.floor(random() * 4), spin = random() * Math.PI * 2;
        const scale = many ? Math.min(1, 2.6 / Math.sqrt(clumpLeaves)) : 1;
        for (let k = 0; k < count; k++) {
          const outer = many && k % 2 === 1;
          const angle = spin + (k / count) * Math.PI * 2 + between(-0.35, 0.35);
          const start = (outer ? between(0.55, 0.9) : between(0, 0.25)) * leafLen() * (many ? 1 : 1);
          let nx = cn[0] + between(-0.12, 0.12), ny = cn[1] + between(-0.12, 0.12), nz = 1;
          const l = Math.hypot(nx, ny, nz);
          const len = leafLen() * scale;
          stamp(x + Math.cos(angle) * start, y + Math.sin(angle) * start, angle, len, len * 0.55, [nx / l, ny / l, nz / l], tone * between(0.9, 1.06));
        }
        heading += bend * gap / total * 2;
        x += Math.cos(heading) * gap;
        y += Math.sin(heading) * gap;
      }
    }
    return { size, data, strokes: grid * grid };
  }
  for (let s = 0; s < strokes; s++) {
    strokeId = random() < 0.55 ? 255 : 128;
    // The stroke's colour: a direction tilted up to `tilt` from straight out.
    const a = random() * Math.PI * 2, t = tilt * Math.sqrt(random());
    const base: [number, number] = [Math.cos(a) * t, Math.sin(a) * t];
    let x = random() * size, y = random() * size, heading = random() * Math.PI * 2;
    const bend = between(-0.25, 0.25), total = between(length[0], length[1]) * size, half = between(width[0], width[1]) * size * 0.5;
    const leafSize = () => between(leaf[0], leaf[1]) * size, tone = between(0.6, 1);
    const step = half * spacing;
    for (let d = 0; d < total; d += step) {
      // A few leaves at each step, spread across the stroke, leaning along it; the stroke tapers at its ends.
      const taper = Math.min(1, d / (half * 1.5), (total - d) / (half * 1.5));
      for (let k = 0; k < perStep; k++) {
        const across = (random() * 2 - 1) * half * taper;
        const sx = x - Math.sin(heading) * across, sy = y + Math.cos(heading) * across;
        // The normal curves a little across the stroke (rounded like a brushed ridge).
        const c = (across / Math.max(half, 1)) * curve;
        // Each leaf its own direction near the stroke's: one flat normal per leaf.
        let nx = base[0] - Math.sin(heading) * c + between(-jitter, jitter), ny = base[1] + Math.cos(heading) * c + between(-jitter, jitter), nz = 1;
        const l = Math.hypot(nx, ny, nz);
        stamp(sx, sy, heading + between(-0.6, 0.6), leafSize(), leafSize() * 0.55, [nx / l, ny / l, nz / l], tone * between(0.93, 1.05));
      }
      heading += bend * step / total * 3;
      x += Math.cos(heading) * step;
      y += Math.sin(heading) * step;
    }
  }
  return { size, data, strokes };
}

export function leafNormalData(options: LeafNormalOptions = {}): { size: number; data: Uint8Array; leaves: number } {
  const { size = 512, seed = 5, leafLength = [34, 58], leafWidth = 0.55, cover = 3, dome = 0.55, tilt = 0.45 } = options;
  const random = generator(seed);
  const between = (a: number, b: number) => a + (b - a) * random();
  const data = new Uint8Array(size * size * 4);
  // Gaps read as a surface facing straight out, in a middle tone.
  for (let i = 0; i < size * size; i++) data.set([128, 128, 255, 160], i * 4);
  const mean = (leafLength[0] + leafLength[1]) / 2;
  const count = Math.round(cover * size * size / (mean * mean * leafWidth * 0.6));
  for (let n = 0; n < count; n++) {
    const x = random() * size, y = random() * size, angle = random() * Math.PI * 2;
    const length = between(leafLength[0], leafLength[1]), width = length * leafWidth * between(0.8, 1.2);
    const tx = between(-tilt, tilt), ty = between(-tilt, tilt), tone = between(0.55, 1), round = between(0.5, 0.9);
    const cos = Math.cos(angle), sin = Math.sin(angle);
    const box = length + width;
    for (let py = Math.floor(y - box); py <= Math.ceil(y + box); py++) for (let px = Math.floor(x - box); px <= Math.ceil(x + box); px++) {
      const dx = px + 0.5 - x, dy = py + 0.5 - y;
      const u = (dx * cos + dy * sin) / length;
      if (u <= 0 || u >= 1) continue;
      const v = -dx * sin + dy * cos;
      const half = 0.5 * width * Math.pow(Math.sin(Math.PI * Math.pow(u, 0.75)), round);
      if (Math.abs(v) >= half) continue;
      // The dome in the leaf's frame: across its width and along its length from its middle.
      const across = (v / half) * dome, along = (u - 0.45) * 1.6 * dome;
      let nx = tx + across * -sin + along * cos, ny = ty + across * cos + along * sin, nz = 1;
      const l = Math.hypot(nx, ny, nz);
      nx /= l; ny /= l; nz /= l;
      const i = ((((py % size) + size) % size) * size + (((px % size) + size) % size)) * 4;
      data[i] = Math.round((nx * 0.5 + 0.5) * 255);
      data[i + 1] = Math.round((ny * 0.5 + 0.5) * 255);
      data[i + 2] = Math.round((nz * 0.5 + 0.5) * 255);
      data[i + 3] = Math.round(tone * 255);
    }
  }
  return { size, data, leaves: count };
}

const textures = new Map<number, THREE.DataTexture>();
/** The stroke-painted map, one per clump size (`clumpLeaves`, 0 for the 4-to-7-leaf brush), made once. */
export function leafNormalTexture(clumpLeaves = 0): THREE.DataTexture {
  const known = textures.get(clumpLeaves);
  if (known) return known;
  const { size, data } = strokeNormalData({ clumpLeaves });
  const texture = new THREE.DataTexture(data, size, size, THREE.RGBAFormat, THREE.UnsignedByteType);
  textures.set(clumpLeaves, texture);
  texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  texture.magFilter = THREE.LinearFilter;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.generateMipmaps = true;
  texture.needsUpdate = true;
  return texture;
}

/** The painted bands: six colours (sRGB) from a cool teal shade to a sunlit green, as the washroom shrub's. */
export const BAND_GLSL = /* glsl */`
vec3 msLeafBand(float t) {
  float level = floor(clamp(t, 0.0, 0.999) * 6.0);
  vec3 c = level < 0.5 ? vec3(0.0232, 0.1022, 0.0844) : level < 1.5 ? vec3(0.0331, 0.1441, 0.0823)
    : level < 2.5 ? vec3(0.0452, 0.1779, 0.0782) : level < 3.5 ? vec3(0.0844, 0.2384, 0.0802)
    : level < 4.5 ? vec3(0.15, 0.3277, 0.0844) : vec3(0.2747, 0.4793, 0.1022);
  return c;
}
// The stroke map read at a world point, in the plane facing the camera through origin, tile meters a repeat: two
// copies, the upper's kept strokes blended over the lower by coverage. Returns the normal's x, y along the plane's
// axes t, b (set here) and the leaf's tone in z.
vec3 msLeafMap(sampler2D map, vec3 world, vec3 origin, float tile, out vec3 t, out vec3 b) {
  vec3 view = normalize(cameraPosition - origin);
  t = normalize(cross(vec3(0.0, 0.0, 1.0), view) + vec3(1e-5, 0.0, 0.0));
  b = cross(view, t);
  vec2 uv = vec2(dot(world - origin, t), dot(world - origin, b)) / tile;
  vec2 normalXY = vec2(0.0);
  float tone = 0.27;
  for (int k = 0; k < 2; k++) {
    float a = k == 0 ? 0.3 : 2.1, s = k == 0 ? 1.0 : 0.83;
    vec2 off = k == 0 ? vec2(0.0) : vec2(0.37, 0.61);
    mat2 turn = mat2(cos(a), sin(a), -sin(a), cos(a));
    vec4 c = texture(map, turn * uv * s + off);
    vec2 xy = c.rg * 2.0 - 1.0;
    xy = vec2(cos(a) * xy.x + sin(a) * xy.y, -sin(a) * xy.x + cos(a) * xy.y);
    float cover = k == 0 ? 1.0 : smoothstep(0.62, 0.9, c.a);
    normalXY = mix(normalXY, xy, cover);
    tone = mix(tone, c.b, cover);
  }
  return vec3(normalXY, tone);
}
// Where a normal falls in the bands: by its angle from the sun, not the cosine of it, so a leaf tipped a given angle
// moves the same way through the bands anywhere on the plant. The cosine flattens toward its ends: on the shaded side
// every leaf of a card would fall in the bottom band, and the card read as one flat dark disc.
// The shaded side takes the sky's fill as a floor, as a painter lifts it: a leaf tipped up toward the sky steps up to
// the second or third band, one turned down stays in the first, so leaves facing away from the sun still band leaf by
// leaf rather than merge into one dark patch.
// msContrast stretches the light about the middle of the bands (unset or 0: 1, as before), so more of a plant falls
// in its darkest and lightest bands.
uniform float msContrast;
float msLeafLight(vec3 n, vec3 sun, float offset) {
  float direct = 1.0 - acos(clamp(dot(n, sun), -1.0, 1.0)) / 3.14159265 + offset;
  float sky = 0.42 * (0.5 + 0.5 * n.z);
  float c = msContrast > 0.0 ? msContrast : 1.0;
  return 0.5 + (max(direct, sky) - 0.5) * c;
}
// The whole plant as one blob: an ellipsoid (msMacroCentre, msMacroRadii) whose surface normal at a point is the
// plant's own facing there. A clump's light shifts by how that blob faces the sun (up to msMacroWeight either way), so
// clumps on the shaded side of the tree read darker as a whole and those toward the sun lighter, each keeping its own
// banding: the tree reads as one object. Weight 0 (unset) leaves the light as it is.
uniform vec3 msMacroCentre;
uniform vec3 msMacroRadii;
uniform float msMacroWeight;
float msMacro(vec3 world, vec3 sun) {
  if (msMacroWeight <= 0.0) return 0.0;
  vec3 r = max(msMacroRadii, vec3(1e-3));
  vec3 n = normalize((world - msMacroCentre) / (r * r) + vec3(0.0, 0.0, 1e-5));
  // Deep inside the blob, the facing fades out (a clump in the middle is neither side).
  float depth = clamp(length((world - msMacroCentre) / r), 0.0, 1.0);
  return msMacroWeight * dot(n, sun) * smoothstep(0.15, 0.7, depth);
}
// Highlights as one more band, above the top one: a leaf whose light (msLeafLight of its own normal from the leaf map)
// passes the highlight level is painted lime, the edge blended over a pixel like the other bands'. Each leaf has one
// normal, so whole leaves light, where the leaf map tips them nearest the sun. Lighter leaves (the map's tone, its
// diffuse) reach it a little sooner. amount 0 turns the band off.
vec3 msLeafPaint(vec3 band, float light, float tone, float amount) {
  float over = light + 0.04 * (tone - 0.5) - 0.92;
  float w = max(fwidth(over), 1e-4);
  // A soft lime, about halfway from the top band toward a neon one.
  vec3 paint = mix(band, vec3(0.40, 0.64, 0.12), smoothstep(-w, w, over) * step(0.001, amount));
  // And above it a neon band, for only the very brightest leaves, where the light runs past the bands' top.
  float neon = light + 0.04 * (tone - 0.5) - 1.0;
  float wn = max(fwidth(neon), 1e-4);
  return mix(paint, vec3(0.55, 0.95, 0.06), smoothstep(-wn, wn, neon) * step(0.001, amount));
}
// The bands with their edges blended over a pixel (w: the value's change across one, from fwidth), so no stair-steps.
vec3 msLeafBandAA(float t, float w) {
  float x = clamp(t, 0.0, 0.999) * 6.0;
  float edge = fract(x);
  float blend = smoothstep(1.0 - 3.0 * w, 1.0, edge);
  return mix(msLeafBand(t), msLeafBand(t + 1.0 / 6.0), blend * step(x, 5.0));
}
`;

/**
 * A flat-faced mesh prepared for the dome: unindexed, each corner carrying its face's normal, its barycentric
 * coordinate and, for each of the face's three edges (the one opposite each corner), the normal of the face across it
 * (the face's own where it has none).
 */
export function domeGeometry(source: THREE.BufferGeometry): THREE.BufferGeometry {
  const geometry = source.index ? source.toNonIndexed() : source.clone();
  const position = geometry.getAttribute("position");
  const count = position.count;
  const p = (i: number) => new THREE.Vector3().fromBufferAttribute(position, i);
  const key = (v: THREE.Vector3) => `${v.x.toFixed(5)},${v.y.toFixed(5)},${v.z.toFixed(5)}`;
  const normals: THREE.Vector3[] = [];
  const byEdge = new Map<string, number[]>();
  for (let f = 0; f < count / 3; f++) {
    const a = p(3 * f), b = p(3 * f + 1), c = p(3 * f + 2);
    normals.push(new THREE.Vector3().subVectors(b, a).cross(new THREE.Vector3().subVectors(c, a)).normalize());
    for (let e = 0; e < 3; e++) {
      const k1 = key(p(3 * f + e)), k2 = key(p(3 * f + (e + 1) % 3));
      const k = k1 < k2 ? `${k1}|${k2}` : `${k2}|${k1}`;
      byEdge.set(k, [...(byEdge.get(k) ?? []), f]);
    }
  }
  const face = new Float32Array(count * 3), bary = new Float32Array(count * 3);
  const edges = [new Float32Array(count * 3), new Float32Array(count * 3), new Float32Array(count * 3)];
  for (let f = 0; f < count / 3; f++) {
    for (let corner = 0; corner < 3; corner++) {
      const i = 3 * f + corner;
      normals[f].toArray(face, i * 3);
      bary[i * 3 + corner] = 1;
      for (let e = 0; e < 3; e++) {
        // The edge opposite corner e joins corners e+1 and e+2.
        const k1 = key(p(3 * f + (e + 1) % 3)), k2 = key(p(3 * f + (e + 2) % 3));
        const other = (byEdge.get(k1 < k2 ? `${k1}|${k2}` : `${k2}|${k1}`) ?? []).find(g => g !== f);
        (other === undefined ? normals[f] : normals[other]).toArray(edges[e], i * 3);
      }
    }
  }
  geometry.setAttribute("aFace", new THREE.BufferAttribute(face, 3));
  geometry.setAttribute("aBary", new THREE.BufferAttribute(bary, 3));
  edges.forEach((values, e) => geometry.setAttribute(`aEdge${e}`, new THREE.BufferAttribute(values, 3)));
  return geometry;
}

/**
 * A smooth mesh (one with vertex normals, such as a canopy hull) prepared for `leafNormalMaterial`: its own normals are
 * already rounded, so the face and edge normals are the vertex normal and no edge bends it further.
 */
export function smoothGeometry(source: THREE.BufferGeometry): THREE.BufferGeometry {
  const geometry = source.index ? source.toNonIndexed() : source.clone();
  if (!geometry.getAttribute("normal")) geometry.computeVertexNormals();
  const normal = geometry.getAttribute("normal") as THREE.BufferAttribute;
  const count = normal.count;
  const values = new Float32Array(normal.array as ArrayLike<number>);
  geometry.setAttribute("aFace", new THREE.BufferAttribute(values, 3));
  for (let e = 0; e < 3; e++) geometry.setAttribute(`aEdge${e}`, new THREE.BufferAttribute(values, 3));
  geometry.setAttribute("aBary", new THREE.BufferAttribute(new Float32Array(count * 3).fill(1), 3));
  return geometry;
}

/**
 * A surface painted by the leaf normal map, for a mesh from `domeGeometry` or `smoothGeometry`. The face's normal bulges: flat in its
 * middle, curving within `band` (a share of the face) of an edge toward the mean of the two faces there, which both
 * faces reach at the edge (as the contour fuzz uses). The map is laid in the face's plane, `tile` meters a repeat, and
 * varied as the washroom concrete varies its images: irregular cells, each sampled at its own turn and offset (its
 * normals turned with it), so no repeat shows. Banded against `sun`.
 */
export function leafNormalMaterial(sun: THREE.Vector3, tile = 0.6, strength = 1, band = 0.35, screen = false, origin = new THREE.Vector3(), clumpLeaves = 0, offset = 0.1): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    uniforms: {
      normalsMap: { value: leafNormalTexture(clumpLeaves) }, sun: { value: sun.clone().normalize() }, tile: { value: tile },
      strength: { value: strength }, offset: { value: offset }, band: { value: band },
      screen: { value: screen ? 1 : 0 }, origin: { value: origin.clone() }, highlight: { value: 1 },
    },
    vertexShader: /* glsl */`
attribute vec3 aFace;
attribute vec3 aBary;
attribute vec3 aEdge0;
attribute vec3 aEdge1;
attribute vec3 aEdge2;
varying vec3 vWorld;
varying vec3 vFace;
varying vec3 vBary;
varying vec3 vEdge0;
varying vec3 vEdge1;
varying vec3 vEdge2;
void main() {
  vec4 world = modelMatrix * vec4(position, 1.0);
  vWorld = world.xyz;
  mat3 m = mat3(modelMatrix);
  vFace = normalize(m * aFace);
  vEdge0 = normalize(m * aEdge0); vEdge1 = normalize(m * aEdge1); vEdge2 = normalize(m * aEdge2);
  vBary = aBary;
  gl_Position = projectionMatrix * viewMatrix * world;
}`,
    fragmentShader: /* glsl */`
uniform sampler2D normalsMap;
uniform vec3 sun;
uniform float tile;
uniform float strength;
uniform float offset;
uniform float band;
uniform float screen;
uniform vec3 origin;
uniform float highlight;
varying vec3 vWorld;
varying vec3 vFace;
varying vec3 vBary;
varying vec3 vEdge0;
varying vec3 vEdge1;
varying vec3 vEdge2;
${BAND_GLSL}
float h21(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float vnoise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(h21(i), h21(i + vec2(1.0, 0.0)), f.x), mix(h21(i + vec2(0.0, 1.0)), h21(i + 1.0), f.x), f.y);
}
// The bulge: toward each edge, the face's normal bends to the mean of it and the face across, smoothly (a pillow).
vec3 bulge() {
  vec3 n = normalize(vFace);
  vec3 bent = n;
  float w0 = 1.0 - smoothstep(0.0, band, vBary.x), w1 = 1.0 - smoothstep(0.0, band, vBary.y), w2 = 1.0 - smoothstep(0.0, band, vBary.z);
  // Quadratic, so the face stays flat in its middle and turns fastest at its rim.
  w0 *= w0; w1 *= w1; w2 *= w2;
  bent += (normalize(n + normalize(vEdge0)) - n) * w0;
  bent += (normalize(n + normalize(vEdge1)) - n) * w1;
  bent += (normalize(n + normalize(vEdge2)) - n) * w2;
  return normalize(bent);
}
void main() {
  vec3 n = bulge();
  vec3 f = normalize(vFace);
  vec3 t = normalize(abs(f.z) < 0.95 ? cross(vec3(0.0, 0.0, 1.0), f) : cross(vec3(1.0, 0.0, 0.0), f)), b = cross(f, t);
  // The map's plane: the face's own, or (screen) one facing the camera through the plant's origin, so leaves are never
  // foreshortened and stay on the plant as the view turns.
  if (screen > 0.5) {
    vec3 view = normalize(cameraPosition - origin);
    t = normalize(cross(vec3(0.0, 0.0, 1.0), view) + vec3(1e-5, 0.0, 0.0));
    b = cross(view, t);
  }
  vec2 uv = vec2(dot(vWorld - origin, t), dot(vWorld - origin, b)) / tile;
  // Two copies of the map at their own turn, scale and offset, the upper laid over the lower stroke by stroke: a
  // stroke of the upper copy covers what is below wholly or not at all, so no leaf is cut and no repeat shows.
  // The lower copy everywhere; the upper copy's kept strokes over it, blended by their (filtered) coverage, so their
  // edges are smooth. Each copy's normal turns back by its own turn before blending.
  vec2 normalXY = vec2(0.0);
  float tone = 0.27;
  for (int k = 0; k < 2; k++) {
    float a = k == 0 ? 0.3 : 2.1, s = k == 0 ? 1.0 : 0.83;
    vec2 off = k == 0 ? vec2(0.0) : vec2(0.37, 0.61);
    mat2 turn = mat2(cos(a), sin(a), -sin(a), cos(a));
    vec4 c = texture2D(normalsMap, turn * uv * s + off);
    vec2 xy = c.rg * 2.0 - 1.0;
    xy = vec2(cos(a) * xy.x + sin(a) * xy.y, -sin(a) * xy.x + cos(a) * xy.y);
    float cover = k == 0 ? 1.0 : smoothstep(0.62, 0.9, c.a);
    normalXY = mix(normalXY, xy, cover);
    tone = mix(tone, c.b, cover);
  }
  vec3 m = vec3(normalXY, sqrt(max(0.0, 1.0 - dot(normalXY, normalXY))));
  vec4 leaf = vec4(0.0, 0.0, tone, 1.0);
  // The leaf's normal tips the bulging surface normal along the map's own axes.
  vec3 shaded = normalize(n * m.z + (t * m.x + b * m.y) * strength);
  float light = msLeafLight(shaded, sun, offset) + msMacro(vWorld, sun);
  vec3 paint = msLeafBandAA(light, fwidth(light)) * (0.78 + 0.3 * leaf.b);
  gl_FragColor = vec4(msLeafPaint(paint, light, leaf.b, highlight), 1.0);
  #include <colorspace_fragment>
}`,
  });
}
