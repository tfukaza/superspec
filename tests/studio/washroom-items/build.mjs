/**
 * Writes the washroom built from items: `generated/washroom-items/washroom-items.studio.json`, a version 2 Studio
 * project whose models are ItemSpec definitions (`items.mjs`) with the materials of the GLB washroom
 * (`../washroom/build.mjs`), and the definitions beside it for `superspec-studio inspect`.
 * Usage: node tests/studio/washroom-items/build.mjs [resolution] [texels per meter, 0 for none]
 */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";
import { materials as washroomMaterials, lawnMaterials, treeMaterials } from "../washroom/build.mjs";
import { buildBatches, buildMeshes, lawnGround } from "../washroom/model.mjs";
import { Mesh, writeGlb } from "../washroom/geometry.mjs";
import { models, facade, building } from "./items.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const examples = resolve(here, "../../../crates/engine/src/materialspec/program/examples");
const out = resolve(here, "../../../generated/washroom-items");
const concrete = "kowloon-concrete.material.ts", steel = "kowloon-rusted-steel.material.ts";

/**
 * A shipped recipe with weathering that follows the named features of its receiver's owner: `layers` (TypeScript
 * source of `layer(...)` calls) are added to the recipe's material and `declarations` before it. The shipped recipes
 * read no named feature yet (the roadmap's weathering layers will); this stands in for them in this project only.
 */
function following(source, declarations, layers) {
  const imports = "import {\n  defineMaterial,", material = "export default defineMaterial({";
  const start = source.indexOf(material), end = source.indexOf("\n});", start);
  if (!source.includes(imports) || start < 0 || end < 0 || /\blayers:/.test(source.slice(start, end))) throw new Error("The recipe is not shaped as this script expects.");
  return `${source.slice(0, start).replace(imports, "import {\n  layer, featureDistance, defineMaterial,")}${declarations}\n${source.slice(start, end)}\n  layers: [\n${layers}\n  ],${source.slice(end)}`;
}

/** Concrete whose grime follows a wall's `base` and a window's `sill`. */
const itemConcrete = source => following(source,
  `const featureGrime = param.scalar({ default: 0.8, range: [0, 1], label: "Feature grime" });`,
  `    layer({ id: "featureBase", mask: mul(featureGrime, mul(0.85, sub(1, smoothstep(0, 0.3, featureDistance("base", 0.3))))), baseColor: vec3(0.035, 0.032, 0.026), roughness: 0.95 }),
    layer({ id: "featureSill", mask: mul(featureGrime, sub(1, smoothstep(0.02, 0.22, featureDistance("sill", 0.22)))), baseColor: vec3(0.03, 0.03, 0.028), roughness: 0.92 }),`);
/** Steel whose rust follows a route's `joint`s: its seams, flanges and couplings. */
const itemSteel = source => following(source,
  `const jointRust = param.scalar({ default: 0.9, range: [0, 1], label: "Joint rust" });`,
  `    layer({ id: "featureJoint", mask: mul(jointRust, sub(1, smoothstep(0.03, 0.12, featureDistance("joint", 0.12)))), baseColor: oxideColor, roughness: rustRoughness }),`);

