import sharp from "sharp";
import { bakeCabinetMarkings } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/bake-cabinet-markings.mjs";

const destination = process.argv[2];
if (!destination) throw new Error("Specify the generated output file.");
await bakeCabinetMarkings(sharp, destination);
