import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { copyImmutable, writeImmutable } from "./immutable-file.mjs";

test("readers see one complete winning payload while companion writers race", async t => {
  const directory = await mkdtemp(join(tmpdir(), "superspec-companions-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const destination = join(directory, "chunk.bin");
  const payloads = [Buffer.alloc(2 << 20, 31), Buffer.alloc(2 << 20, 77)];
  const jobs = Array.from({length: 8}, (_, i) => writeImmutable(destination, payloads[i % 2]));
  const all = Promise.allSettled(jobs);
  let complete = false;
  all.then(() => { complete = true; });
  do {
    let observed;
    try { observed = await readFile(destination); }
    catch (error) { assert.equal(error.code, "ENOENT"); }
    if (observed) assert.ok(payloads.some(bytes => bytes.equals(observed)));
  } while (!complete);
  const results = await all, winner = await readFile(destination);
  for (const [i, result] of results.entries()) {
    if (payloads[i % 2].equals(winner)) assert.equal(result.status, "fulfilled");
    else assert.match(result.reason.message, /different staged companion/);
  }
  assert.deepEqual(await readdir(directory), ["chunk.bin"]);
});

test("copying a companion isolates published bytes from later source edits", async t => {
  const directory = await mkdtemp(join(tmpdir(), "superspec-copy-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const source = join(directory, "source"), destination = join(directory, "published");
  await writeFile(source, "original");
  await copyImmutable(source, destination);
  await writeFile(source, "changed");
  await assert.rejects(copyImmutable(source, destination), /different staged companion/);
  assert.equal(await readFile(destination, "utf8"), "original");
});
