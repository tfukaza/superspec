/** Extract the WorldSpec GPU material bake on its original triangles. Each page contains
 * mesh-local UV charts. The game can light and view all the original surfaces independently. */
import { readFile, readdir, rename, rm, writeFile } from "node:fs/promises";
import { createReadStream } from "node:fs";
import { createHash, randomUUID } from "node:crypto";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Document, NodeIO, TextureInfo } from "@gltf-transform/core";
import { KHRMaterialsEmissiveStrength } from "@gltf-transform/extensions";
import sharp from "sharp";
import { readMaterialEdges } from "./material-edges.mjs";
import { chartPages } from "./chart-pages.mjs";
import { readPrototypeJson } from "./prototype-json.mjs";
import { openBaker, createCanvas, ImageData, bakeIdentity, studio } from "../../tools/headless/host.mjs";
import { bakeBuilding } from "../../tools/headless/building.mjs";

const [directory, only] = process.argv.slice(2);
if (!directory || process.argv.length > 4) throw Error("Usage: node bake-buildings.mjs directory [prototype]");
const placements = JSON.parse(
  await readFile(resolve(directory, "placements.json")),
);
// Keep original UVs and textures for catalog stock. Resolve the exact mesh IDs from
// the native prototype so a newly used catalog item follows the same export path.
async function nativeReceivers(name) {
  const json = await readPrototypeJson(resolve(directory, name + ".glb"));
  const nativeMeshes = json.nodes.filter((node) => node.mesh !== undefined && (
    node.name.endsWith("/lettering") ||
    json.meshes[node.mesh].primitives.every((primitive) => {
      const mat = json.materials[primitive.material];
      return mat?.name === "builtin:display-glass" ||
        (mat?.name?.startsWith("builtin:") && !!mat.pbrMetallicRoughness?.baseColorTexture);
    })
  )).map((node) => node.name);
  const balconies = json.nodes.filter(node => node.mesh !== undefined && /\/balcony\/balcony\/[^/]+$/.test(node.name))
    .map(node => ({
      id: node.name,
      count: json.meshes[node.mesh].primitives.reduce((n, p) => n + json.accessors[p.indices ?? p.attributes.POSITION].count, 0),
    }));
  return { nativeMeshes, balconies };
}
function atlasPages(meshesIn, { nativeMeshes, balconies }, { renderer, gl }) {
  const visible = meshesIn;
  const visibleNames = new Set(visible.map((m) => m.name));
  for (const { id, count } of balconies) {
    const boundId = id.replace(/\/concrete$/, "/suspended-concrete");
    const mesh = visible.find(m => m.name === id || m.name === boundId);
    if (!mesh || (mesh.geometry.index?.count ?? mesh.geometry.attributes.position.count) !== count)
      throw Error("Balcony receiver lost original triangles: " + id);
  }
  for (const name of nativeMeshes)
    if (!visibleNames.has(name)) throw Error("Native mesh missing in bake: " + name);
  const native = new Set(nativeMeshes);
  const meshes = visible.filter((m) => !native.has(m.name));
  const fbo = gl.createFramebuffer(), pad = 2;
  // Render targets have an sRGB color attachment and linear normal/property attachments.
  // Reading bottom row first puts Three UV (0,0) in row zero of the resulting glTF image.
  function bind(tex) {
    const { width: w, height: h } = tex.image;
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferTexture2D(
      gl.FRAMEBUFFER,
      gl.COLOR_ATTACHMENT0,
      gl.TEXTURE_2D,
      renderer.properties.get(tex).__webglTexture,
      0,
    );
    if (
      gl.checkFramebufferStatus(gl.FRAMEBUFFER) !==
      gl.FRAMEBUFFER_COMPLETE
    )
      throw Error("Unreadable texture " + tex.name);
    return [w, h];
  }
  function image(tex, emission, normal) {
    const [w, h] = bind(tex);
    const raw = new Uint8Array(w * h * 4);
    if (emission) {
      // The viewer stores emissive radiance in linear RGBA16F. glTF uses an sRGB
      // texture multiplied by a linear strength. Preserve both the mask and HDR.
      const hdr = new Float32Array(raw.length);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.FLOAT, hdr);
      let peak = 0;
      for (let i = 0; i < hdr.length; i += 4)
        peak = Math.max(peak, hdr[i], hdr[i + 1], hdr[i + 2]);
      emission.strength = Math.ceil(peak);
      for (let i = 0; i < hdr.length; i += 4) {
        for (let c = 0; c < 3; c++) {
          const v = Math.max(0, hdr[i + c]) / (emission.strength || 1);
          raw[i + c] = Math.round(255 * (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055));
        }
        raw[i + 3] = 255;
      }
    } else gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, raw);
    if (gl.getError() !== gl.NO_ERROR) throw Error("Texture readback failed: " + tex.name);
    // glTF's derivative tangent convention reverses green relative to the renderer's source chart.
    if (normal) for (let i = 1; i < raw.length; i += 4) raw[i] = 255 - raw[i];
    const c = createCanvas(w, h);
    c.getContext("2d").putImageData(
      new ImageData(new Uint8ClampedArray(raw.buffer), w, h),
      0,
      0,
    );
    return c;
  }
  function edgeImage(item) {
    const { mat, grazing, specular } = item;
    const { width, height } = mat.map.image;
    const { pixels, active } = readMaterialEdges({
      gl, bind, grazing, specular, width, height,
      texture(name) {
        const slot = mat.userData.liveTextureSlots.get(name);
        return mat.userData.liveUniforms["msLiveTex" + slot + "Map"].value;
      },
    });
    item.edge = active ? [grazing.from.value, grazing.to.value] : null;
    const c = createCanvas(width, height);
    c.getContext("2d").putImageData(new ImageData(pixels, width, height), 0, 0);
    return c;
  }
  const items = meshes
    .map((m) => {
      const mat = m.material,
        tex = mat.map,
        g = m.geometry;
      if (mat.isShaderMaterial)
        throw Error("Unbaked shader on " + m.name);
      let max = 384;
      if (["bench-wood", "aircon-casing", "rusted-pipe"].includes(m.userData.materialSpecId)) max = 768;
      if (m.userData.materialSpecId === "poster-print" || /\/(front|wall|glass|board|tile|dado|plaster|tarp)$/.test(m.name)) max = 1536;
      if (/\/duct(?:-\d+)?\/|\/(?:vent-metal|vent-edge|galvanized)$/.test(m.name)) max = Infinity;
      if (m.name.includes("/wall/") || /\/(wall|wall-round|concrete|suspended-concrete|plinth|trim)$/.test(m.name)) max = Infinity;
      const live = mat.userData.liveDescriptors ?? [];
      const grazing = live.find((op) => op.id === "grazingFalloff")?.arguments;
      const specular = live.find((op) => op.id === "scaleSpecular")?.arguments;
      const scale = tex
        ? Math.min(1, max / Math.max(tex.image.width, tex.image.height))
        : 1;
      // A window without a modeled room reuses the tube-light slot for its flat
      // back panel. Only the recessed room has a physical fluorescent fixture.
      const flatWindow = m.name.endsWith("/window/window/tube-light") &&
        !visibleNames.has(m.name.replace(/tube-light$/, "room-lit"));
      // An imported unwrapped UV can reach outside its allocated chart. Preserve
      // the original neighborhood for such receivers, including the page edges.
      const uv = g.getAttribute("uv");
      let confined = true;
      for (let i = 0; uv && confined && i < uv.count; i++) {
        const u = uv.getX(i), v = uv.getY(i);
        confined = u >= 0 && u <= 1 && v >= 0 && v <= 1;
      }
      return {
        m,
        mat,
        g,
        // room-lit is an authored diffuse room fill, not a physical emitter.
        emitter: !flatWindow && !!mat.emissiveMap && /\/(?:glass|pane|tube-light|lamp-lens)$/.test(m.name),
        strength: 0,
        grazing, specular, edge: null, hasEdge: !!(grazing && specular),
        repack: confined && !mat.transparent && !mat.alphaTest,
        w: tex ? Math.max(4, Math.round(tex.image.width * scale)) : 4,
        h: tex ? Math.max(4, Math.round(tex.image.height * scale)) : 4,
      };
    })
    .sort((a, b) => b.h - a.h);
  const limit = Math.max(2048, ...items.map(({ w, h }) => Math.max(w, h) + 2 * pad));
  const pages = chartPages(items, limit, pad);
  const exportPage = (idx) => {
    const { items, width, height } = pages[idx];
    const keys = ["map", "normalMap", "roughnessMap"];
    if (items.some((item) => item.emitter)) keys.push("emissiveMap");
    if (items.some((item) => item.grazing && item.specular)) keys.push("edge");
    const opaque = items.every(({ mat }) => !mat.transparent && !mat.alphaTest);
    const maps = keys.map(async (key, k) => {
      const atlas = createCanvas(width, height),
        ctx = atlas.getContext("2d");
      ctx.fillStyle = ["#ffffff", "#8080ff", "#00ff00"][k] ?? "#000000";
      ctx.fillRect(0, 0, width, height);
      for (const item of items) {
        const tex = item.mat[key];
        if (key === "edge" ? !(item.grazing && item.specular) : !tex || (key === "emissiveMap" && !item.emitter)) continue;
        const tile = key === "edge" ? edgeImage(item) : image(tex, key === "emissiveMap" ? item : null, key === "normalMap"),
          { x, y, w, h } = item;
        ctx.clearRect(x - pad, y - pad, w + 2 * pad, h + 2 * pad);
        ctx.drawImage(tile, x, y, w, h);
        for (const [sx, dx] of [
          [0, x - pad],
          [tile.width - 1, x + w],
        ])
          ctx.drawImage(tile, sx, 0, 1, tile.height, dx, y, pad, h);
        for (const [sy, dy] of [
          [0, y - pad],
          [tile.height - 1, y + h],
        ])
          ctx.drawImage(tile, 0, sy, tile.width, 1, x, dy, w, pad);
      }
      // Keep one page in flight. Native codecs can encode each finished channel
      // while the GPU and JS thread assemble the next channel of this page.
      if (key === "roughnessMap" || (key === "map" && opaque)) {
        const { data } = ctx.getImageData(0, 0, width, height);
        const image = sharp(data, { raw: { width, height, channels: 4 } })
          .removeAlpha().jpeg({ quality: 90, chromaSubsampling: "4:4:4" });
        return { data: await image.toBuffer(), mime: "image/jpeg" };
      }
      return { data: await atlas.encode("png"), mime: "image/png" };
    });
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    renderer.resetState();
    const parts = items.map(({ m, mat, g, x, y, w, h, strength, edge }) => {
      const geom = g.clone().applyMatrix4(m.matrixWorld);
      const pos = geom.getAttribute("position"),
        norm = geom.getAttribute("normal"),
        uv = geom.getAttribute("uv"),
        col = mat.vertexColors ? geom.getAttribute("color") : null;
      const pts = new Float32Array(pos.count * 3),
        normals = new Float32Array(pts.length),
        tex = new Float32Array(pos.count * 2),
        colors = new Uint8Array(pts.length);
      for (let i = 0; i < pos.count; i++) {
        pts.set([pos.getX(i), pos.getY(i), pos.getZ(i)], 3 * i);
        normals.set([norm.getX(i), norm.getY(i), norm.getZ(i)], 3 * i);
        tex.set(
          [
            (x + (uv ? uv.getX(i) : 0.5) * w) / width,
            (y + (uv ? uv.getY(i) : 0.5) * h) / height,
          ],
          2 * i,
        );
        for (let c = 0; c < 3; c++)
          colors[3 * i + c] = Math.round(
            255 *
              Math.min(
                1,
                (col ? col.getComponent(i, c) : 1) *
                  [mat.color.r, mat.color.g, mat.color.b][c],
              ),
          );
      }
      const indexes = geom.index
        ? new Uint32Array(geom.index.array)
        : null;
      geom.dispose();
      return {
        sourceFinish: m.name.split("/").slice(-2).join(" / "),
        pos: pts,
        shopFitting: /\/part\/(?:stock\/stock|clutter\/clutter)\//.test(m.name)
          || /\/part\/(?:room\/room|bay\/bay)\/(?:paint|metal|timber|shop-timber)$/.test(m.name),
        norm: normals,
        uv: tex,
        color: colors,
        index: indexes,
        strength,
        edge,
        aspect: (w / width) / (h / height),
        alpha: mat.transparent
          ? "BLEND"
          : mat.alphaTest
            ? "MASK"
            : "OPAQUE",
      };
    });
    return { maps, keys, parts };
  };
  return { count: pages.length, exportPage };
}

