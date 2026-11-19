/**
 * Leaf cards: pre-painted billboards of leaf clumps, the way stylised games draw foliage, generated rather than drawn.
 * A card is a clump's silhouette built from whole leaf sprays: sprays start inside the card's shape and point away
 * from its middle, so its edge is made of spray tips and its inside of overlapping leaves. Every texel carries the
 * card's coverage, the leaf's tone, and a normal: a dome over the card (out of the clump at its middle, sideways at
 * its rim) tipped toward the leaf's own spray, so a shade band computed per texel against a clump's normal falls on
 * whole leaves rather than across them.
 *
 * Shapes: `round` (a clump seen whole), `lopsided` (fuller on one side), `long` (a sprig, along +Y). Texel layout
 * (RGBA, float): r, g = the normal's x and y in the card's plane (z = sqrt(1 - x² - y²), out of the card); b = the
 * leaf's tone in (0, 1], its rim darker; a = 0 where no leaf, else the leaf's hue shift in (0, 1]. Deterministic;
 * pure, so it runs in Node.
 */

export type CardShape = "round" | "lopsided" | "long";

export interface CardOptions {
  size?: number;
  seed?: number;
  shape?: CardShape;
  /** A leaflet's length as a share of the card's side. */
  leaf?: number;
  /** How many sprays, per unit of the shape's area in leaf lengths squared. */
  density?: number;
  /** A leaf's fullness: lower is rounder (the exponent of its profile). */
  round?: number;
  /** A leaf's width over its length. */
  width?: number;
  /** The fewest offshoots (short chains of leaves past the edge); up to two more at random. */
  offshoots?: number;
}

export interface LeafCard { size: number; data: Float32Array; sprays: number }

/**
 * An atlas of 2 × `grid` × `grid` cards (dense, then sparse) for the billboards drawn on a plant's clumps (the painterly study's dabs, with
 * leaves in them): coarse cards of big rounded leaves in lobes, round and lopsided by turns, each its own seed. Texel
 * layout (RGBA, bytes): r = the leaf's tone (its rim darker), g = the leaf's hue shift (0.5 none), a = coverage.
 */
export function leafCardAtlas(grid = 4, size = 256, seed = 1, melt = 3): { width: number; height: number; data: Uint8Array; normals: Uint8Array } {
  // Two sets of grid × grid cards, one above the other: dense (rows 0 to grid - 1) and sparse, for a plant's rim.
  const width = grid * size, height = 2 * grid * size, data = new Uint8Array(width * height * 4), normals = new Uint8Array(width * height * 4);
  for (let cell = 0; cell < 2 * grid * grid; cell++) {
    const sparse = cell >= grid * grid;
    const card = leafCard({ size, seed: seed * 101 + cell, shape: cell % 2 ? "lopsided" : "round", leaf: sparse ? 0.12 : 0.13,
      density: sparse ? 0.55 : 1.6, round: 0.5, width: 0.6, offshoots: sparse ? 4 : 1 });
    const painted = melted(card, melt);
    const ox = (cell % grid) * size, oy = Math.floor(cell / grid) * size;
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const i = y * size + x, o = ((oy + y) * width + ox + x) * 4;
      if (painted.coverage[i] <= 0) continue;
      data[o] = Math.round(painted.tone[i] * 255);
      data[o + 1] = Math.round(painted.hue[i] * 255);
      data[o + 3] = Math.round(painted.coverage[i] * 255);
      normals[o] = Math.round((painted.nx[i] * 0.5 + 0.5) * 255);
      normals[o + 1] = Math.round((painted.ny[i] * 0.5 + 0.5) * 255);
      normals[o + 3] = 255;
    }
  }
  return { width, height, data, normals };
}

/**
 * A card as paint: its leaf mask blurred (three box passes of `radius` texels, about a Gaussian) and cut again at a
 * half, so leaves that nearly touch run together like wet paint drawn by its surface tension and sharp slivers round
 * off. Coverage keeps a texel-wide ramp at the cut for filtering. Tone and normals are the blur's coverage-weighted
 * averages; each texel keeps its own leaf's hue, and a texel the paint newly covers takes its nearest leaf's.
 */
