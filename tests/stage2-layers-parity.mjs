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
console.log(`Layered and anchored parity passed: ${index.steps.length} packets.`);
