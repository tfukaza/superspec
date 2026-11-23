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
  const all = Array.from({ length: count }, (_, i) => i);
  let material;
  const sprigs = async (selected, growth) => {
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
  for (const i of selected) {
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
  material ??= mat("Kowloon illustrated canopy").setDoubleSided(true).setAlphaMode("MASK").setAlphaCutoff(0.5)
    .setExtras({ foliageClusterNormals: true, foliagePaintCoverage: true })
    .setBaseColorTexture(doc.createTexture("Painted canopy coverage").setMimeType("image/png")
      .setImage(await sharp(sprigAtlasRgba(), { raw: { width: sprigAtlasSize, height: sprigAtlasSize, channels: 4 } }).png().toBuffer()));
  return { positions, colors, primitive: doc
      .createPrimitive()
      .setMaterial(material)
      .setIndices(attribute(new Uint16Array(indices), "SCALAR"))
      .setAttribute("POSITION", attribute(new Float32Array(positions), "VEC3"))
      .setAttribute("NORMAL", attribute(new Float32Array(normals), "VEC3"))
      .setAttribute("TEXCOORD_0", attribute(new Float32Array(uvs), "VEC2"))
      .setAttribute("COLOR_0", attribute(new Float32Array(colors), "VEC3")) };
  };
  const full = await sprigs(all, growth);
  mesh.addPrimitive(full.primitive);
  const scene = doc.createScene();
  doc.getRoot().setDefaultScene(scene);
  const near = doc.createNode(name).setMesh(mesh).setRotation([-Math.SQRT1_2, 0, 0, Math.SQRT1_2]);
  scene.addChild(near);
  const farBark = batches.find(b => b.foliage?.family === "bark" && b.foliage.level === 2);
  if (farBark && farBark.foliage.distance > 0) {
    const barkMaterial = mesh.listPrimitives()[0].getMaterial();
    const farGeometry = batchGeometry(packet, farBark), far = doc.createMesh(name + "/far");
    far.addPrimitive(doc.createPrimitive().setMaterial(barkMaterial)
      .setIndices(attribute(farGeometry.index.array, "SCALAR"))
      .setAttribute("POSITION", attribute(farGeometry.attributes.position.array, "VEC3"))
      .setAttribute("NORMAL", attribute(farGeometry.attributes.normal.array, "VEC3")));
    // The painted canopy has one authored portable representation. Keep it exact;
    // only the existing lower-detail bark geometry changes at this distance.
    far.addPrimitive(mesh.listPrimitives()[1].clone());
    const farName = name + "/lod/2";
    doc.createNode(farName).setMesh(far).setRotation(near.getRotation());
    const pending = [farName], distances = [0, farBark.foliage.distance];
    // Lower presets start from a cheaper rendition (graphicsVisibility.ts
    // continues the chain from the first level a preset delivers).
    const level = (label, parts, minimum) => {
      const lower = doc.createMesh(name + "/" + label);
      for (const part of parts) lower.addPrimitive(part);
      const node = doc.createNode(name + "/lod/" + label).setMesh(lower).setRotation(near.getRotation());
      if (minimum) node.setExtras({ worldSpecMinGraphics: minimum });
      pending.push(node.getName());
    };
    // Limbs and trunk of an existing bark level: twigs inside the crown are covered by its leaves.
    const limbs = (geometry, highest) => {
      const levels = geometry.attributes.msVertex_level, index = geometry.index.array;
      if (!levels) return index;
      return index.filter((_, i) => [0, 1, 2].every(corner => levels.getX(index[i - i % 3 + corner]) <= highest));
    };
    const barkPart = (geometry, indices) => {
      const remap = new Map(), positions = [], normals = [], packed = [];
      for (const vertex of indices) {
        if (!remap.has(vertex)) {
          remap.set(vertex, remap.size);
          for (let k = 0; k < 3; k++) {
            positions.push(geometry.attributes.position.array[vertex * 3 + k]);
            normals.push(geometry.attributes.normal.array[vertex * 3 + k]);
          }
        }
        packed.push(remap.get(vertex));
      }
      return doc.createPrimitive().setMaterial(barkMaterial)
        .setIndices(attribute(new Uint16Array(packed), "SCALAR"))
        .setAttribute("POSITION", attribute(new Float32Array(positions), "VEC3"))
        .setAttribute("NORMAL", attribute(new Float32Array(normals), "VEC3"));
    };
    // The opaque crown mass is the canopy's shadow hull, fitted inside the leaves it stands behind.
    const hull = batches.find(b => b.foliage?.family === "shadow" && b.foliage.mass);
    const hullGeometry = hull && batchGeometry(packet, hull);
    const massed = !!hullGeometry?.index?.count;
    const thinned = sprigSubset(g.instanceCount, count, STREET_TREE_LEVELS.low.sprigs);
    // Without a crown mass this is also the lowest preset's rendition.
    level("low", [barkPart(barkGeometry, limbs(barkGeometry, 1)), (await sprigs(thinned.indices, thinned.growth)).primitive],
      massed ? "low" : undefined);
    if (massed) {
      const crown = new THREE.Box3().setFromArray(full.positions);
      const mean = new THREE.Color(0, 0, 0), sample = new THREE.Color();
      for (let i = 0; i < full.colors.length; i += 3) mean.add(sample.setRGB(full.colors[i], full.colors[i + 1], full.colors[i + 2]));
      mean.multiplyScalar(3 / full.colors.length);
      const trunk = new THREE.Color("#776957");
      // One opaque primitive holds the crown mass and its trunk, coloured per vertex.
      const mass = (fit, shade, wood, woodIndices) => {
        const from = hullGeometry.boundingBox, size = from.getSize(new THREE.Vector3()), middle = from.getCenter(new THREE.Vector3());
        const scale = crown.getSize(new THREE.Vector3()).multiplyScalar(fit).divide(size), to = crown.getCenter(new THREE.Vector3());
        const positions = [], normals = [], colors = [], indices = [];
        const p = new THREE.Vector3(), n = new THREE.Vector3();
        for (let v = 0; v < hullGeometry.attributes.position.count; v++) {
          p.fromBufferAttribute(hullGeometry.attributes.position, v).sub(middle).multiply(scale).add(to);
          n.fromBufferAttribute(hullGeometry.attributes.normal, v).divide(scale).normalize();
          positions.push(...p.toArray());
          normals.push(...n.toArray());
          // Lighter toward the sky, as the leaves it replaces are.
          const lift = shade * (0.85 + 0.25 * n.z);
          colors.push(mean.r * lift, mean.g * lift, mean.b * lift);
        }
        indices.push(...hullGeometry.index.array);
        const remap = new Map();
        for (const vertex of woodIndices) {
          if (!remap.has(vertex)) {
            remap.set(vertex, positions.length / 3);
            positions.push(...p.fromBufferAttribute(wood.attributes.position, vertex).toArray());
            normals.push(...n.fromBufferAttribute(wood.attributes.normal, vertex).toArray());
            colors.push(trunk.r, trunk.g, trunk.b);
          }
          indices.push(remap.get(vertex));
        }
        return doc.createPrimitive().setMaterial(massMaterial)
          .setIndices(attribute(new Uint16Array(indices), "SCALAR"))
          .setAttribute("POSITION", attribute(new Float32Array(positions), "VEC3"))
          .setAttribute("NORMAL", attribute(new Float32Array(normals), "VEC3"))
          .setAttribute("COLOR_0", attribute(new Float32Array(colors), "VEC3"));
      };
      const massMaterial = mat("Kowloon canopy mass");
      // The leaves hold the crown's whole leaf area and colour; the dark mass
      // inside closes the gaps between them and hides the far side's leaves.
      const { veryLow, far: distant } = STREET_TREE_LEVELS;
      const shell = sprigSubset(g.instanceCount, count, veryLow.sprigs);
      level("very-low", [mass(veryLow.fit, veryLow.shade, farGeometry, farGeometry.index.array),
        (await sprigs(shell.indices, shell.growth)).primitive]);
      const rim = sprigSubset(g.instanceCount, count, distant.sprigs, distant.scale);
      level("distant", [mass(distant.fit, distant.shade, farGeometry, limbs(farGeometry, 0)),
        (await sprigs(rim.indices, rim.growth)).primitive]);
    }
    // Medium and above keep the full canopy at every distance a city reaches.
    const beyond = STREET_TREE_LEVELS.unreached.slice(0, pending.length - 1);
    near.setExtras({ worldSpecMinGraphics: "medium", worldSpecLod: { version: 1, distances: [...distances, ...beyond],
      presetDistances: massed ? { low: STREET_TREE_LEVELS.low.distances, "very-low": STREET_TREE_LEVELS.veryLow.distances }
        : { low: [0], "very-low": [0] } },
      worldSpecPendingTreeLod: pending });
    doc.getRoot().listNodes().find(node => node.getName() === farName).setExtras({ worldSpecMinGraphics: "medium" });
  }
  console.log("Street tree:", count, "painted sprigs in one canopy primitive,", bark.indexCount / 3, "bark triangles");
  return doc;
}