const names = new Set(placements.map(p => p.prototype).filter(n => n.startsWith("building-")));
if (only && !names.has(only)) throw Error("Unknown building prototype " + only);
async function fileDigest(path) {
  const hash = createHash("sha256");
  for await (const bytes of createReadStream(path)) hash.update(bytes);
  return hash.digest("hex");
}
async function producerIdentity(host) {
  const dependencies = [import.meta.url, "./chart-pages.mjs", "./material-edges.mjs", "./prototype-json.mjs", "./package-lock.json",
    "../headless/building.mjs", "../../crates/engine/src/materialspec/program/examples/textures/concrete-plain.png",
    "../../crates/engine/src/materialspec/program/examples/textures/concrete-fine-sand.png"];
  const hashes = [];
  for (const path of dependencies) hashes.push(await fileDigest(new URL(path, import.meta.url)));
  const library = fileURLToPath(new URL("../../assets/materials/", import.meta.url));
  for (const directory of new Set([library, resolve(dirname(studio), "materials")])) {
    let files;
    try { files = await readdir(directory); }
    catch (error) { if (error.code === "ENOENT" && directory !== library) continue; throw error; }
    for (const file of files.sort()) hashes.push([file, await fileDigest(resolve(directory, file))]);
  }
  return [await bakeIdentity(host), process.version, process.platform, process.arch, sharp.versions, hashes];
}
async function matchesBake(target, key) {
  try {
    const receipt = await readFile(target + ".sha256", "utf8");
    const [input, output] = receipt.split(" ");
    return input === key && output === await fileDigest(target);
  } catch (error) { if (error.code === "ENOENT") return false; throw error; }
}
let producer;
for (const name of only ? [only] : names) {
  const receivers = await nativeReceivers(name);
  const { nativeMeshes } = receivers;
  let host;
  try {
    const source = await readFile(resolve(directory, name + ".aml"), "utf8");
    if (!producer) {
      host = openBaker();
      producer = await producerIdentity(host);
    }
    const key = createHash("sha256").update(JSON.stringify([producer, source, receivers])).digest("hex");
    const target = resolve(directory, name + "-pbr.glb");
    if (await matchesBake(target, key)) {
      console.log(name, "reused verified material bake");
      continue;
    }
    host ??= openBaker();
    const baked = await bakeBuilding(host, source, nativeMeshes);
    const { count, exportPage } = atlasPages(baked.meshes, receivers, host);
    const doc = new Document(),
      buffer = doc.createBuffer(),
      scene = doc.createScene().setExtras({ nativeMeshes });
    const emissiveStrength = doc.createExtension(KHRMaterialsEmissiveStrength);
    const root = doc
      .createNode("Z-up")
      .setRotation([-Math.SQRT1_2, 0, 0, Math.SQRT1_2]);
    scene.addChild(root);
    const mesh = doc.createMesh(name);
    root.addChild(doc.createNode(name).setMesh(mesh));
    let tris = 0, emitters = 0;
    for (let i = 0; i < count; i++) {
      const { maps, keys, parts } = exportPage(i);
      const encoded = await Promise.all(maps);
      const textures = encoded.map(({ data, mime }) => doc.createTexture().setImage(data).setMimeType(mime));
      const materials = {};
      function attr(data, type) {
        return doc.createAccessor().setBuffer(buffer).setType(type).setArray(data);
      }
      for (const part of parts) {
        if (part.strength > 0) emitters++;
        // Keep the shop fittings distinct through the packer's material-based join.
        const materialKey = [part.alpha, part.strength, part.edge, part.shopFitting].join("-");
        const mat = (materials[materialKey] ??= doc
          .createMaterial("ItemSpec " + part.alpha)
          .setExtras({ materialSpecNormalFrame: true, ...(part.shopFitting ? { worldSpecOutline: false } : {}) })
          .setDoubleSided(true)
          .setAlphaMode(part.alpha)
          .setBaseColorTexture(textures[0])
          .setNormalTexture(textures[1])
          .setMetallicRoughnessTexture(textures[2]));
        const sources = mat.getExtras().sourceFinishes ?? [];
        if (!sources.includes(part.sourceFinish)) mat.setExtras({ ...mat.getExtras(), sourceFinishes: [...sources, part.sourceFinish].sort() });
        if (part.strength > 0 && !mat.getEmissiveTexture()) {
          mat.setEmissiveTexture(textures[keys.indexOf("emissiveMap")]).setEmissiveFactor([1, 1, 1]);
          if (part.strength > 1) mat.setExtension("KHR_materials_emissive_strength",
            emissiveStrength.createEmissiveStrength().setEmissiveStrength(part.strength));
        }
        if (part.edge && !mat.getOcclusionTexture()) {
          // A zero-strength occlusion texture carries two linear data channels.
          // Readers without the MaterialSpec shader see ordinary glTF PBR.
          mat.setOcclusionTexture(textures[keys.indexOf("edge")]).setOcclusionStrength(0);
          mat.setExtras({ ...mat.getExtras(), materialSpecEdge: { from: part.edge[0], to: part.edge[1] } });
        }
        for (const info of [mat.getBaseColorTextureInfo(), mat.getNormalTextureInfo(),
          mat.getMetallicRoughnessTextureInfo(), mat.getEmissiveTextureInfo(), mat.getOcclusionTextureInfo()]) {
          info?.setMinFilter(TextureInfo.MinFilter.LINEAR_MIPMAP_LINEAR);
        }
        const pos = attr(part.pos, "VEC3");
        const prim = doc
          .createPrimitive()
          .setMaterial(mat)
          .setAttribute("POSITION", pos)
          .setAttribute("NORMAL", attr(part.norm, "VEC3"))
          .setAttribute("_MS_ASPECT", doc.createAccessor().setType("SCALAR").setBuffer(buffer)
            .setArray(new Float32Array(pos.getCount()).fill(part.aspect)))
          .setAttribute("TEXCOORD_0", attr(part.uv, "VEC2"))
          .setAttribute(
            "COLOR_0",
            attr(part.color, "VEC3").setNormalized(true),
          );
        if (part.index)
          prim.setIndices(attr(part.index, "SCALAR"));
        mesh.addPrimitive(prim);
        tris += (prim.getIndices() ?? pos).getCount() / 3;
      }
    }
    const temporary = target + "." + randomUUID() + ".tmp.glb";
    try {
      await new NodeIO().registerExtensions([KHRMaterialsEmissiveStrength]).write(temporary, doc);
      await writeFile(temporary + ".sha256", key + " " + await fileDigest(temporary));
      await rename(temporary, target);
      await rename(temporary + ".sha256", target + ".sha256");
    } finally {
      await rm(temporary, { force: true });
      await rm(temporary + ".sha256", { force: true });
    }
    console.log(name, tris, "triangles", count, "texture pages", emitters, "emissive charts", nativeMeshes.length, "native mesh groups");
    baked.close();
  } finally {
    host?.close();
  }
}
