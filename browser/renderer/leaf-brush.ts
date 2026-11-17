/**
 * The leaf brush: a tileable height field painted with leaves, the noise a canopy's light and shade edge follows instead
 * of fractal noise. A smooth "mass" (large lobes with smaller clumps on them) says how high each place stands; the
 * tile is then covered several times over with leaves, each stamped at the mass where its base lies and pointing
 * downhill, away from the clump it belongs to. A leaf keeps its base's height along its whole length, so it stands
 * proud of the lower ground its blade lies over: every level set of the field is a lobe outlined by leaf tips, as a
 * painter's light patch on a canopy is. Overlapping stamps keep the highest value (the leaf in front), so outlines stay
 * crisp rather than averaged. The field is equalized, so a threshold t covers a share 1 - t of the tile.
 *
 * Deterministic from `seed`; values in [0, 1]. Pure (no three.js), so it runs in Node.
 */

export interface LeafBrushOptions {
  /** Texels per side; the field tiles. */
  size?: number;
  seed?: number;
  /** Large lobes per side of the tile (count = lobes²). */
  lobes?: number;
  /** Clumps per side of the tile. */
  clumps?: number;
  /** The clumps' share of the mass; the lobes have the rest. */
  clumpShare?: number;
  /** Leaf length in texels, the least and the most. */
  leafLength?: [number, number];
  /** A leaf's width over its length. */
  leafWidth?: number;
  /** How many times the leaves cover the tile. */
  cover?: number;
  /** Radians a leaf turns away from straight downhill, at most. */
  spread?: number;
  /** A leaf's rise from its edge to its midrib, in the mass's units. */
  dome?: number;
  /** Random height added to a leaf, at most, so neighbours on one contour are not all equal. */
  jitter?: number;
  /** How far a stroke's outline wanders from its shape, as a share of its width. */
  wobble?: number;
  /** How much dry-brush streaking breaks a stroke's edges and tip (0: clean cut-out leaves). */
  bristle?: number;
  /** How deep the sharp notches cut into a stroke's sides, as a share of its width (0: none). */
  jag?: number;
}

export interface LeafBrush { size: number; data: Float32Array }

/** Mulberry32: a small seeded generator in [0, 1). */
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
 * One leaf, painted as one stroke from base to tip: its base, direction, length and width in texels, a bend and its
 * height; `left` and `right` scale the two sides (a stroke is fuller on one), `tip` shapes the end (low: blunt, high: a
 * thin flick), `key` seeds its outline's wobble and its bristle streaks.
 */
interface Leaf { x: number; y: number; angle: number; length: number; width: number; bend: number; height: number; left: number; right: number; tip: number; key: number }

/** The stroke's look, shared by every leaf. */
interface Stroke { dome: number; wobble: number; bristle: number; jag: number }

/** Value noise in [0, 1] on a lattice keyed by `key`. */
function hash(x: number, y: number, key: number): number {
  let h = Math.imul(x, 0x27d4eb2d) ^ Math.imul(y, 0x165667b1) ^ Math.imul(key, 0x9e3779b1);
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}
function noise(x: number, y: number, key: number): number {
  const ix = Math.floor(x), iy = Math.floor(y), fx = x - ix, fy = y - iy;
  const sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy);
  const a = hash(ix, iy, key), b = hash(ix + 1, iy, key), c = hash(ix, iy + 1, key), d = hash(ix + 1, iy + 1, key);
  return a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy;
}

/** The half-width of a stroke over its length (u from base 0 to tip 1): a soft start, widest about 40% along. */
function halfWidth(u: number, tip: number): number {
  if (u <= 0 || u >= 1) return 0;
  return Math.pow(Math.sin(Math.PI * Math.pow(u, 0.7)), tip);
}

/** Texels the skirt reaches outside a stroke, and how far its value falls per texel (in the mass's units). */
const SKIRT = 1.5, SKIRT_DROP = 0.08;

