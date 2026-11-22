import { rename, rm } from "node:fs/promises";
import { resolve } from "node:path";
import { NodeIO } from "@gltf-transform/core";
import { ALL_EXTENSIONS } from "@gltf-transform/extensions";
import { prune } from "@gltf-transform/functions";
import sharp from "sharp";
import { finishArena } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/arena-finish.mjs";

const directory = resolve(process.argv[2]);
const io = new NodeIO().registerExtensions(ALL_EXTENSIONS);
const doc = await io.read(resolve(directory, "arena.glb"));
await finishArena(doc, directory, { sharp });
await doc.transform(prune());
const target = resolve(directory, "arena-concrete.glb");
const temporary = target + "." + process.pid + ".tmp.glb";
try {
  await io.write(temporary, doc);
  await rename(temporary, target);
} finally {
  await rm(temporary, { force: true });
}
