import * as THREE from "three";

/**
 * Leaf density (prototype): a plant's foliage as a continuous field rather than clumps and sprigs. Every leaf cluster
 * adds a smooth kernel `(1 - d²/r²)²` of radius `r` to a grid, so the value at a point is about how many clusters are
 * within reach of it. Dense cores, thinning shoots and empty air are then levels of one field: a painted surface can
 * be cut at a high level and sparser leaves drawn through the levels below it.
 */
export interface DensityField {
  /** World position of cell (0, 0, 0)'s centre. */
  origin: THREE.Vector3;
  dims: [number, number, number];
  /** Meters per cell. */
  cell: number;
  values: Float32Array;
  /** The highest value: the densest point. */
  max: number;
  /**
   * The value 95% of the leafy cells (those above 2% of `max`) stay under: "dense" for this plant. A lone hot spot (a
   * leader's tip where many shoots meet) would make every other part read sparse against `max`.
   */
  full: number;
}

/** The field of `sites` (`[x, y, z, size]` per cluster), with radius in meters and optional density per site. */
export function densityField(sites: ArrayLike<number>, radius: number, cell = radius / 2, weights?: ArrayLike<number>): DensityField {
  const lo = new THREE.Vector3(Infinity, Infinity, Infinity), hi = new THREE.Vector3(-Infinity, -Infinity, -Infinity);
  for (let i = 0; i < sites.length; i += 4) {
    lo.min(new THREE.Vector3(sites[i], sites[i + 1], sites[i + 2]));
    hi.max(new THREE.Vector3(sites[i], sites[i + 1], sites[i + 2]));
  }
  // A margin of one radius and a cell, so the field falls to zero inside the grid.
  lo.subScalar(radius + cell);
  hi.addScalar(radius + cell);
  const dims = [0, 1, 2].map(a => Math.ceil((hi.getComponent(a) - lo.getComponent(a)) / cell) + 1) as [number, number, number];
  const values = new Float32Array(dims[0] * dims[1] * dims[2]);
  const reach = Math.ceil(radius / cell), r2 = radius * radius;
  for (let i = 0; i < sites.length; i += 4) {
    const c = [sites[i], sites[i + 1], sites[i + 2]];
    const weight = weights?.[i / 4] ?? 1;
    const g = [0, 1, 2].map(a => Math.round((c[a] - lo.getComponent(a)) / cell));
    for (let z = Math.max(0, g[2] - reach); z <= Math.min(dims[2] - 1, g[2] + reach); z++) {
      const dz = lo.z + z * cell - c[2];
      for (let y = Math.max(0, g[1] - reach); y <= Math.min(dims[1] - 1, g[1] + reach); y++) {
        const dy = lo.y + y * cell - c[1];
        for (let x = Math.max(0, g[0] - reach); x <= Math.min(dims[0] - 1, g[0] + reach); x++) {
          const dx = lo.x + x * cell - c[0];
          const s = (dx * dx + dy * dy + dz * dz) / r2;
          if (s < 1) values[x + dims[0] * (y + dims[1] * z)] += weight * (1 - s) * (1 - s);
        }
      }
    }
  }
  let max = 0;
  for (const v of values) max = Math.max(max, v);
  const leafy = [...values].filter(v => v > 0.02 * max).sort((a, b) => a - b);
  const full = leafy.length ? leafy[Math.floor(0.95 * (leafy.length - 1))] : max;
  return { origin: lo, dims, cell, values, max, full };
}

/** The field's value at a world point, trilinear between cells (0 outside the grid). */
export function sampleDensity(field: DensityField, p: THREE.Vector3): number {
  const { origin, dims, cell, values } = field;
  const f = [(p.x - origin.x) / cell, (p.y - origin.y) / cell, (p.z - origin.z) / cell];
  const i = f.map(Math.floor);
  if (i.some((v, a) => v < 0 || v >= dims[a] - 1)) return 0;
  const t = f.map((v, a) => v - i[a]);
  let sum = 0;
  for (let k = 0; k < 8; k++) {
    const o = [k & 1, (k >> 1) & 1, (k >> 2) & 1];
    const w = o.reduce((m, b, a) => m * (b ? t[a] : 1 - t[a]), 1);
    sum += w * values[i[0] + o[0] + dims[0] * (i[1] + o[1] + dims[1] * (i[2] + o[2]))];
  }
  return sum;
}