function stamp(field: Float32Array, size: number, leaf: Leaf, stroke: Stroke): void {
  const cos = Math.cos(leaf.angle), sin = Math.sin(leaf.angle);
  // The leaf's box: from its base along its direction, as wide as it is.
  const tipX = leaf.x + cos * leaf.length, tipY = leaf.y + sin * leaf.length, pad = leaf.width + 2;
  const x0 = Math.floor(Math.min(leaf.x, tipX) - pad), x1 = Math.ceil(Math.max(leaf.x, tipX) + pad);
  const y0 = Math.floor(Math.min(leaf.y, tipY) - pad), y1 = Math.ceil(Math.max(leaf.y, tipY) + pad);
  for (let y = y0; y <= y1; y++) {
    const row = (((y % size) + size) % size) * size;
    for (let x = x0; x <= x1; x++) {
      const dx = x + 0.5 - leaf.x, dy = y + 0.5 - leaf.y;
      const along = dx * cos + dy * sin;
      if (along <= -2 || along >= leaf.length + 2) continue;
      const u = along / leaf.length;
      // A bent leaf: its midrib curves sideways toward the tip.
      const v = -dx * sin + dy * cos - leaf.bend * leaf.length * u * u;
      const side = v < 0;
      // The outline wobbles slowly along the stroke, each side on its own, as a hand-painted edge does.
      // Jags: sharp V-notches where a finer noise crosses its middle, cut into the side.
      const notch = 1 - Math.abs(2 * noise(u * 9, side ? 3.1 : 11.7, leaf.key + 2) - 1);
      const wobble = (1 + stroke.wobble * 2 * (noise(u * 3.5, side ? 0 : 7.3, leaf.key) - 0.5)) * (1 - stroke.jag * notch ** 3);
      const half = Math.max(leaf.width * 0.5 * halfWidth(Math.min(Math.max(u, 0), 1), leaf.tip) * (side ? leaf.left : leaf.right) * wobble, 1e-3);
      const t = Math.min(Math.abs(v) / half, 1);
      // Dry brush: the stroke runs out of paint at a different point in each lane along it, the outer lanes sooner,
      // which frays its tip and the end of its sides into streaks.
      const lane = noise((v / leaf.width) * 7, 0.5, leaf.key + 1);
      const end = 1 - stroke.bristle * (0.4 * lane + 0.3 * t * t);
      // Texels outside the stroke: across its side, or past where its lane ran out.
      const outside = Math.max(Math.abs(v) - half, (u - end) * leaf.length);
      // A short skirt outside the stroke falls steeply from its height, so a threshold finds the outline between
      // texels (bilinear sampling then draws it smooth) without leaving the smears a blended, wide edge would.
      if (outside >= SKIRT) continue;
      const rise = 1 - t * t;
      const value = outside > 0 ? leaf.height - SKIRT_DROP * outside : leaf.height + stroke.dome * rise;
      const index = row + (((x % size) + size) % size);
      if (value > field[index]) field[index] = value;
    }
  }
}

/** Smooth bumps on a tiling plane: centres, radii and heights, summed by `at`. */
function bumps(count: number, size: number, random: () => number, radius: [number, number], height: [number, number]) {
  const cell = size / count, list: { x: number; y: number; r: number; h: number }[] = [];
  for (let gy = 0; gy < count; gy++) for (let gx = 0; gx < count; gx++) {
    list.push({ x: (gx + 0.1 + 0.8 * random()) * cell, y: (gy + 0.1 + 0.8 * random()) * cell,
      r: cell * (radius[0] + (radius[1] - radius[0]) * random()), h: height[0] + (height[1] - height[0]) * random() });
  }
  return list;
}

