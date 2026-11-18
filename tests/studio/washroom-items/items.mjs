/**
 * The washroom of `../washroom/model.mjs` as ItemSpec definitions (version 2) and their placements. Meters, Z up.
 *
 * The walls and the slab are authored in their own frames and placed in the room. The pipe, duct, bench and drain are
 * authored in the layout `model.mjs` uses (the tall wall's face on x = 0, the low wall's on y = 0) and placed by the
 * same quarter turn it applies, (x, y) to (y, -x), so their numbers can be read against that file.
 */

import { readFileSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** Four affine columns: three basis vectors and the translation. */
const placed = (x, y, z, at) => [x, y, z, at];
/** Where a definition written in `model.mjs`'s layout stands: (x, y) becomes (y, -x). */
export const TURNED = placed([0, -1, 0], [1, 0, 0], [0, 0, 1], [0, 0, 0]);

const box = (id, size, position, slot, rotation) => ({ id, generator: { kind: "box", size },
  placement: { position, ...(rotation ? { rotation } : {}) }, slots: { side: slot, top: slot, bottom: slot } });

/**
 * The tall wall: a panel with a framed, glazed window and a sill, the tile panel and its worn remnant in front of it,
 * a notice on the tile and a stencilled number on the concrete under the window. Widening the wall leaves everything
 * where it is (every rule is measured from the start); lowering the window's sill moves the window toward the number.
 */
export const wall = {
  format: "itemspec", version: 2, id: "washroom-wall",
  parameters: {
    width: { type: "number", default: 2.7, min: 2.5, max: 6, unit: "m" },
    height: { type: "number", default: 2.6, min: 2.4, max: 4, unit: "m" },
    windowSill: { type: "number", default: 0.95, min: 0.7, max: 1.2, unit: "m" },
  },
  derived: { middle: "height / 2" },
  parts: [
    { id: "wall", generator: { kind: "panel", width: { parameter: "width" }, height: { parameter: "height" }, thickness: 0.25,
        openings: [{ id: "window", shape: "rectangle", width: 0.6, height: 0.7,
          u: { from: "start", offset: 0.5 }, v: { from: "start", offset: { parameter: "windowSill" } },
          frame: { border: 0.04, depth: 0.08 }, glazing: { thickness: 0.006 }, sill: { depth: 0.06, overhang: 0.03 } }] },
      slots: { front: "concrete", back: "concrete", edge: "concrete", reveal: "concrete", frame: "frame", glazing: "glass", sill: "concrete" } },
    box("tiles", [1.2, 0.012, 0.9], [1.2, -0.012, 0.75], "tile"),
    box("tiles-low", [0.6, 0.012, 0.45], [1.5, -0.012, 0.3], "tile"),
  ],
  decals: [
    { id: "notice", image: "decals/notice.png", size: [0.24, 0.32], on: { part: "tiles", face: "front" },
      u: { from: "start", offset: 0.12 }, v: { from: "start", offset: 0.46 } },
    { id: "number", image: "decals/number.png", size: [0.3, 0.15], on: { part: "wall", face: "front" },
      u: { from: "start", offset: 0.62 }, v: { from: "start", offset: 0.56 } },
  ],
  handles: [
    { id: "width", parameter: "width", at: [{ parameter: "width" }, -0.02, { derived: "middle" }], axis: [1, 0, 0] },
    { id: "height", parameter: "height", at: [1.0, -0.02, { parameter: "height" }], axis: [0, 0, 1] },
  ],
};

/** The low wall, 0.4 m thick, with an arched recess above the bench. */
export const lowWall = {
  format: "itemspec", version: 2, id: "washroom-wall-low",
  parameters: { width: { type: "number", default: 2.3, min: 2.0, max: 6, unit: "m" } },
  parts: [{ id: "wall", generator: { kind: "panel", width: { parameter: "width" }, height: 1.2, thickness: 0.4,
      openings: [{ id: "niche", shape: "arch", mode: "recess", depth: 0.12, width: 0.5, height: 0.56,
        u: { from: "end", offset: 0.25 }, v: { from: "start", offset: 0.58 } }] },
    slots: { front: "concrete", back: "concrete", edge: "concrete", reveal: "concrete" } }],
};

/** The slab, with the patch of floor tile. */
export const slab = {
  format: "itemspec", version: 2, id: "washroom-slab",
  parts: [
    { id: "slab", generator: { kind: "box", size: [3.0, 2.85, 0.2] }, slots: { side: "concrete", top: "concrete", bottom: "concrete" } },
    box("tiles", [1.2, 0.6, 0.012], [1.15, 1.85, 0.2], "tile"),
  ],
};

const flange = (id, at) => ({ kind: "flange", id, at, radius: 0.13, thickness: 0.03, bolts: 6 });
const coupling = (id, offset) => ({ kind: "coupling", id, at: { from: "start", offset: { derived: offset } }, length: 0.07, clearance: 0.018 });
/**
 * The rusted pipe: up from the floor beside the tile and into the tall wall, with a bolted flange at each end and
 * three couplings that keep an even spacing as the riser's height changes. A label wraps the riser between the first
 * two couplings.
 */
export const pipe = {
  format: "itemspec", version: 2, id: "washroom-pipe",
  parameters: {
    height: { type: "number", default: 2.3, min: 1.2, max: 3.5, unit: "m" },
    standoff: { type: "number", default: 0.14, min: 0.14, max: 0.6, unit: "m" },
  },
  derived: { spacing: "(height - 0.1) / 4", second: "2 * spacing", third: "3 * spacing", label: "1.5 * spacing" },
  parts: [{ id: "riser", generator: { kind: "route", section: { kind: "round", radius: 0.09 },
      points: [[{ parameter: "standoff" }, 2.12, 0], [{ parameter: "standoff" }, 2.12, { parameter: "height" }], [0, 2.12, { parameter: "height" }]],
      bendRadius: 0.1, segments: 24, bendSegments: 8,
      fittings: [flange("floor", { from: "start", offset: 0.015 }), coupling("coupling-1", "spacing"), coupling("coupling-2", "second"),
        coupling("coupling-3", "third"), flange("wall", { from: "end", offset: 0.015 })] },
    slots: { body: "steel", fitting: "steel", bolt: "bolt", end: "steel" } }],
  decals: [{ id: "label", image: "decals/label.png", size: [0.16, 0.1], on: { part: "riser" },
    along: { from: "start", offset: { derived: "label" } }, around: 135 }],
  handles: [{ id: "height", parameter: "height", at: [{ parameter: "standoff" }, 2.12, { parameter: "height" }], axis: [0, 0, 1] }],
};

const seam = (id, at) => ({ kind: "flange", id, at, overhang: 0.012, thickness: 0.024 });
const bracket = (id, offset) => ({ kind: "bracket", id, at: { from: "start", offset }, reach: 0.21, direction: [-1, 0, 0] });
const galvanized = (id, size, position, rotation) => box(id, size, position, "galvanized", rotation);
/**
 * The galvanized duct: a run along the tall wall and an elbow down onto the low wall, with seams and two wall
 * brackets, the vent box at its start and the louvered face. Lengthening the run moves the vent along the wall.
 */
export const duct = {
  format: "itemspec", version: 2, id: "washroom-duct",
  parameters: { run: { type: "number", default: 1.67, min: 1.5, max: 4, unit: "m" } },
  derived: { ventY: "run - 0.17", louverY: "ventY + 0.04", louverFarY: "ventY + 0.38", slatY: "ventY + 0.07" },
  parts: [
    { id: "duct", generator: { kind: "route", section: { kind: "rect", width: 0.34, height: 0.3 }, wallThickness: 0.004, up: [0, 0, 1],
        points: [[0.21, { derived: "ventY" }, 2.12], [0.21, -0.17, 2.12], [0.21, -0.17, 1.2]], bendRadius: 0.3, bendSegments: 8,
        fittings: [seam("seam-1", { from: "start", offset: 0.012 }), seam("seam-2", { from: "start", offset: 0.65 }),
          seam("seam-3", { from: "end", offset: 1.1112 }), seam("seam-4", { from: "end", offset: 0.6 }), seam("seam-5", { from: "end", offset: 0.012 }),
          bracket("bracket-1", 0.35), bracket("bracket-2", 1.0)] },
      slots: { body: "galvanized", fitting: "galvanized", end: "galvanized", bracket: "steel" } },
    galvanized("vent", [0.4, 0.45, 0.4], [0.02, { derived: "ventY" }, 1.92]),
    galvanized("louver-bottom", [0.016, 0.37, 0.03], [0.42, { derived: "louverY" }, 1.96]),
    galvanized("louver-top", [0.016, 0.37, 0.03], [0.42, { derived: "louverY" }, 2.25]),
    galvanized("louver-near", [0.016, 0.03, 0.26], [0.42, { derived: "louverY" }, 1.99]),
    galvanized("louver-far", [0.016, 0.03, 0.26], [0.42, { derived: "louverFarY" }, 1.99]),
    ...[2.003, 2.055, 2.107, 2.159, 2.211].map((z, index) =>
      galvanized(`slat-${index + 1}`, [0.004, 0.31, 0.062], [0.414, { derived: "slatY" }, z], [0, 53.13010235, 0])),
    // The dark opening behind the louvers.
    box("shadow", [0.001, 0.31, 0.26], [0.42, { derived: "slatY" }, 1.99], "opening"),
  ],
};

const steel = (id, size, position, rotation) => box(id, size, position, "steel", rotation);
/** A wall bracket of flat steel as three boxes: an arm out from the wall at (x, 0, 0.425), a leg down it and a brace between. */
const benchBracket = (name, x) => [
  steel(`${name}-arm`, [0.04, 0.46, 0.006], [x - 0.02, 0, 0.419]),
  steel(`${name}-leg`, [0.04, 0.006, 0.33], [x - 0.02, 0, 0.095]),
  // From the foot of the leg up to the arm's end: 0.408 m out and 0.298 m up, 36.14 degrees.
  steel(`${name}-brace`, [0.028, 0.505, 0.006], [x - 0.014, 0.006, 0.121], [36.14, 0, 0]),
];
/** The bench: three planks on two wall brackets. */
export const bench = {
  format: "itemspec", version: 2, id: "washroom-bench",
  parts: [
    ...[0.03, 0.185, 0.34].map((y, index) => box(`plank-${index + 1}`, [1.4, 0.14, 0.045], [0.45, y, 0.425], "wood")),
    ...benchBracket("bracket-1", 0.7), ...benchBracket("bracket-2", 1.6),
  ],
};

/** The floor drain: a rim, seven bars cut to the circle and the dark opening under them. */
export const drain = {
  format: "itemspec", version: 2, id: "washroom-drain",
  parts: [
    { id: "rim", generator: { kind: "pipe", length: 0.012, radius: 0.092, wallThickness: 0.014, segments: 24 },
      placement: { position: [1.2, 1.25, 0.002] }, slots: { surface: "steel" } },
    ...[-3, -2, -1, 0, 1, 2, 3].map(bar => {
      const offset = bar * 0.022, reach = Math.sqrt(0.085 ** 2 - offset ** 2);
      return steel(`bar-${bar + 4}`, [+(2 * reach).toFixed(4), 0.012, 0.006], [+(1.2 - reach).toFixed(4), 1.25 + offset - 0.006, 0.005]);
    }),
    { id: "shadow", generator: { kind: "pipe", length: 0.003, radius: 0.085, segments: 24 },
      placement: { position: [1.2, 1.25, 0.002] }, slots: { surface: "opening" } },
  ],
};

const kowloonItems = resolve(dirname(fileURLToPath(import.meta.url)), "../../../crates/engine/src/worldspec/themes/kowloon/items");
/**
 * The Kowloon building side across the lane, beyond the low wall: a composed item (`kowloon-facade-side`) whose riser is
 * a sub-item (`kowloon-duct-riser`) and whose air conditioners and condensers are the Kowloon fixtures as native
 * generators. Its front faces the room (+x); its width runs along +y. Slots name the project materials of
 * `build.mjs`; the fixtures' own slots (`aircon`, `metal`, `paint`) are bound here too.
 */
const kowloonDefinitions = Object.fromEntries(readdirSync(kowloonItems).filter(name => name.endsWith(".item.json"))
  .map(name => [name, readFileSync(resolve(kowloonItems, name), "utf8")]));
/**
 * Project materials for every slot the Kowloon items and their fixtures emit (an item binds those it has). The walls'
 * render is the anchored concrete, an intrinsic base: their grime, ground stain and moss are the weathering layers the
 * items declare on their `wall` slot.
 */
const kowloonSlots = { wall: "render-paint", trim: "concrete-slab", concrete: "concrete-slab", frame: "frame-green", tile: "tile", glass: "glass", pane: "glass",
  galvanized: "galvanized", "vent-metal": "galvanized", "brushed-metal": "galvanized", steel: "flat-steel", metal: "flat-steel", "vent-edge": "flat-steel",
  opening: "openings", "interior-lit": "openings", aircon: "enamel", paint: "enamel", sheet: "enamel", lettering: "enamel", posters: "enamel",
  tarp: "canvas", "balcony-cloth": "canvas", "balcony-plants": "leaves", "shop-timber": "bench-wood", board: "bench-wood", timber: "bench-wood",
  rust: "rusted-pipe" };
export const facade = {
  id: "facade",
  entry: "kowloon-facade-side.item.json",
  definitions: kowloonDefinitions,
  parameters: { width: 12, floors: 4 },
  transform: placed([0, 1, 0], [-1, 0, 0], [0, 0, 1], [-4.5, -8, 0]),
  slots: kowloonSlots,
};
/**
 * The building across the lane (`kowloon-building`), generated from its footprint: by default the reference shop
 * house, 9.8 by 7 m with streets on its front and right and their corner cut. Street edges get shops and signs; the
 * others back doors, few windows, pipes, a riser and duct runs. Its front faces the room; its cut corner is toward the
 * lane's far end (+y). Set `footprint` (counterclockwise plan points) and `streets` (one flag per edge) to reshape it.
 */
export const building = {
  ...facade,
  id: "building",
  entry: "kowloon-building.item.json",
  parameters: {},
  transform: placed([0, 1, 0], [-1, 0, 0], [0, 0, 1], [-4.5, -6.8, 0]),
};

/**
 * The scene's item models: a definition, where it stands and the project material each of its slots shows
 * (`../washroom/build.mjs` names the materials).
 */
export const models = [
  { id: "slab", definition: slab, transform: placed([1, 0, 0], [0, 1, 0], [0, 0, 1], [-0.55, -2.45, -0.2]), slots: { concrete: "concrete-slab", tile: "tile" } },
  { id: "wall-tall", definition: wall, transform: placed([1, 0, 0], [0, 1, 0], [0, 0, 1], [-0.4, 0, 0]),
    slots: { concrete: "concrete-walls", tile: "tile", frame: "flat-steel", glass: "glass" } },
  // The low wall's face looks along +x: its width runs along +y from the room's open end to the corner.
  { id: "wall-low", definition: lowWall, transform: placed([0, 1, 0], [-1, 0, 0], [0, 0, 1], [0, -2.3, 0]), slots: { concrete: "concrete-walls" } },
  { id: "duct", definition: duct, transform: TURNED, slots: { galvanized: "galvanized", steel: "flat-steel", opening: "openings" } },
  { id: "pipe", definition: pipe, transform: TURNED, slots: { steel: "rusted-pipe", bolt: "flat-steel" } },
  { id: "bench", definition: bench, transform: TURNED, slots: { wood: "bench-wood", steel: "flat-steel" } },
  { id: "drain", definition: drain, transform: TURNED, slots: { steel: "flat-steel", opening: "openings" } },
];
