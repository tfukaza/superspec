/** The existing staggered stone paving on walkable faces, repeated every 3.2 metres. */
import { fileURLToPath } from "node:url";

const N = 2048;
export const pavingRepeat = 3.2;
export const pavingCourses = 8;
const clamp = (x) => Math.max(0, Math.min(1, x));
function noise(x, y) {
  const u = ((x % 1024) + 1024) % 1024,
    v = ((y % 1024) + 1024) % 1024;
  const n = Math.sin(u * 127.1 + v * 311.7 + 19.3) * 43758.5453;
  return n - Math.floor(n);
}
function field(u, v, n, m) {
  const x = u * n,
    y = v * m,
    i = Math.floor(x),
    j = Math.floor(y);
  let a = x - i,
    b = y - j;
  a = a * a * (3 - 2 * a);
  b = b * b * (3 - 2 * b);
  const h = (dx, dy) => noise((i + dx + n) % n, (j + dy + m) % m);
  return (1 - b) * ((1 - a) * h(0, 0) + a * h(1, 0)) + b * ((1 - a) * h(0, 1) + a * h(1, 1));
}
export async function pavingTexture(doc, sharp) {
  const stone = await sharp(
    fileURLToPath(new URL("../../../../../../../assets/materials/concrete-shared.png", import.meta.url)),
  )
    .resize(N, N)
    .removeAlpha()
    .raw()
    .toBuffer();
  const rgb = Buffer.alloc(N * N * 3);
  for (let y = 0; y < N; y++)
    for (let x = 0; x < N; x++) {
      const u = x / N,
        v = y / N,
        i = (y * N + x) * 3;
      const n = field(u, v, 22, 16),
        patch = field(u, v, 5, 6),
        grain = (stone[i] / 255 - 0.5) * 0.4 + (n - 0.5) * 0.08;
      const row = Math.floor(v * pavingCourses),
        xx = u * 4 + (row % 2) * 0.5,
        yy = v * pavingCourses;
      const d = Math.min(Math.abs(xx - Math.round(xx)), Math.abs(yy - Math.round(yy)) * 0.5);
      const seam = clamp(1 - d * 115 + (0.5 - n) * 0.9),
        shoulder = clamp(1 - d * 33);
      const slab = noise(Math.floor(xx) % 4, row % pavingCourses);
      // Dry cast slabs vary chiefly at the scale of sand and chips. Keep course
      // joints legible without a second, broad dark shape competing with them.
      const chips = field(u, v, 105, 91);
      const shade =
        0.565 +
        grain +
        // Keep the original aggregate spacing. Finer grit fills the additional texels.
        0.085 * (field(u, v, 768, 768) - 0.5) +
        0.025 * (noise(x, y) - 0.5) +
        0.09 * (chips - 0.5) +
        0.035 * (patch - 0.5) +
        0.065 * (slab - 0.5) -
        0.21 * seam -
        0.045 * shoulder;
      const colors = [1, 0.965, 0.92];
      for (let c = 0; c < 3; c++) rgb[i + c] = Math.round(255 * clamp(shade * colors[c]));
    }
  return doc
    .createTexture("Laid stone paving")
    .setMimeType("image/jpeg")
    .setImage(
      await sharp(rgb, { raw: { width: N, height: N, channels: 3 } })
        .jpeg({ quality: 95 })
        .toBuffer(),
    );
}
