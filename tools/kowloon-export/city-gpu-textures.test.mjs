import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import sharp from "sharp";
import { createRenderGltfLoader, requiresKtx2Transcoder } from "../../adapters/three/src/gltfLoader.ts";
import { chartCoverage, fillUncharted, markTriangle } from "./atlas-coverage.mjs";
import { entryBytes, readCityEntry, settlePublishedFiles } from "./city-data-textures.mjs";
import { flatNormals, gpuTextureProfileName, gpuTextureRoles, normalStrength, stageCityGpuTextures, surfaceConstants } from "./city-gpu-textures.mjs";
import { mobileTextureSize } from "./city-mobile-delivery.mjs";
import { decodeGpuTexture, encodeGpuTexture, gpuTextureKey, gpuTextureProfiles, inspectKtx2, loadBasisEncoder } from "./gpu-texture-codec.mjs";

const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const psnr = (a, b, channels = [0, 1, 2]) => {
  let error = 0;
  for (let i = 0; i < a.length; i += 4) for (const c of channels) error += (a[i + c] - b[i + c]) ** 2;
  return error ? 10 * Math.log10(255 * 255 * (a.length / 4) * channels.length / error) : 99;
};
/** Smooth, non-repeating texels: every codec should reproduce them closely. */
function texels(width, height, pixel) {
  const rgba = new Uint8Array(width * height * 4);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) rgba.set(pixel(x / width, y / height), (y * width + x) * 4);
  return rgba;
}
const png = (rgba, width, height) => sharp(Buffer.from(rgba), { raw: { width, height, channels: 4 } }).png().toBuffer();

test("each role's codec reproduces its texels with a complete mip chain", async () => {
  const basis = await loadBasisEncoder(), width = 64, height = 32;
  const colour = texels(width, height, (u, v) => [40 + 180 * u, 60 + 150 * v, 200 - 120 * u, 255]);
  const bumps = texels(width, height, (u, v) => [128 + 60 * Math.sin(u * 9), 128 + 60 * Math.cos(v * 7), 235, 255]);
  // Thresholds sit a few dB under what these gradients measure, far above a wrong channel order or transfer function.
  for (const [name, source, channels, floor] of [["color", colour, [0, 1, 2], 40], ["color-etc1s", colour, [0, 1, 2], 32],
    ["emissive", colour, [0, 1, 2], 32], ["normal", bumps, [0, 1], 38], ["data", bumps, [0, 1, 2], 38]]) {
    const ktx = encodeGpuTexture(basis, source, width, height, gpuTextureProfiles[name]);
    assert.deepEqual({ ...inspectKtx2(ktx), vkFormat: 0, supercompression: 0 }, { vkFormat: 0, width, height, levels: 7, supercompression: 0 }, name);
    const shown = await decodeGpuTexture(ktx);
    assert.equal(shown.uastc, gpuTextureProfiles[name].codec === "uastc", name);
    assert.equal(shown.hasAlpha, false, name);
    assert.ok(psnr(source, shown.rgba, channels) > floor, `${name}: ${psnr(source, shown.rgba, channels).toFixed(1)} dB`);
    const smallest = await decodeGpuTexture(ktx, 6);
    assert.deepEqual([smallest.width, smallest.height], [1, 1], name);
  }
  const cutout = texels(width, height, (u, v) => [200, 90, 30, u > .5 ? 255 : 0]);
  const shown = await decodeGpuTexture(encodeGpuTexture(basis, cutout, width, height, gpuTextureProfiles.color));
  assert.equal(shown.hasAlpha, true);
  assert.ok(psnr(cutout, shown.rgba, [3]) > 30);
  assert.throws(() => encodeGpuTexture(basis, new Uint8Array(30 * 32 * 4), 30, 32, gpuTextureProfiles.data), /multiples of four/);
});

test("texture keys contain the recipe, the size and every texel", () => {
  const rgba = new Uint8Array(4 * 4 * 4), key = gpuTextureKey(gpuTextureProfiles.data, 4, 4, rgba);
  assert.equal(key, gpuTextureKey({ ...gpuTextureProfiles.data }, 4, 4, rgba));
  assert.notEqual(key, gpuTextureKey(gpuTextureProfiles.normal, 4, 4, rgba));
  assert.notEqual(key, gpuTextureKey({ ...gpuTextureProfiles.data, rdo: 2 }, 4, 4, rgba));
  assert.notEqual(key, gpuTextureKey(gpuTextureProfiles.data, 8, 2, rgba));
  assert.notEqual(key, gpuTextureKey(gpuTextureProfiles.data, 4, 4, Uint8Array.from(rgba, (_, index) => +(index === 63))));
});