export function melted(card: LeafCard, radius: number): { coverage: Float32Array; tone: Float32Array; hue: Float32Array; nx: Float32Array; ny: Float32Array } {
  const { size, data } = card, n = size * size;
  const cover = new Float32Array(n), tone = new Float32Array(n), nx = new Float32Array(n), ny = new Float32Array(n), hue = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const on = data[i * 4 + 3] > 0 ? 1 : 0;
    cover[i] = on; tone[i] = data[i * 4 + 2] * on; nx[i] = data[i * 4] * on; ny[i] = data[i * 4 + 1] * on; hue[i] = data[i * 4 + 3];
  }
  const blur = (field: Float32Array) => {
    let a: Float32Array = field, b: Float32Array = new Float32Array(n);
    for (let pass = 0; pass < 3; pass++) for (const horizontal of [true, false]) {
      for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
        let sum = 0, count = 0;
        for (let k = -radius; k <= radius; k++) {
          const xx = horizontal ? x + k : x, yy = horizontal ? y : y + k;
          if (xx < 0 || yy < 0 || xx >= size || yy >= size) { count++; continue; }
          sum += a[yy * size + xx]; count++;
        }
        b[y * size + x] = sum / count;
      }
      [a, b] = [b, a];
    }
    return a;
  };
  const softCover = blur(cover), softTone = blur(tone), softX = blur(nx), softY = blur(ny);
  const coverage = new Float32Array(n), outTone = new Float32Array(n), outX = new Float32Array(n), outY = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    coverage[i] = Math.min(Math.max((softCover[i] - 0.5) * 6 + 0.5, 0), 1);
    const w = Math.max(softCover[i], 1e-6);
    // A leaf keeps its own tone (its rim and tip) inside it; only texels the paint newly covers take the blur's.
    outTone[i] = cover[i] > 0 ? tone[i] : Math.min(1, softTone[i] / w);
    outX[i] = softX[i] / w; outY[i] = softY[i] / w;
  }
  // Hue: a newly painted texel takes its nearest leaf's, by growing the leaves' hues outward a texel at a time.
  for (let step = 0; step < radius * 3; step++) {
    const next = hue.slice();
    let grew = false;
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const i = y * size + x;
      if (hue[i] > 0 || coverage[i] <= 0) continue;
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const xx = x + dx, yy = y + dy;
        if (xx >= 0 && yy >= 0 && xx < size && yy < size && hue[yy * size + xx] > 0) { next[i] = hue[yy * size + xx]; grew = true; break; }
      }
    }
    hue.set(next);
    if (!grew) break;
  }
  for (let i = 0; i < n; i++) if (coverage[i] > 0 && hue[i] === 0) hue[i] = 0.5;
  return { coverage, tone: outTone, hue, nx: outX, ny: outY };
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

/** The shape's extent from the card's middle, in card halves, toward the direction `angle` (radians; +Y up). */
function extent(shape: CardShape, angle: number, lean: number): number {
  const x = Math.cos(angle), y = Math.sin(angle);
  if (shape === "long") return 1 / Math.sqrt((x / 0.38) ** 2 + (y / 0.92) ** 2);
  if (shape === "lopsided") return 0.8 * (1 + 0.18 * Math.cos(angle - lean)) / 1.18;
  return 0.8;
}

export function leafCard(options: CardOptions = {}): LeafCard {
  const { size = 256, seed = 1, shape = "round", leaf = 0.075, density = 1.6, round = 0.7, width = 0.46, offshoots: fewest = 1 } = options;
  const random = generator(seed * 7919 + 13);
  const between = (a: number, b: number) => a + (b - a) * random();
  const half = size / 2;
  const leafLength = leaf * size;
  const lean = random() * Math.PI * 2;
  // Area of the shape in leaf lengths squared, by sampling its extent.
  let area = 0;
  for (let k = 0; k < 64; k++) area += 0.5 * (extent(shape, (k / 64) * Math.PI * 2, lean) * half) ** 2 * (Math.PI * 2 / 64);
  const sprays = Math.round(density * area / (leafLength * leafLength));
  const data = new Float32Array(size * size * 4);
  // Sprays from the middle outward, the outer ones last so they lie over the inner.
  const list: { x: number; y: number; heading: number; reach: number }[] = [];
  for (let s = 0; s < sprays; s++) {
    const angle = random() * Math.PI * 2;
    const reach = Math.sqrt(random());
    list.push({ x: 0, y: 0, heading: angle, reach });
  }
  list.sort((a, b) => a.reach - b.reach);
  // Lobes: a round card is a few overlapping masses, not one ball. Each spray grows from one of them.
  const lobes = shape === "long" ? [{ x: 0, y: 0, r: 1 }] : Array.from({ length: 3 + Math.floor(random() * 3) }, (_, k) => {
    // Kept inside the card with room for the leaves that stand past a lobe's edge.
    const a = random() * Math.PI * 2, d = k === 0 ? 0 : between(0.2, 0.32);
    return { x: Math.cos(a) * d * half, y: Math.sin(a) * d * half, r: k === 0 ? 0.85 : between(0.5, 0.65) };
  });
  // Single leaves, as a leaf brush stamps them: each at a random place in a lobe, rotated anywhere (leaning outward
  // near the edge), its own size and fullness. Leaves far out are stamped last, so the edge lies over the middle.
  for (const spray of list) {
    for (let n = 0; n < 3; n++) {
      const lobe = lobes[Math.floor(random() * lobes.length)];
      const direction = random() * Math.PI * 2, reach = Math.sqrt(random()) * (0.6 + 0.4 * spray.reach);
      const limit = extent(shape, direction, lean) * half * lobe.r;
      const leafSize = leafLength * between(0.6, 1.25);
      // The leaf's base sits inside the lobe; at the rim it points outward so its tip makes the edge.
      const distance = reach * limit - leafSize * 0.5 * reach;
      const x = half + lobe.x + Math.cos(direction) * distance, y = half + lobe.y + Math.sin(direction) * distance;
      const outward = reach * reach;
      // A rim leaf now and then stands farther out, so the outline is ragged rather than a smooth lobe.
      const push = reach > 0.8 && random() < 0.25 ? between(0.15, 0.45) * leafSize : 0;
      const angle = outward > random() ? direction + between(-0.7, 0.7) : random() * Math.PI * 2;
      stampLeaf(data, size, x + Math.cos(direction) * push, y + Math.sin(direction) * push, angle, leafSize, between(0.6, 1), direction, half, round * between(0.7, 1.4), width * between(0.75, 1.25));
    }
  }
  // Offshoots: one to three short chains of smaller leaves reaching past a lobe's edge, each leaf a little farther out
  // and turned along the chain, the last ones smallest. They stop short of the card's border.
  const offshoots = fewest + Math.floor(random() * 3);
  for (let n = 0; n < offshoots; n++) {
    const lobe = lobes[Math.floor(random() * lobes.length)];
    let heading = random() * Math.PI * 2;
    const start = extent(shape, heading, lean) * half * lobe.r * between(0.75, 0.95);
    let x = half + lobe.x + Math.cos(heading) * start, y = half + lobe.y + Math.sin(heading) * start;
    const count = 2 + Math.floor(random() * 3), bend = between(-0.35, 0.35), tone = between(0.6, 1);
    for (let k = 0; k < count; k++) {
      const leafSize = leafLength * between(0.55, 0.8) * (1 - 0.12 * k);
      const angle = heading + between(-0.6, 0.6);
      // The leaf's far tip must stay inside the card.
      const tipX = x + Math.cos(angle) * leafSize, tipY = y + Math.sin(angle) * leafSize;
      if (Math.hypot(tipX - half, tipY - half) > half * 0.96) break;
      stampLeaf(data, size, x, y, angle, leafSize, tone * between(0.9, 1.08), heading, half, round * between(0.8, 1.3), width * between(0.8, 1.15));
      heading += bend;
      x += Math.cos(heading) * leafSize * 0.6;
      y += Math.sin(heading) * leafSize * 0.6;
    }
  }
  return { size, data, sprays };
}