/** An RGBA image drawn by `paint(x, y)` (each in 0 to 1, y up), as PNG bytes. */
function image(width, height, paint) {
  const png = new PNG({ width, height });
  for (let row = 0; row < height; row++) for (let column = 0; column < width; column++) {
    const [r, g, b, a] = paint((column + 0.5) / width, 1 - (row + 0.5) / height);
    png.data.set([r, g, b, a].map(value => Math.round(Math.max(0, Math.min(1, value)) * 255)), (row * width + column) * 4);
  }
  return PNG.sync.write(png);
}
const inside = (value, low, high) => value >= low && value < high;
/** A seven-segment digit in the unit square: which segments are lit. */
function digit(lit, x, y) {
  const segments = { a: [0.15, 0.85, 0.86, 1], g: [0.15, 0.85, 0.43, 0.57], d: [0.15, 0.85, 0, 0.14], f: [0, 0.18, 0.5, 1], b: [0.82, 1, 0.5, 1], e: [0, 0.18, 0, 0.5], c: [0.82, 1, 0, 0.5] };
  return [...lit].some(name => inside(x, segments[name][0], segments[name][1]) && inside(y, segments[name][2], segments[name][3]));
}
/** The scene's decal images: an enamel notice, a stencilled number and a pipe label. */
export function decalImages() {
  return {
    // A white enamel sign with a red border, a red disc and lines of text.
    "decals/notice.png": image(96, 128, (x, y) => {
      const border = Math.min(x, 1 - x) * 0.75 < 0.05 || Math.min(y, 1 - y) < 0.05;
      const disc = Math.hypot((x - 0.5) * 0.75, y - 0.72) < 0.15;
      const text = inside(x, 0.16, 0.84) && [0.44, 0.34, 0.24, 0.14].some(line => inside(y, line, line + 0.045)) && !(inside(y, 0.14, 0.2) && x > 0.6);
      return border || disc ? [0.62, 0.07, 0.05, 1] : text ? [0.1, 0.1, 0.12, 1] : [0.9, 0.89, 0.84, 1];
    }),
    // "07" in dark paint: only the strokes are opaque, so the concrete shows between them.
    "decals/number.png": image(120, 60, (x, y) => {
      const zero = digit("abcdef", (x - 0.08) / 0.34, (y - 0.1) / 0.8), seven = digit("abc", (x - 0.58) / 0.34, (y - 0.1) / 0.8);
      const lit = (inside(x, 0.08, 0.42) && inside(y, 0.1, 0.9) && zero) || (inside(x, 0.58, 0.92) && inside(y, 0.1, 0.9) && seven);
      return [0.06, 0.07, 0.09, lit ? 0.9 : 0];
    }),
    // A yellow band with a black arrow along the pipe.
    "decals/label.png": image(128, 80, (x, y) => {
      const shaft = inside(x, 0.42, 0.58) && inside(y, 0.15, 0.6);
      const head = y >= 0.55 && y < 0.88 && Math.abs(x - 0.5) < (0.88 - y) * 0.75;
      return shaft || head ? [0.05, 0.05, 0.05, 1] : [0.86, 0.66, 0.06, 1];
    }),
  };
}

/** The project's materials: the GLB washroom's parameter sets, bound to item slots, plus glass for the window. */
export function buildMaterials({ weathering = true } = {}) {
  const variants = { "concrete-walls": "item-concrete.material.ts", galvanized: "item-steel.material.ts", "rusted-pipe": "item-steel.material.ts" };
  const plain = { bandedGloss: 0, silhouette: 0, polishedEdges: 0 };
  return [
    ...washroomMaterials.map(material => ({ ...material, entry: (weathering && variants[material.id]) || material.entry, seed: 7, meshes: [] })),
    { id: "glass", entry: steel, parameters: { ...plain, substrate: [0.03, 0.07, 0.08], metalness: 0.3, roughness: 0.08, edgeRust: 0, surfaceRust: 0, rainRust: 0.15 }, seed: 7, meshes: [] },
  ];
}

/**
 * The Kowloon items' own materials: their render (the anchored concrete, an intrinsic base measured on each part's
 * pattern anchor, with no grime of its own: the items' weathering layers add it where their features say), the air
 * conditioners' enamelled sheet, awning canvas and potted leaves.
 */
