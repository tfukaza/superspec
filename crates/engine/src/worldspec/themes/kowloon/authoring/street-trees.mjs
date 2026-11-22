/** Export the WorldSpec viewer's illustrated leaf whorls and bark to standard
 * game materials. The native batch packet is the same input the viewer draws. */
import { readFile } from "node:fs/promises";
import * as THREE from "three";
import { decodePacket } from "../../../../../../../browser/model/packet.ts";
import { animeTree } from "../../../../../../../browser/renderer/anime-tree.ts";
import { batchGeometry } from "../../../../../../../browser/renderer/batch.ts";
import { ANIME_RAMP } from "../../../../../../../browser/renderer/anime-tree-crown.ts";
import { canopyPaintStrength, sprigAtlasRgba, sprigAtlasSize, sprigGrid, sprigLength, sprigWidth } from "../../../../../../../browser/renderer/canopy-paint.ts";

export async function bakeStreetTree(path, name, { Document, sharp }) {
  const bytes = await readFile(path);
  const packet = decodePacket(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  const batches = packet.manifest.batches;
  const mass = batches.find((b) => b.foliage?.mass)?.foliage.mass;
  const bark = batches.find((b) => b.foliage?.family === "bark" && b.foliage.level === 1);
  if (!bark || mass?.look !== "leaves")
    throw Error("Expected illustrated WorldSpec street tree with bark and crown");
  const doc = new Document(),
    buf = doc.createBuffer(),
    mesh = doc.createMesh(name);
  const attribute = (a, type) => doc.createAccessor().setBuffer(buf).setType(type).setArray(a);
  const mat = (id) => doc.createMaterial(id).setMetallicFactor(0).setRoughnessFactor(1);
  const barkGeometry = batchGeometry(packet, bark);
  mesh.addPrimitive(
    doc
      .createPrimitive()
      .setMaterial(
        mat("Kowloon street bark").setBaseColorFactor([...new THREE.Color("#776957").toArray(), 1]),
      )
      .setIndices(attribute(barkGeometry.index.array, "SCALAR"))
      .setAttribute("POSITION", attribute(barkGeometry.attributes.position.array, "VEC3"))
      .setAttribute("NORMAL", attribute(barkGeometry.attributes.normal.array, "VEC3")),
  );

  const [canopy] = animeTree(
    mass,
    new THREE.Matrix4(),
    "leaves",
    { value: new THREE.Vector3(0, 0, 1) },
    { value: 0 },
  );
  const g = canopy.geometry,
    a = g.attributes;
  // The canonical generator shuffles whole leaves for distance thinning. Use a
  // fixed subset and compensate leaf area as the original distance shader does.
  const count = Math.min(5500, g.instanceCount),
    growth = Math.sqrt(g.instanceCount / count);
  const positions = [],
    normals = [],
    uvs = [],
    colors = [],
    indices = [];
  const foot = new THREE.Vector3(),
    along = new THREE.Vector3(),
    normal = new THREE.Vector3(),
    across = new THREE.Vector3(),
    point = new THREE.Vector3();
  // These are the unshadowed olive pigments at the light end of the reference
  // palette. Sun direction and canopy normals are evaluated in the game.
  const olive = new THREE.Color(ANIME_RAMP[5]), lightOlive = new THREE.Color(ANIME_RAMP[6]);
  const tint = new THREE.Color();
  const up = new THREE.Vector3(0, 0, 1),
    east = new THREE.Vector3(1, 0, 0);
  for (let i = 0; i < count; i++) {
    foot.fromBufferAttribute(a.iOrigin, i);
    along.fromBufferAttribute(a.iAlong, i);
    normal.fromBufferAttribute(a.iShade, i);
    // Fixed leaf planes can be instanced and render identically in the depth
    // and colour passes. Different azimuths keep the crown full from below.
    across.crossVectors(along, Math.abs(along.z) < 0.95 ? up : east).normalize();
    across.applyAxisAngle(along, (a.iTone.getY(i) - 0.5) * Math.PI);
    const length = a.iOrigin.getW(i) * growth,
      width = a.iAlong.getW(i) * growth;
    tint.copy(olive).lerp(lightOlive, 0.12 + 0.52 * a.iTone.getX(i) + 0.35 * a.iTone.getZ(i));
    // Some leaves are buried inside a whorl, a local occlusion independent of the sun.
    tint.multiplyScalar(1 - 0.18 * a.iShade.getW(i));
    const base = positions.length / 3;
    const brush = Math.round(a.iRim.getZ(i) / canopyPaintStrength * (sprigGrid - 1));
    const sprig = Math.min(sprigGrid - 1, Math.floor(a.iRim.getY(i) * sprigGrid));
    for (let v = 0; v < a.position.count; v++) {
      const u = a.position.getX(v),
        t = a.position.getY(v);
      point
        .copy(foot)
        .addScaledVector(along, ((t - 0.5) * sprigLength + 0.5) * length)
        .addScaledVector(across, u * sprigWidth * width);
      point.z -= 0.1 * t * t * length;
      positions.push(...point.toArray());
      normals.push(...normal.toArray());
      uvs.push((brush + u + 0.5) / sprigGrid, (sprig + t) / sprigGrid);
      // The viewer's object-space blur already filtered this leaf's pigment and
      // light-facing normal. Fade its painted fold into the same neighbourhood.
      const highlight = 1 + (t - 0.5) * 0.12 * (1 - a.iRim.getZ(i));
      colors.push(tint.r * highlight, tint.g * highlight, tint.b * highlight);
    }
    for (const index of g.index.array) indices.push(base + index);
  }
  const coverage = doc.createTexture("Painted canopy coverage").setMimeType("image/png")
    .setImage(await sharp(sprigAtlasRgba(), { raw: { width: sprigAtlasSize, height: sprigAtlasSize, channels: 4 } }).png().toBuffer());
  mesh.addPrimitive(
    doc
      .createPrimitive()
      .setMaterial(mat("Kowloon illustrated canopy").setDoubleSided(true).setBaseColorTexture(coverage).setAlphaMode("MASK")
        .setAlphaCutoff(0.5).setExtras({ foliageClusterNormals: true, foliagePaintCoverage: true }))
      .setIndices(attribute(new Uint16Array(indices), "SCALAR"))
      .setAttribute("POSITION", attribute(new Float32Array(positions), "VEC3"))
      .setAttribute("NORMAL", attribute(new Float32Array(normals), "VEC3"))
      .setAttribute("TEXCOORD_0", attribute(new Float32Array(uvs), "VEC2"))
      .setAttribute("COLOR_0", attribute(new Float32Array(colors), "VEC3")),
  );
  const scene = doc.createScene();
  doc.getRoot().setDefaultScene(scene);
  const near = doc.createNode(name).setMesh(mesh).setRotation([-Math.SQRT1_2, 0, 0, Math.SQRT1_2]);
  scene.addChild(near);
  const farBark = batches.find(b => b.foliage?.family === "bark" && b.foliage.level === 2);
  if (farBark && farBark.foliage.distance > 0) {
    const farGeometry = batchGeometry(packet, farBark), far = doc.createMesh(name + "/far");
    far.addPrimitive(doc.createPrimitive().setMaterial(mesh.listPrimitives()[0].getMaterial())
      .setIndices(attribute(farGeometry.index.array, "SCALAR"))
      .setAttribute("POSITION", attribute(farGeometry.attributes.position.array, "VEC3"))
      .setAttribute("NORMAL", attribute(farGeometry.attributes.normal.array, "VEC3")));
    // The painted canopy has one authored portable representation. Keep it exact;
    // only the existing lower-detail bark geometry changes at this distance.
    far.addPrimitive(mesh.listPrimitives()[1].clone());
    const farName = name + "/lod/2";
    doc.createNode(farName).setMesh(far).setRotation(near.getRotation());
    near.setExtras({ worldSpecLod: { version: 1, distances: [0, farBark.foliage.distance] },
      worldSpecPendingTreeLod: farName });
  }
  console.log("Street tree:", count, "painted sprigs in one canopy primitive,", bark.indexCount / 3, "bark triangles");
  return doc;
}

/** Resolve the owner's private references only after glTF-Transform assigns node indices. */
export function finishStreetTreeLods(bytes) {
  const input = Buffer.from(bytes), end = 20 + input.readUInt32LE(12);
  const json = JSON.parse(input.toString("utf8", 20, end)), tail = input.subarray(end);
  let used = false;
  for (const node of json.nodes ?? []) {
    const name = node.extras?.worldSpecPendingTreeLod;
    if (!name) continue;
    const ids = json.nodes.flatMap((candidate, index) => candidate.name === name ? [index] : []);
    if (ids.length !== 1) throw Error("Missing or ambiguous authored street-tree LOD node: " + name);
    delete node.extras.worldSpecPendingTreeLod;
    node.extensions = { ...node.extensions, MSFT_lod: { ids } };
    used = true;
  }
  if (!used) return bytes;
  json.asset.generator = "SuperSpec Kowloon painted tree";
  json.extensionsUsed = [...new Set([...(json.extensionsUsed ?? []), "MSFT_lod"])];
  const text = Buffer.from(JSON.stringify(json)), length = Math.ceil(text.length / 4) * 4;
  const header = Buffer.alloc(20 + length, 0x20);
  input.copy(header, 0, 0, 20);
  header.writeUInt32LE(header.length + tail.length, 8);
  header.writeUInt32LE(length, 12);
  text.copy(header, 20);
  return Buffer.concat([header, tail]);
}
