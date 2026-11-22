import { readFile, writeFile, rename } from "node:fs/promises";
import { launchBrowser } from "../../tools/browser.mjs";
import { createServer } from "vite";
import { dirname, basename, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { Vector3, Quaternion } from "three";
import sharp from "sharp";

// Game-space metres (Y up). Entries run from broad to local, so tighter boxes
// take priority. Capture the installed map after geometry or lighting edits.
const stoaProbes = [
  {
    id: "central-stoa",
    center: [0, 3, 0],
    min: [-36, -2, -36],
    max: [36, 15, 36],
    fade: 4,
  },
  {
    id: "east-arcade",
    center: [21.5, 2, 5],
    min: [14, 0, -16],
    max: [27, 6, 17],
    fade: 1.2,
  },
  {
    id: "liquor-room",
    center: [18.4, 1.55, 10.9],
    min: [15.6, 0, 8.7],
    max: [20.6, 2.95, 13.3],
    fade: 0.35,
  },
  {
    id: "duck-worktop",
    center: [18.46, 1.12, -3.75],
    min: [15.9, 0.03, -6.48],
    max: [19.12, 2.8, -2.48],
    fade: 0.22,
  },
  {
    id: "west-passage",
    center: [-4.65, 1.45, 0.35],
    min: [-6.85, -0.36, -3.85],
    max: [-1.38, 3.45, 3.82],
    fade: 0.30,
  },
];
const width = 128,
  height = 64,
  faceSize = 96;
export async function captureReflectionProbes(input, probes, record = { version: 1, width, height, probes: [] }, captureSize = faceSize) {
  const server = await createServer({
    configFile: false, root: fileURLToPath(new URL(".", import.meta.url)), publicDir: dirname(resolve(input)),
    server: { host: "127.0.0.1", port: 0 },
  });
  await server.listen();
  let browser;
  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    const url = new URL("probe-preview.html", server.resolvedUrls.local[0]);
    url.searchParams.set("asset", "/" + encodeURIComponent(basename(input)));
    await page.goto(url.href);
    await page.waitForFunction(() => typeof window.captureFace === "function");
    for (const probe of probes) {
      const rgb = await capturePanorama(page, probe.center, record.width, record.height, captureSize);
      const previous = record.probes.findIndex(p => p.id === probe.id);
      if (previous < 0) record.probes.push({ ...probe, rgb });
      else record.probes[previous] = { ...probe, rgb };
      console.log("Captured", probe.id);
    }
    return record;
  } finally {
    await browser?.close();
    await server.close();
  }
}

async function capturePanorama(page, center, width, height, faceSize) {
  const faces = [];
  for (const direction of [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]]) {
    const result = await page.evaluate(
      ([position, dir, size]) => window.captureFace(position, dir, size),
      [center, direction, faceSize],
    );
    const pixels = await sharp(Buffer.from(result.data.split(",")[1], "base64")).removeAlpha().raw().toBuffer();
    faces.push({ pixels, inverse: new Quaternion().fromArray(result.quaternion).invert(), forward: new Vector3(...direction) });
  }
  const data = Buffer.alloc(width * height * 3);
  const ray = new Vector3(), local = new Vector3();
  const texel = c => Math.min(faceSize - 1, Math.max(0, Math.floor(c * faceSize)));
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const az = ((x + 0.5) / width - 0.5) * 2 * Math.PI;
    const el = ((y + 0.5) / height - 0.5) * Math.PI;
    ray.set(Math.cos(el) * Math.cos(az), Math.sin(el), Math.cos(el) * Math.sin(az));
    const face = faces.reduce((a, b) => a.forward.dot(ray) > b.forward.dot(ray) ? a : b);
    local.copy(ray).applyQuaternion(face.inverse);
    const ix = texel(0.5 - (0.5 * local.x) / local.z);
    const iy = texel(0.5 + (0.5 * local.y) / local.z);
    const pixel = 3 * (iy * faceSize + ix);
    face.pixels.copy(data, 3 * (y * width + x), pixel, pixel + 3);
  }
  return data.toString("base64");
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [input, output, requested] = process.argv.slice(2);
  if (!input || !output) throw Error("Supply the exported GLB path and a probe JSON destination.");
  const probes = requested ? stoaProbes.filter(probe => probe.id === requested) : stoaProbes;
  if (!probes.length) throw Error("Unknown reflection capture: " + requested);
  const record = requested ? JSON.parse(await readFile(output, "utf8")) : undefined;
  if (record && (record.version !== 1 || record.width !== width || record.height !== height)) {
    throw Error("Rebuild all reflection captures after changing the panorama format");
  }
  const captured = await captureReflectionProbes(input, probes, record);
  const staged = output + "." + process.pid + ".tmp";
  await writeFile(staged, JSON.stringify(captured));
  await rename(staged, output);
}
