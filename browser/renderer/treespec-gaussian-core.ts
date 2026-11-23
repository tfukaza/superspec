import { Vector3 } from "three";
import { sampleDensity, type DensityField } from "./leaf-density.ts";

interface CoreSite {
  center: [number, number, number];
  /** Outer radius of the compact dab, in meters. */
  radius: number;
}

const perimeter = [
  new Vector3(1, 0, 0), new Vector3(-1, 0, 0),
  new Vector3(0, 1, 0), new Vector3(0, -1, 0),
  new Vector3(0, 0, 1), new Vector3(0, 0, -1),
  ...[-1, 1].flatMap(x => [-1, 1].flatMap(y => [-1, 1].map(z => new Vector3(x, y, z).normalize()))),
];

/** Select sparse interior dabs with occupied foliage around their compact footprints. */
export function treeSpecGaussianCoreSites(field: DensityField): CoreSite[] {
  if (field.full <= 0) return [];
  const { values, origin, cell, dims: [width, height, depth] } = field;
  const plane = width * height;
  // A single leaf kernel has a peak of one, even on a very sparse plant.
  const dense = Math.max(2, 0.58 * field.full);
  const occupied = 0.13 * field.full;
  const large = Math.min(0.34, 2.1 * cell);
  const small = Math.min(large, Math.max(0.16, 1.35 * cell));
  const candidates: number[] = [];
  for (let i = 0; i < values.length; i++) {
    if (values[i] >= dense) candidates.push(i);
  }
  candidates.sort((a, b) => values[b] - values[a] || a - b);

  const used = new Uint8Array(values.length);
  const center = new Vector3(), point = new Vector3();
  const sites: CoreSite[] = [];
  for (const index of candidates) {
    if (sites.length === 320) break;
    if (used[index]) continue;
    const x = index % width, y = Math.floor(index / width) % height, z = Math.floor(index / plane);
    center.set(origin.x + x * cell, origin.y + y * cell, origin.z + z * cell);
    const supported = (radius: number) => perimeter.every(direction =>
      sampleDensity(field, point.copy(center).addScaledVector(direction, radius)) >= occupied);
    const radius = supported(large) ? large : small < large && supported(small) ? small : 0;
    if (radius === 0) continue;
    sites.push({ center: center.toArray(), radius });
    const spacing = Math.max(1, Math.round(1.1 * radius / cell));
    for (let dz = Math.max(-spacing, -z); dz <= Math.min(spacing, depth - 1 - z); dz++) {
      for (let dy = Math.max(-spacing, -y); dy <= Math.min(spacing, height - 1 - y); dy++) {
        for (let dx = Math.max(-spacing, -x); dx <= Math.min(spacing, width - 1 - x); dx++) {
          if (dx * dx + dy * dy + dz * dz <= spacing * spacing) used[index + dx + width * dy + plane * dz] = 1;
        }
      }
    }
  }
  return sites;
}
