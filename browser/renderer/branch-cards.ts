/**
 * Branch cards (prototype): billboards painted as a game tree's leaf cards are, a branchlet seen flat, rather than a
 * round clump. A main stem rises from the card's bottom edge with a gentle curve; side twigs leave it at alternating
 * sides, angled out and curving back toward its direction; leaves run along every stem, alternating and angled toward
 * the tip, all about one size, a terminal leaf at each tip and a few missing. So a card's outline is the shape of a
 * spray (long, uneven, open between twigs), and its base can be set where a branch leaves the foliage.
 *
 * Each leaf has a single normal (its own tilt, as the leaf normal map's leaves have), so bands fall on whole leaves;
 * there is no dome over the card. Stems are drawn first and darker, the leaves over them. The card is then melted
 * (leaf-cards.ts `melted`: blurred and cut again), so leaves that nearly touch run together like paint.
 *
 * Texel layout, as `leafCard`'s (RGBA, float): r, g = the normal's x and y in the card's plane; b = tone in (0, 1];
 * a = 0 where empty, else the leaf's hue shift in (0, 1]. Card space: +x right, +y up, the base at the bottom middle.
 * Deterministic and pure, so it runs in Node.
 */
import { type LeafCard, melted } from "./leaf-cards.ts";

/**
 * An atlas of `grid` × `grid` branch cards (the dense spray, each its own seed), melted. Texel layout (RGBA, bytes):
 * r, g = the leaf's normal x and y in the card's plane (0.5 flat), b = its tone, a = coverage. Row 0 is the card's
 * bottom, where its stem starts.
 */
export function branchCardAtlas(grid = 4, size = 256, seed = 1, melt = 2, options: BranchCardOptions = {}): { width: number; height: number; data: Uint8Array } {
  const width = grid * size, height = grid * size, data = new Uint8Array(width * height * 4);
  for (let cell = 0; cell < grid * grid; cell++) {
    const painted = melted(branchCard({ ...options, size, seed: seed * 97 + cell }), melt);
    const ox = (cell % grid) * size, oy = Math.floor(cell / grid) * size;
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const i = y * size + x, o = ((oy + y) * width + ox + x) * 4;
      if (painted.coverage[i] <= 0) continue;
      data[o] = Math.round((painted.nx[i] * 0.5 + 0.5) * 255);
      data[o + 1] = Math.round((painted.ny[i] * 0.5 + 0.5) * 255);
      data[o + 2] = Math.round(Math.min(1, painted.tone[i]) * 255);
      data[o + 3] = Math.round(painted.coverage[i] * 255);
    }
  }
  return { width, height, data };
}

