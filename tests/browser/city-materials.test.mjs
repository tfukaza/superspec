import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import test from "node:test";
import { buildProject } from "../studio/washroom-items/build.mjs";
import { adjacentBuildingContext, allowsSeparateContactSolve, buildingContactGeometry, buildingMaterialFrame, movedBuildingFrame, needsWorldMaterialSolve, splitBuildingSlice } from "../../editor/city-material-frame.ts";

const appearance = new URL("../../crates/engine/src/worldspec/themes/kowloon/materials/", import.meta.url);
const examples = new URL("../../crates/engine/src/materialspec/program/examples/", import.meta.url);

test("city buildings ship the ItemSpec demo's current surface programs and bindings", () => {
  const city = JSON.parse(readFileSync(new URL("street-materials.json", appearance), "utf8"));
  const { project } = buildProject(512, 160, { facade: "building", room: false });
  const building = project.models.find(model => model.id === "building");
  assert.deepEqual(city.roles.building.slots, building.slots);
  assert.deepEqual(city.roles.building.slotDensities, {
    ...building.slotDensities,
    wall: 256, "wall-round": 256, plinth: 256, trim: 256, concrete: 256,
    "suspended-concrete": 256, tile: 256, dado: 256,
  });
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

test("concrete city members use the washroom wall receiver and elevated brackets retain their own finish", async () => {
  const city = JSON.parse(readFileSync(new URL("street-materials.json", appearance), "utf8"));
  const { materials } = await import("../studio/washroom/build.mjs");
  const washroom = materials.find(material => material.id === "concrete-walls");
  for (const role of ["pillar", "wall"]) {
    const member = city.materials.find(material => material.id === city.roles[role].slots.concrete);
    assert.equal(member.entry, washroom.entry);
    assert.deepEqual(member.parameters, washroom.parameters);
    assert.equal(member.seed, 7);
  }
  const balcony = city.materials.find(material => material.id === city.roles.building.slots["suspended-concrete"]);
  assert.equal(balcony.entry, washroom.entry);
  assert.equal(balcony.parameters.groundContact, false);
  assert.equal(balcony.parameters.holes, false);
});

test("city finishes follow horizontal moves and include the actual neighboring contact triangles", () => {
  const building = { id: "p0", entry: "kowloon-building.item.json", kind: "item", parameters: { seed: 12 },
    transform: [[0, 1, 0], [-1, 0, 0], [0, 0, 1], [32, -6, 3]] };
  const owners = new Map([["p0", "city/shop"]]);
  const shell = { objectId: "city/shop", visible: true, bounds: [[28, -6, 3], [32, -1, 14]] };
  const ground = { objectId: "paving", visible: true, bounds: [[0, -20, 0], [50, 20, 0.1]] };
  const frame = buildingMaterialFrame([building], owners, { meshes: [shell, ground] });
  assert.deepEqual(frame.origin, [32, -6, 0]);
  assert.deepEqual(frame.model.transform, [[0, 1, 0], [-1, 0, 0], [0, 0, 1], [0, 0, 3]]);
  assert.deepEqual(building.transform[3], [32, -6, 3]);
  const moved = { ...building, transform: [...building.transform.slice(0, 3), [35, -4, 3]] };
  assert.deepEqual(buildingMaterialFrame([moved], owners, { meshes: [shell] }).model, frame.model);
  const touching = { ...ground, bounds: [[31.9995, -6, 3], [35, -1, 5]], vertexStart: 0, indexStart: 0, indexCount: 6 };
  const contact = buildingMaterialFrame([building], owners, { meshes: [shell, touching] });
  assert.deepEqual(contact.model, frame.model);
  assert.deepEqual(contact.neighbors, [touching]);
  const glb = buildingContactGeometry(contact, {
    indices: Uint32Array.from([0, 1, 2, 3, 4, 5]),
    positions: Float32Array.from([32,-6,3, 35,-6,3, 32,-1,5, 35,-4,3, 35,-6,3, 34.5,-1,5]),
  });
  const offset = new DataView(glb.buffer).getUint32(12, true) + 28;
  // The first triangle touches; the second lies beyond the receiver's X extent.
  assert.deepEqual([...new Float32Array(glb.buffer.slice(offset))], [0,3,0, 3,3,0, 0,5,-5]);
  assert.equal(buildingMaterialFrame([building], owners, { meshes: [shell], batches: [{ ...touching, instanceCount: 2 }] }), undefined);
  assert.equal(buildingMaterialFrame([building, { id: "wall" }], owners, { meshes: [shell] }), undefined);
  assert.equal(buildingMaterialFrame([building], owners, { meshes: [], batches: [{ ...shell, instanceCount: 2 }] }), undefined);
  const pillar = { id: "pillar", entry: "kowloon-pillar.item.json" };
  owners.set("pillar", "paving");
  assert.deepEqual(splitBuildingSlice([pillar, building], owners, { meshes: [shell, ground] }), [[building],[pillar]]);
  assert.deepEqual(splitBuildingSlice([pillar, building], owners, { meshes: [shell, touching] }), [[building],[pillar]]);
  assert.deepEqual(adjacentBuildingContext([pillar], owners, { meshes: [shell, touching] }).neighbors, [shell]);
  const layer = requiredInputs => ({ metadata: { requiredInputs } });
  assert.equal(allowsSeparateContactSolve({ concrete: { requiredInputs: ["features"], layers: [layer(["plane:base.base_color"])] } }), true);
  assert.equal(allowsSeparateContactSolve({ concrete: { requiredInputs: [], layers: [layer(["plane:sunExposure"])] } }), false);
  assert.equal(allowsSeparateContactSolve({ concrete: { requiredInputs: ["plane:shadow"] } }), false);
  assert.equal(allowsSeparateContactSolve({ concrete: {} }), false);
  assert.equal(allowsSeparateContactSolve({ concrete: { requiredInputs: [] } }, { wall: { sceneReachDropped: 0 } }), false);
});

test("a singleton building with external plane inputs or scene reach keeps a world-space material solve", () => {
  const frame = { origin: [20, 0, 0] };
  const local = { materials: { concrete: { requiredInputs: ["features"] } } };
  const external = { materials: { concrete: { requiredInputs: ["plane:sunExposure"] } } };
  const sceneReach = { ...local, items: { building: { sceneReachDropped: 0 } } };
  assert.equal(needsWorldMaterialSolve([{ frame, metadata: local }]), false);
  assert.equal(needsWorldMaterialSolve([{ frame, metadata: external }]), true);
  assert.equal(needsWorldMaterialSolve([{ frame, metadata: sceneReach }]), true);
  assert.equal(needsWorldMaterialSolve([{ frame, metadata: local }, { metadata: external }]), true);
  assert.equal(needsWorldMaterialSolve([{ metadata: external }]), false); // already in world space
  assert.equal(needsWorldMaterialSolve([{ metadata: external, optional: true }]), true); // cropped external contact geometry
});

test("one mover among several material frames retains maps only when all contact and project digests agree", () => {
  const first = { index: 0, key: "a", owner: "city/first", origin: [10,5,0], contactsCovered: true };
  const second = { index: 3, key: "b", owner: "city/second", origin: [50,3,0], contactsCovered: true };
  const pillar = { index: 4, key: "post", contactsCovered: true };
  const prior = [first, second, pillar];
  const moved = { ...second, origin: [53,4,0] };
  assert.deepEqual(movedBuildingFrame(prior, [first, moved, pillar]), { owner: second.owner, from: second.origin, to: moved.origin });
  assert.deepEqual(movedBuildingFrame(prior, prior), { owner: first.owner, from: first.origin, to: first.origin });
  for (const next of [
    [{ ...first, key: "neighbor contacts changed" }, moved, pillar],
    [first, moved, { ...pillar, key: "post contacts changed" }],
    [first, moved, { ...pillar, contactsCovered: false }],
    [first, { ...moved, key: "different footprint" }, pillar],
    [{ ...first, origin: [8,4,0] }, moved, pillar],
    [first, { ...moved, owner: "city/replacement" }, pillar],
    [first, { ...moved, origin: [50,3,2] }, pillar],
    [moved, first, pillar], [first, moved],
  ]) assert.equal(movedBuildingFrame(prior, next), undefined);
});

test("contact companions see both sides of building slice boundaries, including a prop paired with the stationary building", () => {
  const model = (id, x) => ({ id, entry: "kowloon-building.item.json", transform: [[1,0,0],[0,1,0],[0,0,1],[x,0,0]] });
  const first = model("p0", 0), post = { id: "p1", entry: "kowloon-pillar.item.json" }, second = model("p2", 1);
  const owners = new Map([["p0","city/a"],["p1","city/post"],["p2","city/b"]]);
  const mesh = (objectId,x,index) => ({ objectId, bounds: [[x,0,0],[x+1,1,1]], visible: true, vertexStart: 3*index, indexStart: 3*index, indexCount: 3 });
  const geometry = x => ({
    manifest: { meshes: [mesh("city/a",0,0),mesh("city/post",0.5,1),mesh("city/b",x,2)] },
    positions: Float32Array.from([1,0,0, 1,1,0, 1,0,1, 1,0,0, 1,1,0, 1,0,1, x,0,0, x,1,0, x,0,1]),
    indices: Uint32Array.from([0,1,2,0,1,2,0,1,2]),
  });
  const reports = (x) => {
    const packet = geometry(x);
    assert.deepEqual(splitBuildingSlice([first, post], owners, packet.manifest), [[first],[post]]);
    const contexts = [buildingMaterialFrame([first], owners, packet.manifest),
      adjacentBuildingContext([post], owners, packet.manifest),
      buildingMaterialFrame([model(second.id,x)], owners, packet.manifest)];
    return contexts.map(ctx => buildingContactGeometry(ctx, packet));
  };
  const apart = reports(20), touching = reports(1);
  assert.ok(apart[0] && apart[1] && !apart[2]);
  for (const [i, geometry] of touching.entries()) {
    assert.ok(geometry);
    assert.notDeepEqual(geometry, apart[i]);
  }
});
