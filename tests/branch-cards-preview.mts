/**
 * Pictures of the branch cards (browser/renderer/branch-cards.ts), written to OUT (default
 * generated/verification/branch-cards):
 *   cards.png  rows of dense, regular and sparse sprays, four seeds each, melted (MELT texels, default 2) and banded
 *              in the trees' six bands and highlight by a sun from the upper left (by each texel's angle to it, with
 *              the sky's floor, as leaf-normals.ts msLeafLight), on the concept's cream ground
 *   raw.png    the same cards unmelted
 * Run: node tests/branch-cards-preview.mts
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { crc32, deflateSync } from "node:zlib";
import { branchCard, type BranchCardOptions } from "../browser/renderer/branch-cards.ts";
import { melted, type LeafCard } from "../browser/renderer/leaf-cards.ts";

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
// The trees' bands and highlight (leaf-normals.ts), linear, to sRGB bytes.
const linear = [[0.0232, 0.1022, 0.0844], [0.0331, 0.1441, 0.0823], [0.0452, 0.1779, 0.0782], [0.0844, 0.2384, 0.0802],
  [0.15, 0.3277, 0.0844], [0.2747, 0.4793, 0.1022], [0.40, 0.64, 0.12]];
const srgb = (v: number) => Math.round(255 * (v <= 0.0031308 ? 12.92 * v : 1.055 * Math.pow(v, 1 / 2.4) - 0.055));
const cream = [246, 236, 220];
// The sun from the upper left and toward the viewer (card space: +x right, +y up, +z out of the card).
const sun = [-0.5, 0.55, 0.67]; const sl = Math.hypot(...sun); sun.forEach((v, k) => { sun[k] = v / sl; });
const rows: { name: string; options: BranchCardOptions }[] = [
  { name: "dense", options: { twigs: [3, 1], missing: 0.05, step: 0.05 } },
  { name: "regular", options: {} },
  { name: "sparse", options: { twigs: [1, 1], missing: 0.3, step: 0.075 } },
];
const size = 256, gap = 12, columns = 4;
const w = columns * size + (columns + 1) * gap, h = rows.length * size + (rows.length + 1) * gap;
const melt = Number(process.env.MELT ?? 2);

function paint(card: (seed: number, options: BranchCardOptions) => { coverage: Float32Array; tone: Float32Array; nx: Float32Array; ny: Float32Array }, file: string): void {
  const rgb = new Uint8Array(w * h * 3);
  for (let i = 0; i < w * h; i++) rgb.set(cream, i * 3);
  rows.forEach((row, r) => {
    for (let column = 0; column < columns; column++) {
      const c = card(column + 1 + 10 * r, row.options);
      const ox = gap + column * (size + gap), oy = gap + r * (size + gap);
      for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
        const i = y * size + x;
        if (c.coverage[i] < 0.5) continue;
        const nx = c.nx[i], ny = c.ny[i], nz = Math.sqrt(Math.max(0, 1 - nx * nx - ny * ny));
        const direct = 1 - Math.acos(Math.max(-1, Math.min(1, nx * sun[0] + ny * sun[1] + nz * sun[2]))) / Math.PI - 0.05;
        const light = Math.max(direct, 0.42 * (0.5 + 0.5 * ny));
        const band = light > 0.92 ? 6 : Math.max(0, Math.min(5, Math.floor(light * 6)));
        const tone = 0.78 + 0.3 * c.tone[i];
        rgb.set(linear[band].map(v => srgb(Math.min(1, v * tone))), ((oy + size - 1 - y) * w + ox + x) * 3);
      }
    }
  });
  writeFileSync(resolve(out, file), png(w, h, rgb));
}
const raw = (card: LeafCard) => {
  const n = card.size * card.size, coverage = new Float32Array(n), tone = new Float32Array(n), nx = new Float32Array(n), ny = new Float32Array(n);
  for (let i = 0; i < n; i++) { coverage[i] = card.data[i * 4 + 3] > 0 ? 1 : 0; nx[i] = card.data[i * 4]; ny[i] = card.data[i * 4 + 1]; tone[i] = card.data[i * 4 + 2]; }
  return { coverage, tone, nx, ny };
};
paint((seed, options) => melted(branchCard({ ...options, seed, size }), melt), "cards.png");
paint((seed, options) => raw(branchCard({ ...options, seed, size })), "raw.png");
console.log(`Branch cards written to ${out}`);
