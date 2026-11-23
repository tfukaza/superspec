import { writeImmutable } from "./immutable-file.mjs";
import { readFile, writeFile, rename, rm, mkdir } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash, randomUUID } from "node:crypto";
import {
  readSourceLabels,
  pigmentSignature,
  labelsFor,
} from "./material-provenance.mjs";
import { coalesceBindings, mergeEdgeVariants } from "./coalesce-materials.mjs";
import { reflectionPanorama } from "../../adapters/three/src/paintedMetalSky.ts";
import sharp from "sharp";
import { GeometryReader, entryBytes } from "./city-lod.mjs";
import { resolveSubmaterials } from "../../tools/materialspec/submaterials.mjs";
import {
  markCreases,
  strokeMask,
  paintPigment,
  metalCoverage,
} from "./metal-textures.mjs";

const readJson = async (name) =>
  JSON.parse(await readFile(new URL(name, import.meta.url), "utf8"));
const digest = (b) => createHash("sha256").update(b).digest("hex").slice(0, 24);
const known = {
  "restaurant-food": "Food and crockery",
  "restaurant-tiles": "Glazed ceramic tiles",
  "display-glass": "Display glass",
  "kitchen-worktop": "Brushed steel worktop",
  "restaurant-surfaces": "Dining furniture and fittings",
  "kitchen-metal": "Kitchen hardware",
  "shop-register": "Register housing and keys",
  "market-hero-props": "Bucket and stools with hardware",
  "grocer-props": "Bottles, packages and closures",
  "market-trays": "Market trays and contents",
  "snack-packet": "Printed snack packets",
  "m-pave": "Stone paving",
  "m-floor-paving": "Floor paving",
  "m-dark": "Dark architectural finish",
  "m-yellow": "Yellow markings",
  "m-lamp": "Lamp lens",
  "m-wood": "Timber",
  "m-orange-paving": "Orange paving",
  "m-teal-paving": "Teal paving",
  "m-blue-paving": "Blue paving",
};
function describe(mat, owners, metal, sheet) {
  const id = mat.extras?.sourceName ?? mat.name,
    key = id.replace(/^builtin:/, ""),
    e = mat.extras || {};
  const packed = id.startsWith("ItemSpec "),
    concrete = e.worldSpecConcrete;
  const family = concrete
    ? "Masonry"
    : packed
      ? "Building atlases"
      : id.startsWith("builtin:")
        ? "Shop inventory"
        : /canopy|bark|weed|grass|turf|studio\/import/i.test(id)
          ? "Vegetation"
          : id.startsWith("m-")
            ? "Architecture"
            : "Fixtures and trim";
  const surface = concrete
    ? "Concrete"
    : mat.alphaMode === "BLEND"
      ? "Transparent surfaces"
      : metal
        ? "Mixed surfaces with metal"
        : "Opaque surfaces";
  const regions = e.sourceFinishes?.join(", ");
  const only = (test) =>
    e.sourceFinishes?.length && e.sourceFinishes.every((r) => test.test(r));
  const masonry = concrete || only(/^(?:wall|coping|roof) \/| \/ concrete$/);
  const section = only(/ \/ tube-light$/)
    ? "Lamp lenses"
    : only(/ \/ (?:glass|pane)$/)
      ? "Glazing"
      : concrete
        ? "Balcony concrete"
        : mat.alphaMode === "BLEND"
          ? "Glazing"
          : e.worldSpecOutline === false
            ? "Shop fittings"
            : metal
              ? "Architectural hardware"
              : "Architectural finishes";
  const label = packed
    ? [owners.join(" + "), section, "sheet " + sheet].join(" / ")
    : (known[key] ??
      (id.startsWith("studio/import/") ? "Weed blade pigment" : id));
  const small =
    e.worldSpecOutline === false ||
    (id.startsWith("builtin:") &&
      !["kitchen-worktop", "kitchen-metal", "restaurant-surfaces"].includes(
        key,
      ));
  let finish;
  if (metal)
    finish =
      key === "kitchen-worktop"
        ? "polished-worktop"
        : small
          ? "stock-hardware"
          : e.worldSpecPipe ||
              /rust|oxid|weathered/i.test(id + " " + (regions ?? ""))
            ? "weathered-steel"
            : e.worldSpecDuct || /alumin|kitchen|brushed/i.test(id)
              ? "brushed-steel"
              : packed
                ? "weathered-steel"
                : "painted-metal";
  const recipe =
    finish ??
    (masonry
      ? "architectural-concrete"
      : packed
        ? mat.alphaMode === "BLEND"
          ? "architectural-glazing"
          : "opaque-building-atlas"
        : key);
  return {
    family: masonry ? "Masonry" : family,
    label,
    surface: masonry ? "Concrete and masonry" : surface,
    recipe,
    finish,
    wear: concrete
      ? "masonry"
      : finish === "weathered-steel"
        ? "worn"
        : finish
          ? "maintained"
          : undefined,
    reflection: finish
      ? small
        ? "virtual"
        : "box"
      : concrete || e.worldSpecAggregate
        ? "masonry"
        : "none",
  };
}
export async function publishedReflectionField() {
  const field = await readJson("./reflection-probes.json");
  const linear = Float32Array.from({ length: 256 }, (_, i) =>
    i <= 10 ? i / (255 * 12.92) : ((i / 255 + 0.055) / 1.055) ** 2.4,
  );
  for (const probe of field.probes) {
    const rgb = Buffer.from(probe.rgb, "base64");
    if (rgb.length !== field.width * field.height * 3)
      throw Error("Incomplete reflection panorama " + probe.id);
    const filtered = reflectionPanorama(
      Float32Array.from(rgb, (b) => linear[b]),
      field.width,
      field.height,
      probe.id,
    );
    for (let i = 0; i < rgb.length; i++)
      rgb[i] = filtered.image.data[4 * Math.floor(i / 3) + (i % 3)];
    filtered.dispose();
    probe.rgb = rgb.toString("base64");
  }
  field.encoding = "kuwahara-linear-rgb8";
  return field;
}
export async function attachMaterialLibrary(
  json,
  destination,
  prototypeDirectory,
) {
  const scene = json.scenes[json.scene ?? 0],
    catalog = await readJson(
      "../../crates/engine/src/worldspec/themes/kowloon/materials/metal-finishes.json",
    );
  const finishes = resolveSubmaterials(catalog.parameters, catalog.finishes);
  scene.extras.worldSpecFinishes = {
    version: 1,
    finishes,
    wear: resolveSubmaterials(catalog.wearParameters, catalog.wear),
  };
  scene.extras.worldSpecReflectionProbes = await publishedReflectionField();
  const dir = dirname(destination),
    folder = basename(destination, ".glb") + "-data",
    reader = new GeometryReader(json, dir);
  // The old export shares a white vertex-color material between grass blades and
  // the tapered steel street lamp. Split by its original ItemSpec slot.
  for (const mesh of json.meshes.filter((m) =>
    m.name.endsWith("/lamp/lamp/metal"),
  )) {
    for (const p of mesh.primitives) {
      const old = json.materials[p.material];
      if (!old.name.startsWith("studio/import/")) continue;
      const name = "Weathered street-lamp steel";
      let index = json.materials.findIndex((m) => m.name === name);
      if (index < 0)
        index =
          json.materials.push({
            ...old,
            name,
            pbrMetallicRoughness: {
              ...old.pbrMetallicRoughness,
              metallicFactor: 0.54,
              roughnessFactor: 0.63,
            },
          }) - 1;
      p.material = index;
    }
  }
  const aliases = await coalesceBindings(json, dir);
  const uses = json.materials.map(() => []);
  for (const [i, mesh] of json.meshes.entries()) {
    const scale = json.nodes.find((n) => n.mesh === i)?.scale ?? [1, 1, 1];
    for (const prim of mesh.primitives)
      uses[prim.material].push({ mesh: mesh.name, prim, scale });
  }
  const labels = await readSourceLabels(prototypeDirectory, [
    ...new Set(json.meshes.map((m) => m.name)),
  ]);
  const signatures = new Map(),
    groups = new Map(),
    baked = new Map();
  await mkdir(resolve(dir, folder), { recursive: true });
  for (const [i, mat] of json.materials.entries()) {
    const pbr = mat.pbrMetallicRoughness ?? {};
    mat.extras ??= {};
    const original = {
      pigment: pbr.baseColorTexture?.index,
      properties: pbr.metallicRoughnessTexture?.index,
    };

    const group = original.properties ?? -1;
    if (!groups.has(group)) groups.set(group, []);
    groups.get(group).push({ i, mat, pbr, original });
  }
  async function image(index) {
    const img = json.images[json.textures[index].source],
      v = json.bufferViews[img.bufferView];
    const bytes = await readFile(resolve(dir, json.buffers[v.buffer].uri));
    return bytes.subarray(
      v.byteOffset ?? 0,
      (v.byteOffset ?? 0) + v.byteLength,
    );
  }
  async function store(bytes, source, label) {
    const uri = folder + "/finish-" + digest(bytes) + ".bin",
      key = uri + "-" + json.textures[source].sampler;
    if (baked.has(key)) return baked.get(key);
    let buffer = json.buffers.findIndex((b) => b.uri === uri),
      texture;
    if (buffer < 0) {
      await writeImmutable(resolve(dir, uri), bytes);
      buffer = json.buffers.push({ uri, byteLength: bytes.length }) - 1;
    }
    let view = json.bufferViews.findIndex((v) => v.buffer === buffer);
    if (view < 0)
      view = json.bufferViews.push({ buffer, byteLength: bytes.length }) - 1;
    let img = json.images.findIndex((i) => i.bufferView === view);
    if (img < 0)
      img =
        json.images.push({
          name: label,
          bufferView: view,
          mimeType: "image/png",
        }) - 1;
    texture = json.textures.findIndex(
      (t) => t.source === img && t.sampler === json.textures[source].sampler,
    );
    if (texture < 0)
      texture =
        json.textures.push({ ...json.textures[source], source: img }) - 1;
    baked.set(key, texture);
    return texture;
  }
  const inventory = [];
  for (const [source, rows] of groups) {
    // Skip architectural concrete whose glTF factor defaults to 1 but whose finish is dielectric.
    let pixels, w, h;
    if (
      source >= 0 &&
      rows.some(
        (r) => !r.mat.extras.worldSpecConcrete && r.mat.alphaMode !== "BLEND",
      )
    ) {
      const { data, info } = await sharp(await image(source))
        .ensureAlpha()
        .raw()
        .toBuffer({ resolveWithObject: true });
      pixels = data;
      w = info.width;
      h = info.height;
    }
    // The residual is painted at a quarter of the authored texel density: 512 texels of a 2048
    // page, the same share of a larger one, and more of a page already baked at a reduced
    // density. Its crease strokes are a texel or two wide, so that density sets their width on
    // the surface. It is stored at a quarter of its own page, as it always was.
    const density = Math.min(1, ...rows.map((r) => r.mat.extras?.worldSpecTexelDensity ?? 1));
    const share = pixels ? Math.max(512 / Math.max(w, h), 0.25) : 1;
    const scale = Math.min(1, Math.max(share, 0.25 / density));
    const mw = Math.max(1, Math.round(w * scale)),
      mh = Math.max(1, Math.round(h * scale));
    const stored = scale > share
      ? { width: Math.max(1, Math.round(w * share)), height: Math.max(1, Math.round(h * share)), fit: "fill" }
      : undefined;
    const mask = pixels ? new Uint8Array(mw * mh) : null,
      painted = [];
    let coverage;
    for (const { i, mat, pbr, original } of rows) {
      let peak = 0,
        containsMetal = false;
      const owners = [...new Set(uses[i].map((u) => u.mesh))].sort();
      if (
        !mat.extras.sourceFinishes?.length &&
        (mat.extras.sourceName ?? mat.name).startsWith("ItemSpec ") &&
        labels.size &&
        original.pigment !== undefined
      ) {
        if (!signatures.has(original.pigment))
          signatures.set(
            original.pigment,
            await pigmentSignature(await image(original.pigment)),
          );
        const recovered = labelsFor(
          labels,
          signatures.get(original.pigment),
          mat,
        );
        if (recovered.length) mat.extras.sourceFinishes = recovered;
      }
      const regions = mat.extras.sourceFinishes ?? [];
      const sourceName = mat.extras.sourceName ?? mat.name;
      const fixtureBinding = mat.extras.worldSpecTwinStrip
        ? catalog.twinStripBindings[sourceName]
        : undefined;
      const coated = (finishes[fixtureBinding?.finish]?.coatingResponse ?? 0) > 0;
      // Small nonzero values in JPEG property pages can be quantization bleed
      // on walls, lenses and tiles. Respect unambiguous source assignments.
      const dielectric =
        regions.length > 0 &&
        regions.every(
          (r) =>
            /^(?:wall|roof|coping|tiles?-?\d*|tileStart|tileEnd) \/ (?:side|top|bottom|edge|front|reveal)$/.test(
              r,
            ) ||
            / \/ (?:concrete|wall|board|pane|glass|tube-light|balcony-plants)$/.test(
              r,
            ),
        );
      const checkMetal =
        !dielectric &&
        mat.alphaMode !== "BLEND" &&
        !mat.extras.worldSpecConcrete &&
        (pbr.metallicFactor ?? 1) > 0;
      const paths = uses[i];
      for (const { prim, scale } of (checkMetal || coated) && pixels ? paths : []) {
        const uvIndex = prim.attributes.TEXCOORD_0;
        if (uvIndex === undefined) continue;
        const uv = await reader.read(uvIndex);
        const idx = prim.indices === undefined
          ? Uint32Array.from({ length: uv.length / 2 }, (_, i) => i)
          : await reader.read(prim.indices, Uint32Array);
        let localPeak = 0;
        const sample = (u, v) => {
          const x = Math.min(w - 1, Math.max(0, Math.floor(u * w))),
            y = Math.min(h - 1, Math.max(0, Math.floor(v * h)));
          return pixels[(y * w + x) * 4 + 2];
        };
        for (let n = 0; n < idx.length; n += 3) {
          const a = 2 * idx[n],
            b = 2 * idx[n + 1],
            c = 2 * idx[n + 2];
          localPeak = Math.max(
            localPeak,
            sample(
              (uv[a] + uv[b] + uv[c]) / 3,
              (uv[a + 1] + uv[b + 1] + uv[c + 1]) / 3,
            ),
            sample(
              uv[a] * 0.96 + 0.02 * (uv[b] + uv[c]),
              uv[a + 1] * 0.96 + 0.02 * (uv[b + 1] + uv[c + 1]),
            ),
          );
        }
        peak = Math.max(peak, localPeak * (pbr.metallicFactor ?? 1));
        const active =
          localPeak > 14 || (coverage ??= metalCoverage(pixels, w, h))(uv, idx);
        containsMetal ||= active;
        if (active || coated)
          markCreases(
            await reader.read(prim.attributes.POSITION),
            idx,
            uv,
            scale,
            (u, v, s, t) => strokeMask(mask, mw, mh, u, v, s, t),
          );
      }
      const metal =
        checkMetal &&
        (source < 0 ? (pbr.metallicFactor ?? 1) > 0.055 : containsMetal);
      const binding = describe(mat, owners, metal, original.pigment);
      const override =
        fixtureBinding ??
        catalog.bindings?.[sourceName] ??
        catalog.bindings?.[binding.label];
      if (override && (metal || coated)) {
        if (
          !finishes[override.finish] ||
          !scene.extras.worldSpecFinishes.wear[override.wear] ||
          !["virtual", "box"].includes(override.reflection)
        )
          throw Error("Invalid submaterial binding for " + binding.label);
        Object.assign(binding, override, { recipe: override.finish });
      }
      mat.extras.worldSpecSurface = binding;
      if (
        sourceName.startsWith("ItemSpec ") ||
        sourceName.startsWith("studio/import/")
      ) {
        mat.extras.sourceName = sourceName;
        mat.name = binding.label;
      }
      if (binding.finish) painted.push({ mat, pbr, original, binding });
      inventory.push({
        index: i,
        source: sourceName,
        sourceRecords: aliases.flatMap((v, k) => (v === i ? [k] : [])),
        regions: mat.extras.sourceFinishes,
        ...binding,
        owners,
        sampledPeakMetalness:
          source < 0 ? (pbr.metallicFactor ?? 1) : +(peak / 255).toFixed(3),
      });
    }
    if (!painted.length || !pixels) continue;
    // A small residual preserves the full-resolution original and its normal/roughness maps.
    // RGB is the signed pigment correction in log space, A is the geometry crease.
    const resized = await sharp(pixels, {
      raw: { width: w, height: h, channels: 4 },
    })
      .resize(mw, mh, { fit: "fill" })
      .raw()
      .toBuffer();
    const corrections = new Map();
    for (const { mat, original } of painted) {
      const pigment = original.pigment;
      if (!corrections.has(pigment)) {
        const out = Buffer.alloc(mw * mh * 4, 128);
        if (pigment !== undefined) {
          const color = await sharp(await image(pigment))
            .resize(mw, mh, { fit: "fill" })
            .ensureAlpha()
            .raw()
            .toBuffer();
          const filtered = paintPigment(color, resized, mw, mh, 1);
          for (let i = 0; i < out.length; i += 4)
            for (let c = 0; c < 3; c++) {
              const ratio = (filtered[i + c] + 4) / (color[i + c] + 4);
              out[i + c] = Math.min(
                255,
                Math.max(0, 128 + Math.round(64 * Math.log2(ratio))),
              );
            }
        }
        for (let i = 0; i < mask.length; i++) out[4 * i + 3] = mask[i];
        const residual = () => sharp(out, { raw: { width: mw, height: mh, channels: 4 } });
        // The crease in alpha is data beside the correction, not its coverage: reduce them apart.
        const reduced = stored && sharp(await residual().removeAlpha().resize(stored).raw().toBuffer(),
          { raw: { width: stored.width, height: stored.height, channels: 3 } })
          .joinChannel(await residual().extractChannel(3).resize(stored).raw().toBuffer(),
            { raw: { width: stored.width, height: stored.height, channels: 1 } });
        corrections.set(
          pigment,
          await store(
            await (reduced || residual()).png().toBuffer(),
            source,
            "Metal pigment correction and crease mask",
          ),
        );
      }
      mat.extras.worldSpecSurface.detailTexture = corrections.get(pigment);
    }
  }
  return mergeEdgeVariants(
    json,
    inventory.sort((a, b) => a.index - b.index),
    uses,
    reader,
    async (id) =>
      sharp(await image(id))
        .ensureAlpha()
        .raw()
        .toBuffer({ resolveWithObject: true }),
  );
}
if (
  process.argv[1] &&
  pathToFileURL(resolve(process.argv[1])).href === import.meta.url
) {
  const path = resolve(process.argv[2]),
    before = await readFile(path),
    json = JSON.parse(
      before.toString("utf8", 20, 20 + before.readUInt32LE(12)),
    );
  const rows = await attachMaterialLibrary(json, path, process.argv[4]);
  const stage = path + "." + randomUUID() + ".tmp";
  try {
    await writeFile(stage, entryBytes(json), { flag: "wx" });
    if (!(await readFile(path)).equals(before))
      throw Error("Map changed while processing materials");
    await rename(stage, path);
  } finally {
    await rm(stage, { force: true });
  }
  if (process.argv[3])
    await writeFile(process.argv[3], JSON.stringify(rows, null, 2));
  console.log(
    rows.length,
    "materials,",
    rows.filter((r) => r.finish).length,
    "with metal finishes",
  );
}
