import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { createInterface } from "node:readline";
import { resolve } from "node:path";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { writeArchive } from "../editor/studio/archive.ts";
import { buildProject } from "./studio/washroom/build.mjs";
import { SceneUpdates } from "../browser/model/updates.ts";
import { decodePacket } from "../browser/model/packet.ts";
import { decodeNativeReport } from "../browser/runtime/native-report.ts";
import { NativeSession } from "../browser/runtime/native-session.ts";

test("a superseded, cancelled or closed native request aborts a partially read body", async t => {
  const signals = [];
  const controls = [];
  t.mock.method(globalThis, "fetch", async (path, options) => {
    if (path === "/api/session") return Response.json({ session: "test" });
    if (path.endsWith("/complete")) return new Response(Uint8Array.of(17));
    if (!path.endsWith("/scene")) { controls.push(path); return new Response(null, { status: 204 }); }
    signals.push(options.signal);
    return new Response(new ReadableStream({ start(controller) { controller.enqueue(Uint8Array.of(42)); } }),
      { headers: { "X-Revision": options.headers["X-Revision"] } });
  });
  const session = new NativeSession();
  const opened = async () => {
    const response = await session.request("scene", new Blob());
    const reader = response.body.getReader();
    assert.deepEqual((await reader.read()).value, Uint8Array.of(42));
    return reader;
  };
  for (const stop of ["cancel", "close"]) {
    const old = await opened(), superseded = assert.rejects(old.read(), { name: "AbortError" });
    const current = await opened();
    await superseded;
    const cancelled = assert.rejects(current.read(), { name: "AbortError" });
    session[stop]();
    await cancelled;
    assert.ok(signals.every(signal => signal.aborted));
    assert.ok(controls.at(-1).endsWith("/" + stop));
  }
  const response = await session.request("complete", new Blob());
  assert.deepEqual(new Uint8Array(await response.arrayBuffer()), Uint8Array.of(17));
  session.cancel();
  await Promise.resolve();
  assert.equal(controls.length, 2);
});

test("native chart deltas preserve float bits and reject incomplete baselines and masks", () => {
  const mesh = { id: "wall", material: "paint", width: 8, height: 8, density: 4, triangles: 1, offset: 0 };
  const make = (key, words, base) => {
    const header = {
      packetOffset: null, metadata: { materials: {}, shading: {} },
      layouts: { record: 38, gutter: 1, receivers: [mesh] }, layoutCount: 38, layoutOffset: 0,
      shadingBlocks: { version: 1, materials: {}, features: {}, charts: [["wall", "paint", key], ["copy", "paint", key]] },
      chartRecordSpans: [base === undefined ? [0, words.length * 4] : [0, words.length * 4, base]],
    };
    const json = new TextEncoder().encode(JSON.stringify(header));
    const offset = Math.ceil((4 + json.length) / 8) * 8;
    const out = new ArrayBuffer(offset + words.length * 4);
    new DataView(out).setUint32(0, offset - 4, true);
    new Uint8Array(out, 4, offset - 4).fill(32).set(json);
    new Uint32Array(out, offset).set(words);
    return out;
  };
  let held;
  const words = new Uint32Array(38);
  const before = decodeNativeReport(make("old", words), 1, undefined, next => { held = next; });
  const expected = words.slice();
  expected[0] = 0x80000000; expected[31] = 0xbf800000; expected[37] = 0x3f800000;
  const patch = [0x80000001, expected[0], expected[31], 32, expected[37]];
  const result = decodeNativeReport(make("new", patch, "old"), 2, held);
  assert.deepEqual(new Uint32Array(result.layouts.records.buffer), Uint32Array.from([...expected, ...expected]));
  assert.deepEqual(new Uint32Array(before.layouts.records.buffer), new Uint32Array(76));
  assert.throws(() => decodeNativeReport(make("new", patch, "unheld"), 3, held), /Missing retained/);
  for (const malformed of [[1], [0, 64, 1], [0, 0, 1]]) {
    assert.throws(() => decodeNativeReport(make("new", malformed, "old"), 3, held), /native chart delta/);
  }
  assert.deepEqual(decodeNativeReport(make("new", expected), 4).layouts, result.layouts);
});

