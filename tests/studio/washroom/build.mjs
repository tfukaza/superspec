/**
 * Writes the washroom benchmark scene: `generated/washroom/washroom.glb` and a version 2 Studio
 * project that applies the shipped example recipes to it. Usage: node tests/studio/washroom/build.mjs [resolution] [texels per meter, 0 for none]
 */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { buildBatches, buildMeshes } from "./model.mjs";
import { writeGlb } from "./geometry.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const examples = resolve(here, "../../../crates/engine/src/materialspec/program/examples");

const concrete = "kowloon-concrete.material.ts", steel = "kowloon-rusted-steel.material.ts";
const daylightLighting = {
  version: 1,
  ambient: { intensity: 0.24 },
  environment: { source: "studio", intensity: 0.22 },
  lights: [{
    id: "sun", role: "sun", kind: "directional", direction: [-0.72, -0.28, Math.sqrt(1 - 0.72 ** 2 - 0.28 ** 2)],
    color: [1, 0.88, 0.73], intensity: 3, castsShadow: true, shadow: { style: "plain" },
  }],
};
/** Steel without the study's stylized lighting, for parts that are not bare metal. */
const plain = { bandedGloss: 0, silhouette: 0, polishedEdges: 0 };
/** The city's washroom-concrete finish uses restrained color patches and shallow chips. */
const illustrated = {
  aggregateFinish: 1,
  mottlingBreakup: 0.9, chipAmount: 0.28, chipScale: 0.4, chipRelief: 0.0018, edgeWidth: 0.035, edgeChipping: 0.6, edgeContrast: 1.15,
  flatten: 0.85, illustration: 0.5, patchScale: 2.4, blendScale: 4, warmTint: [1.12, 1.03, 0.93], coolTint: [0.9, 0.92, 1.02], brushStrokes: 0.25,
};

/**
 * Every material of the scene: an example recipe, its parameters and the meshes it covers. `bolt` is the template of
 * the scene's instanced batch (`buildBatches`): its material is baked once and every bolt shows it.
 */
export const materials = [
  { id: "concrete-walls", entry: concrete, parameters: { darknessA: 0.14, darknessB: 0.15, hueAmount: 0.12, hueAngle: 40, ...illustrated }, meshes: ["wall-tall", "wall-low"] },
  { id: "concrete-slab", entry: concrete, parameters: { ground: true, darknessA: 0.14, darknessB: 0.15, hueAmount: 0.12, hueAngle: 40, ...illustrated }, meshes: ["slab"] },
  { id: "tile", entry: "tiled-courtyard.material.ts", parameters: { tileSize: [0.15, 0.15], stagger: 0, wear: 0.3, glaze: [0.2, 0.42, 0.3], glazeRoughness: 0.12, missing: 0.1 }, meshes: ["tiles-wall", "tiles-floor"] },
  { id: "galvanized", entry: steel, parameters: { substrate: [0.5, 0.55, 0.57], metalness: 0.85, roughness: 0.38, surfaceRust: 0.12, rainRust: 0.25 }, meshes: ["duct"] },
  { id: "rusted-pipe", entry: steel, parameters: { substrate: [0.3, 0.15, 0.08], metalness: 0.45, roughness: 0.5, oxideLight: [0.36, 0.13, 0.035], surfaceRust: 0.8 }, meshes: ["pipe"] },
  { id: "flat-steel", entry: steel, parameters: { rainRust: 0.5 }, meshes: ["brackets", "drain", "bolt"] },
  { id: "openings", entry: steel, parameters: { ...plain, substrate: [0.012, 0.012, 0.012], metalness: 0, roughness: 1, edgeRust: 0, surfaceRust: 0, rainRust: 0 }, meshes: ["openings"] },
  { id: "bench-wood", entry: "wood.material.ts", parameters: {}, meshes: ["bench"] },
];

/**
 * The slab's material with `painted: true` (roadmap stage 5): a painted ground whose shade boundary follows the sun of the
 * lighting record, drawn by its program in place of the slab's concrete.
 */
export const paintedSlab = { id: "painted-ground", entry: "painted-ground.material.ts", parameters: {}, meshes: ["slab"] };

/**
 * The lawn outside the room (roadmap stage 6): its ground (`painted-lawn`) and its grass, a TreeSpec patch delivered as
 * instanced batches with a foliage look that names `painted-grass`, a material assigned to no mesh. Both come from one
 * document TreeSpec writes (`cargo test --release -p superspec-engine --test treespec_batches -- --ignored
 * write_washroom_lawn`), so the grass stands on the ground it was grown on.
 */
export const lawnMaterials = [
  { id: "painted-lawn", entry: "painted-lawn.material.ts", parameters: {}, meshes: ["lawn"] },
  { id: "painted-grass", entry: "painted-grass.material.ts", parameters: {}, meshes: [] },
];
export const lawnPath = resolve(here, "../../../generated/verification/treespec/washroom-lawn.json");
/** The lawn document (`LAWN` names another). */
export function loadLawn(path = process.env.LAWN ?? lawnPath) {
  return JSON.parse(readFileSync(path, "utf8"));
}

