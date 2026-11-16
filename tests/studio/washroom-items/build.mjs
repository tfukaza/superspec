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
import { materials as washroomMaterials } from "../washroom/build.mjs";
import { buildBatches, buildMeshes } from "../washroom/model.mjs";
import { writeGlb } from "../washroom/geometry.mjs";
import { models } from "./items.mjs";

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
 * `weathering: false` uses the shipped recipes as they are (nothing follows a feature); `batches: false` leaves out the
 * bolts of the brackets and the vent, which are the GLB washroom's instanced batch without its two flange rings (the
 * pipe item makes its own flange bolts).
 */
export function buildProject(resolution = 512, texelDensity = 128, { weathering = true, batches = true, anchored = false, walls } = {}) {
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
  const scene = models.map(({ id, definition, transform, slots }) => ({ id, kind: "item", entry: `items/${definition.id}.item.json`, parameters: {}, transform, slots }));
  let batchList;
  if (batches) {
    // The bolt template is the GLB washroom's own mesh, alone in a model; it is baked once as flat steel.
    assets["bolt.glb"] = writeGlb(buildMeshes().filter(mesh => mesh.name === "bolt"));
    scene.push({ id: "bolt", kind: "glb", name: "bolt.glb" });
    const template = "bolt/node/0/mesh/0/primitive/0";
    materials.find(material => material.id === "flat-steel").meshes.push(template);
    batchList = buildBatches().map(batch => ({ ...batch, mesh: template, instances: batch.instances.slice(16) }));
  }
  const project = {
    schemaVersion: 2, kind: "material", entry: concrete, modules, parameters: {}, seed: 7, resolution,
    ...(texelDensity ? { texelDensity } : {}), inspection: "final", mapping: "world", domain: [1, 1], edges: [], gravity: [0, 0, -1],
    base: { kind: "swatch" }, target: { scope: "mesh", enabled: false },
    // `anchored`: the walls' concrete is measured from each part's pattern anchor, so a resize does not move it.
    ...(anchored ? { mappings: { "concrete-walls": "anchored" } } : {}),
    models: scene, materials, ...(batchList ? { batches: batchList } : {}),
    assets: Object.fromEntries(Object.entries(assets).map(([name, bytes]) => [name, Buffer.from(bytes).toString("base64")])),
  };
  return { project, definitions, assets };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { project, definitions, assets } = buildProject(Number(process.argv[2] ?? 512), Number(process.argv[3] ?? 128));
  for (const [name, content] of [...Object.entries(definitions), ...Object.entries(assets).filter(([name]) => name.startsWith("decals/"))]) {
    mkdirSync(dirname(resolve(out, name)), { recursive: true });
    writeFileSync(resolve(out, name), content);
  }
  writeFileSync(resolve(out, "washroom-items.studio.json"), JSON.stringify(project, null, 2) + "\n");
  console.log(`Wrote ${resolve(out, "washroom-items.studio.json")} (${project.models.length} models, ${project.materials.length} materials, ${Object.keys(definitions).length} item definitions).`);
}
