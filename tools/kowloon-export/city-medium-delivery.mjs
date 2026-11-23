/** Publish a Medium city as a KTX2 delivery: every map at its authored size, colour in
 * UASTC, and quarter-edge distant companions for the textures the Medium streaming policy
 * pairs, so a session starts on those. Geometry, materials and levels are the pack's own. */
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { gpuTextureTiers, stageCityGpuTextures } from "./city-gpu-textures.mjs";

export function stageMediumCity(source, destination, { cacheDirectory, streaming = true, workers } = {}) {
  return stageCityGpuTextures({ source, destination, tier: "medium", maps: { ...gpuTextureTiers.medium, ...(streaming ? { streaming: true } : {}) },
    ...(cacheDirectory ? { cacheDirectory } : {}), ...(workers ? { workers } : {}) });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { values, positionals } = parseArgs({ allowPositionals: true, options: { out: { type: "string" }, "texture-cache": { type: "string" },
    "no-streaming": { type: "boolean" }, workers: { type: "string" } } });
  if (positionals.length !== 1 || !values.out)
    throw Error("Usage: node city-medium-delivery.mjs CITY.glb --out DIR/CITY.glb [--texture-cache DIR] [--no-streaming] [--workers N]\n"
      + "  CITY.glb is a city pack without texture streaming (SUPERSPEC_CITY_TEXTURE_LODS=0).");
  const { records, ...report } = await stageMediumCity(positionals[0], values.out, { cacheDirectory: values["texture-cache"],
    streaming: !values["no-streaming"], workers: values.workers ? Number(values.workers) : undefined });
  console.log(JSON.stringify({ ...report, keptUncompressed: records.filter(record => record.kept).length }, null, 2));
}