export interface BranchCardOptions {
  size?: number;
  seed?: number;
  /** A leaf's length as a share of the card's side. */
  leaf?: number;
  /** Between a stem's leaves on one side, as a share of the card's side. */
  step?: number;
  /** Side twigs off the main stem: the fewest, and how many more at random. */
  twigs?: [number, number];
  /** Share of leaves left out (the spray's sparseness). */
  missing?: number;
  /** A leaf's width over its length. */
  width?: number;
  /** How far the stems bend over (positive: their tips turn down toward the card's sides). */
  droop?: number;
  /** How much farther one side's twigs reach than the other's (0 even, 0.35 the open, lopsided sprays). */
  lopsided?: number;
  /** Twig length as a share of the main stem's (the spray's breadth). */
  reach?: [number, number];
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

interface Stem { points: [number, number][]; side: number }

/** A curved stem from (x, y) heading `angle` (radians from +x), `length` long, turning `bend` in all, in `steps`. */
function grow(x: number, y: number, angle: number, length: number, bend: number, steps = 12): [number, number][] {
  const points: [number, number][] = [[x, y]];
  for (let k = 0; k < steps; k++) {
    angle += bend / steps;
    x += Math.cos(angle) * length / steps;
    y += Math.sin(angle) * length / steps;
    points.push([x, y]);
  }
  return points;
}

/** The point and heading at `t` (0 to 1) along a polyline of equal steps. */
function at(points: [number, number][], t: number): { x: number; y: number; heading: number } {
  const span = (points.length - 1) * Math.min(Math.max(t, 0), 1);
  const i = Math.min(Math.floor(span), points.length - 2), f = span - i;
  const [ax, ay] = points[i], [bx, by] = points[i + 1];
  return { x: ax + (bx - ax) * f, y: ay + (by - ay) * f, heading: Math.atan2(by - ay, bx - ax) };
}

export function branchCard(options: BranchCardOptions = {}): LeafCard {
  // The defaults are the dense, bushy spray (many short twigs fanning from a short stem into a full crown).
  const { size = 256, seed = 1, leaf = 0.12, step = 0.05, twigs = [3, 1], missing = 0.05, width = 0.42, droop = 0.25,
    lopsided = 0.35, reach = [0.55, 0.75] } = options;
  const random = generator(seed * 6007 + 29);
  const between = (a: number, b: number) => a + (b - a) * random();
  const data = new Float32Array(size * size * 4);
  const s = size;
  // The main stem: from the bottom middle, up, leaning a little to one side and bending over.
  const lean = between(-0.25, 0.25);
  const mainLength = between(0.72, 0.84) * s;
  const main = grow(0.5 * s, 0.03 * s, Math.PI / 2 + lean, mainLength, -Math.sign(lean || 1) * droop * between(0.6, 1.2));
  const stems: Stem[] = [{ points: main, side: 0 }];
  // Side twigs, alternating sides up the main stem, each shorter higher up, angled out and curving back up.
  const count = twigs[0] + Math.floor(random() * (twigs[1] + 1));
  let side = random() < 0.5 ? -1 : 1;
  // Lopsided: one side's twigs reach farther than the other's, so a card is an open, uneven spray, not a symmetric
  // spike.
  const longSide = random() < 0.5 ? -1 : 1;
  for (let k = 0; k < count; k++) {
    // From low on the stem, so the spray fans out from near its base.
    const t = 0.1 + 0.6 * (k + between(0.2, 0.8)) / count;
    const base = at(main, t);
    const out = side * between(0.95, 1.4);
    const length = mainLength * between(reach[0], reach[1]) * (side === longSide ? 1 + 0.43 * lopsided : 1 - 0.57 * lopsided) * (1 - 0.3 * t);
    // Back toward the main direction, then over with the droop.
    stems.push({ points: grow(base.x, base.y, base.heading + out, length, -out * 0.45 - side * droop * 0.6), side });
    side = -side;
  }
  // Stems first, thin and darker; the leaves lie over them.
  for (const stem of stems) {
    // Thick enough to outlast the melt (a blur of a few texels cut again at a half takes anything thinner).
    const thick = (stem.side === 0 ? 0.017 : 0.013) * s;
    for (let k = 0; k + 1 < stem.points.length; k++) {
      const [ax, ay] = stem.points[k], [bx, by] = stem.points[k + 1];
      const taper = 1 - 0.6 * k / stem.points.length;
      line(data, size, ax, ay, bx, by, thick * taper, 0.35);
    }
  }
  // Leaves: along each stem from a little past its base to its tip, alternating sides, then one at the tip.
  const leafLength = leaf * s, gap = step * s;
  for (const stem of stems) {
    let length = 0;
    for (let k = 1; k < stem.points.length; k++) length += Math.hypot(stem.points[k][0] - stem.points[k - 1][0], stem.points[k][1] - stem.points[k - 1][1]);
    const start = stem.side === 0 ? 0.22 : 0.08;
    let flip = random() < 0.5 ? -1 : 1;
    for (let d = start * length; d < length - 0.4 * gap; d += gap * between(0.85, 1.15)) {
      flip = -flip;
      if (random() < missing) continue;
      const p = at(stem.points, d / length);
      const angle = p.heading + flip * between(0.65, 1.0);
      stamp(data, size, p.x, p.y, angle, leafLength * between(0.9, 1.1), width * between(0.85, 1.15), between(0.6, 1), random);
    }
    const tip = at(stem.points, 1);
    stamp(data, size, tip.x, tip.y, tip.heading + between(-0.2, 0.2), leafLength * between(0.85, 1.0), width, between(0.6, 1), random);
  }
  return { size, data, sprays: stems.length };
}

/** A stem segment: texels within `half` of it, normal flat, a dark tone, no hue shift. */
function line(data: Float32Array, size: number, ax: number, ay: number, bx: number, by: number, half: number, tone: number): void {
  const dx = bx - ax, dy = by - ay, l2 = dx * dx + dy * dy || 1;
  for (let py = Math.max(0, Math.floor(Math.min(ay, by) - half)); py <= Math.min(size - 1, Math.ceil(Math.max(ay, by) + half)); py++) {
    for (let px = Math.max(0, Math.floor(Math.min(ax, bx) - half)); px <= Math.min(size - 1, Math.ceil(Math.max(ax, bx) + half)); px++) {
      const t = Math.min(Math.max(((px + 0.5 - ax) * dx + (py + 0.5 - ay) * dy) / l2, 0), 1);
      if (Math.hypot(px + 0.5 - ax - t * dx, py + 0.5 - ay - t * dy) > half) continue;
      const i = (py * size + px) * 4;
      data[i] = 0; data[i + 1] = 0; data[i + 2] = tone; data[i + 3] = 0.5;
    }
  }
}

/** A pointed oval leaf from its base at (x, y) along `angle`, with one normal: tipped along itself and at random. */
function stamp(data: Float32Array, size: number, x: number, y: number, angle: number, length: number, widthShare: number, tone: number, random: () => number): void {
  const width = length * widthShare, cos = Math.cos(angle), sin = Math.sin(angle);
  // The leaf's tilt: along its own direction (a leaf angled off its stem faces a little that way) and a random lean.
  let nx = 0.45 * cos + (random() - 0.5) * 1.1, ny = 0.45 * sin + (random() - 0.5) * 1.1;
  const flat = Math.hypot(nx, ny);
  if (flat > 0.9) { nx *= 0.9 / flat; ny *= 0.9 / flat; }
  const hue = 0.05 + 0.95 * random();
  const box = length + width;
  for (let py = Math.max(0, Math.floor(y - box)); py <= Math.min(size - 1, Math.ceil(y + box)); py++) {
    for (let px = Math.max(0, Math.floor(x - box)); px <= Math.min(size - 1, Math.ceil(x + box)); px++) {
      const dx = px + 0.5 - x, dy = py + 0.5 - y;
      const u = (dx * cos + dy * sin) / length;
      if (u <= 0 || u >= 1) continue;
      const v = -dx * sin + dy * cos;
      const halfWidth = 0.5 * width * Math.pow(Math.sin(Math.PI * Math.pow(u, 0.8)), 0.6);
      if (Math.abs(v) >= halfWidth) continue;
      // Lighter toward the tip, a stroke lifting; a little darker at the rim, so overlapping leaves part.
      const rim = Math.abs(v) / halfWidth;
      const shade = (0.92 + 0.12 * u) * (rim > 0.8 ? 0.9 : 1);
      const i = (py * size + px) * 4;
      data[i] = nx; data[i + 1] = ny; data[i + 2] = Math.min(1, tone * shade); data[i + 3] = hue;
    }
  }
}
