import { randomUUID } from "node:crypto";
import { link, lstat, mkdir, open, readFile, rename, rm, rmdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

async function matches(destination, bytes) {
  let info;
  try { info = await lstat(destination); }
  catch (error) { if (error.code === "ENOENT") return false; throw error; }
  if (!info.isFile() || info.size !== bytes.byteLength || !(await readFile(destination)).equals(bytes))
    throw Error(`Refusing to replace a different staged companion: ${destination}`);
  return true;
}

// Filesystems without hard links can still atomically rename a staged file.
// Serialize that final existence check and rename among their publishers.
async function renameExclusive(staged, destination, bytes) {
  const lock = destination + ".publish-lock";
  for (let attempt = 0; ; attempt++) {
    try { await mkdir(lock); break; }
    catch (error) {
      if (error.code !== "EEXIST") throw error;
      if (await matches(destination, bytes)) return;
      if (attempt >= 100) throw Error(`Cannot claim companion publication lock: ${lock}`);
      await delay(20);
    }
  }
  try { if (!await matches(destination, bytes)) await rename(staged, destination); }
  finally { await rmdir(lock); }
}

export async function writeImmutable(destination, bytes) {
  if (await matches(destination, bytes)) return;
  await mkdir(dirname(destination), { recursive: true });
  const staged = destination + "." + randomUUID() + ".tmp";
  const file = await open(staged, "wx");
  try {
    try { await file.writeFile(bytes); }
    finally { await file.close(); }
    try { await link(staged, destination); }
    catch (error) {
      if (error.code === "EEXIST") {
        if (!await matches(destination, bytes)) throw error;
      } else if (["EPERM", "ENOTSUP", "EOPNOTSUPP", "ENOSYS", "EACCES"].includes(error.code)) {
        await renameExclusive(staged, destination, bytes);
      } else throw error;
    }
  } finally { await rm(staged, { force: true }); }
}

export async function copyImmutable(source, destination) {
  if (resolve(source) === resolve(destination)) {
    if (!(await lstat(source)).isFile()) throw Error(`Expected a published companion file: ${source}`);
    return;
  }
  await writeImmutable(destination, await readFile(source));
}
