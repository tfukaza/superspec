import { writeImmutable } from "./immutable-file.mjs";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { NodeIO } from "@gltf-transform/core";
import { createHash } from "node:crypto";
import sharp from "sharp";
import { paintedTuftLayers, tuftPaintImage } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/grass-field.mjs";

/** Supplement standard glTF fallback meshes with TreeSpec brush instances. */
export async function attachPaintedLawns(json, directory, resourceDir, folder, placements) {
  const fields = placements.filter(p => p.prototype.startsWith("grass-field-"));
  if (!fields.length) return;
  const sourceFile = await readFile(resolve(directory, "lawn-sources.json"), "utf8").catch(error => {
    if (error.code === "ENOENT") return null; // Previous builds contain only static tufts.
    throw error;
  });
  if (!sourceFile) return;
  const sources = JSON.parse(sourceFile);
  const templates = [];
  const io = new NodeIO();
  // A consumer substitutes these layers only if every field has an authored template.
  for (const prototype of new Set(fields.map(p => p.prototype))) {
    const file = await readFile(resolve(directory,sources[prototype]));
    const doc = await io.readBinary(file);
    const spec = doc.getRoot().getDefaultScene()?.getExtras().treeSpecPaintedLawn;
    if (spec?.version !== 1) return;
    const turf = doc.getRoot().listMeshes()[0].listPrimitives()[0];
    const layers = paintedTuftLayers(spec.width, spec.depth, {
      positions: turf.getAttribute("POSITION").getArray(),
      normals: turf.getAttribute("NORMAL").getArray(),
      colors: turf.getAttribute("COLOR_0").getArray(),
    });
    templates.push({ prototype, layers, placements: fields.filter(p => p.prototype === prototype) });
  }
  const parts = [];
  let length = 0;
  function append(bytes) {
    const byteOffset = length;
    parts.push(bytes);
    const pad = (4 - bytes.byteLength % 4) % 4;
    parts.push(Buffer.alloc(pad));
    length += bytes.byteLength + pad;
    return {byteOffset,byteLength:bytes.byteLength};
  }
  const atlas = append(await tuftPaintImage(sharp));
  const beds = templates.map(({layers, placements}) => ({
    layers: layers.map(a => append(Buffer.from(a.buffer,a.byteOffset,a.byteLength))),
    placements: placements.map(({position, heading, scale}) => ({position,heading,scale:scale ?? 1})),
  }));
  const bytes = Buffer.concat(parts);
  const hash = createHash("sha256").update(bytes).digest("hex").slice(0,24);
  const name = "chunk-" + hash + ".bin";
  await writeImmutable(resolve(resourceDir,name), bytes);
  const extras = json.scenes[json.scene ?? 0].extras;
  const buffer = extras.paintedLawn?.buffer ?? json.buffers.length;
  json.buffers[buffer] = {uri:folder+"/"+name,byteLength:bytes.length};
  extras.paintedLawn = { version:1, buffer, atlas, beds };
}
