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
// (The ground stain, moss, runoff, top grime and crease-edge recipes the items once named are left out: no item names
// them now, and a project holds at most 64 modules.)
export const kowloonLayerModules = [anchoredConcrete, "kowloon-paint-coat.material.ts", "kowloon-grime-streaks.material.ts",
  "kowloon-wall-dirt.material.ts", "kowloon-soot-band.material.ts", "kowloon-notices.material.ts", "kowloon-rust-run.material.ts", "kowloon-repairs.material.ts", "kowloon-foot-wear.material.ts", "kowloon-dust-streaks.material.ts", "kowloon-underside-grime.material.ts",
  // The fixtures' own recipes: glazing, wall tiles, air conditioner casings and awning canvas; the street's paving and asphalt.
  "kowloon-window-glass.material.ts", "kowloon-wall-tile.material.ts", "kowloon-aircon.material.ts", "kowloon-canvas.material.ts",
  "kowloon-paving.material.ts", "kowloon-asphalt.material.ts", "kowloon-poster-print.material.ts",
  "kowloon-edge-ink.material.ts", "kowloon-sign-board.material.ts", "kowloon-sign-lettering.material.ts", "kowloon-tube-light.material.ts"];
function facadeMaterials() {
  const plain = { bandedGloss: 0, silhouette: 0, polishedEdges: 0 };
  return [
    // The reference shop house's cream render: the concrete's plaster coat, with patches lost to the concrete beneath.
    { id: "render-paint", entry: anchoredConcrete, parameters: { plaster: false, cutouts: false, holes: false, joints: false, chipAmount: 0.3, chipRelief: 0.0012, darknessA: 0.6, darknessB: 0.45, blendContrast: 2.2, hueAmount: 0.04, hueAngle: 60, intactRoughness: 0.85 }, seed: 11, meshes: [] },
    // The fixtures are drawn with their paint as vertex tints (a frame's green, a cage's khaki, an awning's stripes), which
    // the picture keeps over their baked looks (`?bakedtints=on`): so their materials are pale, and add only what age
    // does to the paint: dullness, dirt and rust.
    // Painted steel window frames and railings.
    { id: "frame-green", entry: steel, parameters: { ...plain, substrate: [0.8, 0.8, 0.77], metalness: 0.1, roughness: 0.7, edgeRust: 0.5, surfaceRust: 0.14, rainRust: 0.55 }, seed: 7, meshes: [] },
    { id: "enamel", entry: steel, parameters: { ...plain, substrate: [0.78, 0.76, 0.7], metalness: 0.1, roughness: 0.45, edgeRust: 0.3, surfaceRust: 0.05, rainRust: 0.4 }, seed: 7, meshes: [] },
    // Old glazing with the room painted into it, and what stands behind an open or unglazed light (tinted: the dark of
    // the room, its curtains and drapes).
    { id: "window-glass", entry: "kowloon-window-glass.material.ts", parameters: { tint: [0.05, 0.052, 0.05], sky: 0.6, lamps: 0.1, dirt: 0.6, transmission: 0.3, interior: 0.2 }, seed: 7, meshes: [] },
    { id: "room-dark", entry: steel, parameters: { ...plain, substrate: [0.85, 0.85, 0.85], metalness: 0, roughness: 0.92, edgeRust: 0, surfaceRust: 0, rainRust: 0 }, seed: 7, meshes: [] },
    // The back of a closed bay's recess (behind a gate or shutter): grimy dark plaster, untinted.
    { id: "recess-plaster", entry: steel, parameters: { ...plain, substrate: [0.2, 0.185, 0.16], metalness: 0, roughness: 0.95, edgeRust: 0, surfaceRust: 0.1, rainRust: 0.25 }, seed: 5, meshes: [] },
    // The spandrels' small grey-green tiles, dirty in the joints.
    { id: "facade-tile", entry: "kowloon-wall-tile.material.ts", parameters: { tileSize: 0.065, glaze: [0.2, 0.25, 0.2], spare: [0.26, 0.29, 0.24], replaced: 0.015, missing: 0.03, variation: 0.14, fade: 0.45, grime: 1, jointRuns: 0.45, runReach: 1.4 }, seed: 7, meshes: [] },
    // A shop front's tiled dado: greener and glossier than the spandrels' sage, a few tiles lost from its foot up.
    { id: "dado-tile", entry: "kowloon-wall-tile.material.ts", parameters: { tileSize: 0.065, glaze: [0.1, 0.19, 0.13], spare: [0.16, 0.24, 0.17], replaced: 0.012, missing: 0.06, variation: 0.12, grime: 0.8 }, seed: 11, meshes: [] },
    // Fixtures: printed air conditioner casings, painted steel gone dull and rusty, and thin galvanized sheet.
    { id: "aircon-casing", entry: "kowloon-aircon.material.ts", parameters: { yellowing: 0.25 }, seed: 7, meshes: [] },
    // The plastered walls and floor inside an open shop (tinted): pale, matt and a little grimy.
    { id: "shop-plaster", entry: steel, parameters: { ...plain, substrate: [0.82, 0.8, 0.76], metalness: 0, roughness: 0.92, edgeRust: 0, surfaceRust: 0.06, rainRust: 0.2 }, seed: 3, meshes: [] },
    { id: "poster-print", entry: "kowloon-poster-print.material.ts", parameters: {}, seed: 7, meshes: [] },
    // Signs: the painted ground, worn and stained, and its lettering with paint lost from the strokes (both tinted).
    { id: "sign-board", entry: "kowloon-sign-board.material.ts", parameters: { flaking: 0.8, fading: 0.65, grime: 0.75, rust: 0.7 }, seed: 7, meshes: [] },
    { id: "sign-lettering", entry: "kowloon-sign-lettering.material.ts", parameters: { wear: 0.04, fading: 0.3 }, seed: 7, meshes: [] },
    // Lit tubes in the shops, and a shop's concrete floor: grey, lightly soiled, never dark.
    { id: "tube-light", entry: "kowloon-tube-light.material.ts", parameters: {}, seed: 7, meshes: [] },
    // A lit upper room's walls and ceiling (tinted): a soft warm glow under its own tube, far dimmer than the tube.
    { id: "room-lit", entry: "kowloon-tube-light.material.ts", parameters: { tint: [1, 0.8, 0.55], glow: 0.08, aged: 0, dust: 0, falloff: 0.85 }, seed: 7, meshes: [] },
    { id: "shop-floor", entry: "kowloon-paving.material.ts", parameters: { flag: [3, 3], stagger: 0, jointWidth: 0.002, cracked: 0, fractures: 0.3, stone: [0.62, 0.61, 0.58], variation: 0.04, grime: 0.25, spots: 0.25 }, seed: 4, meshes: [] },
    // Street-weathered galvanized sheet (a roof tank's shell): a neutral dull grey, not the washroom duct's blue-grey.
    { id: "weathered-galvanized", entry: steel, parameters: { substrate: [0.42, 0.41, 0.38], metalness: 0.5, roughness: 0.62, edgeRust: 0.7, surfaceRust: 0.35, rainRust: 0.85 }, seed: 7, meshes: [] },
    // The building's ducts and their fittings: galvanized sheet gone dull, rust spread over it in spots and a faint bloom,
    // more toward the foot of each section, and only a little at the seams (the washroom's item steel paints a full band
    // at every joint, which on a duct reads as a brown ring).
    { id: "duct-galvanized", entry: steel, parameters: { substrate: [0.46, 0.47, 0.46], metalness: 0.6, roughness: 0.5, edgeRust: 0.35, surfaceRust: 0.45, rainRust: 0.75 }, seed: 13, meshes: [] },
    { id: "aged-paint", entry: steel, parameters: { ...plain, substrate: [0.8, 0.79, 0.75], metalness: 0.1, roughness: 0.72, edgeRust: 0.92, surfaceRust: 0.38, rainRust: 0.6 }, seed: 7, meshes: [] },
    { id: "aged-sheet", entry: steel, parameters: { ...plain, substrate: [0.78, 0.78, 0.76], metalness: 0.4, roughness: 0.6, edgeRust: 0.8, surfaceRust: 0.35, rainRust: 0.7 }, seed: 7, meshes: [] },
    { id: "aged-metal", entry: steel, parameters: { ...plain, substrate: [0.72, 0.72, 0.7], metalness: 0.35, roughness: 0.6, edgeRust: 0.9, surfaceRust: 0.4, rainRust: 0.9 }, seed: 5, meshes: [] },
    // Awning canvas, dyed by its tints (plain or striped): the weave, bleaching, soil and stains.
    { id: "canvas", entry: "kowloon-canvas.material.ts", parameters: { clothA: [0.86, 0.85, 0.82], clothB: [0.86, 0.85, 0.82], striped: 0, bleaching: 0, soil: 0.4, wallRuns: 1 }, seed: 7, meshes: [] },
    // The street: flagged pavement and its kerb (their stones shaded by tints), and the asphalt beyond.
    // (The pavement item lays its own stones, so the recipe's flags are made too large to show: one stone a receiver.)
    { id: "paving", entry: "kowloon-paving.material.ts", parameters: { flag: [3, 3], stagger: 0, jointWidth: 0.002, cracked: 0, stone: [0.36, 0.365, 0.37], variation: 0.06, grime: 1, wet: 0.75 }, seed: 7, meshes: [] },
    { id: "kerb", entry: "kowloon-paving.material.ts", parameters: { flag: [3, 3], stagger: 0, jointWidth: 0.002, cracked: 0, stone: [0.5, 0.49, 0.46], variation: 0.05, spots: 0.3, grime: 0.9 }, seed: 9, meshes: [] },
    { id: "asphalt", entry: "kowloon-asphalt.material.ts", parameters: { binder: [0.105, 0.105, 0.1], cracks: 0, patches: 0.1, wearing: 0.12, stains: 0.12 }, seed: 7, meshes: [] },
    { id: "leaves", entry: steel, parameters: { ...plain, substrate: [0.08, 0.22, 0.06], metalness: 0, roughness: 0.75, edgeRust: 0, surfaceRust: 0, rainRust: 0 }, seed: 7, meshes: [] },
  ];
}

