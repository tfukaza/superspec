/**
 * Pictures of the illustrated tree looks (browser/renderer/anime-tree.ts) in the lab page (tests/browser/anime-tree):
 * the lane's plants alone on a plain ground under the avenue view's sun. Build the lab and write the plants first:
 *   node node_modules/vite/bin/vite.js build --config vite.anime-tree.config.ts
 *   LANE_TREE=street TREESPEC_OUTPUT=<repo>/generated/verification/anime-tree/data \
 *     cargo test --release -p superspec-engine --test treespec_batches -- --ignored write_lane_plants
 * LOOKS (default leaves,sprays,shells,impostors,painted) are pictured each as <look>-sun.png (the sun behind the
 * camera), <look>-shade.png (against the sun) and <look>-close.png in OUT (default generated/verification/anime-tree);
 * PLANTS (default 0) are the plants' indexes. ORBIT=<n> adds n pictures a few degrees apart (<look>-orbit-<k>.png);
 * TIME=1 prints each look's build time, instances, triangles and median frame time with every plant of ROW (default
 * 0,1,2,3,4,5,6) drawn. DATA names the plants' document. The real GPU wants
 * BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist".
 */
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";

const root = resolve("generated/anime-tree-lab");
const out = resolve(process.env.OUT ?? "generated/verification/anime-tree");
const data = resolve(process.env.DATA ?? "generated/verification/anime-tree/data/lane-plants.json");
await mkdir(out, { recursive: true });
const looks = (process.env.LOOKS ?? "leaves,sprays,shells,impostors,painted").split(",");
const indexes = (process.env.PLANTS ?? "0").split(",").map(Number);

const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = name === "/plants.json" ? data : resolve(root, name === "/" ? "index.html" : `.${name}`);
    if (path !== data && !path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".json": "application/json" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

const browser = await launchBrowser();
try {
  const [width, height] = (process.env.VIEWPORT ?? "1000x1000").split("x").map(Number);
  const page = await browser.newPage({ viewport: { width, height } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error" || message.type() === "warning") errors.push(`${message.type()}: ${message.text()}`.slice(0, 1500)); });
  await page.goto(`http://127.0.0.1:${server.address().port}/index.html`);
  await page.waitForFunction(() => document.body.dataset.state === "ready", {}, { timeout: 120000 });
  await page.evaluate(() => window.lab.load("/plants.json"));
  const sun = (process.env.SUN ?? "135,40").split(",").map(Number);
  const shoot = async (name, position, target, fov) => {
    await page.evaluate(({ position, target, fov }) => window.lab.view(position, target, fov), { position, target, fov });
    await waitForFrames(page, 2);
    await page.evaluate(({ position, target, fov }) => window.lab.view(position, target, fov), { position, target, fov });
    await writeFile(resolve(out, `${name}.png`), await page.locator("canvas").screenshot());
  };
  for (const look of looks) {
    const shown = await page.evaluate(({ indexes, look, sun }) => window.lab.show(indexes, look, sun), { indexes, look, sun });
    const plant = shown.plants[0], [cx, cy, cz] = plant.centre, top = plant.base[2] + plant.height;
    console.log(`${look}: built in ${shown.ms.toFixed(0)} ms: ${shown.built.map(part => `${part.name} ${part.instances ?? "?"} instances, ${part.triangles ?? "?"} triangles`).join("; ")}`);
    const around = (degrees, distance, lift) => { const a = degrees * Math.PI / 180; return [cx + Math.cos(a) * distance, cy + Math.sin(a) * distance, cz + lift]; };
    // The sun behind the camera and to its left, the camera raised as the avenue view's is; then against the sun.
    await shoot(`${look}-sun`, around(sun[0] + 28, 27, 6), [cx, cy, cz - 1.6], 26);
    await page.evaluate(() => window.lab.wall(false));
    await shoot(`${look}-shade`, around(sun[0] + 208, 27, 3), [cx, cy, cz - 1.6], 26);
    await page.evaluate(() => window.lab.wall(true));
    await shoot(`${look}-close`, around(sun[0] + 40, 9.5, 1.5), [cx, cy, top - 2.2], 30);
    for (let k = 0; k < Number(process.env.ORBIT ?? 0); k++) await shoot(`${look}-orbit-${k}`, around(sun[0] + 28 + 4 * k, 17, 4), [cx, cy, cz - 0.5], 26);
    if (process.env.TIME === "1") {
      const row = (process.env.ROW ?? "0,1,2,3,4,5,6").split(",").map(Number);
      const all = await page.evaluate(({ row, look, sun }) => window.lab.show(row, look, sun), { row, look, sun });
      const middle = all.plants.reduce((sum, p) => sum.map((v, i) => v + p.centre[i] / all.plants.length), [0, 0, 0]);
      const a = (sun[0] + 28) * Math.PI / 180;
      await page.evaluate(({ position, target }) => window.lab.view(position, target, 40), { position: [middle[0] + Math.cos(a) * 42, middle[1] + Math.sin(a) * 42, middle[2] + 10], target: middle });
      const withShadows = await page.evaluate(() => window.lab.measure(60, true)), without = await page.evaluate(() => window.lab.measure(60, false));
      await writeFile(resolve(out, `${look}-row.png`), await page.locator("canvas").screenshot());
      const sum = key => all.built.reduce((total, part) => total + (part[key] ?? 0), 0);
      console.log(`${look} cost, ${row.length} plants at ${width}x${height}: built in ${all.ms.toFixed(0)} ms, ${all.built.length} foliage draws, ${sum("instances")} instances, ${sum("triangles")} foliage triangles; `
        + `frame ${withShadows.ms.toFixed(2)} ms with the shadow pass (${withShadows.calls} calls, ${withShadows.triangles} triangles in the view pass), ${without.ms.toFixed(2)} ms without`);
    }
  }
  if (errors.length) console.log(`page said:\n${[...new Set(errors)].slice(0, 12).join("\n")}`);
  console.log(`Wrote pictures to ${out}`);
} finally {
  await browser.close();
  server.close();
}