const anchoredConcrete = "kowloon-concrete-anchored.material.ts";
/** The weathering layer recipes the Kowloon items name, and the base they go over. */
export const kowloonLayerModules = [anchoredConcrete, "kowloon-ground.material.ts", "kowloon-paint-coat.material.ts", "kowloon-runoff.material.ts", "kowloon-grime-streaks.material.ts", "kowloon-top-grime.material.ts", "kowloon-ground-stain.material.ts", "kowloon-moss.material.ts",
  "kowloon-wall-dirt.material.ts", "kowloon-soot-band.material.ts", "kowloon-notices.material.ts", "kowloon-rust-run.material.ts", "kowloon-repairs.material.ts",
  // The fixtures' own recipes: glazing, wall tiles, air conditioner casings and awning canvas; the street's paving and asphalt.
  "kowloon-window-glass.material.ts", "kowloon-wall-tile.material.ts", "kowloon-aircon.material.ts", "kowloon-canvas.material.ts",
  "kowloon-paving.material.ts", "kowloon-asphalt.material.ts", "kowloon-poster-print.material.ts",
  "kowloon-concrete-edges.material.ts", "kowloon-edge-ink.material.ts", "kowloon-sign-board.material.ts", "kowloon-sign-lettering.material.ts", "kowloon-tube-light.material.ts"];
