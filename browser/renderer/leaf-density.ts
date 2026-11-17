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

/** The field of `sites` (`[x, y, z, size]` per leaf cluster, in the scene) with kernels of `radius` meters. */
export function densityField(sites: ArrayLike<number>, radius: number, cell = radius / 2): DensityField {
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
    const g = [0, 1, 2].map(a => Math.round((c[a] - lo.getComponent(a)) / cell));
    for (let z = Math.max(0, g[2] - reach); z <= Math.min(dims[2] - 1, g[2] + reach); z++) {
      const dz = lo.z + z * cell - c[2];
      for (let y = Math.max(0, g[1] - reach); y <= Math.min(dims[1] - 1, g[1] + reach); y++) {
        const dy = lo.y + y * cell - c[1];
        for (let x = Math.max(0, g[0] - reach); x <= Math.min(dims[0] - 1, g[0] + reach); x++) {
          const dx = lo.x + x * cell - c[0];
          const s = (dx * dx + dy * dy + dz * dz) / r2;
          if (s < 1) values[x + dims[0] * (y + dims[1] * z)] += (1 - s) * (1 - s);
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