test("a native Studio session publishes the latest valid washroom and recovers after invalid input", { timeout: 120000 }, async t => {
  const command = process.env.RUST_STUDIO ?? resolve("target/release/superspec-studio" + (process.platform === "win32" ? ".exe" : ""));
  const child = spawn(command, ["serve", "--ui", resolve("generated/native-viewer"), "--artifacts", resolve("tests/fixtures/aml")], { stdio: ["ignore", "ignore", "pipe"] });
  const exited = new Promise(resolve => child.once("exit", resolve));
  t.after(async () => { child.kill(); if (child.pid) await exited; });
  const address = await new Promise((resolve, reject) => {
    const lines = createInterface({ input: child.stderr });
    lines.on("line", line => { const match = line.match(/listening at (http:\/\/[^/]+)/); if (match) resolve(match[1]); });
    child.once("error", reject);
    child.once("exit", code => reject(Error("Native Studio exited with status " + code)));
  });
  const headers = { "X-SuperSpec": "1" };
  const artifact = await fetch(address + "/artifacts/terrain-course/hill.png");
  assert.equal(artifact.headers.get("content-type"), "image/png");
  assert.deepEqual(Buffer.from(await artifact.arrayBuffer()), await readFile("tests/fixtures/aml/terrain-course/hill.png"));
  assert.equal((await fetch(address + "/artifacts/rust-stage1.aml")).status, 404);
  const open = await fetch(address + "/api/session", { method: "POST", headers });
  assert.equal(open.status, 201);
  const { session } = await open.json();
  const path = address + "/api/session/" + session + "/";
  const post = (action, revision, body, extra = {}) => fetch(path + action, {
    method: "POST", headers: { ...headers, "X-Revision": String(revision), ...extra }, body,
  });
  const bytes = async response => { assert.equal(response.status, 200); return Buffer.from(await response.arrayBuffer()); };
  const project = JSON.stringify(buildProject(64, 128).project);
  const packet = await bytes(await post("scene", 1, project));
  const manifest = JSON.parse(packet.toString("utf8", 8, 8 + packet.readUInt32LE(4)));
  assert.equal(manifest.revision, 1);
  assert.ok(manifest.success && manifest.meshes.length > 4);
  assert.ok(manifest.batches.length > 0);
  assert.equal((await post("scene", 1, project)).status, 409);
  assert.equal((await post("scene", 2, "{}")).status, 422);
  assert.deepEqual(await bytes(await fetch(path + "snapshot", { headers })), packet);
  assert.equal((await post("check", 3, project)).status, 200);
  const exported = await bytes(await post("glb-png", 4, project));
  assert.equal(exported.toString("ascii", 0, 4), "glTF");
  assert.equal((await post("cancel", 6)).status, 204);
  assert.equal((await post("scene", 5, project)).status, 409);
  const tree = await bytes(await post("scene", 7, await readFile("crates/engine/src/treespec/fixtures/street.tree.json"), { "X-Input": "tree" }));
  const plant = JSON.parse(tree.toString("utf8", 8, 8 + tree.readUInt32LE(4)));
  assert.equal(plant.id, "street");
  assert.ok(plant.batches.length > 1);
  const source = await readFile("tests/fixtures/aml/rust-stage1.aml", "utf8");
  const document = { source, revision: 23 };
  const archive = writeArchive([{ name: "project.json", bytes: Buffer.from(JSON.stringify({ ...document, assets: {} })) }]);
  const world = await bytes(await post("aml-scene", 8, archive, { "X-Input": "archive" }));
  assert.equal(world.toString("ascii", 0, 4), "LSP3");
  const worldManifest = JSON.parse(world.toString("utf8", 8, 8 + world.readUInt32LE(4)));
  assert.equal(worldManifest.revision, 23);
  assert.equal(worldManifest.success, true);
  const offline = input => spawnSync(command, ["bake-input", "aml-scene"], {
    input, maxBuffer: 64 * 1024 * 1024, windowsHide: true,
  });
  const rejected = offline(Buffer.from("invalid archive"));
  assert.equal(rejected.status, 1);
  assert.equal(rejected.stdout.length, 0);
  assert.ok(JSON.parse(rejected.stderr).diagnostics.length);
  const baked = offline(Buffer.from(await archive.arrayBuffer()));
  assert.equal(baked.status, 0, baked.stderr.toString());
  assert.deepEqual(baked.stdout, world);
  assert.equal((await post("aml-validate", 9, JSON.stringify(document), { "X-Input": "reuse-8" })).status, 200);
  assert.equal((await post("aml-validate", 10, JSON.stringify(document), { "X-Input": "reuse-9" })).status, 412);
  const exportedWorld = await bytes(await post("aml-glb-png", 11, JSON.stringify(document), { "X-Input": "reuse-8" }));
  assert.equal(exportedWorld.toString("ascii", 0, 4), "glTF");
  const ktx = await bytes(await post("glb", 12, project));
  const gltf = JSON.parse(ktx.toString("utf8", 20, 20 + ktx.readUInt32LE(12)));
  assert.ok(gltf.extensionsUsed.includes("KHR_texture_basisu"));
  const washroom = JSON.parse(project);
  const companions = Object.entries(washroom.assets);
  washroom.assets = Object.fromEntries(companions.map(([name], i) => [name, "assets/" + i]));
  const shadingArchive = writeArchive([{ name: "project.json", bytes: Buffer.from(JSON.stringify(washroom)) },
    ...companions.map(([, value], i) => ({ name: "assets/" + i, bytes: Buffer.from(value, "base64") }))]);
  const full = await bytes(await post("shade-layouts", 13, shadingArchive, { "X-Input": "archive" }));
  const { assets: _paths, ...shadingInput } = washroom;
  const lean = await bytes(await post("shade-layouts", 14, JSON.stringify({ ...shadingInput, includeScenePacket: false }), { "X-Input": "reuse-13" }));
  const unpack = frame => {
    const end = 4 + frame.readUInt32LE(0);
    return { info: JSON.parse(frame.toString("utf8", 4, end)), data: frame.subarray(end) };
  };
  const normal = unpack(full), shadingOnly = unpack(lean);
  assert.ok(normal.data.length > normal.info.packetOffset && normal.info.layoutCount > 0);
  assert.deepEqual(shadingOnly.info, { ...normal.info, packetOffset: null });
  assert.deepEqual(shadingOnly.data, normal.data.subarray(0, normal.info.packetOffset));
  const update = async (serial, draft, mode) => {
    const body = mode === "archive" ? writeArchive([{ name: "project.json", bytes: Buffer.from(JSON.stringify({ ...draft, assets: {} })) }]) : JSON.stringify(draft);
    const data = await bytes(await post("aml-update", serial, body, { "X-Input": mode }));
    return { tag: data.toString("ascii", 0, 4), header: JSON.parse(data.toString("utf8", 8, 8 + data.readUInt32LE(4))), size: data.length, buffer: Uint8Array.from(data).buffer };
  };
  const first = await update(15, document, "archive");
  assert.equal(first.tag, "LSU1");
  assert.equal(first.header.baseRevision, null);
  const unknown = "<unrecognized/>";
  const badSource = source.replace("<box", "<!-- courtyard 🌿 -->" + unknown + "<box");
  const invalid = await update(16, { source: badSource, revision: 24, baseRevision: 23 }, "reuse-15");
  assert.equal(invalid.header.success, false);
  const [finding] = invalid.header.diagnostics;
  assert.equal(finding.code, "AML_CHILD");
  assert.equal(finding.source.offset, badSource.indexOf(unknown));
  assert.equal(badSource.slice(finding.source.offset, finding.source.end), unknown);
  const rename = { source: source.replace('id="rust-stage1"', 'id="court-revised"')
    .replace("<box", "<!-- courtyard 🌿 -->\n  <box"), revision: 25 };
  const edited = await update(17, { ...rename, baseRevision: 23 }, "reuse-15");
  assert.equal(edited.header.baseRevision, 23);
  assert.equal(edited.header.retainedScene, true);
  assert.ok(edited.header.meshes.every(Number.isInteger) && edited.size < first.size);
  const renamedScene = new SceneUpdates();
  renamedScene.decodePreview(first.buffer);
  assert.equal(renamedScene.decodePreview(invalid.buffer).manifest.success, false);
  assert.equal(renamedScene.revision, 23);
  const renamed = renamedScene.decodePreview(edited.buffer);
  const fresh = offline(Buffer.from(await writeArchive([{ name: "project.json", bytes: Buffer.from(JSON.stringify({ ...rename, assets: {} })) }]).arrayBuffer()));
  assert.equal(fresh.status, 0, fresh.stderr.toString());
  const independent = decodePacket(Uint8Array.from(fresh.stdout).buffer);
  for (const key of ["manifest", "positions", "normals", "colors", "uvs", "indices", "assets", "batches"])
    assert.deepEqual(renamed[key], independent[key], key);
  const recovered = await update(18, { source, revision: 26, baseRevision: 24 }, "reuse-15");
  assert.equal(recovered.header.baseRevision, null);
  assert.ok(recovered.header.meshes.every(id => id === null));
  const decoder = new SceneUpdates();
  decoder.decode(recovered.buffer);
  const moved = { source: source.replace('id="bench" position="1 2 0"', 'id="bench" position="2 2 0"'), revision: 27 };
  const delta = await update(19, { ...moved, baseRevision: 26 }, "reuse-15");
  assert.ok(delta.header.meshes.includes(null) && delta.header.meshes.some(Number.isInteger));
  const actual = decoder.decode(delta.buffer);
  const clean = offline(Buffer.from(await writeArchive([{ name: "project.json", bytes: Buffer.from(JSON.stringify({ ...moved, assets: {} })) }]).arrayBuffer()));
  assert.equal(clean.status, 0, clean.stderr.toString());
  const expected = decodePacket(Uint8Array.from(clean.stdout).buffer);
  for (const key of ["manifest", "positions", "normals", "colors", "uvs", "indices", "assets", "batches"])
    assert.deepEqual(actual[key], expected[key], key);
  const treeSource = '<architecture version="1" id="park"><city><tree id="elm" position="0 0 0"/></city></architecture>';
  const translated = treeSource.replace('position="0 0 0"', 'position="1 2 0"');
  const park = await update(20, { source: treeSource, revision: 28, batchSpans: true }, "reuse-15");
  const patch = await update(21, { source: translated, revision: 29, baseRevision: 28, batchSpans: true, foliageMassRefs: true }, "reuse-15");
  assert.equal(patch.header.batchSpans.length, 9);
  assert.ok(patch.header.foliageMassRefs.length > 0);
  for (const [index] of patch.header.foliageMassRefs) assert.equal(patch.header.manifest.batches[index].foliage.mass, undefined);
  assert.ok(patch.size < park.size);
  const trees = new SceneUpdates();
  trees.decode(park.buffer);
  const corrupt = structuredClone(patch.header);
  corrupt.foliageMassRefs[0][1] = park.header.manifest.batches.length;
  const message = Buffer.from(patch.buffer);
  const text = Buffer.from(JSON.stringify(corrupt));
  const headerLength = Math.ceil(text.length / 4) * 4;
  const frame = Buffer.alloc(8 + headerLength, 32);
  frame.write("LSU1");
  frame.writeUInt32LE(headerLength, 4);
  text.copy(frame, 8);
  const damaged = Buffer.concat([frame, message.subarray(8 + message.readUInt32LE(4))]);
  assert.throws(() => trees.decode(Uint8Array.from(damaged).buffer), /canopy/);
  assert.equal(trees.revision, 28);
  const movedTree = trees.decode(patch.buffer);
  const treeFull = await update(22, { source: translated, revision: 30 }, "reuse-15");
  assert.equal(treeFull.header.batchSpans, undefined);
  const cleanTree = new SceneUpdates().decode(treeFull.buffer);
  assert.equal(treeFull.header.foliageMassRefs, undefined);
  assert.deepEqual(movedTree.batches, cleanTree.batches);
  assert.deepEqual(movedTree.manifest, { ...cleanTree.manifest, revision: 29 });
  if (process.env.CHECK_WASM_PARITY === "1") {
    const wasm = await import("../generated/wasm/superspec_wasm.js");
    wasm.initSync({ module: await readFile("generated/wasm/superspec_wasm_bg.wasm") });
    const reference = new wasm.Compiler();
    try {
      assert.deepEqual(world, Buffer.from(reference.compile(source, document.revision)));
      const check = (frame, text, revision, base) => assert.deepEqual(Buffer.from(frame.buffer),
        Buffer.from(reference.preview_packet(text, revision, base)));
      check(first, source, 23);
      check(invalid, badSource, 24, 23);
      assert.throws(() => reference.compile_update_against(badSource, 24, 23));
      check(edited, rename.source, 25, 23);
      check(recovered, source, 26, 24);
      check(delta, moved.source, 27, 26);
      const localTrees = new SceneUpdates();
      localTrees.decode(reference.compile_update_against(treeSource, 28, undefined, true).buffer);
      const localTree = localTrees.decode(reference.compile_update_against(translated, 29, 28, true, true).buffer);
      const cleanLocalTree = decodePacket(reference.compile(translated, 29).buffer);
      assert.deepEqual(localTree.batches, cleanLocalTree.batches);
      assert.deepEqual(localTree.manifest, cleanLocalTree.manifest);
    } finally { reference.free(); }
  }
  const referenceFrame = await bytes(await post("shade-layouts", 23, shadingArchive, { "X-Input": "archive" }));
  const decodeReport = (frame, base, retain) => decodeNativeReport(Uint8Array.from(frame).buffer, 1, base, retain);
  const expectedLayout = decodeReport(referenceFrame);
  const compactScene = { ...shadingInput, includeScenePacket: false, heldShadingBlocks: [], packedFeatureCoords: true, chartRecordDeltas: true };
  const chartFrame = async (revision, input) => bytes(await post("shade-layouts", revision, JSON.stringify(input), { "X-Input": "reuse-23" }));
  const baseline = await chartFrame(24, compactScene);
  let held;
  const firstReport = decodeReport(baseline, undefined, next => { held = next; });
  assert.deepEqual(firstReport.metadata, expectedLayout.metadata);
  assert.deepEqual(firstReport.layouts, expectedLayout.layouts);
  const inventory = held.keys();
  assert.ok(inventory.some(key => key.startsWith("c")) && inventory.some(key => key.startsWith("m")));
  const deltaReport = await chartFrame(25, { ...compactScene, heldShadingBlocks: inventory });
  assert.ok(deltaReport.length < baseline.length / 3);
  assert.equal(unpack(deltaReport).info.layoutCount, 0);
  assert.throws(() => decodeReport(deltaReport), /Missing retained/);
  assert.deepEqual(decodeReport(deltaReport, held).layouts, expectedLayout.layouts);
  // One changed receiver and one unadvertised feature block must be sent alongside
  // resident blocks, even though all native compiler caches are warm.
  const simple = expectedLayout.layouts.receivers.filter(r => r.density > 1)
    .sort((a, b) => a.triangles - b.triangles)[0];
  assert.ok(simple);
  const resized = { ...compactScene, texelDensities: { [simple.id]: 1 } };
  const partialInventory = inventory.filter((key, i) => key.startsWith("c") || i % 3 !== 0);
  const partial = await chartFrame(26, { ...resized, heldShadingBlocks: partialInventory });
  const independentReport = await chartFrame(27, { ...resized, heldShadingBlocks: undefined });
  const recoveredReport = decodeReport(partial, held);
  const allData = decodeReport(independentReport);
  assert.ok(unpack(partial).info.layoutCount > 0 && partial.length < independentReport.length);
  assert.ok(unpack(partial).info.chartRecordSpans.some(span => span.length === 3), "changed chart sent against its held baseline");
  assert.deepEqual(recoveredReport.metadata, allData.metadata);
  assert.deepEqual(recoveredReport.layouts, allData.layouts);
  assert.equal((await post("close", 28)).status, 204);
  assert.equal((await fetch(path + "snapshot", { headers })).status, 410);
  const sessions = [];
  for (let i = 0; i < 5; i++) {
    const response = await fetch(address + "/api/session", { method: "POST", headers });
    assert.equal(response.status, 201);
    sessions.push((await response.json()).session);
  }
  assert.equal((await fetch(address + "/api/session/" + sessions[0] + "/snapshot", { headers })).status, 410);
  // An editor can retain an old revision after the native host starts a new session.
  // Changed charts must then be raw even if the client still owns that older chart key.
  const newSessionInput = { ...resized, assets: washroom.assets, heldShadingBlocks: inventory };
  const newSessionFrame = await bytes(await fetch(address + "/api/session/" + sessions.at(-1) + "/shade-layouts", {
    method: "POST", headers: { ...headers, "X-Revision": "1", "X-Input": "archive" },
    body: writeArchive([{ name: "project.json", bytes: Buffer.from(JSON.stringify(newSessionInput)) },
      ...companions.map(([, value], i) => ({ name: "assets/" + i, bytes: Buffer.from(value, "base64") }))]),
  }));
  assert.ok(unpack(newSessionFrame).info.chartRecordSpans.every(span => span.length === 2));
  assert.deepEqual(decodeReport(newSessionFrame, held).layouts, allData.layouts);
});