/** The pedestrian avenue of the scene (`avenue.json`, shared with `treespec_batches::write_lane_plants`). */
export const avenue = JSON.parse(readFileSync(resolve(here, "avenue.json"), "utf8"));

/**
 * The `kowloon/pedestrian-avenue` item an avenue record describes, where it stands and the furthest the avenue reaches
 * along y (`far`). The generator runs along its +X and reaches its width toward -Y, so it is placed with X along the
 * record's `along` and -Y along its `across`, which must lie to the right of `along` (no reflection).
 */
export function pedestrianAvenue({ origin, along, across, length, width, height, median, shrubs }) {
  if (along[0] * across[1] - along[1] * across[0] >= 0) throw new Error("The avenue's `across` must lie clockwise of `along` (to its right).");
  const definition = { format: "itemspec", version: 2, id: "pedestrian-avenue",
    parts: [{ id: "avenue", generator: { kind: "native", name: "kowloon/pedestrian-avenue", parameters: {
      length, width, height, medianOffset: median.offset, medianWidth: median.width, medianStart: median.start, medianEnd: median.end,
      planterKerb: median.kerb, planterRise: median.rise, soilDepth: median.soil,
      // A pair of benches against the planter beside each shrub, with a litter bin past their ends.
      benches: shrubs.count, benchFirst: shrubs.first, benchSpacing: shrubs.spacing, bins: true, seed: 7 } } }] };
  const transform = [[along[0], along[1], 0], [-across[0], -across[1], 0], [0, 0, 1], origin];
  const far = Math.max(...[0, length].flatMap(s => [0, width].map(t => origin[1] + along[1] * s + across[1] * t)));
  return { definition, transform, far };
}

