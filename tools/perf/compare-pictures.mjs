/**
 * Compares the pictures of two measurement runs (tests/item-building-perf.mjs) channel by channel.
 * Usage: node tools/perf/compare-pictures.mjs <directory a> <directory b> [name.png ...]
 * Prints, per picture: the share of pixels that differ at all and by more than 2 and 8 of 255, the largest
 * difference and the mean absolute difference. Exits 1 when a picture differs by more than MAX_MEAN (default 0.5)
 * on average or more than MAX_SHARE (default 0.01) of its pixels differ by more than 8.
 */
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { PNG } from "pngjs";

const [a, b, ...names] = process.argv.slice(2);
const pictures = names.length ? names : readdirSync(a).filter(name => name.endsWith(".png"));
let failed = false;
for (const name of pictures) {
  const one = PNG.sync.read(readFileSync(resolve(a, name))), two = PNG.sync.read(readFileSync(resolve(b, name)));
  if (one.width !== two.width || one.height !== two.height) { console.log(`${name}: sizes differ`); failed = true; continue; }
  let any = 0, over2 = 0, over8 = 0, largest = 0, sum = 0;
  const pixels = one.width * one.height;
  for (let pixel = 0; pixel < pixels; pixel++) {
    let worst = 0;
    for (let channel = 0; channel < 3; channel++) {
      const difference = Math.abs(one.data[pixel * 4 + channel] - two.data[pixel * 4 + channel]);
      sum += difference; worst = Math.max(worst, difference);
    }
    if (worst > 0) any++;
    if (worst > 2) over2++;
    if (worst > 8) over8++;
    largest = Math.max(largest, worst);
  }
  const mean = sum / (pixels * 3), share = count => `${(100 * count / pixels).toFixed(3)}%`;
  console.log(`${name}: ${share(any)} of pixels differ (${share(over2)} by more than 2, ${share(over8)} by more than 8), largest ${largest}, mean ${mean.toFixed(4)} of 255`);
  if (mean > Number(process.env.MAX_MEAN ?? 0.5) || over8 / pixels > Number(process.env.MAX_SHARE ?? 0.01)) failed = true;
}
process.exitCode = failed ? 1 : 0;
