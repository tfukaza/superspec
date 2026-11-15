import concrete from "../../crates/engine/src/materialspec/program/examples/chipped-concrete.ms?raw";
import metal from "../../crates/engine/src/materialspec/program/examples/painted-metal.ms?raw";
import common from "../../crates/engine/src/materialspec/program/examples/common.ms?raw";
import concreteTs from "../../crates/engine/src/materialspec/program/examples/chipped-concrete.material.ts?raw";
import metalTs from "../../crates/engine/src/materialspec/program/examples/painted-metal.material.ts?raw";
import courtyardTs from "../../crates/engine/src/materialspec/program/examples/tiled-courtyard.material.ts?raw";
import commonTs from "../../crates/engine/src/materialspec/program/examples/common.material.ts?raw";
import kowloonNoise from "../../crates/engine/src/materialspec/program/examples/kowloon-noise.material.ts?raw";
import kowloonConcrete from "../../crates/engine/src/materialspec/program/examples/kowloon-concrete.material.ts?raw";
import kowloonSteel from "../../crates/engine/src/materialspec/program/examples/kowloon-rusted-steel.material.ts?raw";
import concretePlainUrl from "../../crates/engine/src/materialspec/program/examples/textures/concrete-plain.png?url";
import concreteSandUrl from "../../crates/engine/src/materialspec/program/examples/textures/concrete-fine-sand.png?url";
import type { StudioProject } from "./project.ts";

/** Both source languages ship together; the `.ms` examples stay the default entry for existing projects. */
export const materialExamples = {
  "chipped-concrete.ms": concrete, "painted-metal.ms": metal, "common.ms": common,
  "chipped-concrete.material.ts": concreteTs, "painted-metal.material.ts": metalTs,
  "tiled-courtyard.material.ts": courtyardTs, "common.material.ts": commonTs,
  "kowloon-noise.material.ts": kowloonNoise, "kowloon-concrete.material.ts": kowloonConcrete,
  "kowloon-rusted-steel.material.ts": kowloonSteel,
};

/** Companion images the Kowloon concrete recipe samples, by the relative names it uses. */
const kowloonTextureUrls: Record<string, string> = {
  "textures/concrete-plain.png": concretePlainUrl, "textures/concrete-fine-sand.png": concreteSandUrl,
};

/** Starters that need companion images before they compile. */
export function needsKowloonTextures(starter: string): boolean {
  return starter === "kowloon-concrete.material.ts" || starter === "kowloon-yard";
}

/** Fetches the concrete study's source images once per page. */
let kowloonTextures: Promise<Record<string, Uint8Array>> | undefined;
export function loadKowloonTextures(): Promise<Record<string, Uint8Array>> {
  kowloonTextures ??= Promise.all(Object.entries(kowloonTextureUrls).map(async ([name, url]) => {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Could not load ${name}.`);
    return [name, new Uint8Array(await response.arrayBuffer())] as const;
  })).then(Object.fromEntries);
  return kowloonTextures;
}

/** A concrete wall and slab beside a rusted steel crate and a painted post: two models, four materials. */
export const kowloonYard = {
  models: [
    { id: "yard", kind: "aml" as const, source: `<architecture version="1" id="yard" name="Concrete yard">`  +
      `<material id="plain" color="#9a978f"/>` +
      `<box id="wall" width="1.2" depth="0.22" height="1.1" material="plain"/>` +
      `<box id="slab" position="0 0.7 0" width="1.4" depth="0.8" height="0.08" material="plain"/>` +
      `</architecture>` },
    { id: "steel", kind: "aml" as const, source: `<architecture version="1" id="steel" name="Steel fixtures">` +
      `<material id="steel" color="#566368"/>` +
      `<box id="crate" position="0.95 0.65 0" width="0.4" depth="0.3" height="0.25" material="steel"/>` +
      `<box id="post" position="-0.85 0.55 0" width="0.08" depth="0.08" height="0.9" material="steel"/>` +
      `</architecture>` },
  ],
  materials: [
    { id: "concrete-wall", entry: "kowloon-concrete.material.ts", parameters: {}, seed: 7, meshes: ["yard/wall"] },
    { id: "concrete-slab", entry: "kowloon-concrete.material.ts", parameters: { ground: true }, seed: 7, meshes: ["yard/slab"] },
    { id: "rusted-steel", entry: "kowloon-rusted-steel.material.ts", parameters: {}, seed: 7, meshes: ["steel/crate"] },
    { id: "painted-steel", entry: "kowloon-rusted-steel.material.ts", seed: 7, meshes: ["steel/post"],
      parameters: { substrate: [0.042, 0.095, 0.085], metalness: 0.16, roughness: 0.29 } },
  ],
};
const itemFinish = `material 1;
param roughness: scalar = 0.45 range(0.0, 1.0);
param tint: vec3 = vec3(0.12, 0.24, 0.22) range(0.0, 1.0);
let brushing = noise(surface * vec2(180.0, 5.0), seed("brushing"));
output base_color = tint * (0.88 + brushing * 0.12);
output roughness = roughness;
output metalness = 0.7;
output height = brushing * 0.00002;
`;

/** Creates editable local geometry with one material request and an exposed finish control. */
export function itemSource(kind: "pipe" | "duct" | "glb", asset = "model.glb"): string {
  const number = (value: number, min: number, max: number, role = "geometry") => ({ type: "number", role, default: value, min, max });
  const parameters: Record<string, unknown> = { roughness: number(0.45, 0, 1, "finish") };
  let generator: Record<string, unknown> = { kind: "glb", asset };
  if (kind !== "glb") {
    parameters.length = { ...number(2, 0.05, 100), unit: "m" };
    generator = { kind, length: { parameter: "length" }, wallThickness: 0.01 };
    if (kind === "pipe") {
      parameters.radius = { ...number(0.1, 0.02, 5), unit: "m" };
      Object.assign(generator, { radius: { parameter: "radius" }, segments: 24, capped: true });
    } else {
      parameters.width = { ...number(0.5, 0.03, 20), unit: "m" };
      parameters.height = { ...number(0.3, 0.03, 20), unit: "m" };
      Object.assign(generator, { width: { parameter: "width" }, height: { parameter: "height" } });
    }
  }
  return JSON.stringify({ format: "itemspec", version: 1, id: kind === "glb" ? "imported-item" : kind,
    parameters, generator, slots: kind === "glb" ? {} : { surface: { entry: "materials/metal.ms", parameters: { roughness: { parameter: "roughness" } }, seed: 0 } },
    collision: "mesh" }, null, 2) + "\n";
}

/** Initializes complete, portable source modules rather than an opaque compiled preview. */
export function initialProject(kind: StudioProject["kind"]): StudioProject {
  return { schemaVersion: 1, kind, entry: kind === "material" ? "chipped-concrete.ms" : "main.item.json",
    modules: kind === "material" ? { ...materialExamples } : { "main.item.json": itemSource("pipe"), "materials/metal.ms": itemFinish },
    parameters: {}, seed: 7, resolution: kind === "material" ? 256 : 96, inspection: "final", mapping: "object", domain: [1, 1],
    edges: [], gravity: [0, 0, -1],
    assets: {}, base: { kind: "swatch" }, target: { scope: "mesh", enabled: false } };
}
