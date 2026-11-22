import { readFileSync } from "node:fs";

const source = (path, format) => readFileSync(new URL(path, import.meta.url), format);
const theme = "../crates/engine/src/worldspec/themes/kowloon/materials/street-materials.json";
const examples = "../crates/engine/src/materialspec/program/examples/";

/** Build a standalone Material Studio project for existing closed GLB meshes. Each
 * primitive is a separate receiver; keep caps and sides in the same primitive. Pass
 * IDs such as "concrete/node/3/mesh/1/primitive/0". The output retains source geometry. */
export function concreteProject(glb, receiverIds, {
  elevated = [], resolution = 2048, texelDensity = 160,
} = {}) {
  const catalog = JSON.parse(source(theme, "utf8"));
  const modules = {}, assets = { "members.glb": Buffer.from(glb).toString("base64") };
  const entry = "kowloon-concrete.material.ts";
  for (const module of [entry, "kowloon-noise.material.ts"]) modules[module] = source(examples + module, "utf8");
  for (const asset of ["textures/concrete-plain.png", "textures/concrete-fine-sand.png"])
    assets[asset] = source(examples + asset).toString("base64");
  const groups = [["washroom-concrete", receiverIds], ["balcony-concrete", elevated]];
  const materials = groups.filter(([, meshes]) => meshes.length).map(([id, meshes]) => ({
    ...catalog.materials.find(m => m.id === id), meshes,
  }));
  return {
    schemaVersion: 2, kind: "material", entry, parameters: {}, seed: 7, modules, assets,
    resolution, texelDensity, profile: resolution > 512 ? "production" : "preview",
    mapping: "world", domain: [1, 1], edges: [], gravity: [0, 0, -1], inspection: "final",
    base: { kind: "swatch" }, target: { scope: "mesh", enabled: false },
    models: [{ id: "concrete", kind: "glb", name: "members.glb" }], materials,
  };
}
