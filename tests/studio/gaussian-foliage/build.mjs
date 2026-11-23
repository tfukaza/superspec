import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { ground } from "./site.mjs";
import { treeDemoPresets } from "./tree-demo-presets.mjs";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const out = resolve(root, "generated/treespec-gaussian-foliage");
const studio = process.env.STUDIO_CLI ?? "superspec-studio";
const fixture = name => JSON.parse(readFileSync(resolve(root, "crates/engine/src/treespec/fixtures", name), "utf8"));
const shrub = fixture("shrub.tree.json");
Object.assign(shrub.leaves, { spacing: 0.055, size: [0.087, 0.137], aspect: 0.65, cards: 5 });
shrub.lods[0].leafCards = shrub.leaves.cards;
const grass = fixture("meadow.grass.json");
grass.density = 62;
grass.blade.height = [0.06, 0.21];
grass.blade.width = [0.010, 0.020];
grass.clumping.strength = 0.38;

mkdirSync(out, { recursive: true });
const sitePath = resolve(out, "site.json");
writeFileSync(sitePath, JSON.stringify({ ground }));
const trees = Object.entries(treeDemoPresets).map(([name, preset]) => {
  const file = name + ".tree.json";
  return [file, preset.customize(fixture(file)), []];
});
for (const [name, definition, args] of [
  ["brush.tree.json", shrub, []],
  ["brush.grass.json", grass, ["--site", sitePath]],
  ...trees,
]) {
  const source = resolve(out, name);
  writeFileSync(source, JSON.stringify(definition, null, 2));
  const packet = execFileSync(studio, ["scene", source, ...args], { maxBuffer: 32 * 1024 * 1024 });
  writeFileSync(resolve(out, name.replace(".json", ".lsp3")), packet);
}
console.log("Built TreeSpec shrub, grass, birch and poplar packets in " + out);