/**
 * The avenue's street lamps (`lamps` of an avenue record): one `kowloon/street-lamp` part a post, in scene coordinates
 * (the item stands untransformed), each row's arms reaching toward the avenue's middle; the row nearer the building
 * carries a banner on its posts. A lamp's arm reaches along its -Y, so its X runs `across` turned a quarter clockwise.
 */
export function avenueLamps({ origin, along, across, width, lamps }) {
  const parts = [];
  for (const [row, offset] of lamps.offsets.entries()) {
    const inward = offset < width / 2 ? 1 : -1;
    for (let i = 0; i < lamps.count; i++) {
      const s = lamps.first + i * lamps.spacing;
      const at = [0, 1].map(k => +(origin[k] + along[k] * s + across[k] * offset).toFixed(4));
      parts.push({ id: `lamp-${row}-${i}`, generator: { kind: "native", name: "kowloon/street-lamp", parameters: {
        height: lamps.height, reach: lamps.reach, banner: inward > 0 } },
        placement: { position: [...at, origin[2]], along: [-across[1] * inward, across[0] * inward] } });
    }
  }
  return { format: "itemspec", version: 2, id: "avenue-lamps", parts };
}

/**
 * `weathering: false` uses the shipped recipes as they are (nothing follows a feature); `batches: false` leaves out the
 * bolts of the brackets and the vent, which are the GLB washroom's instanced batch without its two flange rings (the
 * pipe item makes its own flange bolts).
 */
