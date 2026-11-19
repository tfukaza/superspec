/**
 * A contact sheet of pictures with captions:
 *   node tests/anime-tree-sheet.mjs <out.png> [--cols=N] [--cell=WIDTHxHEIGHT] "<caption>=<picture.png>" ...
 * Each picture fills its cell, cropped about its middle (COVER=0 fits it whole instead; `caption=picture.png@x,y,w,h`
 * crops that part of the picture first, in its pixels).
 */
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { launchBrowser } from "../tools/browser.mjs";

const [out, ...rest] = process.argv.slice(2);
const option = name => rest.find(arg => arg.startsWith(`--${name}=`))?.split("=")[1];
const entries = rest.filter(arg => !arg.startsWith("--")).map(arg => { const at = arg.indexOf("="); return { caption: arg.slice(0, at), path: arg.slice(at + 1) }; });
const cols = Number(option("cols") ?? Math.min(4, entries.length));
const [cellWidth, cellHeight] = (option("cell") ?? "480x480").split("x").map(Number);
const cells = [];
for (const { caption, path } of entries) {
  const [file, crop] = path.split("@");
  const data = (await readFile(resolve(file))).toString("base64");
  const box = crop?.split(",").map(Number);
  // A crop is drawn by scaling the picture so the crop fills the cell and shifting it into place.
  const style = box
    ? "max-width:none;position:absolute"
    : `width:100%;height:100%;object-fit:${process.env.COVER === "0" ? "contain" : "cover"}`;
  cells.push(`<figure><div class="frame"><img src="data:image/png;base64,${data}" style="${style}" data-crop="${crop ?? ""}"></div><figcaption>${caption}</figcaption></figure>`);
}
const html = `<!doctype html><style>
body{margin:0;background:#1b1b1b;font:15px/1.3 system-ui,sans-serif;color:#eee}
main{display:grid;grid-template-columns:repeat(${cols},${cellWidth}px);gap:6px;padding:6px;width:max-content}
figure{margin:0}.frame{width:${cellWidth}px;height:${cellHeight}px;overflow:hidden;position:relative;background:#d2ccc4}
img{display:block}figcaption{padding:4px 2px 6px}
</style><main>${cells.join("")}</main>`;
const browser = await launchBrowser();
try {
  const page = await browser.newPage({ viewport: { width: cols * (cellWidth + 6) + 6, height: 600 } });
  await page.setContent(html);
  await page.waitForFunction(() => [...document.images].every(image => image.complete && image.naturalWidth > 0));
  await page.evaluate(cellWidth => {
    for (const img of document.querySelectorAll("img[data-crop]")) {
      const c = img.dataset.crop.split(",").map(Number);
      if (c.length !== 4) continue;
      const s = cellWidth / c[2];
      Object.assign(img.style, { width: `${img.naturalWidth * s}px`, height: "auto", left: `${-c[0] * s}px`, top: `${-c[1] * s}px` });
    }
  }, cellWidth);
  await writeFile(resolve(out), await page.locator("main").screenshot());
  console.log(`Wrote ${resolve(out)}`);
} finally { await browser.close(); }