test("roles choose the codec and the content checks read only what they claim", () => {
  const name = (roles, alpha, codec) => gpuTextureProfileName(new Set(roles), alpha, codec);
  assert.equal(name(["color"], false), "color-etc1s");
  assert.equal(name(["color"], true), "color", "cutouts keep UASTC alpha");
  assert.equal(name(["color"], false, "uastc"), "color");
  assert.equal(name(["color", "emissive"], false), "color-etc1s");
  assert.equal(name(["emissive"], false), "emissive");
  assert.equal(name(["normal"], false), "normal");
  for (const roles of [["mr"], ["occlusion"], ["detail"], ["grain"], ["normal", "mr"], []]) assert.equal(name(roles, true), "data");
  assert.equal(flatNormals(Uint8Array.from([128, 128, 255, 255, 127, 128, 255, 9])), true);
  assert.equal(flatNormals(Uint8Array.from([128, 128, 255, 255, 129, 128, 255, 255])), false);
  assert.equal(flatNormals(Uint8Array.from([128, 128, 254, 255])), false);
  // Two charted texels average to 0.4 rough and 0.2 metal; atlas fill and gaps are not surface.
  assert.deepEqual(surfaceConstants(Uint8Array.from([255, 51, 0, 255, 9, 153, 102, 255, 0, 255, 0, 255, 0, 0, 0, 255])), { roughness: .4, metalness: .2 });
  assert.deepEqual(surfaceConstants(Uint8Array.from([0, 255, 0, 255])), { roughness: 1, metalness: 0 });
});

test("block-aligned mobile pages are whole 4x4 blocks and the default sizes are unchanged", () => {
  const low = { textureScale: .25, maxDimension: 512 };
  assert.deepEqual(mobileTextureSize(2052, 2048, low), { width: 512, height: 511 });
  assert.deepEqual(mobileTextureSize(2052, 2048, { ...low, blockAligned: true }), { width: 512, height: 512 });
  assert.deepEqual(mobileTextureSize(126, 31, { ...low, blockAligned: true }), { width: 128, height: 32 });
  assert.deepEqual(mobileTextureSize(126, 31, low), { width: 126, height: 31 });
  assert.deepEqual(mobileTextureSize(3710, 32, { ...low, blockAligned: true }), { width: 512, height: 16 });
});

test("small charts on white atlas fill keep their colour in a quarter-edge companion", async () => {
  // Sixteen 6x6 charts on a 16-texel pitch: at a quarter of the edge each chart is under two texels wide.
  const size = 64, mask = new Uint8Array(size * size), charts = [];
  for (let row = 0; row < 4; row++) for (let column = 0; column < 4; column++) {
    const x = column * 16 + 5, y = row * 16 + 5, colour = [40 + column * 60, 200 - row * 50, 30 + ((row + column) % 4) * 60];
    charts.push({ x, y, colour });
    markTriangle(mask, size, size, x, y, x + 6, y, x + 6, y + 6);
    markTriangle(mask, size, size, x, y, x + 6, y + 6, x, y + 6);
  }
  const page = texels(size, size, (u, v) => {
    const chart = charts.find(chart => u * size >= chart.x && u * size < chart.x + 6 && v * size >= chart.y && v * size < chart.y + 6);
    return chart ? [...chart.colour, 255] : [255, 255, 255, 255];
  });
  // The mask is exactly the charts: 16 of 6x6 texels, no ring of fill around them.
  assert.equal(mask.reduce((sum, value) => sum + value, 0), 16 * 36);
  for (const chart of charts) assert.equal(mask[(chart.y + 5) * size + chart.x] + mask[chart.y * size + chart.x + 5], 2);
  // A sliver that covers no texel centre still marks where it is.
  const sliver = new Uint8Array(size * size);
  markTriangle(sliver, size, size, 20.1, 9.6, 29.9, 9.7, 20.1, 9.8);
  assert.deepEqual([sliver.reduce((sum, value) => sum + value, 0), sliver[9 * size + 23]], [1, 1]);
  const companion = async rgba => sharp(Buffer.from(rgba), { raw: { width: size, height: size, channels: 4 } }).resize(16, 16, { fit: "fill" }).raw().toBuffer();
  /** What a viewer samples at the middle of a chart: the four companion texels around it. */
  const worst = small => Math.max(...charts.map(chart => {
    const cx = (chart.x + 3) / 4, cy = (chart.y + 3) / 4;
    return Math.max(...[0, 1, 2].map(channel => {
      let sum = 0;
      for (const [dx, dy] of [[-.5, -.5], [.5, -.5], [-.5, .5], [.5, .5]]) sum += small[(Math.floor(cy + dy) * 16 + Math.floor(cx + dx)) * 4 + channel];
      return Math.abs(sum / 4 - chart.colour[channel]);
    }));
  }));
  const filled = fillUncharted(page, size, size, mask);
  for (const chart of charts) for (let c = 0; c < 3; c++) assert.equal(filled[((chart.y + 2) * size + chart.x + 2) * 4 + c], chart.colour[c], "charted texels are untouched");
  assert.ok(worst(await companion(page)) > 60, "unfilled, the white fill washes the charts out");
  assert.ok(worst(await companion(filled)) <= 12, `filled: ${worst(await companion(filled)).toFixed(1)} of 255`);
  // Nothing to do, nothing changed.
  assert.equal(fillUncharted(page, size, size, new Uint8Array(size * size).fill(1)), page);
  assert.equal(fillUncharted(page, size, size, new Uint8Array(size * size)), page);
});