export function buildProject(resolution = 512, texelDensity = 128, { weathering = true, batches = true, anchored = false, walls, facade: withFacade = false, lane = null, room = true } = {}) {
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
  // `lane` (a TreeSpec document, `lane-plants.json`): the front street's road between the room and the building, and the
  // pedestrian avenue along the building's right side (`avenue.json`), with its median planted: trees and shrubs in a
  // row and dense groundcover around them, so the planting always comes with the avenue.
  if (lane) {
    for (const name of [...lawnMaterials, ...treeMaterials].map(material => material.entry)) {
      modules[name] = readFileSync(resolve(here, "../washroom/materials", name), "utf8");
    }
    assets["lane.glb"] = writeGlb([lawnGround(new Mesh("lawn", [0.3, 0.48, 0.14]), lane.ground)]);
    scene.push({ id: "lane", kind: "glb", name: "lane.glb" });
    const { definition: avenueItem, transform: avenueAt, far } = pedestrianAvenue(avenue);
    modules["items/pedestrian-avenue.item.json"] = JSON.stringify(avenueItem, null, 2) + "\n";
    scene.push({ id: "avenue", kind: "item", entry: "items/pedestrian-avenue.item.json", parameters: {}, transform: avenueAt,
      slots: withFacade ? { paving: "paving", kerb: "kerb", soil: "room-dark", metal: "aged-metal", timber: "bench-wood" }
        : { paving: "concrete-slab", kerb: "concrete-slab", soil: "concrete-slab", metal: "flat-steel", timber: "bench-wood" },
      // Its slabs, kerbs and soil are shaded per vertex: a coarse bake of the large paving keeps the allowance for the building.
      slotDensities: { paving: 48, kerb: 96, soil: 24, metal: 96, timber: 128 } });
    // Street lamps in two rows along the avenue (`lamps` of `avenue.json`).
    modules["items/avenue-lamps.item.json"] = JSON.stringify(avenueLamps(avenue), null, 2) + "\n";
    scene.push({ id: "lamps", kind: "item", entry: "items/avenue-lamps.item.json", parameters: {}, transform: [[1, 0, 0], [0, 1, 0], [0, 0, 1], [0, 0, 0]],
      slots: withFacade ? { metal: "aged-metal", lamp: "tube-light", banner: "canvas" } : { metal: "flat-steel", lamp: "flat-steel", banner: "bench-wood" },
      slotDensities: { metal: 128, lamp: 96, banner: 160 } });
    // The road: thin slabs with a rounded rim, so an open edge does not read as a thick floating block. Without the room
    // the front street is a whole street: its carriageway runs far out both ways along the building's front, 7.3 m from
    // the near kerb (the avenue's start) to a far kerb and a 4 m pavement of its own; with the room it stops short of
    // the room's slab. The yard under and behind the building runs on past the avenue's mouth (3 m beyond its far side).
    const kerbLine = avenue.origin[0], farKerb = room ? null : kerbLine + 7.3;
    const roadFrom = -9.5, roadTo = Math.max(5.5, far + 3), wall = -4.5;
    const slab = (id, [x0, x1], [y0, y1]) => {
      const definition = { format: "itemspec", version: 2, id,
        parts: [{ id: "paving", generator: { kind: "box", size: [+(x1 - x0).toFixed(3), +(y1 - y0).toFixed(3), 0.06], bevel: 0.025, bevelSegments: 2 }, slots: { side: "concrete", top: "concrete", bottom: "concrete" } }] };
      modules[`items/${id}.item.json`] = JSON.stringify(definition, null, 2) + "\n";
      scene.push({ id, kind: "item", entry: `items/${id}.item.json`, parameters: {}, transform: [[1, 0, 0], [0, 1, 0], [0, 0, 1], [x0, y0, -0.06]],
        slots: { concrete: withFacade ? "asphalt" : "concrete-slab" }, slotDensities: { concrete: 64 } });
    };
    if (farKerb === null) slab("lane-paving", [-13.6, -0.6], [roadFrom, roadTo]);
    else {
      slab("lane-paving", [-13.6, wall], [roadFrom, roadTo]);
      slab("lane-road", [wall, farKerb], [-60, 50]);
      // The far pavement faces the building across the road: its kerb on the far kerb line, its stones running from +y
      // to -y (the pavement runs along its +X, its kerb toward its -Y).
      const farPavement = { format: "itemspec", version: 2, id: "far-pavement",
        parts: [{ id: "pavement", generator: { kind: "native", name: "kowloon/pavement", parameters: { length: 100, after: 10, depth: 4, height: 0.15, drain: true, manhole: true, seed: 11 } } }] };
      modules["items/far-pavement.item.json"] = JSON.stringify(farPavement, null, 2) + "\n";
      scene.push({ id: "far-pavement", kind: "item", entry: "items/far-pavement.item.json", parameters: {}, transform: [[0, -1, 0], [1, 0, 0], [0, 0, 1], [farKerb + 4, 50, 0.15]],
        slots: withFacade ? { paving: "paving", kerb: "kerb", metal: "aged-metal" } : { paving: "concrete-slab", kerb: "concrete-slab", metal: "flat-steel" },
        slotDensities: { paving: 48, kerb: 96, metal: 96 } });
    }
    // Past the building's back the avenue's near side is closed by a neighbour: a two-storey slice of an adjoining
    // building (`kowloon/party`, a stub) along the avenue's near edge, its windows toward the avenue, from just past
    // the building's own back neighbour (1.47 m deep at the reference seed) to beyond the avenue's end. Its back shows
    // from 2 m up; a plain wall closes it below.
    if (withFacade && process.env.NEIGHBOUR !== "0") {
      const length = 24, depth = 1.84353; // the stub's depth at seed 41 (its front's distance from its back).
      // Painted and weathered as the building's own party stubs are: the building's wall layers, over runoff zones
      // hung below the stub's tops, courses and sills (both read from the building's definition).
      const buildingItem = JSON.parse(building.definitions["kowloon-building.item.json"]);
      const party = buildingItem.parts.find(part => part.id === "party");
      const zones = (party?.zones ?? []).map(({ u0, length, ...zone }) => ({ ...zone, length: typeof length === "number" ? length : 3 }));
      const neighbour = { format: "itemspec", version: 2, id: "avenue-neighbour", parameters: { paint: { ...buildingItem.parameters.paint, default: "#b0a590" } }, ...(buildingItem.layers?.wall ? { layers: { wall: buildingItem.layers.wall } } : {}), parts: [
        { id: "stub", ...(zones.length ? { zones } : {}), generator: { kind: "native", name: "kowloon/party", parameters: { kind: "stub", length, own: 2, top: 2, units: 0.3, bars: 0.2, cables: 1, seed: 41 } } },
        { id: "back", generator: { kind: "box", size: [length + 0.14, 0.2, 2.1] }, slots: { side: "wall", top: "wall", bottom: "wall" }, placement: { position: [-0.07, 0.01, 0] } }] };
      modules["items/avenue-neighbour.item.json"] = JSON.stringify(neighbour, null, 2) + "\n";
      scene.push({ id: "neighbour", kind: "item", entry: "items/avenue-neighbour.item.json", parameters: {},
        transform: [[-1, 0, 0], [0, -1, 0], [0, 0, 1], [wall - 7 - 1.49, avenue.origin[1] - 0.01 - depth, 0]], slots: building.slots,
        slotDensities: { wall: 64, trim: 64 } });
    }
    // The median is planted with groundcover (`lane-groundcover`, a painted mass), not grass: under it the lawn recipe is
    // the planting's dark soil and leaf litter (a deep olive-brown), so what shows between the mounds reads as shade, not
    // a bright strip; with a deeper, less blue shade.
    const mutedLawn = withFacade ? { band0: [0.06, 0.06, 0.04], band1: [0.09, 0.09, 0.06], band2: [0.11, 0.11, 0.07], band3: [0.13, 0.14, 0.08],
      band4: [0.16, 0.18, 0.09], darkPatches: 0.6, meadowAmount: 0.1 } : {};
    const mutedShade = withFacade ? { shadeTint: [0.24, 0.31, 0.4] } : {};
    materials.push(...lawnMaterials.map(material => ({ ...material, seed: 7, meshes: material.meshes.map(() => "lane/node/0/mesh/0/primitive/0"),
      parameters: { ...material.parameters, ...mutedShade, ...(material.id === "painted-lawn" ? mutedLawn : {}) } })),
      // The bark a grey-brown with a near-neutral shade, not the washroom's warm brown under a blue shade (which reads
      // mauve beside the plaster).
      ...treeMaterials.map(material => ({ ...material, seed: 7, ...(withFacade && material.id === "painted-bark"
        ? { parameters: { light: [0.34, 0.31, 0.26], dark: [0.17, 0.15, 0.12], shadeTint: [0.56, 0.58, 0.64] } } : {}) })));
    batchList = [...(batchList ?? []), ...lane.batches];
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
