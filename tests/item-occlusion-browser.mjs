/**
 * The occluder service's planes (`shadow`, `occlusion`, `sunExposure`) in the Material Studio, on a generated copy of
 * the washroom built from items (tests/studio/washroom-items) whose slab shows a project-local material that reads
 * them: moss where the floor is closed in or sheltered, and a subtle darkening where the sun's disk is blocked.
 *
 * 1. The GPU preview bakes the planes in the authoring worker (Wasm), binds them to the CPU fields of the slab's hybrid
 *    plan and draws its maps; moss shows on the slab and nowhere it did not before.
 * 2. The CPU bake (`?preview=bake`) of the same project reads the same planes; the slab's picture is compared. Each plane
 *    is baked once (the 64 px draft's are planes of their own atlas), and a moved sun there bakes `shadow` alone too.
 * 3. Moving the sun in the lighting panel bakes `shadow` again and nothing else: `occlusion` and `sunExposure` are held
 *    by key, and only the meshes whose material reads `shadow` are baked again. Planes are the page's: an export on the
 *    build worker finds every plane the preview lane baked and casts no ray.
 *
 * RESOLUTION (default 256) sets the texture size; SCREENSHOTS (default generated/verification/occlusion) names a
 * directory for PNGs and the generated project. Nothing under tests/studio is changed: the project is a copy.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { PNG } from "pngjs";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom-items/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const resolution = Number(process.env.RESOLUTION ?? 256);
const shots = resolve(process.env.SCREENSHOTS ?? "generated/verification/occlusion");
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "index.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".png": "image/png" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

/** Moss where the surface is closed in or sheltered, on plain concrete darkened a little where the sun is blocked. */
const MOSS = `material 1;
let closed = external("occlusion", 0);
let open = external("sunExposure", 1);
let blocked = external("shadow", 0);
let damp = clamp(closed * 1.6 + (1 - open) * 0.5, 0, 1);
let grain = noise(surface * 9, seed("moss"));
let patch = noise(surface * 2.5, seed("patch"));
let moss = smoothstep(0.35, 0.6, damp * (0.75 + 0.5 * patch) + (grain - 0.5) * 0.18);
let concrete = vec3(0.52, 0.5, 0.47) * (0.9 + 0.2 * grain) * (1 - 0.3 * blocked);
output base_color = mix(concrete, vec3(0.1, 0.22, 0.05) * (0.7 + 0.6 * grain), moss);
output roughness = mix(0.75, 0.95, moss);
output metalness = 0;
`;

/** Pixels of a convex quad (canvas CSS pixels) by a predicate, or differing between two pictures by a tolerance. */
function region(a, quad, clientWidth, visit) {
  const scale = a.width / clientWidth;
  const inside = (x, y) => {
    let sign = 0;
    for (let i = 0; i < quad.length; i++) {
      const [x0, y0] = quad[i], [x1, y1] = quad[(i + 1) % quad.length];
      const cross = (x1 - x0) * (y - y0) - (y1 - y0) * (x - x0);
      if (cross !== 0) { if (sign && Math.sign(cross) !== sign) return false; sign = Math.sign(cross); }
    }
    return true;
  };
  let total = 0;
  for (let y = 0; y < a.height; y++) for (let x = 0; x < a.width; x++) {
    if (!inside(x / scale, y / scale)) continue;
    total++;
    visit((y * a.width + x) * 4);
  }
  assert.ok(total > 1000, `the region covers the slab (${total} pixels)`);
  return total;
}
const mossy = (image, quad, clientWidth) => {
  let green = 0;
  const pixels = region(image, quad, clientWidth, at => {
    const [r, g, b] = [image.data[at], image.data[at + 1], image.data[at + 2]];
    if (g > r * 1.25 && g > b * 1.5) green++;
  });
  return { pixels, green, share: +(green / pixels).toFixed(4) };
};
const differing = (a, b, quad, clientWidth, tolerance) => {
  let changed = 0, sum = 0, worst = 0;
  const pixels = region(a, quad, clientWidth, at => {
    const difference = Math.max(Math.abs(a.data[at] - b.data[at]), Math.abs(a.data[at + 1] - b.data[at + 1]), Math.abs(a.data[at + 2] - b.data[at + 2]));
    worst = Math.max(worst, difference); sum += difference;
    if (difference > tolerance) changed++;
  });
  return { pixels, changed, share: +(changed / pixels).toFixed(4), mean: +(sum / pixels).toFixed(2), worst };
};

