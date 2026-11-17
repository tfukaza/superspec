/**
 * Pictures of the leaf brush (browser/renderer/leaf-brush.ts), written to OUT (default generated/verification/leaf-brush):
 *   field.png      the height field, tiled 2x2 so a seam would show
 *   levels.png     the field cut at five thresholds (low to high, left to right)
 *   terminator.png a light-to-shade gradient whose edge the brush displaces at three amounts (top to bottom), in the
 *                  canopy's lit green and shade-tinted green, with plain fbm at the same amount on the right to compare
 *   closeup.png    a corner cut at 0.5, four times larger, to judge single strokes
 * SEED and SIZE pick the brush; WOBBLE, BRISTLE, JAG, COVER and SPREAD override its options.  Run: node tests/leaf-brush-preview.mts
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { crc32, deflateSync } from "node:zlib";
import { leafBrush } from "../browser/renderer/leaf-brush.ts";

const out = resolve(process.env.OUT ?? "generated/verification/leaf-brush");
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

const started = performance.now();
const tuned = Object.fromEntries((["wobble", "bristle", "jag", "cover", "spread"] as const)
  .filter(name => process.env[name.toUpperCase()] !== undefined).map(name => [name, Number(process.env[name.toUpperCase()])]));
const brush = leafBrush({ size: Number(process.env.SIZE ?? 512), seed: Number(process.env.SEED ?? 1), ...tuned });
const ms = performance.now() - started;
const { size, data } = brush;
const at = (x: number, y: number) => data[(((y % size) + size) % size) * size + (((x % size) + size) % size)];

// A plain fbm of the same tile for comparison (value noise, four octaves).
const hash = (x: number, y: number) => { const s = Math.sin(x * 127.1 + y * 311.7) * 43758.5453; return s - Math.floor(s); };
function fbm(x: number, y: number): number {
  let total = 0, amplitude = 0.5, frequency = 6 / size;
  for (let o = 0; o < 4; o++) {
    const px = x * frequency, py = y * frequency, ix = Math.floor(px), iy = Math.floor(py), fx = px - ix, fy = py - iy;
    const sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy);
    const a = hash(ix, iy), b = hash(ix + 1, iy), c = hash(ix, iy + 1), d = hash(ix + 1, iy + 1);
    total += amplitude * (a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy);
    amplitude *= 0.5; frequency *= 2;
  }
  return total / 0.9375;
}

{ // field.png
  const w = size * 2, rgb = new Uint8Array(w * w * 3);
  for (let y = 0; y < w; y++) for (let x = 0; x < w; x++) rgb.fill(Math.round(at(x, size * 2 - 1 - y) * 255), (y * w + x) * 3, (y * w + x) * 3 + 3);
  writeFileSync(resolve(out, "field.png"), png(w, w, rgb));
}
{ // levels.png
  const levels = [0.2, 0.35, 0.5, 0.62, 0.75], gap = 8, w = levels.length * size + (levels.length - 1) * gap, rgb = new Uint8Array(w * size * 3).fill(255);
  levels.forEach((level, k) => {
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const v = at(x, size - 1 - y), i = (y * w + k * (size + gap) + x) * 3;
      const on = Math.min(Math.max((v - level) * 60 + 0.5, 0), 1);
      rgb[i] = rgb[i + 1] = rgb[i + 2] = Math.round(255 - on * 215);
    }
  });
  writeFileSync(resolve(out, "levels.png"), png(w, size, rgb));
}
{ // terminator.png: lit and shade greens from the canopy recipe, the edge displaced by the brush (left) or fbm (right).
  const lit = [0.3, 0.52, 0.13], shade = [0.3 * 0.3, 0.52 * 0.4, 0.13 * 0.68];
  const amounts = [0.25, 0.5, 0.8], gap = 8, w = size * 2 + gap, h = amounts.length * size + (amounts.length - 1) * gap;
  const rgb = new Uint8Array(w * h * 3).fill(255);
  const encode = (c: number) => Math.round(255 * (c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055));
  amounts.forEach((amount, row) => {
    for (const side of [0, 1]) for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const noise = side === 0 ? at(x, y) : fbm(x, y);
      // A diagonal gradient across the tile (lit top-left), its zero line pushed by the noise about its middle.
      const d = ((x + y) / size - 1) * -0.9 + amount * (noise - 0.5);
      const band = Math.min(Math.max(d * 40 + 0.5, 0), 1);
      const i = ((row * (size + gap) + y) * w + side * (size + gap) + x) * 3;
      for (let c = 0; c < 3; c++) rgb[i + c] = encode(shade[c] + (lit[c] - shade[c]) * band);
    }
  });
  writeFileSync(resolve(out, "terminator.png"), png(w, h, rgb));
}
{ // closeup.png: a corner of the tile cut at 0.5, four times larger (bilinear, as a GPU samples it), in the two greens.
  const lit = [0.3, 0.52, 0.13], shade = [0.3 * 0.3, 0.52 * 0.4, 0.13 * 0.68], scale = 4, span = 200, w = span * scale;
  const rgb = new Uint8Array(w * w * 3);
  const encode = (c: number) => Math.round(255 * (c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055));
  for (let y = 0; y < w; y++) for (let x = 0; x < w; x++) {
    const fx = x / scale - 0.5, fy = (w - 1 - y) / scale - 0.5, ix = Math.floor(fx), iy = Math.floor(fy), tx = fx - ix, ty = fy - iy;
    const v = at(ix, iy) * (1 - tx) * (1 - ty) + at(ix + 1, iy) * tx * (1 - ty) + at(ix, iy + 1) * (1 - tx) * ty + at(ix + 1, iy + 1) * tx * ty;
    const band = Math.min(Math.max((v - 0.5) * 120 + 0.5, 0), 1), i = (y * w + x) * 3;
    for (let c = 0; c < 3; c++) rgb[i + c] = encode(shade[c] + (lit[c] - shade[c]) * band);
  }
  writeFileSync(resolve(out, "closeup.png"), png(w, w, rgb));
}
console.log(`Leaf brush ${size}² generated in ${ms.toFixed(1)} ms; pictures in ${out}`);