/** Toward the denser side at `p`: the field's gradient by central differences, unnormalized. */
export function gradient(field: DensityField, p: THREE.Vector3): THREE.Vector3 {
  const h = field.cell * 0.5, g = new THREE.Vector3();
  for (let a = 0; a < 3; a++) {
    const plus = p.clone(), minus = p.clone();
    plus.setComponent(a, p.getComponent(a) + h);
    minus.setComponent(a, p.getComponent(a) - h);
    g.setComponent(a, sampleDensity(field, plus) - sampleDensity(field, minus));
  }
  return g;
}

/**
 * The way out of the foliage at `p` by `field` (its falling gradient), normalized; `fallback` where it is flat. With a
 * field of a wide kernel this is the plant's broad form, not the bumps of single clusters: a painter's light.
 */
export function outward(field: DensityField, p: THREE.Vector3, fallback: THREE.Vector3): THREE.Vector3 {
  const n = gradient(field, p).negate();
  return n.lengthSq() < 1e-12 ? fallback.clone().normalize() : n.normalize();
}

/**
 * Replaces a geometry's normals with `field`'s outward direction at each vertex (shading by the broad form), keeping
 * `mix` of the geometry's own (its clumps' form).
 */
export function shadeNormals(geometry: THREE.BufferGeometry, field: DensityField, mix = 0): THREE.BufferGeometry {
  const position = geometry.getAttribute("position"), normal = geometry.getAttribute("normal");
  const p = new THREE.Vector3(), own = new THREE.Vector3();
  for (let i = 0; i < position.count; i++) {
    p.fromBufferAttribute(position, i);
    own.fromBufferAttribute(normal, i);
    const n = outward(field, p, own).lerp(own.normalize(), mix).normalize();
    normal.setXYZ(i, n.x, n.y, n.z);
  }
  normal.needsUpdate = true;
  return geometry;
}

// A cube's corners are numbered by their offsets (bit 0 x, bit 1 y, bit 2 z); six tetrahedra share its 0-7 diagonal.
const TETRAHEDRA = [[0, 1, 3, 7], [0, 3, 2, 7], [0, 2, 6, 7], [0, 6, 4, 7], [0, 4, 5, 7], [0, 5, 1, 7]];

/**
 * The surface where the field crosses `level` (a share of `full`): the part of the foliage dense enough to paint as
 * one mass. Marching tetrahedra over the grid, vertices shared along the grid's edges, each triangle turned to face
 * away from the denser side, and each vertex's normal the field's falling gradient there, so the surface is smooth
 * however coarse the grid.
 */