test("coverage follows each material's own UVs, so surfaces sharing a page get their own constants", async () => {
  // Two quads on one roughness page: paint on the left half, bare metal on the right, green fill elsewhere.
  const uv = Float32Array.from([0, 0, .5, 0, .5, .5, 0, .5, .5, 0, 1, 0, 1, .5, .5, .5]), indices = Uint16Array.from([0, 1, 2, 0, 2, 3]);
  const views = [Buffer.from(uv.buffer, 0, 32), Buffer.from(uv.buffer, 32, 32), Buffer.from(indices.buffer)];
  const info = () => ({ pbrMetallicRoughness: { metallicRoughnessTexture: { index: 0 } } });
  const json = { textures: [{ source: 0 }], materials: [info(), info(), { pbrMetallicRoughness: { metallicRoughnessTexture: { index: 0, extensions: { KHR_texture_transform: { scale: [3, 1] } } } } }],
    bufferViews: [{}, {}, {}], accessors: [{ bufferView: 0, componentType: 5126, count: 4, type: "VEC2" }, { bufferView: 1, componentType: 5126, count: 4, type: "VEC2" },
      { bufferView: 2, componentType: 5123, count: 6, type: "SCALAR" }],
    meshes: [{ primitives: [{ attributes: { TEXCOORD_0: 0 }, indices: 2, material: 0 }, { attributes: { TEXCOORD_0: 1 }, indices: 2, material: 1 },
      { attributes: { TEXCOORD_0: 1 }, indices: 2, material: 2 }] }] };
  const coverage = chartCoverage(json, async index => views[index], texture => texture.source), users = coverage.users(0);
  assert.deepEqual(users.map(([material]) => material), [0, 1, 2]);
  const page = texels(32, 32, (u, v) => v >= .5 ? [0, 255, 0, 255] : u < .5 ? [255, 204, 0, 255] : [255, 51, 255, 255]);
  const paint = await coverage.mask([users[0]], 32, 32), metal = await coverage.mask([users[1]], 32, 32), both = await coverage.mask(users.slice(0, 2), 32, 32);
  assert.deepEqual([paint[8 * 32 + 8], paint[8 * 32 + 24], paint[24 * 32 + 8], metal[8 * 32 + 24], metal[8 * 32 + 8]], [1, 0, 0, 1, 0]);
  const round = ({ roughness, metalness }) => [+roughness.toFixed(2), +metalness.toFixed(2)];
  // A page mean would call both surfaces half metal. One texel of the neighbour at the seam is the tolerance.
  assert.deepEqual(round(surfaceConstants(page, paint)).map((value, i) => Math.abs(value - [.8, 0][i]) < .07), [true, true]);
  assert.deepEqual(round(surfaceConstants(page, metal)).map((value, i) => Math.abs(value - [.2, 1][i]) < .07), [true, true]);
  assert.deepEqual(round(surfaceConstants(page, both)), [.5, .5]);
  assert.deepEqual(round(surfaceConstants(page)), [.5, .5], "without coverage, charted texels are told from the fill colours");
  // A texture that repeats has no unused area, and one without the UV set cannot be judged.
  assert.equal(await coverage.mask([users[2]], 32, 32), null);
  json.meshes[0].primitives[0].attributes = {};
  assert.equal(await coverage.mask([users[0]], 32, 32), null);
});

