/**
 * Instanced batches end to end in a browser: packets and GLBs the Rust exporter wrote are shown by the viewer as
 * instanced meshes (bounds, culling, shadows, per-instance attributes, picking) and loaded by three.js's stock
 * GLTFLoader (`EXT_mesh_gpu_instancing` and the one-node-per-instance fallback). Also the viewer's time-driven mode.
 *
 * The inputs are the fixtures of `crates/export/tests/batches.rs`; write them first:
 *   BATCH_FIXTURE_OUTPUT=$PWD/generated/batches cargo test -p superspec-export --test batches
 * SCREENSHOT names a PNG of the viewport.
 *
 * With a project and the native tool's output for it beside the fixtures, the same project is delivered through the
 * production Wasm module and must give the same bytes (the washroom, whose bolts are an instanced batch):
 *   node tests/studio/washroom/build.mjs 64 128 && cp generated/washroom/washroom.studio.json generated/batches/
 *   superspec-studio scene generated/batches/washroom.studio.json > generated/batches/washroom.lsp3
 *   superspec-studio glb generated/batches/washroom.studio.json --textures png > generated/batches/washroom.glb
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, writeFile, copyFile, cp } from "node:fs/promises";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { resolve, sep, extname } from "node:path";
import { build } from "vite";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";

const fixtures = resolve(process.env.BATCH_FIXTURES ?? "generated/batches");
const root = resolve("generated/batch-scene-browser");
const names = ["batches.lsp3", "batches.lsu1", "batches.glb", "batches-nodes.glb"];
const inputs = {};
for (const name of names) {
  try { inputs[name] = await readFile(resolve(fixtures, name)); }
  catch { throw new Error(`Missing ${name} in ${fixtures}: run the export crate's batch tests with BATCH_FIXTURE_OUTPUT set to that directory.`); }
}
await build({ configFile: false, root: "tests/browser", publicDir: false, logLevel: "warn",
  build: { outDir: root, emptyOutDir: true, rollupOptions: { input: resolve("tests/browser/rust-batches.html") } } });
for (const name of names) await copyFile(resolve(fixtures, name), resolve(root, name));
// The native tool's delivery of a project, when it was made (see above).
const delivered = ["washroom.studio.json", "washroom.lsp3", "washroom.glb"];
const parity = delivered.every(name => existsSync(resolve(fixtures, name)));
if (parity) {
  for (const name of delivered) await copyFile(resolve(fixtures, name), resolve(root, name));
  for (const file of ["superspec_wasm.js", "superspec_wasm_bg.wasm"]) await copyFile(resolve("generated/wasm", file), resolve(root, file));
  await cp(resolve("generated/wasm/snippets"), resolve(root, "snippets"), { recursive: true });
}
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "rust-batches.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".glb": "model/gltf-binary", ".wasm": "application/wasm", ".json": "application/json" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

/** The placements `crates/export/tests/batches.rs` authors, written out here independently of any exporter. */
const half = Math.SQRT1_2;
const authored = [
  { translation: [1, 2, 3], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  { translation: [-4, 0.5, 0], rotation: [0, 0, half, half], scale: [2, 2, 0.5] },
  { translation: [0, 8, 1.25], rotation: [half, 0, 0, half], scale: [1, 1, 1] },
];
/** A point of the mesh as a placement puts it (Z-up): scale, turn by the quaternion, move. */
function place({ translation, rotation: [x, y, z, w], scale }, point) {
  const p = point.map((value, axis) => value * scale[axis]);
  const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  const q = [x, y, z], t = cross(q, p).map(value => value * 2), u = cross(q, t);
  return p.map((value, axis) => value + w * t[axis] + u[axis] + translation[axis]);
}
const near = (found, expected, message, tolerance = 1e-5) => {
  assert.equal(found.length, expected.length, message);
  found.forEach((value, index) => assert.ok(Math.abs(value - expected[index]) <= tolerance, `${message}: ${found} is not ${expected}`));
};

let browser;
try {
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 960, height: 640 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => window.driver);

  // The viewer: one instanced mesh for the batch, beside the ordinary mesh.
  for (const [name, update] of [["batches.lsp3", false], ["batches.lsu1", true]]) {
    const drawn = await page.evaluate(([bytes, update]) => window.driver.apply(bytes, update), [[...inputs[name]], update]);
    assert.deepEqual(drawn.map(mesh => [mesh.name, mesh.instanced, mesh.count]), [["single", false, 1], ["bolts", true, 3]], name);
    const batch = drawn[1];
    assert.equal(batch.vertices, 3, "the batch's mesh is uploaded once");
    assert.ok(batch.castShadow && batch.receiveShadow && batch.frustumCulled, "a batch casts and receives shadows and is culled by its own bounds");
    // The triangle (0,0,0) (1,0,0) (0,1,0) under each authored placement gives the bounds, derived here by hand.
    const corners = authored.flatMap(placement => [[0, 0, 0], [1, 0, 0], [0, 1, 0], [1, 1, 0]].map(point => place(placement, point)));
    const expected = [0, 1, 2].map(axis => Math.min(...corners.map(point => point[axis]))).concat([0, 1, 2].map(axis => Math.max(...corners.map(point => point[axis]))));
    near(batch.bounds.flat(), expected, `${name}: the instanced mesh's bounds cover every instance`);
    near(batch.tints, [1, 1, 1, 0.5, 0.25, 0.125, 2, 1, 1], "tints are instance colors");
    near(batch.values, [0.25, 0, 0, 1, 0.5, 1, 0, 1, 0.75, 2, 0, 1], "instance values are an instanced attribute");
    assert.equal(batch.valuesInstanced, true);
    assert.equal(batch.roughness, 0.5, "every instance shows the batch's one material");
    // The second instance's matrix: a quarter turn about Z, scaled (2, 2, 0.5), at (-4, 0.5, 0).
    near(batch.matrix, [0, 2, 0, 0, -2, 0, 0, 0, 0, 0, 0.5, 0, -4, 0.5, 0, 1], "placements become instance matrices");
  }
  // Culling uses the bounds of every instance: a camera that sees only the far instance still draws the batch.
  assert.equal(await page.evaluate(() => window.driver.culled("bolts", [0, 20, 1.5], [0, 8, 1.5])), false, "the far instance alone keeps the batch drawn");
  assert.equal(await page.evaluate(() => window.driver.culled("bolts", [0, 20, 1.5], [0, 40, 1.5])), true, "a camera looking away culls it");
  // Picking reports the batch and the instance's index.
  for (const index of [0, 1, 2]) {
    const pick = await page.evaluate(index => window.driver.pick("bolts", index), index);
    assert.deepEqual(pick.reported, { id: "bolts", instance: index }, `picking instance ${index}`);
    assert.deepEqual(pick.dataset, { id: "bolts", instance: index });
  }
  await waitForFrames(page);
  const rendering = await page.evaluate(() => window.driver.rendering());
  assert.equal(rendering.shadows, true);
  assert.equal(rendering.shadowStyle, true, "the batch is drawn with the illustration shadow style");
  const frame = await page.evaluate(() => window.driver.frame());
  console.log(`Batch frame: ${JSON.stringify(frame)}, rendering ${JSON.stringify(rendering)}`);
  if (process.env.SCREENSHOT) await writeFile(process.env.SCREENSHOT, await page.locator("#viewport").screenshot());

  // The time-driven mode: off until asked for, advancing a clock while on, still while the page is hidden.
  assert.equal(await page.evaluate(() => window.driver.time()), 0, "the viewer renders on demand by default");
  const off = await page.evaluate(() => window.driver.animate(false, 300));
  assert.equal(off.after, 0); assert.equal(off.budget, null);
  const on = await page.evaluate(() => window.driver.animate(true, 1300));
  assert.equal(on.continuous, "true");
  assert.ok(on.after > 0.5 && on.after < 1.6, `about a second passed: ${on.after}`);
  assert.ok(on.budget && on.budget.frames > 5 && on.budget.intervalMs > 0 && on.budget.calls > 0, JSON.stringify(on.budget));
  assert.ok(/ms\/frame/.test(on.readout), "the frame budget is shown in the viewport");
  console.log(`Frame budget: ${JSON.stringify(on.budget)}`);
  assert.equal(await page.evaluate(() => window.driver.hide(true)), "true");
  const hidden = await page.evaluate(async () => { const before = window.driver.time(); await new Promise(done => setTimeout(done, 400)); return [before, window.driver.time()]; });
  assert.equal(hidden[0], hidden[1], "time stands still while the page is hidden");
  assert.equal(await page.evaluate(() => window.driver.hide(false)), "false");
  const resumed = await page.evaluate(async () => { const before = window.driver.time(); await new Promise(done => setTimeout(done, 400)); return [before, window.driver.time()]; });
  assert.ok(resumed[1] > resumed[0] && resumed[1] - resumed[0] < 0.6, `time resumes without a jump: ${resumed}`);
  const stopped = await page.evaluate(() => window.driver.animate(false, 300));
  assert.equal(stopped.before, stopped.after, "turned off, the viewer renders on demand again");
  assert.equal(stopped.readout, null);

  // three.js's stock loader reads the exported batch as an instanced mesh. glTF is Y-up: (x, y, z) becomes (x, z, -y).
  const upright = ([x, y, z]) => [x, z, -y];
  const instanced = await page.evaluate(() => window.driver.load("/batches.glb"));
  assert.equal(instanced.issues.numErrors, 0, JSON.stringify(instanced.issues.messages));
  assert.ok(instanced.used.includes("EXT_mesh_gpu_instancing"));
  const loaded = instanced.meshes.find(mesh => mesh.instanced);
  assert.ok(loaded, "the batch loads as a three.js InstancedMesh");
  assert.equal(loaded.count, 3);
  assert.equal(instanced.meshes.length, 2, "one instanced mesh and the ordinary one");
  loaded.placed.forEach((point, index) => near(point, upright(place(authored[index], [1, 0, 0])), `loaded instance ${index}`));
  near(loaded.instance, [0.25, 0, 0, 1, 0.5, 1, 0, 1, 0.75, 2, 0, 1], "_INSTANCE survives the round trip");
  near(loaded.tint, [1, 1, 1, 0.5, 0.25, 0.125, 2, 1, 1], "_TINT survives the round trip");
  assert.equal(loaded.material, "steel");
  // The fallback: plain nodes, one per instance, on one geometry.
  const nodes = await page.evaluate(() => window.driver.load("/batches-nodes.glb"));
  assert.equal(nodes.issues.numErrors, 0, JSON.stringify(nodes.issues.messages));
  assert.equal(nodes.issues.numWarnings, 0, JSON.stringify(nodes.issues.messages));
  assert.deepEqual(nodes.used, []);
  const copies = nodes.meshes.filter(mesh => mesh.extras.batchId === "bolts");
  assert.equal(copies.length, 3);
  assert.ok(copies.every(mesh => !mesh.instanced && mesh.geometry === copies[0].geometry), "the instances share one geometry");
  copies.forEach((mesh, index) => {
    assert.equal(mesh.extras.instance, index);
    near(mesh.placed[0], upright(place(authored[index], [1, 0, 0])), `fallback instance ${index}`);
  });
  near(copies[1].extras.tint, [0.5, 0.25, 0.125], "the fallback keeps tints in the node's extras");
  // The native tool and the production Wasm module deliver the same project to the same bytes, batches included.
  if (parity) {
    const sha = async name => createHash("sha256").update(await readFile(resolve(fixtures, name))).digest("hex");
    const wasm = await page.evaluate(() => window.driver.deliver("/washroom.studio.json"));
    assert.equal(wasm.batches.length, 1, "the delivered scene holds the project's batch");
    assert.equal(wasm.batches[0].collision, "none");
    assert.ok(wasm.batches[0].material, "the batch has its template's baked material");
    assert.ok(!wasm.meshes.includes(wasm.batches[0].template), "the template mesh is not delivered as a mesh");
    assert.equal(wasm.packet, await sha("washroom.lsp3"), "native and Wasm scene packets with batches are byte-identical");
    assert.equal(wasm.glb, await sha("washroom.glb"), "native and Wasm GLB exports with batches are byte-identical");
    // And the exported project loads in the stock loader: the bolts as one instanced mesh with baked maps.
    const exported = await page.evaluate(() => window.driver.load("/washroom.glb"));
    assert.equal(exported.issues.numErrors, 0, JSON.stringify(exported.issues.messages));
    const bolts = exported.meshes.find(mesh => mesh.instanced);
    assert.equal(bolts?.count, wasm.batches[0].instanceCount);
    assert.equal(exported.meshes.filter(mesh => !mesh.instanced).length, wasm.meshes.length);
    console.log(`Native and Wasm deliver the washroom with its batch identically: packet ${wasm.packetBytes} bytes, GLB ${wasm.glbBytes} bytes, ${bolts.count} instances of ${wasm.batches[0].template}.`);
  } else console.log("Native/Wasm delivery of a project with batches was not compared: no native output beside the fixtures.");
  assert.deepEqual(errors, []);
  console.log(`Instanced batches render in the viewer and load in three.js's GLTFLoader (validator: ${instanced.issues.numWarnings} warnings, ${instanced.issues.numInfos} infos with the extension).`);
} finally {
  await browser?.close();
  await new Promise(done => server.close(done));
}
