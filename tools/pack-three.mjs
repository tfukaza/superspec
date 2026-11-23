import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const destination = process.argv[2] ? resolve(process.argv[2]) : resolve(root, "generated/render-engine");
mkdirSync(destination, { recursive: true });
const npm = process.env.npm_execpath;
if (!npm) throw new Error("Run this script with npm run pack:three.");
const result = spawnSync(process.execPath, [npm, "pack", "./adapters/three", "--pack-destination", destination], {
  cwd: root,
  stdio: "inherit",
});
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
