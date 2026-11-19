/**
 * Writes the shared grime streak pattern, `assets/materials/grime-streaks.png`: rain runoff below a source edge as the
 * Kowloon wall condition draws it (crates/engine/src/worldspec/themes/kowloon/wall.rs, `runoff_at` and
 * `deposit_detail`), baked once into a tile that weathering layers repeat and skew instead of evaluating the noise
 * per texel (`kowloon-grime-streaks.material.ts`).
 *
 * The tile is edge space: x runs along the source edge over one period (`PERIOD` meters) and wraps; y runs down from
 * the edge (top row) to the longest streak (`LONGEST` meters, bottom row). Every noise is periodic in x, so the tile
 * repeats along any edge without a seam. Alpha is the deposit's coverage; red, green and blue are white.
 * Usage: node tools/grime/streak-pattern.mjs [width] [height]
 */
import { writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";

const PERIOD = 4;
const LONGEST = 2.8;
const width = Number(process.argv[2] ?? 2048), height = Number(process.argv[3] ?? 1024);

/** A lattice hash in [0, 1). */
function hash(x, y, seed) {
  let h = (Math.imul(x | 0, 374761393) ^ Math.imul(y | 0, 668265263) ^ Math.imul(seed | 0, 2246822519)) >>> 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177) >>> 0;
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}
/** Value noise whose lattice wraps every `cells` along x, so it repeats over the tile. */
function noise(x, y, cells, seed) {
  const x0 = Math.floor(x), y0 = Math.floor(y), fx = x - x0, fy = y - y0;
  const sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy);
  const wrap = value => ((value % cells) + cells) % cells;
  const at = (i, j) => hash(wrap(x0 + i), y0 + j, seed);
  return (at(0, 0) * (1 - sx) + at(1, 0) * sx) * (1 - sy) + (at(0, 1) * (1 - sx) + at(1, 1) * sx) * sy;
}
/** Octaves of periodic noise; `frequency` is cells per meter along x, rounded so a period holds whole cells. */
function fractal(u, z, frequency, verticalFrequency, octaves, seed) {
  let sum = 0, weight = 0, amplitude = 1;
  for (let octave = 0; octave < octaves; octave++) {
    const cells = Math.max(1, Math.round(PERIOD * frequency * 2 ** octave));
    sum += amplitude * noise(u / PERIOD * cells, z * verticalFrequency * 2 ** octave, cells, seed + octave * 101);
    weight += amplitude; amplitude *= 0.5;
  }
  return sum / weight;
}
const smoothstep = (a, b, x) => { const t = Math.min(1, Math.max(0, (x - a) / (b - a))); return t * t * (3 - 2 * t); };
const periodic = (u, frequency, row, seed) => {
  const cells = Math.max(1, Math.round(PERIOD * frequency));
  return noise(u / PERIOD * cells, row, cells, seed);
};

const png = new PNG({ width, height });
for (let row = 0; row < height; row++) {
  for (let column = 0; column < width; column++) {
    const u = (column + 0.5) / width * PERIOD;
    const drop = (row + 0.5) / height * LONGEST;
    // Each column's streak: its strength and its reach.
    const streak = smoothstep(0.3, 0.78, periodic(u, 3.4, 0.7, 53));
    const strength = 0.2 + 0.8 * streak;
    const reach = 0.3 + (LONGEST - 0.3) * strength * periodic(u, 2.1, 1.3, 71);
    const streaked = strength * Math.max(1 - drop / reach, 0) ** 1.4;
    // Narrow runs of their own, where water finds a notch in the edge: a few per meter, some of them long.
    const notch = smoothstep(0.62, 0.86, periodic(u, 9.5, 2.2, 97)) * (0.55 + 0.45 * periodic(u, 4.3, 6.1, 113));
    const notchReach = 0.35 + (LONGEST - 0.35) * periodic(u, 6.7, 5.1, 131) ** 1.5;
    const run = notch * Math.max(1 - drop / notchReach, 0) ** 1.1;
    // The lip under the edge is broken: heavy in places, almost clean in others, never a ruled line.
    const lip = (0.12 + 0.6 * smoothstep(0.25, 0.8, periodic(u, 5.3, 9.3, 211))) * Math.exp(-drop / (0.04 + 0.12 * periodic(u, 2.9, 11.7, 223)));
    const stain = Math.min(Math.max(streaked, run) + lip, 1);
    // The deposit's detail: fine streaks that wander a little, broken patches and a grain.
    const wander = (periodic(u, 3.1, drop * 1.4, 419) - 0.5) * 0.06;
    const fine = fractal(u + wander, drop, 17, 2.3, 3, 421);
    const broken = fractal(u, drop, 13, 11, 2, 423);
    const grain = noise(u / PERIOD * Math.round(PERIOD * 71), drop * 67, Math.round(PERIOD * 71), 431);
    const detail = 0.12 + 0.62 * smoothstep(0.36, 0.68, fine) + 0.18 * broken + 0.08 * grain;
    // Many thin streaks, one to four centimeters wide, each sharp where it starts under the edge and thinning as it
    // falls: what a wall shows most of. They wander a little and are not softened by the deposit's detail.
    const drift = (periodic(u, 2.3, drop * 0.9, 433) - 0.5) * 0.05;
    const thread = smoothstep(0.44, 0.52, periodic(u + drift, 30, 3.3, 151)) * (0.5 + 0.5 * periodic(u, 8.1, 7.7, 157));
    const threadReach = 0.45 + 1.9 * periodic(u, 29, 8.8, 163) ** 1.1;
    const thin = thread * Math.max(1 - drop / threadReach, 0) ** 0.7 * (0.75 + 0.25 * grain);
    const coverage = Math.min(Math.max(stain * 0.85 * detail * 1.9, thin), 1);
    png.data.set([255, 255, 255, Math.round(coverage * 255)], (row * width + column) * 4);
  }
}
const out = resolve(dirname(fileURLToPath(import.meta.url)), "../../assets/materials/grime-streaks.png");
writeFileSync(out, PNG.sync.write(png));
console.log(`Wrote ${out} (${width} by ${height}: ${PERIOD} m along the edge, ${LONGEST} m down).`);
