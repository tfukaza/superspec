/**
 * The spray brush: a tile of whole leaves arranged in sprays, for edges made of leaves a viewer can count. Where the
 * height-field leaf brush (leaf-brush.ts) is thresholded per pixel, and so slices through leaves at whatever level the
 * threshold falls, this tile says for every texel which leaf covers it, where that leaf's centre is and how far the
 * leaf reaches, so an edge pass decides each leaf once, at its centre: a leaf is wholly in or wholly out.
 *
 * A spray is a short curved stem with leaflets in pairs along it and one at its end, each an ovate leaf with a pointed
 * tip angled toward the spray's end, as compound leaves are drawn in painted backgrounds. Sprays cover the tile in
 * every direction, gaps left between leaves. A leaf's reach rises along its spray, so a spray's end leaflets stand out
 * past an edge before its base does; the reaches are equalized over the leaves. Leaves later in the order lie over
 * earlier ones.
 *
 * Texel layout (RGBA, float): r = the leaf's reach in [0, 1]; g, b = the offset from the texel to the leaf's centre, in
 * texels; a = 0 in a gap, else the leaf's tone in (0, 1]. Deterministic from `seed`; pure, so it runs in Node.
 */

export interface SprayOptions {
  /** Texels per side; the tile wraps. */
  size?: number;
  seed?: number;
  /** A leaflet's length in texels, the least and the most. */
  leafLength?: [number, number];
  /** A leaflet's width over its length. */
  leafWidth?: number;
  /** Pairs of leaflets on a spray, the fewest and the most. */
  pairs?: [number, number];
  /** How many times the sprays' leaves would cover the tile. */
  cover?: number;
}

export interface SprayBrush { size: number; data: Float32Array; leaves: number }

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

interface Leaf { x: number; y: number; angle: number; length: number; width: number; reach: number; tone: number; round: number }

/** Half-width of an ovate leaf at `u` from its base (0) to its tip (1): fullest near 40%, a point at the tip. */
function profile(u: number, round: number): number {
  if (u <= 0 || u >= 1) return 0;
  return Math.pow(Math.sin(Math.PI * Math.pow(u, 0.75)), round);
}

export function leafSprays(options: SprayOptions = {}): SprayBrush {
  const { size = 1024, seed = 3, leafLength = [44, 64], leafWidth = 0.46, pairs = [2, 4], cover = 1.35 } = options;
  const random = generator(seed);
  const between = (a: number, b: number) => a + (b - a) * random();
  const leaves: Leaf[] = [];
  const meanLength = (leafLength[0] + leafLength[1]) / 2;
  const leafArea = meanLength * meanLength * leafWidth * 0.6;
  const target = cover * size * size / leafArea;
  while (leaves.length < target) {
    // A spray: a stem from a base point, curving a little, leaflets in pairs along it and one at its end.
    const count = Math.round(between(pairs[0], pairs[1] + 0.999) - 0.5);
    const length = between(leafLength[0], leafLength[1]);
    const step = length * 0.62;
    let x = random() * size, y = random() * size, heading = random() * Math.PI * 2;
    const bend = between(-0.12, 0.12), sprayReach = random(), tone = between(0.55, 1);
    for (let k = 0; k <= count; k++) {
      const along = k / Math.max(count, 1);
      const reach = 0.65 * sprayReach + 0.35 * along;
      const leafSize = length * between(0.88, 1.08) * (1 - 0.18 * along);
      if (k === count) {
        // The end leaflet, along the stem.
        leaves.push({ x, y, angle: heading, length: leafSize * 1.1, width: leafSize * 1.1 * leafWidth, reach: reach + 0.05, tone, round: between(0.55, 0.8) });
        break;
      }
      for (const side of [-1, 1]) {
        // A pair: each leaflet angled toward the spray's end.
        const angle = heading + side * between(0.75, 1.05);
        leaves.push({ x, y, angle, length: leafSize, width: leafSize * leafWidth, reach, tone: tone * between(0.92, 1.05), round: between(0.55, 0.8) });
      }
      heading += bend;
      x += Math.cos(heading) * step;
      y += Math.sin(heading) * step;
    }
  }
  // Reaches equalized over the leaves, so a threshold t keeps a share 1 - t of them.
  const order = leaves.map((_, i) => i).sort((a, b) => leaves[a].reach - leaves[b].reach);
  order.forEach((i, rank) => { leaves[i].reach = rank / Math.max(leaves.length - 1, 1); });
  const data = new Float32Array(size * size * 4);
  for (const leaf of leaves) {
    const cos = Math.cos(leaf.angle), sin = Math.sin(leaf.angle);
    // The leaf's centre, half way along it.
    const cx = leaf.x + cos * leaf.length * 0.5, cy = leaf.y + sin * leaf.length * 0.5;
    const reachBox = leaf.length + leaf.width;
    for (let py = Math.floor(leaf.y - reachBox); py <= Math.ceil(leaf.y + reachBox); py++) {
      for (let px = Math.floor(leaf.x - reachBox); px <= Math.ceil(leaf.x + reachBox); px++) {
        const dx = px + 0.5 - leaf.x, dy = py + 0.5 - leaf.y;
        const u = (dx * cos + dy * sin) / leaf.length;
        if (u <= 0 || u >= 1) continue;
        const v = -dx * sin + dy * cos;
        if (Math.abs(v) >= 0.5 * leaf.width * profile(u, leaf.round)) continue;
        const index = ((((py % size) + size) % size) * size + (((px % size) + size) % size)) * 4;
        data[index] = leaf.reach;
        data[index + 1] = cx - (px + 0.5);
        data[index + 2] = cy - (py + 0.5);
        data[index + 3] = leaf.tone;
      }
    }
  }
  return { size, data, leaves: leaves.length };
}