/** A published city in miniature: geometry and images share one buffer, as packs do. */
async function fixture(t, { meshopt = false } = {}) {
  const root = await mkdtemp(join(tmpdir(), "city-gpu-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const source = join(root, "source", "city.glb");
  await mkdir(join(dirname(source), "city-data"), { recursive: true });
  const pictures = {
    colour: [texels(30, 18, (u, v) => [30 + 200 * u, 90 + 100 * v, 60, 255]), 30, 18],
    flat: [texels(16, 16, () => [128, 128, 255, 255]), 16, 16],
    bumps: [texels(256, 256, (u, v) => [128 + 50 * Math.sin(u * 12), 128 + 50 * Math.cos(v * 9), 240, 255]), 256, 256],
    surface: [texels(256, 256, (u, v) => u < .5 ? [255, 102, 51, 255] : [0, 255, 0, 255]), 256, 256],
    glow: [texels(64, 64, (u, v) => u > .9 && v > .9 ? [250, 220, 90, 255] : [0, 0, 0, 255]), 64, 64],
    edge: [texels(32, 32, (u, v) => [255 * u, 255 * v, 0, 255]), 32, 32],
    detail: [texels(16, 16, (u, v) => [255 * u, 255 * u, 255 * u, 255 * v]), 16, 16],
    shop: [texels(256, 256, (u, v) => [200 * u + 20, 60, 180 * v + 30, 255]), 256, 256],
  };
  const geometry = Buffer.from(Float32Array.from([0, 0, 0, 1, 0, 0, 0, 1, 0]).buffer), indices = Buffer.from(Uint16Array.from([0, 1, 2, 0]).buffer);
  const parts = [], bufferViews = [], images = [];
  let end = 0;
  const add = bytes => {
    bufferViews.push({ buffer: 0, byteOffset: end, byteLength: bytes.length });
    parts.push(bytes, Buffer.alloc((4 - bytes.length % 4) % 4));
    end += Math.ceil(bytes.length / 4) * 4;
    return bufferViews.length - 1;
  };
  // Images sit between the two geometry views, so geometry must be repacked around them.
  const positionView = add(geometry);
  for (const [name, [rgba, width, height]] of Object.entries(pictures)) images.push({ name, mimeType: "image/png", bufferView: add(await png(rgba, width, height)) });
  images.push({ name: "shop again", mimeType: "image/png", bufferView: add(await png(...pictures.shop)) });
  const indexView = add(indices);
  await writeFile(join(dirname(source), "city-data", "chunk.bin"), Buffer.concat(parts, end));
  const index = name => images.findIndex(image => image.name === name);
  const json = { asset: { version: "2.0" }, scene: 0, extensionsUsed: ["KHR_mesh_quantization"], extensionsRequired: ["KHR_mesh_quantization"],
    scenes: [{ nodes: [0], extras: { worldSpec: true, worldSpecConcreteDetail: { version: 1, texture: index("edge") } } }],
    nodes: [{ mesh: 0 }], meshes: [{ primitives: [{ attributes: { POSITION: 0 }, indices: 1, material: 0 }, { attributes: { POSITION: 0 }, indices: 1, material: 1 }] }],
    accessors: [{ bufferView: positionView, componentType: 5126, count: 3, type: "VEC3", min: [0, 0, 0], max: [1, 1, 0] },
      { bufferView: indexView, componentType: 5123, count: 3, type: "SCALAR" }],
    buffers: [{ uri: "city-data/chunk.bin", byteLength: end }], bufferViews, images,
    samplers: [{ wrapS: 10497, wrapT: 10497 }],
    textures: images.map((_, source) => ({ sampler: 0, source })),
    materials: [
      { name: "wall", pbrMetallicRoughness: { baseColorTexture: { index: index("colour") }, metallicRoughnessTexture: { index: index("surface") }, roughnessFactor: .5 },
        normalTexture: { index: index("flat") }, occlusionTexture: { index: index("edge") }, emissiveTexture: { index: index("glow") },
        extras: { worldSpecSurface: { family: "Masonry", detailTexture: index("detail") } } },
      { name: "goods", pbrMetallicRoughness: { baseColorTexture: { index: index("shop") } }, normalTexture: { index: index("bumps") },
        extras: { worldSpecSurface: { family: "Shop inventory" } } },
      { name: "more goods", pbrMetallicRoughness: { baseColorTexture: { index: index("shop again") } }, extras: { worldSpecSurface: { family: "Shop inventory" } } },
    ] };
  if (meshopt) {
    // As a mobile pack: the view's bytes live in the extension and its own buffer is the fallback.
    const view = bufferViews[positionView];
    json.buffers.push({ byteLength: 36, extensions: { EXT_meshopt_compression: { fallback: true } } });
    bufferViews[positionView] = { buffer: 1, byteOffset: 0, byteLength: 36, byteStride: 12,
      extensions: { EXT_meshopt_compression: { buffer: 0, byteOffset: view.byteOffset, byteLength: view.byteLength, byteStride: 12, count: 3, mode: "ATTRIBUTES" } } };
  }
  const manifest = { sourceSha256: hash(entryBytes(json, Buffer.alloc(0))), primitives: [] };
  const text = Buffer.from(JSON.stringify(manifest)), header = Buffer.alloc(12 + Math.ceil(text.length / 4) * 4);
  header.write("SLOD"); header.writeUInt32LE(1, 4); header.writeUInt32LE(text.length, 8); text.copy(header, 12);
  await writeFile(join(dirname(source), "city-data", "lod.bin"), Buffer.concat([header, Buffer.from([5, 6, 7, 8])]));
  json.scenes[0].extras.cityLod = "city-data/lod.bin";
  await writeFile(source, entryBytes(json, Buffer.alloc(0)));
  const options = { source, cacheDirectory: join(root, "cache"), workers: 2 };
  const staged = async (folder, extra) => {
    const destination = join(root, folder, "city.glb"), report = await stageCityGpuTextures({ ...options, destination, ...extra });
    const { json } = readCityEntry(await readFile(destination)), files = new Map();
    const bytes = async view => {
      const place = view.extensions?.EXT_meshopt_compression ?? view, uri = json.buffers[place.buffer].uri;
      if (!files.has(uri)) files.set(uri, await readFile(join(dirname(destination), uri)));
      return files.get(uri).subarray(place.byteOffset ?? 0, (place.byteOffset ?? 0) + place.byteLength);
    };
    const image = async texture => {
      const record = json.images[json.textures[texture].extensions.KHR_texture_basisu.source];
      assert.equal(record.mimeType, "image/ktx2");
      return { name: record.name, view: record.bufferView, buffer: json.bufferViews[record.bufferView].buffer, ...await decodeGpuTexture(await bytes(json.bufferViews[record.bufferView])) };
    };
    return { destination, report, json, bytes, image };
  };
  return { root, source, json, pictures, geometry, indices, options, staged };
}

test("a staged city requires KTX2 for every texture and keeps geometry, bindings and provenance", async t => {
  const f = await fixture(t), before = await readFile(f.source);
  const { json, report, bytes, image, destination } = await f.staged("medium", { tier: "medium", measure: true });
  assert.ok(json.extensionsRequired.includes("KHR_texture_basisu") && json.extensionsUsed.includes("KHR_texture_basisu"));
  assert.ok(json.extensionsRequired.includes("KHR_mesh_quantization"));
  assert.ok(json.textures.every(texture => texture.source === undefined && Number.isInteger(texture.extensions.KHR_texture_basisu.source)));
  assert.equal(json.scenes[0].extras.worldSpecTextureLod, undefined, "Medium publishes no distant companions");
  // Geometry bytes are exact and its buffer keeps index 0, without the images.
  assert.deepEqual(Buffer.from(await bytes(json.bufferViews[json.accessors[0].bufferView])), f.geometry);
  assert.deepEqual(Buffer.from(await bytes(json.bufferViews[json.accessors[1].bufferView])), f.indices);
  assert.equal(json.buffers[0].byteLength, 44);
  const wall = json.materials[0], goods = json.materials[1];
  // The exactly flat normal page is gone; the textured one is not.
  assert.equal(wall.normalTexture, undefined);
  assert.equal(report.droppedFlatNormals, 1);
  assert.ok(goods.normalTexture);
  assert.equal(json.images.some(record => record.name === "flat"), false);
  // 30x18 becomes whole blocks; nothing else changes size at Medium.
  const colour = await image(wall.pbrMetallicRoughness.baseColorTexture.index);
  assert.deepEqual([colour.name, colour.width, colour.height, colour.uastc], ["colour", 32, 20, true], "Medium colour is UASTC");
  const resized = await sharp(Buffer.from(f.pictures.colour[0]), { raw: { width: 30, height: 18, channels: 4 } }).resize({ width: 32, height: 20, fit: "fill" }).raw().toBuffer();
  assert.ok(psnr(resized, colour.rgba) > 30);
  for (const [texture, name, picture, channels] of [[goods.normalTexture.index, "bumps", f.pictures.bumps, [0, 1]],
    [wall.pbrMetallicRoughness.metallicRoughnessTexture.index, "surface", f.pictures.surface, [1, 2]],
    [wall.occlusionTexture.index, "edge", f.pictures.edge, [0, 1]], [wall.emissiveTexture.index, "glow", f.pictures.glow, [0, 1, 2]]]) {
    const shown = await image(texture);
    assert.deepEqual([shown.name, shown.width, shown.height], [name, picture[1], picture[2]]);
    assert.ok(psnr(picture[0], shown.rgba, channels) > 30, `${name}: ${psnr(picture[0], shown.rgba, channels).toFixed(1)} dB`);
  }
  // References outside glTF materials follow the renumbered textures.
  const detail = await image(wall.extras.worldSpecSurface.detailTexture);
  assert.deepEqual([detail.name, detail.hasAlpha, detail.uastc], ["detail", true, true]);
  assert.ok(psnr(f.pictures.detail[0], detail.rgba, [3]) > 30);
  assert.equal((await image(json.scenes[0].extras.worldSpecConcreteDetail.texture)).name, "edge");
  assert.deepEqual([...gpuTextureRoles(json).values()].map(roles => [...roles].join("+")).sort(),
    ["color", "color", "color", "detail", "emissive", "mr", "normal", "occlusion+grain"].sort());
  // Identical pages are stored once.
  const shop = await image(goods.pbrMetallicRoughness.baseColorTexture.index), again = await image(json.materials[2].pbrMetallicRoughness.baseColorTexture.index);
  assert.equal(shop.view, again.view);
  assert.equal(report.uniqueTextures, 7);
  // Resident sizes: 16 bytes a 4x4 block on desktop, 8 for opaque ETC1S on a phone.
  assert.equal(report.byRole.detail.desktopBytes, 16 * (16 + 4 + 1 + 1 + 1));
  assert.equal(report.byRole.emissive.phoneBytes, 8 * (256 + 64 + 16 + 4 + 1 + 1 + 1));
  assert.equal(report.rgba8Bytes > report.desktopBytes * 3, true);
  assert.ok(report.byRole.normal.psnr.min > 30);
  // The LOD sidecar records the new entry; the source pack is untouched.
  const lod = await readFile(join(dirname(destination), json.scenes[0].extras.cityLod)), length = lod.readUInt32LE(8);
  const withoutLod = structuredClone(json); delete withoutLod.scenes[0].extras.cityLod;
  assert.equal(JSON.parse(lod.toString("utf8", 12, 12 + length)).sourceSha256, hash(entryBytes(withoutLod, Buffer.alloc(0))));
  assert.deepEqual([...lod.subarray(-4)], [5, 6, 7, 8]);
  assert.deepEqual(await readFile(f.source), before);
  await assert.rejects(stageCityGpuTextures({ ...f.options, destination: join(dirname(f.source), "other.glb") }), /separate directory/);
  await assert.rejects(stageCityGpuTextures({ ...f.options, source: destination, destination: join(f.root, "twice", "city.glb") }), /already uses KTX2/);
  // A second publication reuses every encoded texture and writes the same entry.
  const repeat = await f.staged("repeat", { tier: "medium" });
  assert.deepEqual([repeat.report.encodedTextures, repeat.report.reusedTextures], [0, 7]);
  assert.deepEqual(await readFile(repeat.destination), await readFile(destination));
});

test("meshopt views of a mobile pack are repacked through their extension", async t => {
  const f = await fixture(t, { meshopt: true });
  const { json, bytes } = await f.staged("mobile", { tier: "medium" });
  const view = json.bufferViews[json.accessors[0].bufferView];
  assert.deepEqual([view.buffer, view.byteOffset, view.byteLength], [1, 0, 36], "the fallback placement is untouched");
  assert.equal(view.extensions.EXT_meshopt_compression.buffer, 0);
  assert.deepEqual(Buffer.from(await bytes(view)), f.geometry);
  assert.deepEqual(json.buffers[1], { byteLength: 36, extensions: { EXT_meshopt_compression: { fallback: true } } });
});

test("Low halves normal and roughness pages, quarters emission and pairs distant companions", async t => {
  const f = await fixture(t);
  const { json, report, image } = await f.staged("low", { tier: "low" });
  const wall = json.materials[0], goods = json.materials[1];
  assert.deepEqual((({ width, height }) => [width, height])(await image(wall.pbrMetallicRoughness.metallicRoughnessTexture.index)), [128, 128]);
  assert.deepEqual((({ width, height }) => [width, height])(await image(wall.emissiveTexture.index)), [16, 16]);
  assert.deepEqual((({ width, height }) => [width, height])(await image(wall.occlusionTexture.index)), [32, 32]);
  const normal = await image(goods.normalTexture.index);
  assert.deepEqual([normal.width, normal.height], [128, 128]);
  // Shop goods stream: their colour pages start as quarter-edge companions in the first-frame buffers.
  const pairs = json.scenes[0].extras.worldSpecTextureLod.pairs, near = goods.pbrMetallicRoughness.baseColorTexture.index;
  assert.deepEqual(pairs.map(pair => pair.near).sort(), [near, json.materials[2].pbrMetallicRoughness.baseColorTexture.index].sort());
  const far = await image(pairs.find(pair => pair.near === near).far), full = await image(near);
  assert.deepEqual([full.width, full.height, far.width, far.height], [256, 256, 64, 64]);
  assert.deepEqual([full.uastc, json.scenes[0].extras.worldSpecTextureLod.exclusive], [false, true], "reduced tiers publish colour as ETC1S");
  assert.equal(json.textures[pairs[0].far].sampler, json.textures[pairs[0].near].sampler);
  assert.notEqual(far.buffer, full.buffer, "near textures are fetched separately");
  assert.equal(far.buffer, (await image(wall.emissiveTexture.index)).buffer);
  const expected = await sharp(Buffer.from(f.pictures.shop[0]), { raw: { width: 256, height: 256, channels: 4 } }).resize(64, 64).raw().toBuffer();
  assert.ok(psnr(expected, far.rgba) > 28);
  assert.equal(report.texturePairs, 2);
  // The reduced tiers compress data maps harder; a strong normal page survives the weak-normal cut.
  assert.deepEqual([report.byRole.mr.codecs, report.byRole.normal.codecs, report.droppedWeakNormals], [{ "data-small": 1 }, { normal: 1 }, 0]);
  assert.ok(normalStrength(f.pictures.bumps[0]) > 30);
  // A page that tilts normals by a degree or so is not worth its memory at Low. Its material falls back to the surface normal.
  const faint = texels(64, 64, (u, v) => [128 + 3 * Math.sin(u * 20), 128 + 3 * Math.cos(v * 20), 255, 255]);
  assert.ok(normalStrength(faint) > 1.5 && normalStrength(faint) < 3.5);
  assert.equal(normalStrength(f.pictures.flat[0]), 0);
  const { json: source, tail } = readCityEntry(await readFile(f.source)), entry = join(f.root, "faint", "city.glb");
  await mkdir(join(dirname(entry), "city-data"), { recursive: true });
  const bytes = await png(faint, 64, 64), chunk = await readFile(join(dirname(f.source), "city-data", "chunk.bin"));
  source.buffers[0].byteLength = chunk.length + bytes.length;
  source.images.push({ name: "faint", mimeType: "image/png", bufferView: source.bufferViews.push({ buffer: 0, byteOffset: chunk.length, byteLength: bytes.length }) - 1 });
  source.materials[2].normalTexture = { index: source.textures.push({ sampler: 0, source: source.images.length - 1 }) - 1 };
  delete source.scenes[0].extras.cityLod;
  await writeFile(join(dirname(entry), "city-data", "chunk.bin"), Buffer.concat([chunk, bytes]));
  await writeFile(entry, entryBytes(source, tail));
  const weak = await stageCityGpuTextures({ ...f.options, source: entry, destination: join(f.root, "faint-low", "city.glb"), tier: "low" });
  assert.equal(weak.droppedWeakNormals, 1);
  const staged = readCityEntry(await readFile(weak.destination)).json;
  assert.equal(staged.materials[2].normalTexture, undefined);
  assert.ok(staged.materials[1].normalTexture);
  const kept = await stageCityGpuTextures({ ...f.options, source: entry, destination: join(f.root, "faint-medium", "city.glb"), tier: "medium" });
  assert.deepEqual([kept.droppedWeakNormals, kept.droppedFlatNormals], [0, 1], "Medium keeps every map that does anything");
  assert.ok(report.firstFrame.desktopBytes < report.desktopBytes && report.firstFrame.downloadBytes < report.textureBytes);
});

test("Very Low keeps colour, emission and finish detail with constant roughness and metalness", async t => {
  const f = await fixture(t);
  const { json, report, image } = await f.staged("very-low", { tier: "very-low" });
  const wall = json.materials[0];
  // Charted texels are G 102 and B 51 of 255: 0.4 rough and 0.2 metal, times the authored 0.5 roughness factor.
  assert.deepEqual([wall.pbrMetallicRoughness.roughnessFactor, wall.pbrMetallicRoughness.metallicFactor], [.2, .2]);
  assert.equal(wall.pbrMetallicRoughness.metallicRoughnessTexture ?? wall.normalTexture ?? wall.occlusionTexture ?? json.materials[1].normalTexture, undefined);
  assert.deepEqual(json.materials[1].pbrMetallicRoughness.roughnessFactor, undefined, "a material without the map keeps its factors");
  assert.deepEqual(json.images.filter(record => record.name).map(record => record.name).sort(), ["colour", "detail", "edge", "glow", "shop", "shop again"]);
  assert.equal((await image(wall.extras.worldSpecSurface.detailTexture)).name, "detail");
  assert.equal((await image(json.scenes[0].extras.worldSpecConcreteDetail.texture)).name, "edge", "the grain reference outlives the occlusion slot");
  assert.equal((await image(wall.emissiveTexture.index)).width, 16);
  assert.equal(report.droppedImages, 3);
  assert.deepEqual([report.byRole.mr.dropped, report.byRole.normal.dropped, report.byRole["grain+occlusion"].dropped], [1, 2, 0]);
});

test("a viewer without a transcoder path refuses a KTX2 city once and still loads other packs", async () => {
  const scene = extensions => ({ asset: { version: "2.0" }, scenes: [{ nodes: [0] }], nodes: [{ name: "structure" }], ...extensions });
  assert.equal(requiresKtx2Transcoder(scene({})), false);
  assert.equal(requiresKtx2Transcoder(scene({ extensionsUsed: ["KHR_texture_basisu"] })), false, "optional KTX2 keeps its PNG fallback");
  const required = scene({ extensionsUsed: ["KHR_texture_basisu"], extensionsRequired: ["KHR_texture_basisu"] });
  assert.equal(requiresKtx2Transcoder(required), true);
  await assert.rejects(createRenderGltfLoader().parseAsync(JSON.stringify(required), ""), /transcoderPath/);
  const plain = await createRenderGltfLoader().parseAsync(JSON.stringify(scene({ extensionsUsed: ["KHR_texture_basisu"] })), "");
  assert.ok(plain.scene.getObjectByName("structure"));
  assert.throws(() => createRenderGltfLoader(undefined, { transcoderPath: "basis/" }), /renderer is required/);
});

test("republishing removes only the files an earlier publication of that entry recorded", async t => {
  const root = await mkdtemp(join(tmpdir(), "city-files-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(join(root, "city-data"));
  for (const name of ["old.bin", "kept.bin", "shared.bin", "stranger.bin"]) await writeFile(join(root, "city-data", name), name);
  const entry = join(root, "city.glb"), pack = (...names) => ({ buffers: names.map(name => ({ uri: "city-data/" + name })) });
  assert.deepEqual(await settlePublishedFiles(entry, pack("old.bin", "kept.bin", "shared.bin")), []);
  assert.deepEqual(await settlePublishedFiles(join(root, "other.glb"), pack("shared.bin")), []);
  assert.deepEqual(await settlePublishedFiles(entry, pack("kept.bin")), ["city-data/old.bin"]);
  // A file another entry records, and one nobody recorded, stay.
  assert.deepEqual((await readdir(join(root, "city-data"))).sort(), ["kept.bin", "shared.bin", "stranger.bin"]);
  assert.deepEqual(JSON.parse(await readFile(entry + ".files.json", "utf8")), { version: 1, entry: "city.glb", files: ["city-data/kept.bin"] });
  assert.deepEqual(await settlePublishedFiles(entry, pack("kept.bin")), []);
});