/**
 * The plants on the lawn (roadmap stage 7): a shrub and a small broadleaf tree, TreeSpec plants delivered as instanced
 * batches with foliage looks of the plant families, every level of detail and a shadow proxy each. Their bark names
 * `painted-bark` and their leaves and hulls `painted-canopy`, materials assigned to no mesh. The document is written by
 * `cargo test --release -p superspec-engine --test treespec_batches -- --ignored write_washroom_trees`.
 */
export const treeMaterials = [
  { id: "painted-bark", entry: "painted-bark.material.ts", parameters: {}, meshes: [] },
  { id: "painted-canopy", entry: "painted-canopy.material.ts", parameters: {}, meshes: [] },
];
export const treesPath = resolve(here, "../../../generated/verification/treespec/washroom-trees.json");
/** The plants document (`TREES_DOCUMENT` names another). */
export function loadTrees(path = process.env.TREES_DOCUMENT ?? treesPath) {
  return JSON.parse(readFileSync(path, "utf8"));
}

/**
 * `batches: false` builds the scene without its instanced batch (the bolts) and their template mesh; `painted: true`
 * gives the slab the painted ground (`paintedSlab`) instead of concrete; `grass` (true, or a lawn document) adds the
 * lawn and its grass (`lawnMaterials`); `trees` (true, or a plants document) adds the plants standing on that lawn
 * (`treeMaterials`), and with them the lawn. `daylight` adds a clerestory, a soffit and a directional lighting setup.
 */
export function buildProject(resolution = 512, texelDensity = 128, { batches = true, painted = false, grass = false, trees = false, daylight = false } = {}) {
  const plants = trees === true ? loadTrees() : trees || null;
  const lawn = grass === true || (plants && !grass) ? loadLawn() : grass || null;
  const meshes = buildMeshes({ batches, lawn, daylight });
  const glb = writeGlb(meshes);
  const identity = name => {
    const index = meshes.findIndex(mesh => mesh.name === name);
    if (index < 0) throw new Error(`No mesh named ${name}.`);
    return `washroom/node/${index}/mesh/${index}/primitive/0`;
  };
  const example = name => readFileSync(resolve(examples, name), "utf8");
  const modules = Object.fromEntries(["kowloon-noise.material.ts", concrete, steel, "tiled-courtyard.material.ts"].map(name => [name, example(name)]));
  for (const name of ["wood.material.ts", ...(painted ? ["painted-ground.material.ts"] : []), ...(lawn ? lawnMaterials.map(material => material.entry) : []),
    ...(plants ? treeMaterials.map(material => material.entry) : [])]) {
    modules[name] = readFileSync(resolve(here, "materials", name), "utf8");
  }
  const chosen = [...(painted ? materials.map(material => material.id === "concrete-slab" ? paintedSlab : material) : materials), ...(lawn ? lawnMaterials : []),
    ...(plants ? treeMaterials : [])];
  const assets = { "washroom.glb": glb };
  for (const name of ["textures/concrete-plain.png", "textures/concrete-fine-sand.png"]) assets[name] = readFileSync(resolve(examples, name));
  const project = {
    schemaVersion: 2, kind: "material", entry: concrete, modules, parameters: {}, seed: 7, resolution,
    ...(texelDensity ? { texelDensity } : {}), inspection: "final", mapping: "world", domain: [1, 1], edges: [], gravity: [0, 0, -1],
    base: { kind: "swatch" }, target: { scope: "mesh", enabled: false },
    models: [{ id: "washroom", kind: "glb", name: "washroom.glb" }],
    ...(daylight ? { lighting: structuredClone(daylightLighting) } : {}),
    materials: chosen.map(material => {
      const receivers = daylight && material.id === "concrete-walls" ? [...material.meshes, "soffit"] : material.meshes;
      return { ...material, seed: 7, meshes: receivers.filter(mesh => batches || mesh !== "bolt").map(identity) };
    }),
    ...(batches || lawn ? { batches: [...(batches ? buildBatches().map(batch => ({ ...batch, mesh: identity(batch.mesh) })) : []), ...(lawn?.batches ?? []), ...(plants?.batches ?? [])] } : {}),
    assets: Object.fromEntries(Object.entries(assets).map(([name, bytes]) => [name, Buffer.from(bytes).toString("base64")])),
  };
  return { glb, project };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const daylight = process.env.DAYLIGHT === "1";
  const out = resolve(here, "../../../generated/washroom", daylight ? "daylight" : "");
  const { glb, project } = buildProject(Number(process.argv[2] ?? 512), Number(process.argv[3] ?? 128), {
    painted: process.env.PAINTED === "1", grass: process.env.GRASS === "1",
    trees: process.env.TREES === "1", daylight,
  });
  mkdirSync(out, { recursive: true });
  writeFileSync(resolve(out, "washroom.glb"), glb);
  writeFileSync(resolve(out, "washroom.studio.json"), JSON.stringify(project, null, 2) + "\n");
  console.log(`Wrote ${resolve(out, "washroom.studio.json")} (${project.materials.length} materials, ${glb.length} byte model).`);
}