/** Renditions below the published one. Distances are reference metres (a
 * 1080-pixel, 70-degree view); lower presets reach them sooner as their
 * drawing buffer and geometry bias shrink. `scale` is against a full-canopy
 * leaf; without it the kept leaves grow to hold the canopy's whole leaf area. */
export const STREET_TREE_LEVELS = {
  low: { sprigs: 2200, distances: [0, 43, 71] },
  veryLow: { sprigs: 900, fit: 0.74, shade: 0.4, distances: [0, 73] },
  far: { sprigs: 400, scale: 3, fit: 0.74, shade: 0.4 },
  unreached: [1000, 2000, 4000],
};

/** An even thinning of the `drawn` shuffled leaves of a crown that grew
 * `total`: the first `count` of them. */
export function sprigSubset(total, drawn, count, scale) {
  const indices = Array.from({ length: Math.min(count, drawn) }, (_, i) => i);
  return { indices, growth: scale ? scale * Math.sqrt(total / drawn) : Math.sqrt(total / indices.length) };
}

/** Resolve the owner's private references only after glTF-Transform assigns node indices. */
export function finishStreetTreeLods(bytes) {
  const input = Buffer.from(bytes), end = 20 + input.readUInt32LE(12);
  const json = JSON.parse(input.toString("utf8", 20, end)), tail = input.subarray(end);
  let used = false;
  for (const node of json.nodes ?? []) {
    const pending = node.extras?.worldSpecPendingTreeLod;
    if (!pending) continue;
    const ids = [pending].flat().map(name => {
      const found = json.nodes.flatMap((candidate, index) => candidate.name === name ? [index] : []);
      if (found.length !== 1) throw Error("Missing or ambiguous authored street-tree LOD node: " + name);
      return found[0];
    });
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
