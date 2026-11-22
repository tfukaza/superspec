import { MathUtils } from "three";

/** Four sprigs, each painted at four brush widths. Row zero of each cell is
 * the foot of the stem in both Three's DataTexture and the exported glTF UVs. */
export const sprigGrid = 4;
const cell = 64;
export const sprigAtlasSize = cell * sprigGrid;
export const sprigWidth = 2.1;
export const sprigLength = 1.28;
export const canopyPaintStrength = 0.92;

type Leaflet = { x: number; y: number; dx: number; dy: number; width: number; value: number };

/** The local frame of each blade. Three or four offset pairs flank the stem,
 * with a little leaf standing at its tip. The inner pairs are longest. */
function sprigBlades(variant: number): Leaflet[] {
  const blades: Leaflet[] = [];
  const pairs = variant % 2 ? 4 : 3;
  const lean = (variant - 1.5) * 0.047;
  for (let pair = 0; pair < pairs; pair++) {
    const t = pair / (pairs - 1);
    for (const side of [-1, 1]) {
      const y = 0.11 + 0.55 * t + 0.045 * Math.sin(2.8 * pair + 3.2 * variant + side);
      const spread = 0.26 + 0.10 * Math.sin(0.8 + 2.4 * t + 0.6 * variant) - 0.075 * t;
      blades.push({
        x: 0.5 + lean * (y - 0.25), y, dx: side * spread,
        dy: 0.14 + 0.10 * Math.sin(0.6 + 2 * t + 0.8 * variant + 0.5 * side),
        width: 0.074 - 0.016 * t, value: 0.84 + 0.13 * (0.5 + 0.5 * Math.sin(1.5 * variant + pair * 2.3 + side * 3.8)),
      });
    }
  }
  blades.push({ x: 0.5 + lean * 0.44, y: 0.69, dx: 0.058 * Math.cos(variant), dy: 0.24, width: 0.052, value: 0.99 });
  return blades;
}

/** Paint colour times coverage and coverage separately. Beyond one leaflet
 * width the little gaps close into a soft brush mass. In the thin exposed
 * tips the only filter is subpixel antialiasing. */
function bakedSprig(variant: number, softness: number): { alpha: Float32Array; pigment: Float32Array } {
  const blades = sprigBlades(variant), lean = (variant - 1.5) * 0.047;
  const alpha = new Float32Array(cell * cell), pigment = new Float32Array(cell * cell);
  for (let y = 0; y < cell; y++) for (let x = 0; x < cell; x++) {
    const u = (x + 0.5) / cell, v = (y + 0.5) / cell;
    let a = v > 0.075 && v < 0.76 ? 1 - MathUtils.smoothstep(Math.abs(u - 0.5 - lean * (v - 0.25)), 0.006, 0.016) : 0;
    let tone = 0.79;
    for (const leaf of blades) {
      const len = Math.hypot(leaf.dx, leaf.dy), dx = u - leaf.x, dy = v - leaf.y;
      const t = (dx * leaf.dx + dy * leaf.dy) / (len * len);
      if (t <= 0 || t >= 1) continue;
      const side = (dx * leaf.dy - dy * leaf.dx) / len;
      const belly = Math.sin(Math.PI * t) * (1.14 - 0.3 * t);
      const w = leaf.width * belly * (1 + softness * 0.33);
      const edge = Math.abs(side) - w;
      const paint = 1 - MathUtils.smoothstep(edge, -0.008, 0.008);
      if (paint > a) {
        a = paint;
        tone = leaf.value * (0.92 + 0.07 * t + 0.035 * MathUtils.smoothstep(side, -0.017, 0.017));
      }
    }
    const i = y * cell + x;
    alpha[i] = a; pigment[i] = a * tone;
  }
  return { alpha, pigment };
}

function gaussian(src: Float32Array, sigma: number, horizontal: boolean): Float32Array {
  const out = new Float32Array(src.length), radius = Math.ceil(3 * sigma);
  const weights = Array.from({ length: 2 * radius + 1 }, (_, i) => Math.exp(-0.5 * ((i - radius) / sigma) ** 2));
  const total = weights.reduce((sum, w) => sum + w, 0);
  for (let y = 0; y < cell; y++) for (let x = 0; x < cell; x++) {
    let value = 0;
    for (let i = -radius; i <= radius; i++) {
      const u = horizontal ? x + i : x, v = horizontal ? y : y + i;
      if (u >= 0 && u < cell && v >= 0 && v < cell) value += src[v * cell + u] * weights[i + radius];
    }
    out[y * cell + x] = value / total;
  }
  return out;
}

