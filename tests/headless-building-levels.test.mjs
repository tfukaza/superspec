// The building bake shades a prototype through a project that regenerates the building item
// from parameters derived in editor/city-project.ts. Those must describe the building the
// compiled scene drew, including its graphics detail, or the bake's receivers are another
// building's. Run after `npm run build:headless`; the native half needs STUDIO_CLI (or
// target/release/superspec-studio) built from the same tree.
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import test from "node:test";

const core = new URL("../generated/headless/core.js", import.meta.url);
const built = existsSync(core);
const { modelsOf, cityProject, decodePacket, atlasLayouts } = built ? await import(core.href) : {};

function packetOf(attributes) {
  return {
    manifest: {
      meshes: [{ objectId: "city/prototype" }],
      architecture: { metadata: [{ kind: "city", children: [{ id: "prototype", kind: "building", attributes: { position: [0, 0, 0], ...attributes } }] }] },
    },
  };
}

test("a building's shading model carries its graphics detail and front trade", { skip: !built && "generated/headless/core.js is not built" }, () => {
  const parameters = attributes => modelsOf(packetOf(attributes)).models[0].parameters;
  const authored = parameters({ floors: 5, seed: 8 });
  assert.equal("graphicsDetail" in authored, false);
  for (const level of [0, 1, 2]) {
    const { graphicsDetail, ...rest } = parameters({ floors: 5, seed: 8, "graphics-detail": level });
    assert.equal(graphicsDetail, level);
    assert.deepEqual(rest, authored, "the level changes nothing else");
  }
  assert.equal(parameters({ model: "reference-corner", "graphics-detail": 2 }).graphicsDetail, 2);
  assert.equal("variantBudget" in authored, false, "unstated, the budget is the level's own");
  for (const budget of [0, 1, 8, 64]) {
    const { variantBudget, ...rest } = parameters({ floors: 5, seed: 8, "variant-budget": budget });
    assert.equal(variantBudget, budget);
    assert.deepEqual(rest, authored, "the budget changes nothing else");
  }
  const both = parameters({ model: "reference-corner", "graphics-detail": 1, "variant-budget": 16 });
  assert.deepEqual([both.graphicsDetail, both.variantBudget], [1, 16]);
  const traded = parameters({ floors: 5, seed: 8, "front-trade": "grocer" });
  assert.equal(traded.shopPlans[0][0], "open");
  assert.equal(traded.tradePlans[0][0], "grocer");
  assert.deepEqual(parameters({ floors: 5, seed: 8 }), authored, "a front trade leaves the shared families alone");
});

const host = built ? await import("../tools/headless/host.mjs") : undefined;
const native = host && existsSync(host.studio);

test("the shading project regenerates the meshes the compiled building delivers at every level",
  { skip: !native && "the native Studio CLI or generated/headless/core.js is not built", timeout: 600_000 }, async () => {
    const counts = [];
    // The last case states a budget other than its level's own (2 at level 1): the project must
    // regenerate that building's catalogue picks, not the level's.
    for (const [level, budget] of [[undefined], [1], [2], [1, 16]]) {
      const detail = (level === undefined ? "" : ` graphics-detail="${level}"`) + (budget === undefined ? "" : ` variant-budget="${budget}"`);
      const source = `<architecture version="1" id="building-0"><city><building id="prototype" position="0 0 0" floors="5" seed="8"${detail} /></city></architecture>`;
      const packet = decodePacket(await host.compile("aml-scene", { source, revision: 1 }));
      assert.ok(packet.manifest.success, JSON.stringify(packet.manifest.diagnostics));
      const { models, owners } = modelsOf(packet);
      assert.equal(models.length, 1);
      assert.equal(models[0].parameters.variantBudget, budget);
      const project = cityProject(models, packet, {});
      // The bundle carries its own copy of every item definition and recipe, which the project
      // hands to the native compiler. A copy older than the tree regenerates another building.
      if (level === undefined) {
        const sources = ["worldspec/themes/kowloon/items/", "worldspec/themes/kowloon/materials/", "materialspec/program/examples/"]
          .map(directory => new URL("../crates/engine/src/" + directory, import.meta.url));
        assert.ok("kowloon-building.item.json" in project.modules);
        for (const [name, bundled] of Object.entries(project.modules)) {
          const source = sources.map(directory => new URL(name, directory)).find(existsSync);
          assert.ok(source, name);
          assert.equal(bundled, await readFile(source, "utf8"), `${name} changed after generated/headless was built: run npm run build:headless`);
        }
      }
      for (const name of ["concrete-plain.png", "concrete-fine-sand.png"])
        project.assets["textures/" + name] = await readFile(new URL("../crates/engine/src/materialspec/program/examples/textures/" + name, import.meta.url));
      const { metadata, layouts } = await host.compile("shade-layouts", { ...project, includeScenePacket: false });
      assert.ok(layouts, metadata.layoutError);
      const toWorld = id => { const at = id.indexOf("/"); return owners.get(id.slice(0, at)) + id.slice(at); };
      const delivered = packet.manifest.meshes.filter(mesh => mesh.visible).map(mesh => mesh.id).sort();
      const regenerated = Object.values(metadata.items).flatMap(item => item.parts.flatMap(part => part.meshes)).map(toWorld).sort();
      assert.deepEqual(regenerated, delivered, `level ${level}`);
      // Every receiver the bake would shade, balconies, shop rooms and trees included, is a delivered mesh.
      const visible = new Set(delivered);
      assert.deepEqual(Object.keys(metadata.shading).map(toWorld).filter(id => !visible.has(id)), [], `level ${level}`);
      // The bake draws each receiver from its regenerated layout (browser/renderer/atlas.ts), so a
      // layout must hold exactly the triangles the compiled scene delivers for that mesh. This is
      // what tools/kowloon-export/bake-buildings.mjs checks on the balconies after a bake.
      const indexCounts = new Map(packet.manifest.meshes.filter(mesh => mesh.visible).map(mesh => [mesh.id, mesh.indexCount]));
      const charts = atlasLayouts(layouts);
      let receivers = 0, balconies = 0;
      for (const local of Object.keys(metadata.shading)) {
        const chart = charts.get(local);
        if (!chart) continue; // Catalog stock keeps its own texture coordinates and is not charted.
        const id = toWorld(local);
        assert.equal(chart.triangles * 3, indexCounts.get(id), `level ${level}: ${id}`);
        receivers++;
        if (/\/balcony\/balcony\/[^/]+$/.test(id)) balconies++;
      }
      assert.ok(receivers > 50 && balconies > 0, `level ${level}: ${receivers} charted receivers, ${balconies} on balconies`);
      if (budget === undefined) counts.push(packet.manifest.indexCount);
    }
    assert.ok(counts[0] > counts[1] && counts[1] > counts[2], `index counts by level ${counts}`);
  });