function facadeMaterials() {
  const plain = { bandedGloss: 0, silhouette: 0, polishedEdges: 0 };
  return [
    // The reference shop house's cream render: the concrete's plaster coat, with patches lost to the concrete beneath.
    { id: "render-paint", entry: anchoredConcrete, parameters: { plaster: false, cutouts: false, holes: false, joints: false, chipAmount: 0.1, chipRelief: 0.001, darknessA: 0.12, darknessB: 0.12, blendContrast: 1, hueAmount: 0, hueAngle: 60 }, seed: 11, meshes: [] },
    // The fixtures are drawn with their paint as vertex tints (a frame's green, a cage's khaki, an awning's stripes), which
    // the picture keeps over their baked looks (`?bakedtints=on`): so their materials are pale, and add only what age
    // does to the paint: dullness, dirt and rust.
    // Painted steel window frames and railings.
    { id: "frame-green", entry: steel, parameters: { ...plain, substrate: [0.8, 0.8, 0.77], metalness: 0.1, roughness: 0.7, edgeRust: 0.5, surfaceRust: 0.14, rainRust: 0.55 }, seed: 7, meshes: [] },
    { id: "enamel", entry: steel, parameters: { ...plain, substrate: [0.78, 0.76, 0.7], metalness: 0.1, roughness: 0.45, edgeRust: 0.3, surfaceRust: 0.05, rainRust: 0.4 }, seed: 7, meshes: [] },
    // Old glazing with the room painted into it, and what stands behind an open or unglazed light (tinted: the dark of
    // the room, its curtains and drapes).
    { id: "window-glass", entry: "kowloon-window-glass.material.ts", parameters: { tint: [0.135, 0.127, 0.112], sky: 1, lamps: 0.22, dirt: 0.6 }, seed: 7, meshes: [] },
    { id: "room-dark", entry: steel, parameters: { ...plain, substrate: [0.85, 0.85, 0.85], metalness: 0, roughness: 0.92, edgeRust: 0, surfaceRust: 0, rainRust: 0 }, seed: 7, meshes: [] },
    // The spandrels' small grey-green tiles, dirty in the joints.
    { id: "facade-tile", entry: "kowloon-wall-tile.material.ts", parameters: { tileSize: 0.065, glaze: [0.205, 0.26, 0.19], spare: [0.235, 0.29, 0.22], replaced: 0.012, missing: 0.03, variation: 0.12, grime: 0.8 }, seed: 7, meshes: [] },
    // Fixtures: printed air conditioner casings, painted steel gone dull and rusty, and thin galvanized sheet.
    { id: "aircon-casing", entry: "kowloon-aircon.material.ts", parameters: { yellowing: 0.25 }, seed: 7, meshes: [] },
    // The plastered walls and floor inside an open shop (tinted): pale, matt and a little grimy.
    { id: "shop-plaster", entry: steel, parameters: { ...plain, substrate: [0.82, 0.8, 0.76], metalness: 0, roughness: 0.92, edgeRust: 0, surfaceRust: 0.06, rainRust: 0.2 }, seed: 3, meshes: [] },
    { id: "poster-print", entry: "kowloon-poster-print.material.ts", parameters: {}, seed: 7, meshes: [] },
    // Signs: the painted ground, worn and stained, and its lettering with paint lost from the strokes (both tinted).
    { id: "sign-board", entry: "kowloon-sign-board.material.ts", parameters: { flaking: 0.8, fading: 0.65, grime: 0.75, rust: 0.7 }, seed: 7, meshes: [] },
    { id: "sign-lettering", entry: "kowloon-sign-lettering.material.ts", parameters: { wear: 0.3, fading: 0.3 }, seed: 7, meshes: [] },
    // Lit tubes in the shops, and a shop's concrete floor: grey, lightly soiled, never dark.
    { id: "tube-light", entry: "kowloon-tube-light.material.ts", parameters: {}, seed: 7, meshes: [] },
    { id: "shop-floor", entry: "kowloon-paving.material.ts", parameters: { flag: [3, 3], stagger: 0, jointWidth: 0.002, cracked: 0, fractures: 0.3, stone: [0.62, 0.61, 0.58], variation: 0.04, grime: 0.25, spots: 0.25 }, seed: 4, meshes: [] },
    { id: "aged-paint", entry: steel, parameters: { ...plain, substrate: [0.8, 0.79, 0.75], metalness: 0.1, roughness: 0.72, edgeRust: 0.7, surfaceRust: 0.3, rainRust: 0.6 }, seed: 7, meshes: [] },
    { id: "aged-sheet", entry: steel, parameters: { ...plain, substrate: [0.78, 0.78, 0.76], metalness: 0.4, roughness: 0.6, edgeRust: 0.8, surfaceRust: 0.35, rainRust: 0.7 }, seed: 7, meshes: [] },
    { id: "aged-metal", entry: steel, parameters: { ...plain, substrate: [0.72, 0.72, 0.7], metalness: 0.35, roughness: 0.6, edgeRust: 0.75, surfaceRust: 0.3, rainRust: 0.7 }, seed: 5, meshes: [] },
    // Awning canvas, dyed by its tints (plain or striped): the weave, bleaching, soil and stains.
    { id: "canvas", entry: "kowloon-canvas.material.ts", parameters: { clothA: [0.86, 0.85, 0.82], clothB: [0.86, 0.85, 0.82], striped: 0, bleaching: 0, soil: 0.22 }, seed: 7, meshes: [] },
    // The street: flagged pavement and its kerb (their stones shaded by tints), and the asphalt beyond.
    // (The pavement item lays its own stones, so the recipe's flags are made too large to show: one stone a receiver.)
    { id: "paving", entry: "kowloon-paving.material.ts", parameters: { flag: [3, 3], stagger: 0, jointWidth: 0.002, cracked: 0, stone: [0.52, 0.51, 0.48], variation: 0.05, grime: 1 }, seed: 7, meshes: [] },
    { id: "kerb", entry: "kowloon-paving.material.ts", parameters: { flag: [3, 3], stagger: 0, jointWidth: 0.002, cracked: 0, stone: [0.5, 0.49, 0.46], variation: 0.05, spots: 0.3, grime: 0.9 }, seed: 9, meshes: [] },
    { id: "asphalt", entry: "kowloon-asphalt.material.ts", parameters: { binder: [0.105, 0.105, 0.1], cracks: 0, patches: 0.1, wearing: 0.12, stains: 0.12 }, seed: 7, meshes: [] },
    { id: "leaves", entry: steel, parameters: { ...plain, substrate: [0.08, 0.22, 0.06], metalness: 0, roughness: 0.75, edgeRust: 0, surfaceRust: 0, rainRust: 0 }, seed: 7, meshes: [] },
  ];
}

