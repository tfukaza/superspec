/**
 * Pictures of the stroke-painted leaf normal map (browser/renderer/leaf-normals.ts), in OUT (default
 * generated/verification/leaf-normals):
 *   normals.png  the map in normal-map colours, tiled 2x2
 *   banded.png   the map on a flat face banded in the shrub's six colours, for suns from three directions
 * Run: node tests/leaf-normals-preview.mts
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { crc32, deflateSync } from "node:zlib";
import { strokeNormalData } from "../browser/renderer/leaf-normals.ts";

const out = resolve(process.env.OUT ?? "generated/verification/leaf-normals");
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
const { size, data, strokes } = strokeNormalData({ clumpLeaves: Number(process.env.BRUSH ?? 0) });
console.log(`${strokes} strokes on a ${size}² tile`);
{
  const w = size * 2, rgb = new Uint8Array(w * w * 3);
  for (let y = 0; y < w; y++) for (let x = 0; x < w; x++) {
    const i = ((y % size) * size + (x % size)) * 4;
    const nx = data[i] / 127.5 - 1, ny = data[i + 1] / 127.5 - 1, nz = Math.sqrt(Math.max(0, 1 - nx * nx - ny * ny));
    const k = 0.75 + 0.35 * data[i + 2] / 255;
    rgb.set([data[i], data[i + 1], Math.round((nz * 0.5 + 0.5) * 255)].map(v => Math.min(255, Math.round(v * k))), (y * w + x) * 3);
  }
  writeFileSync(resolve(out, "normals.png"), png(w, w, rgb));
}
{
  const bands = ["#2a5a52", "#336a51", "#3c754f", "#528650", "#6c9b52", "#8fb85a"].map(h => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16)));
  const suns = [[-0.5, 0.5, 0.7], [0.6, 0.2, 0.75], [0, -0.7, 0.7]].map(s => { const l = Math.hypot(...s); return s.map(v => v / l); });
  const gap = 8, w = suns.length * size + (suns.length - 1) * gap, rgb = new Uint8Array(w * size * 3).fill(255);
  suns.forEach((sun, k) => {
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const i = (y * size + x) * 4;
      const nx = data[i] / 127.5 - 1, ny = data[i + 1] / 127.5 - 1, nz = Math.sqrt(Math.max(0, 1 - nx * nx - ny * ny));
      const t = (nx * sun[0] + ny * sun[1] + nz * sun[2]) * 0.5 + 0.5 + 0.1;
      const band = bands[Math.max(0, Math.min(5, Math.floor(t * 6)))];
      rgb.set(band.map(v => Math.min(255, Math.round(v * (0.78 + 0.3 * data[i + 2] / 255)))), (y * w + k * (size + gap) + x) * 3);
    }
  });
  writeFileSync(resolve(out, "banded.png"), png(w, size, rgb));
}
console.log(`Pictures in ${out}`);