let browser;
try {
  const { project } = buildProject(resolution, undefined, { weathering: false });
  project.modules["occlusion-moss.ms"] = MOSS;
  Object.assign(project.materials.find(material => material.id === "concrete-slab"), { entry: "occlusion-moss.ms", parameters: {} });
  await mkdir(shots, { recursive: true });
  await writeFile(resolve(shots, "washroom-items-occlusion.studio.json"), JSON.stringify(project, null, 2) + "\n");
  const plain = buildProject(resolution, undefined, { weathering: false }).project;
  browser = await launchBrowser();

  const open = async (candidate, query) => {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    const json = async key => JSON.parse(await page.evaluate(key => document.querySelector("#viewport").dataset[key] ?? "null", key));
    const ready = async () => {
      await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state), {}, { timeout: 900000 });
      assert.equal(await page.locator("#status").getAttribute("data-state"), "ready", `${await page.locator("#status").textContent()}\n${(await page.locator("#diagnostics button").allTextContents()).join("\n")}`);
    };
    const gpu = !/preview=bake/.test(query);
    const baked = async () => {
      await ready();
      if (gpu) await page.waitForFunction(() => document.querySelector("#viewport").dataset.preview === "gpu-bake"
        && JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "{}").state === "baked", {}, { timeout: 900000 });
      await waitForFrames(page, 3);
      return gpu ? json("sceneBake") : null;
    };
    const shot = async name => {
      await page.evaluate(() => { const box = document.querySelector("#item-handles"); if (box?.checked) { box.checked = false; box.dispatchEvent(new Event("change")); } });
      await waitForFrames(page, 3);
      const bytes = await page.locator("#viewport canvas").screenshot();
      await writeFile(resolve(shots, `${name}.png`), bytes);
      return PNG.sync.read(bytes);
    };
    await page.goto(`http://127.0.0.1:${server.address().port}/material.html?${query}`);
    await ready();
    const began = Date.now();
    await page.locator("#open-project").setInputFiles({ name: "washroom-items-occlusion.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(candidate)) });
    await page.waitForFunction(() => document.querySelector("#viewport").dataset.items !== undefined, {}, { timeout: 900000 });
    const status = await baked();
    const ms = Date.now() - began;
    const clientWidth = await page.evaluate(() => document.querySelector("#viewport canvas").clientWidth);
    const project2d = points => page.evaluate(points => {
      const scene = window.__scene, canvas = document.querySelector("#viewport canvas"), three = window.__three;
      return points.map(point => {
        const at = new three.Vector3(...point).project(scene.camera);
        return [(at.x + 1) / 2 * canvas.clientWidth, (1 - at.y) / 2 * canvas.clientHeight];
      });
    }, points);
    // The open floor of the slab (top at z 0) between the walls, in front of the tall wall.
    const floor = await project2d([[0.1, -2.2, 0.001], [2.3, -2.2, 0.001], [2.3, -0.05, 0.001], [0.1, -0.05, 0.001]]);
    return { page, errors, json, baked, shot, ms, status, clientWidth, floor };
  };

  const query = process.env.QUERY ?? "ao=off&gpuprobe&shadows=off";
  // The shipped slab, for the moss the planes add.
  const before = await open(plain, query);
  const plainShot = await before.shot(`slab-plain-${resolution}`);
  const plainMoss = mossy(plainShot, before.floor, before.clientWidth);
  assert.equal(await before.json("occlusion"), null, "a project none of whose materials reads a plane bakes none");
  assert.equal(before.errors.length, 0, before.errors.join("\n"));
  await before.page.close();

  // 1. The GPU preview.
  const gpu = await open(project, query);
  const first = await gpu.json("occlusion");
  console.log(`GPU preview at ${resolution} px opened and baked in ${gpu.ms} ms; occluder service: ${JSON.stringify(first)}`);
  assert.ok(first, "the occluder service ran for the slab's material");
  for (const name of ["shadow", "occlusion", "sunExposure"]) assert.ok(first.baked[name] > 0, `${name} was baked`);
  assert.ok(first.meshes.length > 0 && first.meshes.every(mesh => mesh.startsWith("slab/")), `only the slab's receivers read planes: ${first.meshes.join(", ")}`);
  assert.deepEqual(gpu.status.unsupported, {}, "every receiver shows GPU-baked maps");
  const gpuShot = await gpu.shot(`slab-moss-gpu-${resolution}`);
  const gpuMoss = mossy(gpuShot, gpu.floor, gpu.clientWidth);
  console.log(`Moss on the open floor: ${gpuMoss.green} of ${gpuMoss.pixels} pixels green (${(gpuMoss.share * 100).toFixed(1)} %), shipped slab ${plainMoss.green} (${(plainMoss.share * 100).toFixed(1)} %)`);
  assert.ok(gpuMoss.share > 0.01 && plainMoss.share < 0.002, "moss grows where the planes say, and only with them");

  // 3. A moved sun: `shadow` alone is baked again, and only what reads it is drawn again.
  const run = first.run;
  const sunMoved = Date.now();
  await gpu.page.evaluate(() => {
    const input = document.querySelector("#lighting-azimuth");
    input.value = String((Number(input.value) + 70) % 360);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await gpu.page.waitForFunction(run => JSON.parse(document.querySelector("#viewport").dataset.occlusion ?? "{}").run > run, run, { timeout: 900000 });
  const moved = await gpu.json("occlusion");
  const rebaked = await gpu.baked();
  const sunMs = Date.now() - sunMoved;
  const drawn = rebaked.details.map(line => line.slice(0, line.indexOf(":")));
  console.log(`Sun moved 70 degrees: settled in ${sunMs} ms; occluder service ${JSON.stringify(moved)}; bakes drawn ${rebaked.drawn}, reused ${rebaked.reused} (nodes ${rebaked.nodesDrawn} drawn, ${rebaked.nodesReused} reused); drawn: ${drawn.join(", ")}`);
  assert.deepEqual(Object.keys(moved.baked), ["shadow"], "a moved sun bakes the shadow plane alone");
  assert.ok(moved.held.occlusion > 0 && moved.held.sunExposure > 0, "occlusion and sun exposure are held by key");
  assert.ok(drawn.length > 0 && drawn.every(mesh => mesh.startsWith("slab/")), `only the meshes reading shadow are baked again: ${drawn.join(", ")}`);
  const sunShot = await gpu.shot(`slab-moss-gpu-sun-${resolution}`);
  const shadowMoved = differing(gpuShot, sunShot, gpu.floor, gpu.clientWidth, 4);
  console.log(`The floor after the sun moved: ${shadowMoved.changed} of ${shadowMoved.pixels} pixels differ by more than 4/255 (mean ${shadowMoved.mean}/255, largest ${shadowMoved.worst}/255)`);
  assert.ok(shadowMoved.changed > 100, "the shadow darkening moved with the sun");
  // Planes are the page's, not a worker's: a CPU build on the build worker (a PNG export) finds every plane the GPU
  // preview lane baked and casts no ray.
  const exported = moved.run;
  await gpu.page.evaluate(() => { const box = document.querySelector("#export-ktx2"); box.checked = false; document.querySelector("#export-glb").click(); });
  await gpu.page.waitForFunction(run => JSON.parse(document.querySelector("#viewport").dataset.occlusion ?? "{}").run > run, exported, { timeout: 900000 });
  const reused = await gpu.json("occlusion");
  console.log(`Export on the build worker: occluder service ${JSON.stringify(reused)}`);
  assert.deepEqual(reused.baked, {}, "the export bakes no plane another worker of the page baked");
  for (const name of ["shadow", "occlusion", "sunExposure"]) assert.equal(reused.held[name], moved.baked[name] ?? moved.held[name], `the export holds ${name}`);
  assert.deepEqual(reused.rays, {}, "and casts no ray");
  assert.equal(gpu.errors.length, 0, gpu.errors.join("\n"));
  await gpu.page.close();

  // 2. The CPU bake of the same project, with the shipped sun.
  const cpu = await open(project, `${query}&preview=bake`);
  const cpuShot = await cpu.shot(`slab-moss-cpu-${resolution}`);
  const cpuMoss = mossy(cpuShot, cpu.floor, cpu.clientWidth);
  const close = differing(gpuShot, cpuShot, gpu.floor, gpu.clientWidth, 16);
  const opened = await cpu.json("occlusion");
  console.log(`CPU bake: occluder service ${JSON.stringify(opened)}; moss ${cpuMoss.green} of ${cpuMoss.pixels} pixels; GPU against CPU on the floor: ${close.changed} of ${close.pixels} pixels differ by more than 16/255 (${(close.share * 100).toFixed(2)} %, mean ${close.mean}/255, largest ${close.worst}/255)`);
  assert.ok(cpuMoss.share > 0.01, "the CPU bake grows the same moss");
  assert.ok(close.share < 0.05, "the GPU preview of the planes is the CPU bake's picture");
  // Opening builds a quick 64 px draft and then the full size: each atlas's planes once, the full ones in the second run.
  assert.equal(opened.run, resolution >= 128 ? 2 : 1, "the draft's planes and the full planes, each baked by one run");
  assert.deepEqual(opened.held, {}, "a fresh page holds no plane");
  assert.deepEqual(opened.baked, first.baked, "the full-size planes are baked once");
  // A moved sun on the CPU-baked page builds again: `shadow` alone is baked, the other planes are found by key.
  const cpuSun = Date.now();
  await cpu.page.evaluate(() => {
    const input = document.querySelector("#lighting-azimuth");
    input.value = String((Number(input.value) + 70) % 360);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await cpu.page.waitForFunction(run => JSON.parse(document.querySelector("#viewport").dataset.occlusion ?? "{}").run >= run, opened.run + (resolution >= 128 ? 2 : 1), { timeout: 900000 });
  await cpu.baked();
  const cpuMoved = await cpu.json("occlusion");
  console.log(`CPU bake, sun moved 70 degrees: settled in ${Date.now() - cpuSun} ms; occluder service ${JSON.stringify(cpuMoved)}`);
  assert.deepEqual(Object.keys(cpuMoved.baked), ["shadow"], "a moved sun bakes the shadow plane alone");
  assert.deepEqual(cpuMoved.held, { occlusion: opened.baked.occlusion, sunExposure: opened.baked.sunExposure }, "occlusion and sun exposure are held by key");
  const cpuSunShot = await cpu.shot(`slab-moss-cpu-sun-${resolution}`);
  assert.ok(differing(cpuShot, cpuSunShot, cpu.floor, cpu.clientWidth, 4).changed > 100, "the CPU bake's shadow darkening moved with the sun");
  assert.equal(cpu.errors.length, 0, cpu.errors.join("\n"));
  await cpu.page.close();
  console.log(`Occlusion planes in the Studio passed at ${resolution} px.`);
} finally {
  await browser?.close();
  server.close();
}
