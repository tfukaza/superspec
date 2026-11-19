/**
 * Native and Wasm agree on an item's weathering zones: builds the zoned wall the native engine wrote
 * (`ZONE_PARITY_OUTPUT=<dir> cargo test -p superspec-engine --test item_zones native_zone_scene_packet_for_parity`)
 * with the Wasm module in `generated/wasm` and compares the delivered packets, baked textures included, byte for byte.
 * Usage: node tests/item-zone-parity.mjs <dir>
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { initSync, Authoring } from "../generated/wasm/superspec_wasm.js";

const directory = resolve(process.argv[2] ?? "generated/item-zone-parity");
initSync({ module: readFileSync(resolve("generated/wasm/superspec_wasm_bg.wasm")) });
const session = new Authoring();
const reply = JSON.parse(session.preview_project(readFileSync(resolve(directory, "project.json"), "utf8"), 1));
assert.equal(reply.success, true, JSON.stringify(reply.diagnostics));
const wasm = Buffer.from(session.packet()), native = readFileSync(resolve(directory, "packet.bin"));
assert.equal(wasm.length, native.length, "the packets have the same length");
assert.ok(wasm.equals(native), "the packets are the same bytes");
console.log(`Item zone parity passed: ${native.length} bytes identical natively and in Wasm.`);
