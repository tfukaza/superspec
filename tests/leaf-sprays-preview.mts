/**
 * Pictures of the spray brush (browser/renderer/leaf-sprays.ts), written to OUT (default generated/verification/leaf-sprays):
 *   tile.png   the tile, each leaf in its tone (gaps cream), tiled 2x2 so a seam would show
 *   edge.png   the edge rule the stroke-edge pass applies, on a round mass in shade with a lit band across its top: near
 *              an edge, a leaf is wholly in or out by its reach and the signed distance at its centre
 * SEED, SIZE, BAND (pixels), AMOUNT and BITE pick the brush and the rule.  Run: node tests/leaf-sprays-preview.mts
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { crc32, deflateSync } from "node:zlib";
import { leafSprays } from "../browser/renderer/leaf-sprays.ts";

const out = resolve(process.env.OUT ?? "generated/verification/leaf-sprays");
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

const started = performance.now();
const brush = leafSprays({ size: Number(process.env.SIZE ?? 1024), seed: Number(process.env.SEED ?? 3) });
console.log(`Spray brush ${brush.size}², ${brush.leaves} leaves, in ${(performance.now() - started).toFixed(0)} ms`);
const { size, data } = brush;
const texel = (x: number, y: number) => ((((Math.floor(y) % size) + size) % size) * size + (((Math.floor(x) % size) + size) % size)) * 4;

const cream = hex("#f6ecdc"), lit = hex("#a9c64e"), shade = hex("#2e6b4c");
{ // tile.png
  const w = size * 2, rgb = new Uint8Array(w * w * 3);
  for (let y = 0; y < w; y++) for (let x = 0; x < w; x++) {
    const i = texel(x, y), o = (y * w + x) * 3, a = data[i + 3];
    const c = a === 0 ? cream : shade.map((s, k) => s + (lit[k] - s) * data[i] * 0.9 + 18 * (a - 0.75));
    rgb.set(c.map(v => Math.max(0, Math.min(255, Math.round(v)))), o);
  }
  writeFileSync(resolve(out, "tile.png"), png(w, w, rgb));
}
{ // edge.png
  const band = Number(process.env.BAND ?? 40), amount = Number(process.env.AMOUNT ?? 0.9), bite = Number(process.env.BITE ?? 0.3);
  const w = 1200, h = 900, centre = [600, 520], radius = 340;
  // Signed distances (positive inside the front side): the mass's outline, and the lit band's edge across its top.
  const outline = (x: number, y: number) => radius - Math.hypot(x - centre[0], y - centre[1]);
  const litEdge = (x: number, y: number) => (440 - y) + 0.35 * (x - 600);
  /** Whether the front side holds pixel (x, y) across the edge measured by `signed`. */
  const holds = (x: number, y: number, signed: (x: number, y: number) => number) => {
    const s = signed(x, y);
    if (s >= bite * band) return true;
    if (s <= -band * (1 + amount)) return false;
    const i = texel(x, y);
    if (data[i + 3] === 0) return false;
    const cx = x + data[i + 1], cy = y + data[i + 2];
    return signed(cx, cy) / band + (data[i] - 0.5) * 2 * amount > 0;
  };
  const rgb = new Uint8Array(w * h * 3);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const o = (y * w + x) * 3;
    let c = cream;
    if (holds(x, y, outline)) {
      c = holds(x, y, litEdge) ? lit : shade;
      const i = texel(x, y);
      // A leaf near an edge keeps its own tone.
      const near = Math.abs(outline(x, y)) < band * 1.5 || Math.abs(litEdge(x, y)) < band * 1.5;
      if (near && data[i + 3] > 0) c = c.map(v => v * (0.9 + 0.16 * data[i + 3]));
    }
    rgb.set(c.map(v => Math.max(0, Math.min(255, Math.round(v)))), o);
  }
  writeFileSync(resolve(out, "edge.png"), png(w, h, rgb));
}
console.log(`Pictures in ${out}`);