export function leafBrush(options: LeafBrushOptions = {}): LeafBrush {
  const {
    size = 512, seed = 1, lobes = 3, clumps = 8, clumpShare = 0.45, leafLength = [12, 30], leafWidth = 0.6,
    cover = 3.5, spread = 1.1, dome = 0.025, jitter = 0.06, wobble = 0.35, bristle = 0.9, jag = 0.6,
  } = options;
  const random = generator(seed);
  const between = (a: number, b: number) => a + (b - a) * random();
  const large = bumps(lobes, size, random, [0.7, 1.1], [0.5, 1]);
  const small = bumps(clumps, size, random, [0.6, 0.95], [0.4, 1]);
  const wrap = (d: number) => d - size * Math.round(d / size);
  const sum = (list: ReturnType<typeof bumps>, x: number, y: number) => {
    let total = 0;
    for (const b of list) {
      const dx = wrap(x - b.x), dy = wrap(y - b.y), q = (dx * dx + dy * dy) / (b.r * b.r);
      if (q < 1) total += b.h * (1 - q) * (1 - q);
    }
    return total;
  };
  // The mass on a coarse grid (bilinear between), so leaves read it and its slope cheaply.
  const step = 4, grid = size / step, mass = new Float32Array(grid * grid);
  let low = Infinity, high = -Infinity;
  for (let y = 0; y < grid; y++) for (let x = 0; x < grid; x++) {
    const m = (1 - clumpShare) * sum(large, x * step, y * step) + clumpShare * sum(small, x * step, y * step);
    mass[y * grid + x] = m; low = Math.min(low, m); high = Math.max(high, m);
  }
  for (let i = 0; i < mass.length; i++) mass[i] = (mass[i] - low) / (high - low || 1);
  const sample = (x: number, y: number) => {
    const gx = x / step, gy = y / step, ix = Math.floor(gx), iy = Math.floor(gy), fx = gx - ix, fy = gy - iy;
    const at = (i: number, j: number) => mass[(((j % grid) + grid) % grid) * grid + (((i % grid) + grid) % grid)];
    return at(ix, iy) * (1 - fx) * (1 - fy) + at(ix + 1, iy) * fx * (1 - fy) + at(ix, iy + 1) * (1 - fx) * fy + at(ix + 1, iy + 1) * fx * fy;
  };
  // Leaves until they cover the tile `cover` times, lowest first so a higher leaf lies over a lower one.
  const meanLength = (leafLength[0] + leafLength[1]) / 2;
  const area = meanLength * meanLength * leafWidth * 0.62;
  const count = Math.round(cover * size * size / area);
  const leaves: Leaf[] = [];
  for (let i = 0; i < count; i++) {
    const x = random() * size, y = random() * size, m = sample(x, y);
    const gx = sample(x + 2, y) - sample(x - 2, y), gy = sample(x, y + 2) - sample(x, y - 2);
    // Downhill, away from the clump's top; where the mass is flat, any way.
    const downhill = gx * gx + gy * gy > 1e-10 ? Math.atan2(-gy, -gx) : between(0, Math.PI * 2);
    const length = between(leafLength[0], leafLength[1]), angle = downhill + between(-spread, spread);
    // The leaf's base lies a third of its length uphill of the point it takes its height from, so the leaves of a
    // clump's top overlap there instead of leaving a star-shaped hole where they all part.
    leaves.push({
      x: x - Math.cos(angle) * length * 0.35, y: y - Math.sin(angle) * length * 0.35, angle, length, width: length * leafWidth * between(0.85, 1.15),
      bend: between(-0.3, 0.3), height: m + between(0, jitter),
      // A stroke is fuller on one side than the other, and ends blunt or in a thin flick.
      left: between(0.7, 1.15), right: between(0.7, 1.15), tip: between(0.35, 1.0), key: i,
    });
  }
  leaves.sort((a, b) => a.height - b.height);
  // Where no leaf falls, the ground: the mass just below the leaves there, so a gap is no hole through a lit lobe.
  const field = new Float32Array(size * size);
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) field[y * size + x] = sample(x + 0.5, y + 0.5) - jitter;
  const stroke = { dome, wobble, bristle, jag };
  for (const leaf of leaves) stamp(field, size, leaf, stroke);
  // Equalize: each value becomes its rank, so a threshold t leaves a share 1 - t of the tile above it.
  const order = new Uint32Array(field.length).map((_, i) => i).sort((a, b) => field[a] - field[b]);
  const equalized = new Float32Array(field.length);
  for (let rank = 0; rank < order.length; rank++) equalized[order[rank]] = rank / (order.length - 1);
  return { size, data: equalized };
}
