import { mkdir } from "node:fs/promises";
import sharp from "sharp";
import { bakeBattenLens } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/bake-batten-lens.mjs";

const destination = process.argv[2];
if (!destination) throw new Error("Specify the generated output directory.");
await mkdir(destination, { recursive: true });
await bakeBattenLens(sharp, destination);
