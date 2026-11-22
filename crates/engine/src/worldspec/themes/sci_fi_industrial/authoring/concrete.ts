export const concreteResolution = 2048;

function noise(cells: number, seed: number) {
  const values = new Float32Array(cells * cells);
  for (let i = 0; i < values.length; i++) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    values[i] = seed / 0x100000000;
  }
  return (x: number, y: number) => {
    const sx = x * cells / concreteResolution, sy = y * cells / concreteResolution;
    const ix = Math.floor(sx), iy = Math.floor(sy);
    const nextX = (ix + 1) % cells, nextY = (iy + 1) % cells;
    const fx = sx - ix, fy = sy - iy;
    const tx = fx * fx * (3 - 2 * fx), ty = fy * fy * (3 - 2 * fy);
    const a = values[iy * cells + ix], b = values[iy * cells + nextX];
    const c = values[nextY * cells + ix], d = values[nextY * cells + nextX];
    const top = a + tx * (b - a), bottom = c + tx * (d - c);
    return top + ty * (bottom - top);
  };
}

function polishField() {
  const octaves = [7, 14, 28, 56, 112, 224].map((cells, i) => noise(cells, 0x6b359f01 + i * 23167));
  const warpX = noise(9, 0x52ab47c9), warpY = noise(9, 0x14b37e52);
  const size = concreteResolution;
  return (x: number, y: number) => {
    const u = (x + size + 132 * (warpX(x, y) - 0.5)) % size;
    const v = (y + size + 132 * (warpY(x, y) - 0.5)) % size;
    let value = 0, weight = 0.5, total = 0;
    for (const sample of octaves) {
      value += weight * sample(u, v);
      total += weight;
      weight *= 0.64;
    }
    return value / total;
  };
}

/** Seamless two-metre tile for clean cast concrete. Roughness carries the polished patches. */
export function* concreteMaps() {
  const size = concreteResolution;
  const mix = noise(5, 0x22da17f1), trowel = noise(13, 0x112ef2a7);
  const sand = noise(387, 0x486cc903), fines = noise(941, 0x76c506a2);
  const sheen = polishField();
  const color = new Uint8ClampedArray(size * size * 4);
  const properties = new Uint8ClampedArray(color.length);
  const relief = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const i = y * size + x, p = 4 * i;
      const paste = mix(x, y), finish = trowel(x, y);
      const grain = sand(x, y), grit = fines(x, y);
      const pores = Math.max(0, 0.17 - grain) * Math.max(0, 0.60 - grit) * 9;
      const value = 173 + 7 * (paste - 0.5) + 5 * (finish - 0.5)
        + 11 * (grain - 0.5) + 5 * (grit - 0.5) - 12 * pores;
      color[p] = value;
      color[p + 1] = value + 1;
      color[p + 2] = value + 1;
      color[p + 3] = 255;
      // glTF packs perceptual roughness in green and metalness in blue.
      const polish = Math.max(0, Math.min(1, (sheen(x, y) - 0.27) / 0.46));
      properties[p] = 255;
      properties[p + 1] = 255 * (0.65 - 0.15 * polish + 0.05 * (grain - 0.5));
      properties[p + 3] = 255;
      relief[i] = 0.055 * grain + 0.018 * grit - 0.055 * pores;
    }
  }
  yield ["modern-concrete-color.png", color] as const;
  yield ["modern-concrete-roughness.png", properties] as const;

  const normal = new Uint8ClampedArray(color.length), mask = size - 1;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const nx = relief[y * size + ((x - 1) & mask)] - relief[y * size + ((x + 1) & mask)];
      const ny = relief[((y + 1) & mask) * size + x] - relief[((y - 1) & mask) * size + x];
      const length = Math.hypot(nx, ny, 1);
      const p = 4 * (y * size + x);
      normal[p] = 127.5 + 127.5 * nx / length;
      normal[p + 1] = 127.5 + 127.5 * ny / length;
      normal[p + 2] = 127.5 + 127.5 / length;
      normal[p + 3] = 255;
    }
  }
  yield ["modern-concrete-normal.png", normal] as const;
}
