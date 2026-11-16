/**
 * The washroom-items project with the split materials of the weathering-layer migration: the walls use the anchored
 * Kowloon concrete (intrinsic, on each part's pattern anchor) under top grime, ground stain, moss, worn edges and
 * grime along the base and under the sill; the duct and the pipe use the shipped steel under a joint-rust layer. Layer
 * parameters are the defaults, except the base grime, which matches the washroom-items walls' `featureGrime` 0.8.
 * Shared by tests/stage2-weathering-browser.mjs (CPU bake) and tests/stage2-gpu-layers-browser.mjs (GPU preview).
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const examples = resolve(dirname(fileURLToPath(import.meta.url)), "../../../crates/engine/src/materialspec/program/examples");

/** The layer modules the split project adds. */
export const LAYER_MODULES = ["kowloon-concrete-anchored", "kowloon-top-grime", "kowloon-ground", "kowloon-ground-stain", "kowloon-moss",
  "kowloon-concrete-edges", "kowloon-feature-grime", "kowloon-joint-rust"];

/** A copy of `project` (from `buildProject`) with the split materials on the walls, the duct and the pipe. */
export function split(project) {
  const copy = structuredClone(project);
  for (const name of LAYER_MODULES) copy.modules[`${name}.material.ts`] = readFileSync(resolve(examples, `${name}.material.ts`), "utf8");
  const layer = (name, parameters = {}, follow) => ({ entry: `${name}.material.ts`, parameters, seed: 7, ...(follow ? { follow: { follow } } : {}) });
  const walls = copy.materials.find(material => material.id === "concrete-walls");
  walls.entry = "kowloon-concrete-anchored.material.ts";
  delete walls.parameters.featureGrime;
  walls.layers = [
    layer("kowloon-top-grime"), layer("kowloon-ground-stain"), layer("kowloon-moss"), layer("kowloon-concrete-edges"),
    // The washroom-items walls' grime along the base and under the sill (`featureGrime` 0.8).
    layer("kowloon-feature-grime", { amount: 0.68, near: 0, far: 0.3, color: [0.035, 0.032, 0.026], roughness: 0.95 }, "base"),
    layer("kowloon-feature-grime", {}, "sill"),
  ];
  copy.mappings = { ...(copy.mappings ?? {}), "concrete-walls": "anchored" };
  for (const id of ["galvanized", "rusted-pipe"]) {
    const steel = copy.materials.find(material => material.id === id);
    steel.entry = "kowloon-rusted-steel.material.ts";
    const oxide = Object.fromEntries(Object.entries(steel.parameters).filter(([name]) => ["rustRepeat", "rustRoughness", "oxideDark", "oxideLight"].includes(name)));
    steel.layers = [layer("kowloon-joint-rust", { amount: 0.9, ...oxide })];
  }
  return copy;
}