let sharedAtlas: Uint8Array | undefined;
export function sprigAtlasRgba(): Uint8Array {
  if (sharedAtlas) return sharedAtlas;
  const bytes = new Uint8Array(4 * sprigAtlasSize ** 2);
  const widths = [0.42, 1.1, 2.5, 4.9];
  for (let variant = 0; variant < sprigGrid; variant++) for (let brush = 0; brush < sprigGrid; brush++) {
    const s = brush / (sprigGrid - 1), ink = bakedSprig(variant, s);
    const blur = (values: Float32Array) => gaussian(gaussian(values, widths[brush], true), widths[brush] * 0.8, false);
    const alpha = blur(ink.alpha), pigment = blur(ink.pigment);
    for (let y = 0; y < cell; y++) for (let x = 0; x < cell; x++) {
      const i = y * cell + x, j = ((variant * cell + y) * sprigAtlasSize + brush * cell + x) * 4;
      // Keep two empty texels around each cell and ease the outer brush tail
      // into them. A clipped Gaussian otherwise leaves straight card ends and
      // carries its neighbours into the silhouette under bilinear filtering.
      const inset = Math.min(x, y, cell - 1 - x, cell - 1 - y);
      const gutter = MathUtils.smoothstep(inset, 1, 6);
      const tone = MathUtils.lerp(0.91, alpha[i] > 1e-4 ? pigment[i] / alpha[i] : 0.91, gutter);
      // Keep the same neutral leaf pigment through transparent gutters.
      bytes[j] = bytes[j + 1] = bytes[j + 2] = Math.round(255 * (1.055 * tone ** (1 / 2.4) - 0.055));
      bytes[j + 3] = Math.round(255 * gutter * Math.min(1, alpha[i] * (1 + 0.42 * s)));
    }
  }
  sharedAtlas = bytes;
  return bytes;
}

/** Low-pass the light-facing normals and pigments of nearby leaf cards. Each bin
 * contains the unfiltered samples. Shading stays responsive to the scene's sun.
 * The brush spans a few leaf lengths, independent of camera distance. */
export function blurCanopyCards(origins: number[], shades: number[], tones: number[], length: number): number[] {
  const binSize = 1.6 * length;
  type Bin = { x: number; y: number; z: number; count: number; nx: number; ny: number; nz: number; tone: number; shade: number };
  const bins = new Map<string, Bin>();
  const count = tones.length / 3;
  const mask = new Array<number>(count).fill(0);
  const coordinate = (i: number, axis: number) => (origins[4 * i + axis] - origins[axis]) / binSize;
  for (let i = 0; i < count; i++) {
    const j = 4 * i, x = coordinate(i, 0), y = coordinate(i, 1), z = coordinate(i, 2);
    const key = `${Math.floor(x)},${Math.floor(y)},${Math.floor(z)}`;
    let bin = bins.get(key);
    if (!bin) {
      bin = { x: 0, y: 0, z: 0, count: 0, nx: 0, ny: 0, nz: 0, tone: 0, shade: 0 };
      bins.set(key, bin);
    }
    bin.count++; bin.x += x; bin.y += y; bin.z += z;
    bin.nx += shades[j]; bin.ny += shades[j + 1]; bin.nz += shades[j + 2];
    bin.tone += tones[3 * i]; bin.shade += shades[j + 3];
  }
  for (const bin of bins.values()) {
    bin.x /= bin.count; bin.y /= bin.count; bin.z /= bin.count;
  }
  // Leaves in a whorl share their station's foot, apart from a tiny offset.
  // Cache a brush sample per quarter bin to amortize the Gaussian lookup.
  const samples = new Map<string, { nx: number; ny: number; nz: number; tone: number; shade: number; density: number }>();
  for (let i = 0; i < count; i++) {
    const j = i * 4, k = i * 3;
    const buried = MathUtils.smoothstep(shades[j + 3], 0.48, 0.88);
    if (!buried || tones[k + 2] > 0) continue;
    const x = Math.round(4 * coordinate(i, 0)) / 4, y = Math.round(4 * coordinate(i, 1)) / 4;
    const z = Math.round(4 * coordinate(i, 2)) / 4;
    const key = `${x},${y},${z}`;
    let sample = samples.get(key);
    if (!sample) {
      let density = 0, nx = 0, ny = 0, nz = 0, tone = 0, shade = 0;
      for (let a = Math.floor(x) - 1; a <= Math.floor(x) + 1; a++) {
        for (let b = Math.floor(y) - 1; b <= Math.floor(y) + 1; b++) {
          for (let c = Math.floor(z) - 1; c <= Math.floor(z) + 1; c++) {
            const bin = bins.get(`${a},${b},${c}`);
            if (!bin) continue;
            const d2 = (bin.x - x) ** 2 + (bin.y - y) ** 2 + (bin.z - z) ** 2;
            const w = Math.exp(-1.15 * d2);
            density += w * bin.count;
            nx += w * bin.nx; ny += w * bin.ny; nz += w * bin.nz;
            tone += w * bin.tone; shade += w * bin.shade;
          }
        }
      }
      const magnitude = Math.hypot(nx, ny, nz);
      if (magnitude < 1e-6) continue;
      sample = { nx: nx / magnitude, ny: ny / magnitude, nz: nz / magnitude, tone: tone / density, shade: shade / density, density };
      samples.set(key, sample);
    }
    // An isolated whorl has up to ten leaves. Only overlapping whorls can
    // develop a broad stroke, with the open tips and crown fringe left crisp.
    const amount = canopyPaintStrength * buried * MathUtils.smoothstep(sample.density, 11, 29);
    const nx = MathUtils.lerp(shades[j], sample.nx, amount), ny = MathUtils.lerp(shades[j + 1], sample.ny, amount);
    const nz = MathUtils.lerp(shades[j + 2], sample.nz, amount), magnitude = Math.hypot(nx, ny, nz);
    if (!amount || magnitude < 1e-6) continue;
    shades[j] = nx / magnitude; shades[j + 1] = ny / magnitude; shades[j + 2] = nz / magnitude;
    shades[j + 3] = MathUtils.lerp(shades[j + 3], sample.shade, amount);
    tones[k] = MathUtils.lerp(tones[k], sample.tone, amount);
    mask[i] = amount;
  }
  return mask;
}
