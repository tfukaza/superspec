import { createHash, randomUUID } from "node:crypto";
import { mkdir, readFile, realpath, rename, rm, stat, writeFile } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import sharp from "sharp";
import { cityCompanionUris, entryBytes, localPath, readCityEntry, realDestination, retargetLod, verifiedLod } from "./city-data-textures.mjs";
import { copyImmutable, writeImmutable } from "./immutable-file.mjs";

const WEBP = "EXT_texture_webp";
const allowed = new Set(["EXT_mesh_gpu_instancing", "KHR_materials_emissive_strength", "KHR_mesh_quantization", "MSFT_lod", "KHR_lights_punctual", "KHR_texture_transform"]);
const digest = bytes => createHash("sha256").update(bytes).digest("hex").slice(0, 24);

export async function exactWebp(png) {
  const decode = bytes => sharp(bytes).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  const meta = await sharp(png).metadata();
  // Keep the original for profiles or precision the browser could interpret differently.
  if (meta.depth !== "uchar" || meta.hasProfile || meta.orientation || (meta.pages ?? 1) > 1) return;
  const webp = await sharp(png).webp({ lossless: true, exact: true, effort: 2 }).toBuffer();
  if (webp.length + 128 >= png.length) return;
  const before = await decode(png), after = await decode(webp);
  if (before.info.width === after.info.width && before.info.height === after.info.height && before.data.equals(after.data)) return webp;
}

function selectImages(json) {
  const users = new Map();
  for (const texture of json.textures ?? []) {
    if (!Number.isInteger(texture.source)) continue;
    const list = users.get(texture.source) ?? [];
    list.push(texture); users.set(texture.source, list);
  }
  const reserved = new Set((json.accessors ?? []).flatMap(a => [a.bufferView, a.sparse?.indices?.bufferView, a.sparse?.values?.bufferView]));
  // WorldSpec probe data refers to bufferViews by index.
  for (const scene of json.scenes ?? []) {
    const extras = scene.extras;
    if (extras?.worldSpecTextureLod) throw Error("Make a web delivery from the core city pack before enabling texture residency.");
    if (extras?.worldSpecBounce) reserved.add(extras.worldSpecBounce.bufferView);
  }
  const images = new Map();
  for (const [index, image] of (json.images ?? []).entries()) {
    const view = image.bufferView;
    if (!Number.isInteger(view)) continue;
    if (images.has(view)) {
      images.set(view, null);
      continue;
    }
    const textures = users.get(index) ?? [];
    images.set(view, !reserved.has(view) && !image.uri && !image.extensions &&
      image.mimeType === "image/png" && textures.length && textures.every(t => !t.extensions)
      ? { image, index, textures } : null);
  }
  return images;
}

export async function stageCityWebDelivery(source, destination) {
  source = await realpath(resolve(source));
  const directory = dirname(source), output = await realDestination(dirname(resolve(destination)));
  if (output === directory) throw Error("Stage the web pack in a separate directory.");
  destination = resolve(output, basename(destination));
  const { json, tail } = readCityEntry(await readFile(source));
  if (tail.length || json.buffers?.some(b => !b.uri || b.extensions) || json.bufferViews?.some(v => v.extensions) ||
    [...(json.extensionsUsed ?? []), ...(json.extensionsRequired ?? [])].some(name => !allowed.has(name)))
    throw Error("Expected a published WorldSpec city with external buffers and supported extensions.");
  const originalCompanions = cityCompanionUris(json), originalEntry = await stat(source);
  let originalBytes = originalEntry.size;
  for (const uri of originalCompanions) originalBytes += (await stat(localPath(directory, uri))).size;
  const lod = await verifiedLod(json, tail, directory);
  const images = selectImages(json), folder = basename(destination, ".glb") + "-data";
  const views = json.buffers.map(() => []);
  for (const [index, view] of json.bufferViews.entries()) views[view.buffer].push({ index, view });
  const produced = new Set(), queue = [...json.buffers.keys()];
  let converted = 0, imageBytesSaved = 0;
  const work = async () => {
    while (queue.length) {
      const index = queue.pop(), buffer = json.buffers[index];
      const ordered = views[index].sort((a, b) => (a.view.byteOffset ?? 0) - (b.view.byteOffset ?? 0));
      if (!ordered.some(({ index }) => images.get(index))) continue;
      const original = await readFile(localPath(directory, buffer.uri));
      if (original.length !== buffer.byteLength) throw Error(`Invalid buffer size: ${buffer.uri}`);
      const changes = [];
      let end = 0;
      for (const { index, view } of ordered) {
        const offset = view.byteOffset ?? 0;
        // Overlapping or unaligned views are left intact.
        if (offset < end || offset % 4 || offset + view.byteLength > original.length) {
          changes.length = 0;
          break;
        }
        end = offset + view.byteLength;
        const candidate = images.get(index);
        if (!candidate) continue;
        const bytes = await exactWebp(original.subarray(offset, end));
        if (bytes) changes.push({ view, offset, end, bytes, candidate });
      }
      if (!changes.length) continue;
      const parts = [];
      let delta = 0, cursor = 0, replacement = 0;
      for (const { view } of ordered) {
        const offset = view.byteOffset ?? 0;
        const change = changes[replacement];
        view.byteOffset = offset - delta;
        if (change?.view !== view) continue;
        replacement++;
        const { bytes, end, candidate } = change;
        const removed = view.byteLength - bytes.length, pad = removed % 4;
        parts.push(original.subarray(cursor, offset), bytes, Buffer.alloc(pad));
        cursor = end;
        delta += removed - pad;
        view.byteLength = bytes.length;
        candidate.image.mimeType = "image/webp";
        for (const texture of candidate.textures) {
          delete texture.source;
          texture.extensions = { [WEBP]: { source: candidate.index } };
        }
        converted++;
        imageBytesSaved += removed - pad;
      }
      const packed = Buffer.concat([...parts, original.subarray(cursor)]);
      const uri = folder + "/web-" + digest(packed) + ".bin";
      await writeImmutable(localPath(output, uri), packed);
      produced.add(uri);
      buffer.uri = uri;
      buffer.byteLength = packed.length;
    }
  };
  await Promise.all(Array.from({ length: 4 }, work));
  if (converted) {
    json.extensionsUsed = [...(json.extensionsUsed ?? []), WEBP];
    json.extensionsRequired = [...(json.extensionsRequired ?? []), WEBP];
    await retargetLod(json, lod, output, folder);
    if (lod) produced.add(json.scenes[json.scene ?? 0].extras.cityLod);
  }
  const final = entryBytes(json, tail);
  let deliveryBytes = final.length;
  for (const uri of cityCompanionUris(json)) {
    const target = localPath(output, uri);
    if (!produced.has(uri)) await copyImmutable(localPath(directory, uri), target);
    deliveryBytes += (await stat(target)).size;
  }
  await mkdir(output, { recursive: true });
  const pending = destination + "." + randomUUID() + ".tmp";
  try {
    await writeFile(pending, final, { flag: "wx" });
    await rename(pending, destination);
  } finally { await rm(pending, { force: true }); }
  return { source, destination, converted, imageBytesSaved, originalBytes, deliveryBytes };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { values, positionals } = parseArgs({ allowPositionals: true, options: { out: { type: "string" } } });
  if (positionals.length !== 1 || !values.out) throw Error("Usage: node city-web-delivery.mjs ENTRY.glb --out WEB.glb");
  const report = await stageCityWebDelivery(positionals[0], values.out);
  await writeFile(resolve(dirname(report.destination), "city-web-delivery.json"), JSON.stringify(report, null, 2) + "\n");
  console.log(report);
}
