/**
 * Writes the procedural building's scene project (the one tests/item-facade-browser.mjs and
 * tests/item-building-perf.mjs open in the Studio) to a file, for native timings
 * (crates/engine/tests/item_building_bench.rs).
 *
 * Usage: node tools/perf/item-building-project.mjs <output.studio.json>
 * RESOLUTION (1024), TEXELS (160), LANE=1 (the lane's plants, written by treespec_batches) and
 * BUILDING_PARAMETERS (default the reference shop house's plan) are as in those scripts.
 */
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { buildProject } from "../../tests/studio/washroom-items/build.mjs";

const output = resolve(process.argv[2] ?? "generated/verification/perf/building.studio.json");
const lane = process.env.LANE === "1" ? JSON.parse(await readFile(resolve("generated/verification/treespec/lane-plants.json"), "utf8")) : null;
const { project } = buildProject(Number(process.env.RESOLUTION ?? 1024), Number(process.env.TEXELS ?? 160), { facade: "building", lane, room: false, grass: false });
const building = project.models.find(model => model.id === "building");
const given = process.env.BUILDING_PARAMETERS ?? "tests/studio/washroom-items/reference-shop-house.json";
const overrides = given.endsWith(".json") ? await readFile(resolve(given), "utf8") : given;
Object.assign(building.parameters, Object.fromEntries(Object.entries(JSON.parse(overrides)).filter(([key]) => !key.startsWith("/"))));
if (project.resolution > 512) project.profile = "production";
await mkdir(dirname(output), { recursive: true });
const text = JSON.stringify(project);
await writeFile(output, text);
console.log(`Wrote ${output}: ${(text.length / 1048576).toFixed(1)} MiB (${Object.entries(project).filter(([, value]) => JSON.stringify(value).length > 65536).map(([key, value]) => `${key} ${(JSON.stringify(value).length / 1048576).toFixed(1)}`).join(", ")} MiB)`);