/** An ovate leaf from (x, y) along `angle`; its texels take its tone and a normal from the card's dome tipped toward `outward`. */
function stampLeaf(data: Float32Array, size: number, x: number, y: number, angle: number, length: number, tone: number, outward: number, half: number, round: number, widthShare: number): void {
  const width = length * widthShare;
  // The leaf's hue, kept in coverage's place: (0, 1], 0 meaning no leaf. Warm or cool by a little, per leaf.
  const hue = 0.05 + 0.95 * (((Math.sin(x * 12.9898 + y * 78.233 + angle * 37.719) * 43758.5453) % 1 + 1) % 1);
  const cos = Math.cos(angle), sin = Math.sin(angle);
  const box = length + width;
  // The leaf's own tilt: toward the spray's outward direction and a little along itself.
  const tiltX = 0.35 * Math.cos(outward) + 0.15 * cos, tiltY = 0.35 * Math.sin(outward) + 0.15 * sin;
  for (let py = Math.max(0, Math.floor(y - box)); py <= Math.min(size - 1, Math.ceil(y + box)); py++) {
    for (let px = Math.max(0, Math.floor(x - box)); px <= Math.min(size - 1, Math.ceil(x + box)); px++) {
      const dx = px + 0.5 - x, dy = py + 0.5 - y;
      const u = (dx * cos + dy * sin) / length;
      if (u <= 0 || u >= 1) continue;
      const v = -dx * sin + dy * cos;
      const halfWidth = 0.5 * width * Math.pow(Math.sin(Math.PI * Math.pow(u, 0.75)), round);
      if (Math.abs(v) >= halfWidth) continue;
      // A leaf's rim a little darker, so a leaf over another reads as its own.
      const rim = Math.max(Math.abs(v) / halfWidth, u < 0.12 ? 1 - u / 0.12 : 0, u > 0.85 ? (u - 0.85) / 0.15 : 0);
      // ... and lighter toward its tip, a painter's stroke loading paint as it lifts.
      const shade = (1 - 0.2 * Math.min(Math.max((rim - 0.72) / 0.28, 0), 1)) * (0.9 + 0.14 * u);
      // The dome: sideways at the card's rim, out of the card at its middle; then the leaf's tilt.
      let nx = (px + 0.5 - half) / half, ny = (py + 0.5 - half) / half;
      const r = Math.hypot(nx, ny);
      if (r > 0.95) { nx *= 0.95 / r; ny *= 0.95 / r; }
      nx = nx * 0.85 + tiltX; ny = ny * 0.85 + tiltY;
      const flat = Math.hypot(nx, ny);
      if (flat > 0.97) { nx *= 0.97 / flat; ny *= 0.97 / flat; }
      const i = (py * size + px) * 4;
      data[i] = nx; data[i + 1] = ny; data[i + 2] = Math.min(1, tone * shade); data[i + 3] = hue;
    }
  }
}
