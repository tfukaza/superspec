// Work in the packed primitive's UVs. Welding positions across UV seams keeps
// internal triangulation out of the mask while letting each chart draw its rim.
export function markCreases(positions, indices, uv, scale, stamp) {
  const ids = new Uint32Array(positions.length / 3),
    weld = new Map(),
    edges = new Map();
  for (let i = 0; i < ids.length; i++) {
    const key = positions.subarray(3 * i, 3 * i + 3).join(",");
    if (!weld.has(key)) weld.set(key, weld.size);
    ids[i] = weld.get(key);
  }
  function draw(a, b) {
    stamp(uv[a * 2], uv[a * 2 + 1], uv[b * 2], uv[b * 2 + 1]);
  }
  for (let i = 0; i < indices.length; i += 3) {
    const a = indices[i],
      b = indices[i + 1],
      c = indices[i + 2];
    const side = (p, k) =>
      (positions[3 * p + k] - positions[3 * a + k]) * scale[k];
    const x = side(b, 1) * side(c, 2) - side(b, 2) * side(c, 1);
    const y = side(b, 2) * side(c, 0) - side(b, 0) * side(c, 2);
    const z = side(b, 0) * side(c, 1) - side(b, 1) * side(c, 0);
    const len = Math.hypot(x, y, z);
    if (!len) continue;
    for (const [p, q] of [
      [a, b],
      [b, c],
      [c, a],
    ]) {
      const key =
        Math.min(ids[p], ids[q]) * ids.length + Math.max(ids[p], ids[q]);
      const prev = edges.get(key);
      if (!prev)
        edges.set(key, {
          p,
          q,
          x: x / len,
          y: y / len,
          z: z / len,
          open: true,
        });
      else {
        prev.open = false;
        if ((prev.x * x + prev.y * y + prev.z * z) / len < 0.76) {
          draw(prev.p, prev.q);
          draw(p, q);
        }
      }
    }
  }
  for (const e of edges.values()) if (e.open) draw(e.p, e.q);
}

export function strokeMask(mask, w, h, u, v, s, t) {
  // A wrapping chart can cover many repeats. Its seam has no unique texture-space edge.
  if (Math.min(u, v, s, t) < -0.001 || Math.max(u, v, s, t) > 1.001) return;
  const dx = (s - u) * w,
    dy = (t - v) * h,
    steps = Math.max(1, Math.ceil(Math.max(Math.abs(dx), Math.abs(dy))));
  for (let i = 0; i <= steps; i++) {
    const x = u * w + (i * dx) / steps - 0.5,
      y = v * h + (i * dy) / steps - 0.5;
    for (
      let j = Math.max(0, Math.floor(y - 1));
      j <= Math.min(h - 1, Math.ceil(y + 1));
      j++
    )
      for (
        let k = Math.max(0, Math.floor(x - 1));
        k <= Math.min(w - 1, Math.ceil(x + 1));
        k++
      ) {
        const value = Math.max(0, 1 - Math.hypot(k - x, j - y) / 1.65);
        mask[j * w + k] = Math.max(mask[j * w + k], Math.round(value * 255));
      }
  }
}

// Four overlapping 2x2 sectors. Keep the least-variance neighborhood and
// suppress samples across the metal/dielectric boundary of a shared atlas.
export function paintPigment(source, properties, width, height, gain) {
  const result = Buffer.from(source);
  for (let y = 1; y < height - 1; y++)
    for (let x = 1; x < width - 1; x++) {
      const i = 4 * (y * width + x),
        metal = properties[i + 2] / 255;
      if (metal <= 0.055) continue;
      let best = Infinity;
      for (const dy of [-width, width])
        for (const dx of [-1, 1]) {
          let r = 0,
            g = 0,
            b = 0,
            sum = 0,
            squares = 0,
            count = 0;
          for (const offset of [0, dx, dy, dy + dx]) {
            const k = i + 4 * offset;
            if (
              Math.abs(properties[k + 2] - properties[i + 2]) > 32 ||
              source[k + 3] !== source[i + 3]
            )
              continue;
            const l =
              0.2126 * source[k] +
              0.7152 * source[k + 1] +
              0.0722 * source[k + 2];
            r += source[k];
            g += source[k + 1];
            b += source[k + 2];
            sum += l;
            squares += l * l;
            count++;
          }
          const variance =
            count < 2 ? Infinity : squares / count - (sum / count) ** 2;
          if (variance >= best) continue;
          best = variance;
          const blend = gain * Math.min(1, metal * 2);
          result[i] = Math.round(source[i] + blend * (r / count - source[i]));
          result[i + 1] = Math.round(
            source[i + 1] + blend * (g / count - source[i + 1]),
          );
          result[i + 2] = Math.round(
            source[i + 2] + blend * (b / count - source[i + 2]),
          );
        }
    }
  return result;
}
// Check narrow metallic islands that vertex/centroid samples can miss.
// Row prefix counts make even a full, nonmetallic atlas chart inexpensive.
export function metalCoverage(pixels, w, h) {
  const stride = w + 1,
    rows = new Uint32Array(stride * h);
  let occupied = false;
  for (let y = 0; y < h; y++) {
    let count = 0;
    for (let x = 0; x < w; x++)
      rows[y * stride + x + 1] = count += Number(
        pixels[4 * (y * w + x) + 2] > 14,
      );
    occupied ||= count > 0;
  }
  return (uv, indices) => {
    if (!occupied) return false;
    const wrap = (n, size) => ((n % size) + size) % size;
    for (let i = 0; i < indices.length; i += 3) {
      const p = [indices[i], indices[i + 1], indices[i + 2]]
        .map((k) => [uv[2 * k] * w, uv[2 * k + 1] * h])
        .sort((a, b) => a[1] - b[1]);
      const intersect = (a, b, y) =>
        a[0] + ((b[0] - a[0]) * (y - a[1])) / (b[1] - a[1]);
      for (let row = Math.ceil(p[0][1] - 0.5); row < p[2][1] - 0.5; row++) {
        const y = row + 0.5,
          n = y < p[1][1] ? 0 : 1;
        const a = intersect(p[0], p[2], y),
          b = intersect(p[n], p[n + 1], y);
        const lo = Math.ceil(Math.min(a, b) - 0.5),
          hi = Math.floor(Math.max(a, b) - 0.5);
        if (hi < lo) continue;
        const at = wrap(row, h) * stride,
          start = wrap(lo, w),
          end = wrap(hi, w);
        if (
          hi - lo + 1 >= w
            ? rows[at + w] > 0
            : end >= start
              ? rows[at + end + 1] > rows[at + start]
              : rows[at + end + 1] > 0 || rows[at + w] > rows[at + start]
        )
          return true;
      }
    }
    return false;
  };
}
