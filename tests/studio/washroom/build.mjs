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
const out = resolve(here, "../../../generated/washroom");

const concrete = "kowloon-concrete.material.ts", steel = "kowloon-rusted-steel.material.ts";
/** Steel without the study's stylized lighting, for parts that are not bare metal. */
const plain = { bandedGloss: 0, silhouette: 0, polishedEdges: 0 };

/**
 * Every material of the scene: an example recipe, its parameters and the meshes it covers. `bolt` is the template of
 * the scene's instanced batch (`buildBatches`): its material is baked once and every bolt shows it.
 */
export const materials = [
  { id: "concrete-walls", entry: concrete, parameters: { darknessA: 0.2, darknessB: 0.22, hueAmount: 0.12, hueAngle: 40 }, meshes: ["wall-tall", "wall-low"] },
  { id: "concrete-slab", entry: concrete, parameters: { ground: true, darknessA: 0.2, darknessB: 0.22, hueAmount: 0.12, hueAngle: 40 }, meshes: ["slab"] },
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
 * `batches: false` builds the scene without its instanced batch (the bolts) and their template mesh; `painted: true`
 * gives the slab the painted ground (`paintedSlab`) instead of concrete.
 */
export function buildProject(resolution = 512, texelDensity = 128, { batches = true, painted = false } = {}) {
  const meshes = buildMeshes({ batches });
  const glb = writeGlb(meshes);
  const identity = name => {
    const index = meshes.findIndex(mesh => mesh.name === name);
    if (index < 0) throw new Error(`No mesh named ${name}.`);
    return `washroom/node/${index}/mesh/${index}/primitive/0`;
  };
  const example = name => readFileSync(resolve(examples, name), "utf8");
  const modules = Object.fromEntries(["kowloon-noise.material.ts", concrete, steel, "tiled-courtyard.material.ts"].map(name => [name, example(name)]));
  for (const name of ["wood.material.ts", ...(painted ? ["painted-ground.material.ts"] : [])]) modules[name] = readFileSync(resolve(here, "materials", name), "utf8");
  const chosen = painted ? materials.map(material => material.id === "concrete-slab" ? paintedSlab : material) : materials;
  const assets = { "washroom.glb": glb };
  for (const name of ["textures/concrete-plain.png", "textures/concrete-fine-sand.png"]) assets[name] = readFileSync(resolve(examples, name));
  const project = {
    schemaVersion: 2, kind: "material", entry: concrete, modules, parameters: {}, seed: 7, resolution,
    ...(texelDensity ? { texelDensity } : {}), inspection: "final", mapping: "world", domain: [1, 1], edges: [], gravity: [0, 0, -1],
    base: { kind: "swatch" }, target: { scope: "mesh", enabled: false },
    models: [{ id: "washroom", kind: "glb", name: "washroom.glb" }],
    materials: chosen.map(material => ({ ...material, seed: 7, meshes: material.meshes.filter(mesh => batches || mesh !== "bolt").map(identity) })),
    ...(batches ? { batches: buildBatches().map(batch => ({ ...batch, mesh: identity(batch.mesh) })) } : {}),
    assets: Object.fromEntries(Object.entries(assets).map(([name, bytes]) => [name, Buffer.from(bytes).toString("base64")])),
  };
  return { glb, project };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { glb, project } = buildProject(Number(process.argv[2] ?? 512), Number(process.argv[3] ?? 128), { painted: process.env.PAINTED === "1" });
  mkdirSync(out, { recursive: true });
  writeFileSync(resolve(out, "washroom.glb"), glb);
  writeFileSync(resolve(out, "washroom.studio.json"), JSON.stringify(project, null, 2) + "\n");
  console.log(`Wrote ${resolve(out, "washroom.studio.json")} (${project.materials.length} materials, ${glb.length} byte model).`);
}
