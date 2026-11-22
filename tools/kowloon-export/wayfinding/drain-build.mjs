import { Document, NodeIO } from "@gltf-transform/core";
import { buildDrain } from "../../../crates/engine/src/worldspec/themes/kowloon/authoring/wayfinding/drain-build.mjs";

const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
const station = JSON.parse(Buffer.concat(chunks));
await new NodeIO().write(process.argv[2], await buildDrain(station, Document));
