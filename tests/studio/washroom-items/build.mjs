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
    { id: "facade-tile", entry: "kowloon-wall-tile.material.ts", parameters: { tileSize: 0.12, jointWidth: 0.011, glaze: [0.2, 0.25, 0.2], spare: [0.26, 0.29, 0.24], replaced: 0.015, missing: 0.03, variation: 0.14, fade: 0.45, grime: 0.8, jointRuns: 0.45, runReach: 1.4 }, seed: 7, meshes: [] },
    // A shop front's tiled dado: greener and glossier than the spandrels' sage, a few tiles lost from its foot up.
    { id: "dado-tile", entry: "kowloon-wall-tile.material.ts", parameters: { tileSize: 0.12, jointWidth: 0.011, glaze: [0.1, 0.19, 0.13], spare: [0.16, 0.24, 0.17], replaced: 0.012, missing: 0.06, variation: 0.12, grime: 0.8 }, seed: 11, meshes: [] },
    // Fixtures: printed air conditioner casings, painted steel gone dull and rusty, and thin galvanized sheet.
    { id: "aircon-casing", entry: "kowloon-aircon.material.ts", parameters: { yellowing: 0.25 }, seed: 7, meshes: [] },
    // The plastered walls and floor inside an open shop (tinted): pale, matt and a little grimy.
    { id: "shop-plaster", entry: steel, parameters: { ...plain, substrate: [0.82, 0.8, 0.76], metalness: 0, roughness: 0.92, edgeRust: 0, surfaceRust: 0.06, rainRust: 0.2 }, seed: 3, meshes: [] },
    { id: "poster-print", entry: "kowloon-poster-print.material.ts", parameters: {}, seed: 7, meshes: [] },
    // Signs: the painted ground, worn and stained, and its lettering with paint lost from the strokes (both tinted).
    { id: "sign-board", entry: "kowloon-sign-board.material.ts", parameters: { flaking: 0.8, fading: 0.65, grime: 0.75, rust: 0.7 }, seed: 7, meshes: [] },
    { id: "sign-lettering", entry: "kowloon-sign-lettering.material.ts", parameters: { wear: 0.2, fading: 0.3 }, seed: 7, meshes: [] },
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
    // Street furniture: painted cast iron and steel left out for decades (tinted: a post's dark green, a bin's faded colour),
    // duller and rustier than the fixtures on the walls; a lamp's lens, warm and dim by day; bench slats gone grey-brown
    // (cool and pale under their brown tint); and the avenue's slabs, a mid grey that is damp in patches.
    { id: "street-metal", entry: steel, parameters: { ...plain, substrate: [0.62, 0.62, 0.6], metalness: 0.2, roughness: 0.75, edgeRust: 0.85, surfaceRust: 0.4, rainRust: 0.85 }, seed: 17, meshes: [] },
    { id: "lamp-lens", entry: "kowloon-tube-light.material.ts", parameters: { tint: [1, 0.82, 0.55], glow: 0.35, aged: 0.2, dust: 0.6 }, seed: 7, meshes: [] },
    { id: "weathered-timber", entry: "wood.material.ts", parameters: { light: [0.42, 0.6, 0.92], dark: [0.2, 0.28, 0.42] }, seed: 7, meshes: [] },
    { id: "avenue-paving", entry: "kowloon-paving.material.ts", parameters: { flag: [3, 3], stagger: 0, jointWidth: 0.002, cracked: 0, stone: [0.66, 0.66, 0.65], variation: 0.08, grime: 1, spots: 0.5, wet: 0.5 }, seed: 21, meshes: [] },
    { id: "aged-paint", entry: steel, parameters: { ...plain, substrate: [0.8, 0.79, 0.75], metalness: 0.1, roughness: 0.72, edgeRust: 0.92, surfaceRust: 0.38, rainRust: 0.6 }, seed: 7, meshes: [] },
    { id: "aged-sheet", entry: steel, parameters: { ...plain, substrate: [0.78, 0.78, 0.76], metalness: 0.4, roughness: 0.6, edgeRust: 0.8, surfaceRust: 0.35, rainRust: 0.7 }, seed: 7, meshes: [] },
    { id: "aged-metal", entry: steel, parameters: { ...plain, substrate: [0.72, 0.72, 0.7], metalness: 0.35, roughness: 0.6, edgeRust: 0.9, surfaceRust: 0.4, rainRust: 0.9 }, seed: 5, meshes: [] },
    // Awning canvas, dyed by its tints (plain or striped): the weave, bleaching, soil and stains.
    { id: "canvas", entry: "kowloon-canvas.material.ts", parameters: { clothA: [0.86, 0.85, 0.82], clothB: [0.86, 0.85, 0.82], striped: 0, bleaching: 0, soil: 0.4, wallRuns: 1 }, seed: 7, meshes: [] },
    // The street: flagged pavement and its kerb (their stones shaded by tints), and the asphalt beyond.
    // (The pavement item lays its own stones, so the recipe's flags are made too large to show: one stone a receiver.)
    { id: "paving", entry: "kowloon-paving.material.ts", parameters: { flag: [3, 3], stagger: 0, jointWidth: 0.002, cracked: 0, stone: [0.62, 0.62, 0.61], variation: 0.06, grime: 1, wet: 0.6 }, seed: 7, meshes: [] },
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
      // Large square slabs laid in a grid with straight joints both ways, no darker bands; black bollards close the ends.
      slab: 0.75, course: 0.75, stagger: false, bands: 0, bollardSpacing: 1.3, bollardColor: "#1d1e1c",
      // A pair of benches against the planter beside each shrub, with a litter bin past their ends (a greyed green:
      // the tint multiplies the metal bound to it, so a near-neutral one leaves that material to decide).
      benches: shrubs.count, benchFirst: shrubs.first, benchSpacing: shrubs.spacing, bins: true, binColor: "#7d8778", seed: 7 } } }] };
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
  // Rust and chipped paint rise from each post's foot (a short line through its base, which the wear layer follows).
  const features = parts.map(part => ({ name: "foot", part: part.id, points: [[-0.05, 0, 0.02], [0.05, 0, 0.02]] }));
  const layers = { metal: [{ entry: "kowloon-foot-wear.material.ts", parameters: { reach: 0.6, strength: 0.95, chips: 0.3, color: [0.2, 0.13, 0.08] }, seed: 33, follow: { foot: "foot" } }] };
  return { format: "itemspec", version: 2, id: "avenue-lamps", parts, features, layers };
}