/**
 * `weathering: false` uses the shipped recipes as they are (nothing follows a feature); `batches: false` leaves out the
 * bolts of the brackets and the vent, which are the GLB washroom's instanced batch without its two flange rings (the
 * pipe item makes its own flange bolts).
 */
export function buildProject(resolution = 512, texelDensity = 128, { weathering = true, batches = true, anchored = false, walls, facade: withFacade = false, lane = null, room = true, grass = true } = {}) {
  const example = name => readFileSync(resolve(examples, name), "utf8");
  const modules = Object.fromEntries(["kowloon-noise.material.ts", concrete, steel, "tiled-courtyard.material.ts"].map(name => [name, example(name)]));
  modules["wood.material.ts"] = readFileSync(resolve(here, "../washroom/materials/wood.material.ts"), "utf8");
  if (weathering) {
    modules["item-concrete.material.ts"] = itemConcrete(modules[concrete]);
    modules["item-steel.material.ts"] = itemSteel(modules[steel]);
  }
  const definitions = {};
  for (const model of models) {
    definitions[`items/${model.definition.id}.item.json`] = JSON.stringify(model.definition, null, 2) + "\n";
  }
  Object.assign(modules, definitions);
  const assets = { ...decalImages() };
  for (const name of ["textures/concrete-plain.png", "textures/concrete-fine-sand.png"]) assets[name] = readFileSync(resolve(examples, name));
  const materials = buildMaterials({ weathering });
  // `walls` names another recipe for the walls (with its default parameters): one that reads no receiver bounds shows
  // what a resize leaves of a pattern when only the items decide it.
  if (walls) Object.assign(materials.find(material => material.id === "concrete-walls"), { entry: walls, parameters: {} });
  // `anchored`: the walls' concrete is measured from each part's pattern anchor, so a resize does not move it.
  const mappings = anchored ? { "concrete-walls": "anchored" } : {};
  // `room: false` leaves out the washroom's items and bolts (the concrete material demo), so only what `facade` and `lane` add is left.
  const scene = (room ? models : []).map(({ id, definition, transform, slots }) => ({ id, kind: "item", entry: `items/${definition.id}.item.json`, parameters: {}, transform, slots }));
  // `facade`: the Kowloon building side across the lane (true), or the whole building ("building"): composed items whose
  // sides and riser are sub-items and whose fixtures are native Kowloon generators. Their definitions are modules under
  // their own file names, as their entries name them.
  if (withFacade) {
    const model = withFacade === "building" ? building : facade;
    for (const [name, text] of Object.entries(model.definitions)) modules[name] = text;
    for (const name of kowloonLayerModules) modules[name] ??= example(name);
    mappings["render-paint"] = "anchored";
    // A round corner's wall (`kowloon-corner`, slot `wall-round`) is the same render read by the wall's own texture
    // coordinates: the distance round the building's outline and the height, in meters, which the anchored walls on
    // either side of the corner measure too, so the pattern runs through it.
    mappings["render-paint-round"] = "uv";
    // The signs' own slots stay plain, as the shipped catalog materials: the glyph atlas and the board.
    for (const name of ["lettering.png", "signboard.png", "grime-streaks.png"]) assets[`superspec-material:${name}`] = readFileSync(resolve(here, "../../../assets/materials", name));
    materials.push(...facadeMaterials());
    materials.push({ ...materials.find(material => material.id === "render-paint"), id: "render-paint-round", meshes: [] });
    scene.push({ id: model.id, kind: "item", entry: model.entry, parameters: { ...model.parameters }, transform: model.transform, slots: model.slots,
      // Small printed surfaces are baked finer than the walls (texels a meter): lettering strokes and poster print.
      slotDensities: { lettering: 800, posters: 640 } });
  }
  let batchList;
  if (batches && room) {
    // The bolt template is the GLB washroom's own mesh, alone in a model; it is baked once as flat steel.
    assets["bolt.glb"] = writeGlb(buildMeshes().filter(mesh => mesh.name === "bolt"));
    scene.push({ id: "bolt", kind: "glb", name: "bolt.glb" });
    const template = "bolt/node/0/mesh/0/primitive/0";
    materials.find(material => material.id === "flat-steel").meshes.push(template);
    batchList = buildBatches().map(batch => ({ ...batch, mesh: template, instances: batch.instances.slice(16) }));
  }
  // `lane` (a TreeSpec document, `lane-plants.json`): the lane between the room and the building, paved, with a patch of
  // painted grass toward its open end and trees and shrubs in front of the building.
  if (lane) {
    for (const name of [...lawnMaterials, ...treeMaterials].map(material => material.entry)) {
      modules[name] = readFileSync(resolve(here, "../washroom/materials", name), "utf8");
    }
    // `grass: false` leaves out the grass patch and its ground, keeping the trees and the paving.
    if (grass) {
      assets["lane.glb"] = writeGlb([lawnGround(new Mesh("lawn", [0.3, 0.48, 0.14]), lane.ground)]);
      scene.push({ id: "lane", kind: "glb", name: "lane.glb" });
    }
    // The road: a thin slab with a rounded rim, so its open edge does not read as a thick floating block.
    const paving = { format: "itemspec", version: 2, id: "lane-paving",
      parts: [{ id: "paving", generator: { kind: "box", size: [13, 15, 0.06], bevel: 0.025, bevelSegments: 2 }, slots: { side: "concrete", top: "concrete", bottom: "concrete" } }] };
    modules["items/lane-paving.item.json"] = JSON.stringify(paving, null, 2) + "\n";
    scene.push({ id: "paving", kind: "item", entry: "items/lane-paving.item.json", parameters: {}, transform: [[1, 0, 0], [0, 1, 0], [0, 0, 1], [-13.6, -9.5, -0.06]], slots: { concrete: withFacade ? "asphalt" : "concrete-slab" } });
    materials.push(...(grass ? lawnMaterials.map(material => ({ ...material, seed: 7, meshes: material.meshes.map(() => "lane/node/0/mesh/0/primitive/0") })) : []),
      ...treeMaterials.map(material => ({ ...material, seed: 7 })));
    batchList = [...(batchList ?? []), ...lane.batches.filter(batch => grass || batch.objectId !== "lane")];
  }
  const project = {
    schemaVersion: 2, kind: "material", entry: concrete, modules, parameters: {}, seed: 7, resolution,
    ...(texelDensity ? { texelDensity } : {}), inspection: "final", mapping: "world", domain: [1, 1], edges: [], gravity: [0, 0, -1],
    base: { kind: "swatch" }, target: { scope: "mesh", enabled: false },
    // `anchored`: the walls' concrete is measured from each part's pattern anchor, so a resize does not move it.
    ...(Object.keys(mappings).length ? { mappings } : {}),
    models: scene, materials, ...(batchList ? { batches: batchList } : {}),
    assets: Object.fromEntries(Object.entries(assets).map(([name, bytes]) => [name, Buffer.from(bytes).toString("base64")])),
  };
  return { project, definitions, assets };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { project, definitions, assets } = buildProject(Number(process.argv[2] ?? 512), Number(process.argv[3] ?? 128), { facade: process.env.FACADE === "building" ? "building" : process.env.FACADE === "1" });
  for (const [name, content] of [...Object.entries(definitions), ...Object.entries(assets).filter(([name]) => name.startsWith("decals/"))]) {
    mkdirSync(dirname(resolve(out, name)), { recursive: true });
    writeFileSync(resolve(out, name), content);
  }
  writeFileSync(resolve(out, "washroom-items.studio.json"), JSON.stringify(project, null, 2) + "\n");
  console.log(`Wrote ${resolve(out, "washroom-items.studio.json")} (${project.models.length} models, ${project.materials.length} materials, ${Object.keys(definitions).length} item definitions).`);
}
