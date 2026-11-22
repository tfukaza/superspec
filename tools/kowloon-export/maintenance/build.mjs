import { Document, NodeIO } from "@gltf-transform/core";
import sharp from "sharp";
import { buildMaintenance } from "../../../crates/engine/src/worldspec/themes/kowloon/authoring/maintenance/build.mjs";
import { maintenanceStations } from "./layout.mjs";

const input = [];
for await (const chunk of process.stdin) input.push(chunk);
const stations = maintenanceStations(JSON.parse(Buffer.concat(input)));
if (stations.length) {
  const doc = await buildMaintenance(stations, { Document, sharp });
  await new NodeIO().write(process.argv[2], doc);
}
console.log(JSON.stringify({ stations }));