export function densitySurface(field: DensityField, level: number): THREE.BufferGeometry {
  const { origin, dims, cell, values } = field;
  const threshold = level * field.full;
  const at = (x: number, y: number, z: number) => x + dims[0] * (y + dims[1] * z);
  const point = (i: number) => new THREE.Vector3(
    origin.x + (i % dims[0]) * cell, origin.y + (Math.floor(i / dims[0]) % dims[1]) * cell, origin.z + Math.floor(i / (dims[0] * dims[1])) * cell);
  const positions: number[] = [], normals: number[] = [], indices: number[] = [];
  const shared = new Map<number, number>();
  const cells = values.length;
  // The vertex where the grid edge between corners i (inside) and o (outside) crosses the level.
  const crossing = (i: number, o: number) => {
    const key = Math.min(i, o) * cells + Math.max(i, o);
    let v = shared.get(key);
    if (v === undefined) {
      const t = (values[i] - threshold) / (values[i] - values[o]);
      const p = point(i).lerp(point(o), t);
      const n = gradient(field, p).negate();
      if (n.lengthSq() < 1e-12) n.set(0, 0, 1);
      n.normalize();
      v = positions.length / 3;
      positions.push(p.x, p.y, p.z);
      normals.push(n.x, n.y, n.z);
      shared.set(key, v);
    }
    return v;
  };
  const triangle = (a: number, b: number, c: number) => {
    if (a === b || b === c || a === c) return;
    const p = [a, b, c].map(k => new THREE.Vector3(positions[3 * k], positions[3 * k + 1], positions[3 * k + 2]));
    const face = new THREE.Vector3().subVectors(p[1], p[0]).cross(new THREE.Vector3().subVectors(p[2], p[0]));
    const out = new THREE.Vector3();
    for (const k of [a, b, c]) out.add(new THREE.Vector3(normals[3 * k], normals[3 * k + 1], normals[3 * k + 2]));
    if (face.dot(out) < 0) indices.push(a, c, b); else indices.push(a, b, c);
  };
  for (let z = 0; z < dims[2] - 1; z++) for (let y = 0; y < dims[1] - 1; y++) for (let x = 0; x < dims[0] - 1; x++) {
    const corner = [0, 1, 2, 3, 4, 5, 6, 7].map(k => at(x + (k & 1), y + ((k >> 1) & 1), z + ((k >> 2) & 1)));
    for (const tet of TETRAHEDRA) {
      const ids = tet.map(k => corner[k]);
      const inside = ids.filter(i => values[i] > threshold), outside = ids.filter(i => values[i] <= threshold);
      if (inside.length === 0 || outside.length === 0) continue;
      if (inside.length === 1) triangle(crossing(inside[0], outside[0]), crossing(inside[0], outside[1]), crossing(inside[0], outside[2]));
      else if (inside.length === 3) triangle(crossing(inside[0], outside[0]), crossing(inside[1], outside[0]), crossing(inside[2], outside[0]));
      else {
        const [a, b] = inside, [c, d] = outside;
        const q = [crossing(a, c), crossing(a, d), crossing(b, d), crossing(b, c)];
        triangle(q[0], q[1], q[2]);
        triangle(q[0], q[2], q[3]);
      }
    }
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute(normals, 3));
  geometry.setIndex(indices);
  return geometry;
}

/**
 * Debug view: a dot per cell whose value is above `floor` of `full`, coloured from deep blue (sparse) through green
 * to yellow (dense) and sized by its value, so cores, gradients and gaps show from any side. `bands` > 0 snaps
 * the colours to that many steps, to read the levels a surface or a billboard tier would be cut at.
 */
export function densityPoints(field: DensityField, { floor = 0.05, size = 6, bands = 0 } = {}): THREE.Points {
  const { origin, dims, cell, values, full } = field;
  const positions: number[] = [], colors: number[] = [], scales: number[] = [];
  const ramp = [new THREE.Color(0x2b1d6e), new THREE.Color(0x1f6f8b), new THREE.Color(0x3fae5a), new THREE.Color(0xf2e14c)];
  for (let z = 0; z < dims[2]; z++) for (let y = 0; y < dims[1]; y++) for (let x = 0; x < dims[0]; x++) {
    let v = Math.min(1, values[x + dims[0] * (y + dims[1] * z)] / full);
    if (v < floor) continue;
    if (bands > 0) v = Math.min(1, Math.ceil(v * bands) / bands);
    positions.push(origin.x + x * cell, origin.y + y * cell, origin.z + z * cell);
    const at = v * (ramp.length - 1), k = Math.min(Math.floor(at), ramp.length - 2);
    const c = ramp[k].clone().lerp(ramp[k + 1], at - k);
    colors.push(c.r, c.g, c.b);
    // Sparse cells small, so the dense core shows through the fringe around it.
    scales.push(0.12 + 0.88 * v * v);
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("color", new THREE.Float32BufferAttribute(colors, 3));
  geometry.setAttribute("scale", new THREE.Float32BufferAttribute(scales, 1));
  const material = new THREE.ShaderMaterial({
    uniforms: { size: { value: size } },
    vertexShader: /* glsl */`
attribute float scale;
attribute vec3 color;
uniform float size;
varying vec3 vColor;
void main() {
  vColor = color;
  vec4 view = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * view;
  gl_PointSize = size * scale * 10.0 / -view.z;
}`,
    fragmentShader: /* glsl */`
varying vec3 vColor;
void main() {
  vec2 d = gl_PointCoord - 0.5;
  if (dot(d, d) > 0.25) discard;
  gl_FragColor = vec4(vColor, 1.0);
}`,
  });
  const points = new THREE.Points(geometry, material);
  points.frustumCulled = false;
  return points;
}
