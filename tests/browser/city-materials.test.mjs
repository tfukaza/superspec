import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import test from "node:test";
import { buildProject } from "../studio/washroom-items/build.mjs";

const appearance = new URL("../../crates/engine/src/worldspec/themes/kowloon/appearance/", import.meta.url);
const examples = new URL("../../crates/engine/src/materialspec/program/examples/", import.meta.url);

test("city buildings ship the ItemSpec demo's current surface programs and bindings", () => {
  const city = JSON.parse(readFileSync(new URL("street-materials.json", appearance), "utf8"));
  const { project } = buildProject(512, 160, { facade: "building", room: false });
  const building = project.models.find(model => model.id === "building");
  assert.deepEqual(city.roles.building.slots, building.slots);
  assert.deepEqual(city.roles.building.slotDensities, building.slotDensities);
  for (const id of new Set(Object.values(building.slots))) {
    const actual = city.materials.find(material => material.id === id);
    const demo = project.materials.find(material => material.id === id);
    assert.ok(actual && demo, id);
    for (const field of ["entry", "parameters", "seed"])
      assert.deepEqual(actual[field], demo[field], id + " " + field);
    const recipe = [appearance, examples].map(base => new URL(actual.entry, base)).find(existsSync);
    assert.equal(readFileSync(recipe, "utf8").trim(), project.modules[demo.entry].trim(), id);
  }
});
