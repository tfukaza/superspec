/**
 * The branch card family (browser/renderer/branch-cards.ts): the regular open spray in three sizes with leaves of
 * one size in pixels, so a bigger card carries more leaves, drawn at their relative sizes, banded in the trees'
 * bands as tests/branch-cards-preview.mts does. Rows: the medium card evenly leafed (the first is seed 11, the pick),
 * then small, medium and large with gradual density (FADE, default 0.5). Written to OUT (default
 * generated/verification/branch-cards) as family.png. Run: node tests/branch-family-preview.mts
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { crc32, deflateSync } from "node:zlib";
import { branchCard, type BranchCardOptions } from "../browser/renderer/branch-cards.ts";
import { melted } from "../browser/renderer/leaf-cards.ts";
import { strokeNormalData } from "../browser/renderer/leaf-normals.ts";

// The leaf pattern normal map the trees use (16-leaf brush), laid as on a tree: a 1 m tile, the medium card 0.75 m,
// so 256 card texels are 0.75 m. MAP=0 leaves it off. Its leaves tip each card texel's own normal, as the shader does.
const map = strokeNormalData({ clumpLeaves: 16 });
const mapStrength = process.env.MAP === "0" ? 0 : Number(process.env.MAP ?? 1);
const texelsPerTile = 256 / 0.75;
function mapAt(x: number, y: number): [number, number, number] {
  const u = Math.floor(((x / texelsPerTile) % 1 + 1) % 1 * map.size), v = Math.floor(((y / texelsPerTile) % 1 + 1) % 1 * map.size);
  const i = (v * map.size + u) * 4;
  return [map.data[i] / 127.5 - 1, map.data[i + 1] / 127.5 - 1, map.data[i + 2] / 255];
}

const out = resolve(process.env.OUT ?? "generated/verification/branch-cards");
mkdirSync(out, { recursive: true });
function png(width: number, height: number, rgb: Uint8Array): Buffer {
  const chunk = (type: string, data: Buffer) => {
    const head = Buffer.alloc(8); head.writeUInt32BE(data.length, 0); head.write(type, 4, "ascii");
    const tail = Buffer.alloc(4); tail.writeUInt32BE(crc32(Buffer.concat([head.subarray(4), data])) >>> 0, 0);
    return Buffer.concat([head, data, tail]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0); header.writeUInt32BE(height, 4); header[8] = 8; header[9] = 2;
  const raw = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0; y < height; y++) raw.set(rgb.subarray(y * width * 3, (y + 1) * width * 3), y * (width * 3 + 1) + 1);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", header), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}
const linear = [[0.0232, 0.1022, 0.0844], [0.0331, 0.1441, 0.0823], [0.0452, 0.1779, 0.0782], [0.0844, 0.2384, 0.0802],
  [0.15, 0.3277, 0.0844], [0.2747, 0.4793, 0.1022], [0.40, 0.64, 0.12]];
const srgb = (v: number) => Math.round(255 * (v <= 0.0031308 ? 12.92 * v : 1.055 * Math.pow(v, 1 / 2.4) - 0.055));
const cream = [246, 236, 220];
const sun = [-0.5, 0.55, 0.67]; const sl = Math.hypot(...sun); sun.forEach((v, k) => { sun[k] = v / sl; });

// One leaf length and spacing in pixels for every size (the medium card's: 0.08 and 0.06 of 256).
const leafPx = 0.08 * 256, stepPx = 0.06 * 256;
const fade = Number(process.env.FADE ?? 0.35);
export const FAMILY: Record<string, { size: number; options: BranchCardOptions }> = {
  small: { size: 176, options: { twigs: [1, 1], twiglets: [0, 1], missing: 0.04 } },
  medium: { size: 256, options: { twigs: [2, 2], twiglets: [1, 1], missing: 0.08 } },
  large: { size: 368, options: { twigs: [4, 1], twiglets: [2, 1], missing: 0.03 } },
  "round small": { size: 176, options: { round: true, missing: 0.04 } },
  "round medium": { size: 256, options: { round: true, missing: 0.04 } },
  "round large": { size: 368, options: { round: true, missing: 0.03 } },
};
const rows: { kind: string; fade: number }[] = ["small", "medium", "large", "round small", "round medium", "round large"].map(kind => ({ kind, fade }));
const cell = 368, gap = 12, columns = 4;
const w = columns * cell + (columns + 1) * gap, h = rows.length * cell + (rows.length + 1) * gap;
const rgb = new Uint8Array(w * h * 3);
for (let i = 0; i < w * h; i++) rgb.set(cream, i * 3);
rows.forEach((row, r) => {
  const { size, options } = FAMILY[row.kind];
  for (let column = 0; column < columns; column++) {
    const card = melted(branchCard({ ...options, size, seed: 11 + column, leaf: leafPx / size, step: stepPx / size, fade: row.fade, stemRef: 256 }), 2);
    // Bottom-aligned and centred in its cell, at its own size.
    const ox = gap + column * (cell + gap) + (cell - size) / 2, oy = gap + r * (cell + gap) + (cell - size);
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const i = y * size + x;
      if (card.coverage[i] < 0.5) continue;
      let nx = card.nx[i], ny = card.ny[i], nz = Math.sqrt(Math.max(0, 1 - nx * nx - ny * ny));
      // The leaf map at this texel's place on the tree (bottom-up), its leaf tipping the card's normal.
      const [mx, my, mt] = mapAt(ox + x, (r * cell) + y);
      const mz = Math.sqrt(Math.max(0, 1 - mx * mx - my * my));
      nx = nx * mz + mx * mapStrength; ny = ny * mz + my * mapStrength; nz = nz * mz;
      const nl = Math.hypot(nx, ny, nz); nx /= nl; ny /= nl; nz /= nl;
      card.tone[i] = mapStrength > 0 ? 0.5 * card.tone[i] + 0.5 * mt * 1.6 : card.tone[i];
      const direct = 1 - Math.acos(Math.max(-1, Math.min(1, nx * sun[0] + ny * sun[1] + nz * sun[2]))) / Math.PI - 0.05;
      const light = Math.max(direct, 0.42 * (0.5 + 0.5 * ny));
      const band = light > 0.92 ? 6 : Math.max(0, Math.min(5, Math.floor(light * 6)));
      const tone = 0.78 + 0.3 * card.tone[i];
      rgb.set(linear[band].map(v => srgb(Math.min(1, v * tone))), ((oy + size - 1 - y) * w + Math.round(ox) + x) * 3);
    }
  }
});
writeFileSync(resolve(out, "family.png"), png(w, h, rgb));
console.log(`Branch card family written to ${out}`);
