/**
 * Pictures of the leaf cards (browser/renderer/leaf-cards.ts), written to OUT (default generated/verification/leaf-cards):
 *   cards.png   a row per shape (round, lopsided, long), three seeds each, shaded in four bands by a sun from the upper
 *               left against each texel's normal, on the concept's cream ground
 *   normals.png the same cards' normals as colours, and coverage
 * Run: node tests/leaf-cards-preview.mts
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { crc32, deflateSync } from "node:zlib";
import { leafCard, leafCardAtlas, type CardShape } from "../browser/renderer/leaf-cards.ts";

const out = resolve(process.env.OUT ?? "generated/verification/leaf-cards");
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
const hex = (h: string) => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16));
const bands = ["#1d4a45", "#2e6b4c", "#5a9447", "#a9c64e"].map(hex);
const cream = hex("#f6ecdc");
const shapes: CardShape[] = ["round", "lopsided", "long"];
const size = 256, gap = 12, columns = 3;
const w = columns * size + (columns + 1) * gap, h = shapes.length * size + (shapes.length + 1) * gap;
const shaded = new Uint8Array(w * h * 3), normals = new Uint8Array(w * h * 3);
for (let i = 0; i < w * h; i++) { shaded.set(cream, i * 3); normals.set([40, 40, 40], i * 3); }
// The sun from the upper left and a little toward the viewer (card space: +x right, +y up, +z out).
const sun = [-0.55, 0.6, 0.58]; const sl = Math.hypot(...sun); sun.forEach((v, k) => { sun[k] = v / sl; });
shapes.forEach((shape, row) => {
  for (let column = 0; column < columns; column++) {
    const card = leafCard({ shape, seed: column + 1 });
    const ox = gap + column * (size + gap), oy = gap + row * (size + gap);
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const i = (y * size + x) * 4;
      if (card.data[i + 3] === 0) continue;
      const nx = card.data[i], ny = card.data[i + 1], nz = Math.sqrt(Math.max(0, 1 - nx * nx - ny * ny));
      const t = (nx * sun[0] + ny * sun[1] + nz * sun[2] + 0.31) / 2 + 0.5;
      const band = Math.max(0, Math.min(3, Math.floor(t * 4)));
      const tone = 0.9 + 0.16 * card.data[i + 2];
      // Card rows run top to bottom on screen, +y up in the card.
      const o = ((oy + size - 1 - y) * w + ox + x) * 3;
      shaded.set(bands[band].map(v => Math.min(255, Math.round(v * tone))), o);
      normals.set([Math.round((nx * 0.5 + 0.5) * 255), Math.round((ny * 0.5 + 0.5) * 255), Math.round(nz * 255)], o);
    }
  }
});
writeFileSync(resolve(out, "cards.png"), png(w, h, shaded));

{ // atlas.png: the billboard atlas, each card in one band as a dab on a clump takes it, with its leaves' tone and hue.
  const atlas = leafCardAtlas(4, 256, 1, Number(process.env.MELT ?? 3));
  const side = atlas.width, tall = atlas.height, rgb = new Uint8Array(side * tall * 3);
  for (let i = 0; i < side * tall; i++) rgb.set(cream, i * 3);
  for (let y = 0; y < tall; y++) for (let x = 0; x < side; x++) {
    const i = (y * side + x) * 4;
    if (atlas.data[i + 3] === 0) continue;
    const cell = Math.floor(y / 256) * 4 + Math.floor(x / 256);
    const color = cardColor(bands[cell % 4], atlas.data[i] / 255, atlas.data[i + 1] / 255);
    rgb.set(color, (y * side + x) * 3);
  }
  // Dense cards on top, sparse below.
  writeFileSync(resolve(out, "atlas.png"), png(side, tall, rgb));
  const touching: number[] = [];
  for (let cell = 0; cell < 32; cell++) {
    const ox = (cell % 4) * 256, oy = Math.floor(cell / 4) * 256;
    let hit = 0;
    for (let k = 0; k < 256; k++) for (const [x, y] of [[k, 1], [k, 254], [1, k], [254, k]]) if (atlas.data[((oy + y) * side + ox + x) * 4 + 3]) hit++;
    if (hit) touching.push(cell);
  }
  console.log(`Cards touching their border: ${touching.length ? touching.join(", ") : "none"}`);
}

/** A card texel's colour from its band: the leaf's tone scales it and its hue leans it warm or cool (as the shader does). */
function cardColor(band: number[], tone: number, hue: number): number[] {
  const lean = (hue - 0.5) * 2;
  const warm = [1.07, 1.03, 0.82], cool = [0.9, 1.0, 1.1];
  return band.map((v, k) => Math.max(0, Math.min(255, Math.round(v * (0.8 + 0.32 * tone) * (lean > 0 ? 1 + (warm[k] - 1) * lean : 1 + (cool[k] - 1) * -lean)))));
}
writeFileSync(resolve(out, "normals.png"), png(w, h, normals));
console.log(`Leaf cards written to ${out}`);
