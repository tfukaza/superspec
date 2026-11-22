import { Document, NodeIO } from "@gltf-transform/core";
import { buildVines } from "../../../crates/engine/src/worldspec/themes/kowloon/authoring/vines/build.mjs";
import { growthSites } from "./layout.mjs";

const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
const sites = growthSites(JSON.parse(Buffer.concat(chunks)));
if (sites.length) await new NodeIO().write(process.argv[2], await buildVines(sites, Document));
console.log(JSON.stringify({ sites: sites.length }));