/** A small seeded stream (mulberry32) for the scene's own choices: numbers in [0, 1). */
function seeded(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Where a point `s` along and `t` across an avenue record stands in the scene (x, y), and the item axes that face
 * toward or away from its near side: a Kowloon fixture's front faces its -Y, so one standing on the far side facing
 * the avenue runs along `far` and one on the near side along `near`.
 */
function avenueFrame({ origin, along, across }) {
  const at = (s, t) => [0, 1].map(k => +(origin[k] + along[k] * s + across[k] * t).toFixed(4));
  return { at, far: [across[1], -across[0]], near: [-across[1], across[0]] };
}

/** The stations along an avenue that its trees, lamps and the benches stand at. */
function avenueStations({ trees, lamps, shrubs }) {
  const row = ({ first, spacing, count }) => Array.from({ length: count }, (_, i) => first + i * spacing);
  return { trees: row(trees), lamps: row(lamps), benches: row(shrubs) };
}

/**
 * The neighbours closing the avenue's far side (`frontage` of an avenue record): a row of `units` buildings of seeded
 * widths, each a ground storey of shopfronts with shutters, gates and awnings (`kowloon-ground-floor`) under a slice of
 * an adjoining building (`kowloon/party`, a stub `depth` deep with its own storeys, its front's ground storey left
 * blank behind the shopfronts) set `setback` behind them, and blade signs over the walkway between the trees and clear
 * of the lamps. One item in scene coordinates (`definition`), with where its units meet along the avenue (`piers`);
 * painted and weathered as `party` (the building's own party part) is.
 */
export function avenueFrontage(record, party) {
  const { origin, width, frontage } = record;
  const { at, far } = avenueFrame(record);
  const { trees, lamps } = avenueStations(record);
  const random = seeded(frontage.seed);
  const weights = Array.from({ length: frontage.units }, () => 0.8 + 0.45 * random());
  const total = weights.reduce((a, b) => a + b, 0), span = frontage.to - frontage.from;
  const paints = ["#b9ad96", "#a8a294", "#c4b9a2", "#9fa39a", "#b5a48a", "#c2b49a"];
  const shops = ["shutter", "gate", "shop", "shutter", "door", "gate", "service"];
  // Fixed canvas awnings 0.85 m deep only: a tarp's poles and a striped canvas reach out to the lamps' row.
  const awnings = ["awning", "none", "awning", "awning", "none"];
  const parts = [], piers = [];
  let s0 = frontage.from;
  for (const [i, weight] of weights.entries()) {
    const unit = +(span * weight / total).toFixed(3), s1 = s0 + unit, seed = frontage.seed * 100 + i;
    const storeys = frontage.storeys[0] + Math.floor(random() * (frontage.storeys[1] - frontage.storeys[0] + 1));
    const paint = paints[Math.floor(random() * paints.length)];
    const count = Math.max(1, Math.floor((unit - 0.7) / 2.1));
    const pick = list => Array.from({ length: count }, () => list[Math.floor(random() * list.length)]);
    // The item's x runs from the unit's far end (s1) back toward the avenue's start.
    // (The shop storeys share one paint, `shopPaint`: their paint coat is a layer of the render's material, and every
    // paint is another layer of it, which may compose at most 8.)
    parts.push({ id: `shops-${i}`, generator: { kind: "item", entry: "kowloon-ground-floor.item.json", parameters: {
      width: unit, height: 3.35, seed, paint: frontage.shopPaint, shopWidth: 2.1, shopPlan: pick(shops), awningPlan: pick(awnings), awningLower: 0.2, meters: true } },
      placement: { position: [...at(s1, width), origin[2]], along: far } });
    parts.push({ id: `storeys-${i}`, ...(party?.zones?.length ? { zones: party.zones } : {}), generator: { kind: "native", name: "kowloon/party", parameters: {
      // (`top` over its parapet: nothing stands behind the row, and its back faces away from every picture).
      kind: "stub", length: unit, own: storeys, ground: 3.35, storey: 3, top: 80, depth: frontage.depth, shops: true, door: false, stacks: 0,
      // (The first unit's end faces the front street: windows across it, a soil stack and vents over a dark plinth,
      // not a blank wall.)
      cables: 1, bars: 0.15, units: 0.4, color: paint, seed, ...(i === 0 ? { gable: "end", plinth: 1 } : {}) } },
      placement: { position: [...at(s1, width + frontage.setback + frontage.depth), origin[2]], along: far } });
    s0 = s1;
    if (i + 1 < weights.length) piers.push(+s1.toFixed(3));
  }
  // Blade signs from the first floor, between the trees and a little past each lamp.
  for (const [k, lamp] of lamps.entries()) {
    const s = lamp + 0.9;
    if (s < frontage.from + 0.6 || s > frontage.to - 0.6 || trees.some(tree => Math.abs(tree - s) < 1.8)) continue;
    parts.push({ id: `sign-${k}`, generator: { kind: "item", entry: "kowloon-sign.item.json", parameters: {
      mount: k % 2 ? "hung" : "blade", height: 3.2, reach: 1.15, seed: frontage.seed * 10 + k } },
      placement: { position: [...at(s, width + frontage.setback), origin[2] + 3.75], along: far } });
  }
  // And one on the first unit's end, by its corner, over the front street (a fixture's front faces its -Y: toward the
  // avenue's start, against `along`).
  parts.push({ id: "sign-end", generator: { kind: "item", entry: "kowloon-sign.item.json", parameters: {
    mount: "blade", height: 3.2, reach: 1.05, seed: frontage.seed * 10 + 9 } },
    placement: { position: [...at(frontage.from, width + frontage.setback + 0.22), origin[2] + 3.75], along: [record.along[1], -record.along[0]] } });
  return { definition: { format: "itemspec", version: 2, id: "avenue-frontage", parts }, piers };
}

/**
 * What stands about on the avenue's walkways, along the facades and clear of the median, the benches and the lamps:
 * pairs of potted plants by the far shopfronts' piers, crates and cartons stacked out of the shops, parked bicycles and
 * loaded hand carts on both sides, and blade signs over the near walkway from the neighbour past the building's back
 * (`near`: where that neighbour's front starts and ends along the avenue, and the height of its signs' foot);
 * `piers`: where the far shopfronts' units meet along the avenue. One item in scene coordinates.
 */
export function avenueClutter(record, near, piers) {
  const { origin, width, frontage } = record;
  const { at, far: alongFar, near: alongNear } = avenueFrame(record);
  const { trees, lamps } = avenueStations(record);
  const z = origin[2], parts = [];
  const place = (s, t, along) => ({ position: [...at(s, t), z], along });
  const turned = (along, angle) => [along[0] * Math.cos(angle) - along[1] * Math.sin(angle), along[0] * Math.sin(angle) + along[1] * Math.cos(angle)].map(v => +v.toFixed(4));
  const plant = (id, s, t, model, scale) => parts.push({ id, generator: { kind: "native", name: "kowloon/plant", parameters: { model, scale, rotation: (s * 1.7) % 6.28, textured: false } },
    placement: place(s, t, alongFar) });
  const stock = (id, s, t, along, kind, wide, seed) => parts.push({ id, generator: { kind: "native", name: "kowloon/street-stock", parameters: { width: wide, kind, seed } },
    placement: place(s, t, along) });
  const cart = (id, s, t, along, kind, seed) => parts.push({ id, generator: { kind: "native", name: "kowloon/street-cart", parameters: { kind, seed } },
    placement: place(s, t, along) });
  // The far side: the shop fronts stand on `width`; pots and stock keep within a metre of them.
  const front = width, random = seeded(frontage.seed + 7);
  const models = ["rubber-plant", "snake-plant", "trailing-pothos"];
  for (const [k, s] of piers.entries()) {
    if (lamps.some(lamp => Math.abs(lamp - s) < 0.7)) continue;
    if (k % 3 === 1) stock(`crates-${k}`, s, front, alongFar, ["crates", "cartons", "mixed"][k % 3], 1.3, 300 + k);
    else {
      plant(`pot-${k}-a`, s - 0.3, front - 0.3, models[k % 3], 1.5 + 0.3 * random());
      plant(`pot-${k}-b`, s + 0.32, front - 0.28, models[(k + 1) % 3], 1.3 + 0.3 * random());
    }
  }
  // Parked against the far shopfronts and the near neighbour, along the walkway, a hand cart a little askew.
  cart("bicycle-far", 12.6, front - 0.5, turned(alongFar, Math.PI), "bicycle", 3);
  cart("trolley-far", 26.4, front - 0.55, turned(alongFar, 0.25), "trolley", 5);
  cart("bicycle-near", 19.6, 0.45, alongNear, "bicycle", 8);
  cart("trolley-near", 13.2, 0.55, turned(alongNear, -0.2), "trolley", 9);
  stock("cartons-near", 30.6, 0, alongNear, "cartons", 1.2, 41);
  plant("pot-near-a", 24.4, 0.3, "rubber-plant", 1.6);
  plant("pot-near-b", 24.95, 0.28, "snake-plant", 1.4);
  // Blade and hung signs over the near walkway from the neighbour, as on the far side.
  for (const [k, lamp] of lamps.entries()) {
    const s = lamp + 0.6;
    if (s < near.from + 0.6 || s > near.to - 0.6 || trees.some(tree => Math.abs(tree - s) < 1.8)) continue;
    parts.push({ id: `sign-near-${k}`, generator: { kind: "item", entry: "kowloon-sign.item.json", parameters: {
      mount: k % 2 ? "blade" : "hung", height: 3, reach: 1.1, seed: 70 + k } },
      placement: { position: [...at(s, -0.01), z + near.sign], along: alongNear } });
  }
  return { format: "itemspec", version: 2, id: "avenue-clutter", parts };
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
  // (Without the room its items are not placed: their definitions stay out of the project, which holds at most 64 modules.)
  if (room) Object.assign(modules, definitions);
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
      slots: withFacade ? { paving: "avenue-paving", kerb: "kerb", soil: "room-dark", metal: "street-metal", timber: "weathered-timber" }
        : { paving: "concrete-slab", kerb: "concrete-slab", soil: "concrete-slab", metal: "flat-steel", timber: "bench-wood" },
      // Its slabs, kerbs and soil are shaded per vertex: a coarse bake of the large paving keeps the allowance for the building.
      slotDensities: { paving: 48, kerb: 96, soil: 24, metal: 96, timber: 128 } });
    // Street lamps in two rows along the avenue (`lamps` of `avenue.json`).
    modules["items/avenue-lamps.item.json"] = JSON.stringify(avenueLamps(avenue), null, 2) + "\n";
    scene.push({ id: "lamps", kind: "item", entry: "items/avenue-lamps.item.json", parameters: {}, transform: [[1, 0, 0], [0, 1, 0], [0, 0, 1], [0, 0, 0]],
      slots: withFacade ? { metal: "street-metal", lamp: "lamp-lens", banner: "canvas" } : { metal: "flat-steel", lamp: "flat-steel", banner: "bench-wood" },
      slotDensities: { metal: 64, lamp: 64, banner: 96 } });
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
      // (and the yard runs out of the pictures past the building's left end too).
      slab("lane-paving", [-13.6, wall], [-60, roadTo]);
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
    // from 2 m up; a party wall closes it below.
    if (withFacade) {
      const length = 24, depth = 1.85; // the stub's depth (its front's distance from its back).
      // Painted and weathered as the building's own party stubs are: the building's wall layers, over runoff zones
      // hung below the stub's tops, courses and sills (both read from the building's definition).
      const buildingItem = JSON.parse(building.definitions["kowloon-building.item.json"]);
      const party = buildingItem.parts.find(part => part.id === "party");
      const zones = (party?.zones ?? []).map(({ u0, length, ...zone }) => ({ ...zone, length: typeof length === "number" ? length : 3 }));
      // (Their streaks half again as strong as the building's: plain walls with little else on them.)
      const neighbourLayers = (buildingItem.layers?.wall ?? []).map(layer => layer.entry === "kowloon-grime-streaks.material.ts"
        ? { ...layer, parameters: { ...layer.parameters, strength: +(layer.parameters.strength * 1.7).toFixed(2), longRuns: 1.4 } } : layer);
      const neighbour = { format: "itemspec", version: 2, id: "avenue-neighbour", parameters: { paint: { ...buildingItem.parameters.paint, default: "#aaa391" } }, ...(neighbourLayers.length ? { layers: { wall: neighbourLayers } } : {}), parts: [
        // Three storeys, so the narrow street is closed on both sides past the building's back while the building's
        // fourth storey and roof still stand above it. Its foot is painted dark as the building's plinth is (1 m), so
        // the plinth runs on along the street front past the building's back.
        { id: "stub", ...(zones.length ? { zones } : {}), generator: { kind: "native", name: "kowloon/party", parameters: { kind: "stub", length, own: 3, top: 2, depth, units: 0.3, bars: 0.2, cables: 1, plinth: 1, seed: 41 } } },
        // Below that, a stained party wall (2 to 3.3 m by seed, under a coping) along its back, facing away from the avenue.
        { id: "back", generator: { kind: "native", name: "kowloon/party", parameters: { kind: "wall", length, top: 2.4, seed: 41 } },
          placement: { position: [length, 0, 0], along: [-1, 0] } }] };
      modules["items/avenue-neighbour.item.json"] = JSON.stringify(neighbour, null, 2) + "\n";
      scene.push({ id: "neighbour", kind: "item", entry: "items/avenue-neighbour.item.json", parameters: {},
        transform: [[-1, 0, 0], [0, -1, 0], [0, 0, 1], [wall - 7 - 1.49, avenue.origin[1] - 0.01 - depth, 0]], slots: building.slots,
        // Coarser than the building: its four storeys of windows, units and drying racks stay within the bake's allowance.
        slotDensities: { wall: 40, trim: 40, frame: 40, pane: 24, metal: 40, paint: 40, sheet: 32, aircon: 40, "balcony-cloth": 32 } });
      // The far side closed by a row of neighbours with shops at street level (`frontage` of `avenue.json`), painted and
      // weathered as the neighbour is, and what stands about on the walkways.
      // (The shopfronts' ground storeys weather by their own layers: a slot composes at most 8 over its base, so they are
      // an item of their own, without the neighbour's.)
      const own = { parameters: neighbour.parameters, ...(neighbour.layers ? { layers: neighbour.layers } : {}) };
      const { definition: frontage, piers } = avenueFrontage(avenue, { zones });
      const shopParts = frontage.parts.filter(part => part.id.startsWith("shops-"));
      // The units are painted three ways, turn about: a paint is a layer of its item's wall material, so each paint is
      // an item of its own (upper storeys and shop storeys apart), not a further layer of one.
      const unitPaints = ["#b9ad96", "#a2a297", "#c0ae8e"];
      const unitOf = part => Number(part.id.match(/^(?:shops|storeys)-(\d+)$/)?.[1] ?? 0) % unitPaints.length;
      const painted = paint => ({ paint: { ...neighbour.parameters.paint, default: paint } });
      const groups = unitPaints.flatMap((paint, group) => [
        [`frontage${group ? `-${group}` : ""}`, { ...frontage, id: `avenue-frontage-${group}`, parts: frontage.parts.filter(part => !shopParts.includes(part) && unitOf(part) === group),
          ...own, parameters: painted(paint) }],
        [`frontage-shops${group ? `-${group}` : ""}`, { ...frontage, id: `avenue-frontage-shops-${group}`,
          parts: shopParts.filter(part => unitOf(part) === group).map(part => ({ ...part, generator: { ...part.generator, parameters: { ...part.generator.parameters, paint } } })),
          parameters: painted(paint) }]]);
      for (const [id, definition] of groups) {
        modules[`items/avenue-${id}.item.json`] = JSON.stringify(definition, null, 2) + "\n";
        // Baked coarser than the building: secondary, and a long row of receivers within the preview's allowance.
        scene.push({ id, kind: "item", entry: `items/avenue-${id}.item.json`, parameters: {}, transform: [[1, 0, 0], [0, 1, 0], [0, 0, 1], [0, 0, 0]],
          slots: building.slots, slotDensities: { wall: 32, trim: 32, metal: 32, frame: 32, pane: 24, paint: 32, sheet: 32, aircon: 32,
            "balcony-cloth": 32, recess: 24, plinth: 32, dado: 32, lettering: 160, posters: 120, board: 96, tarp: 48, rust: 32, "shop-timber": 32,
            side: 32, glass: 32, opening: 32 } });
      }
      const along = x => +(avenue.origin[0] - x).toFixed(3);
      const clutter = avenueClutter(avenue, { from: along(wall - 7 - 1.49), to: along(wall - 7 - 1.49 - length), sign: 3.6 }, piers);
      modules["items/avenue-clutter.item.json"] = JSON.stringify(clutter, null, 2) + "\n";
      scene.push({ id: "clutter", kind: "item", entry: "items/avenue-clutter.item.json", parameters: {}, transform: [[1, 0, 0], [0, 1, 0], [0, 0, 1], [0, 0, 0]],
        slots: building.slots, slotDensities: { metal: 40, "balcony-plants": 32, lettering: 160, posters: 120, board: 96, rust: 32 } });
    }
    // The median is planted with groundcover (`lane-groundcover`, a painted mass), not grass: under it the lawn recipe is
    // the planting's dark soil and leaf litter (a deep olive-brown), so what shows between the mounds reads as shade, not
    // a bright strip; with a deeper, less blue shade.
    const mutedLawn = withFacade ? { band0: [0.06, 0.06, 0.04], band1: [0.09, 0.09, 0.06], band2: [0.11, 0.11, 0.07], band3: [0.13, 0.14, 0.08],
      band4: [0.16, 0.18, 0.09], darkPatches: 0.6, meadowAmount: 0.1 } : {};
    const mutedShade = withFacade ? { shadeTint: [0.24, 0.31, 0.4] } : {};
    materials.push(...lawnMaterials.map(material => ({ ...material, seed: 7, meshes: material.meshes.map(() => "lane/node/0/mesh/0/primitive/0"),
      parameters: { ...material.parameters, ...mutedShade, ...(material.id === "painted-lawn" ? mutedLawn : {}) } })),
      // Beside the building the bark is the avenue's own recipe (street-bark.material.ts): a warm grey-brown painted in
      // flat steps with bold strokes, not the washroom's streaked brown under a blue shade (mauve beside the plaster).
      ...treeMaterials.map(material => ({ ...material, seed: 7, ...(withFacade && material.id === "painted-bark"
        ? { entry: "street-bark.material.ts", parameters: {} } : {}) })));
    if (withFacade) modules["street-bark.material.ts"] = readFileSync(resolve(here, "street-bark.material.ts"), "utf8");
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
