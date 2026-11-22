import { Document, NodeIO } from "@gltf-transform/core";
import { buildWayfinding } from "../../../crates/engine/src/worldspec/themes/kowloon/authoring/wayfinding/build.mjs";

const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
const station = JSON.parse(Buffer.concat(chunks));
await new NodeIO().write(process.argv[2], await buildWayfinding(station, Document));
