/**
 * Native and Wasm agree on an item scene: builds the project the native engine wrote
 * (`ITEM_SCENE_PARITY_OUTPUT=<dir> cargo test -p superspec-engine --test item_scene native_item_scene_packet_for_parity`)
 * with the Wasm module in `generated/wasm` and compares the delivered packets byte for byte.
 * Usage: node tests/item-scene-parity.mjs <dir>
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { initSync, Authoring } from "../generated/wasm/superspec_wasm.js";

const directory = resolve(process.argv[2] ?? "generated/item-scene-parity");
initSync({ module: readFileSync(resolve("generated/wasm/superspec_wasm_bg.wasm")) });
const session = new Authoring();
session.set_asset("notice.png", readFileSync(resolve(directory, "notice.png")));
const project = readFileSync(resolve(directory, "project.json"), "utf8");
const reply = JSON.parse(session.preview_project(project, 1));
assert.equal(reply.success, true, JSON.stringify(reply.diagnostics));
const wasm = Buffer.from(session.packet()), native = readFileSync(resolve(directory, "packet.bin"));
assert.equal(wasm.length, native.length, "the packets have the same length");
assert.ok(wasm.equals(native), "the packets are the same bytes");
// The same holds after an edit that regenerates one part: the session keeps the other part's meshes.
const edited = JSON.parse(project);
edited.models[0].parameters.width = 3.4;
const second = JSON.parse(session.preview_project(JSON.stringify(edited), 2));
assert.equal(second.success, true, JSON.stringify(second.diagnostics));
assert.deepEqual(second.metadata.items.wall.rebuilt, ["wall"]);
console.log(`Item scene parity passed: ${native.length} bytes identical natively and in Wasm; a width edit regenerated ${second.metadata.items.wall.rebuilt.join(", ")} only.`);
