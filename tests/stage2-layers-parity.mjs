/**
 * Native and Wasm agree on layered and anchored slots: builds, in one Wasm session, the sequence of
 * projects the native engine wrote (`LAYERS_PARITY_OUTPUT=<dir> cargo test --release -p superspec-engine
 * --features authoring --test material_layers native_layered_packets_for_parity`): a cold stack of the split
 * Kowloon concrete on the washroom wall item, a base edit, a layer edit, a layer added and one removed, the wall
 * widened, and the anchored base alone at two widths. Each delivered packet must be the native one byte for byte.
 * Usage: node tests/stage2-layers-parity.mjs <dir> [wasm dir, default generated/wasm]
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const directory = resolve(process.argv[2] ?? "generated/verification/stage2/layers-parity");
const wasm = resolve(process.argv[3] ?? "generated/wasm");
const { initSync, Authoring } = await import(pathToFileURL(resolve(wasm, "superspec_wasm.js")).href);
initSync({ module: readFileSync(resolve(wasm, "superspec_wasm_bg.wasm")) });
const index = JSON.parse(readFileSync(resolve(directory, "index.json"), "utf8"));
const session = new Authoring();
for (const { name, file } of index.assets) session.set_asset(name, readFileSync(resolve(directory, file)));
let revision = 0;
for (const step of index.steps) {
  revision++;
  const started = performance.now();
  const reply = JSON.parse(session.preview_project(readFileSync(resolve(directory, `${step}.json`), "utf8"), revision));
  const elapsed = performance.now() - started;
  assert.equal(reply.success, true, `${step}: ${JSON.stringify(reply.diagnostics)}`);
  const packet = Buffer.from(session.packet()), native = readFileSync(resolve(directory, `${step}.bin`));
  assert.equal(packet.length, native.length, `${step}: the packets have the same length`);
  assert.ok(packet.equals(native), `${step}: the packets are the same bytes`);
  console.log(`${step}: ${native.length} bytes identical natively and in Wasm (Wasm build ${elapsed.toFixed(0)} ms)`);
}
// The same sequence shared with helper sessions, as the browser's bake helpers share it (`distribute_bakes`): each
// helper bakes its rows of every layered stack, keeping what it made for its rows of the next step.
const shares = Number(process.env.SHARES ?? 3);
const distributed = new Authoring(), helpers = Array.from({ length: shares }, () => new Authoring());
const assets = Object.fromEntries(index.assets.map(({ name, file }) => [name, readFileSync(resolve(directory, file))]));
for (const [name, bytes] of Object.entries(assets)) distributed.set_asset(name, bytes);
revision = 0;
let shared = 0;
for (const step of index.steps) {
  revision++;
  const source = readFileSync(resolve(directory, `${step}.json`), "utf8");
  distributed.distribute_bakes(true);
  let reply;
  // As the authoring worker does: a build reports every large bake, the helpers bake them all, and it builds again.
  for (let round = 0; ; round++) {
    reply = JSON.parse(distributed.preview_project(source, revision));
    const pending = [];
    for (let bake = distributed.take_pending_bake(); bake; bake = distributed.take_pending_bake()) pending.push(bake);
    if (!pending.length) break;
    assert.ok(round < 2, `${step}: the shares are taken`);
    for (const bake of pending) {
      const described = JSON.parse(bake);
      shared += described.request.layers?.length ? 1 : 0;
      for (const [at, helper] of helpers.entries()) {
        helper.clear_assets();
        for (const name of [...described.images, ...(described.planes ?? [])]) helper.set_asset(name, assets[name]);
        distributed.provide_bake_share(helper.bake_share(bake, at, shares));
      }
    }
  }
  distributed.distribute_bakes(false);
  assert.equal(reply.success, true, `${step} (distributed): ${JSON.stringify(reply.diagnostics)}`);
  assert.ok(Buffer.from(distributed.packet()).equals(readFileSync(resolve(directory, `${step}.bin`))), `${step}: the distributed packet is the same bytes`);
}
assert.ok(shared > 0, "layered stacks were shared with helpers");
console.log(`Layered and anchored parity passed: ${index.steps.length} packets, and ${index.steps.length} again with ${shared} layered bakes shared across ${shares} helper sessions.`);
