import { readFile } from "node:fs/promises";
import { addDuctBranch } from "./service-duct-branch.mjs";
import { addDuctRegister } from "./service-duct-register.mjs";
import { ductPanelFaces, addDuctPanels } from "./service-duct-panels.mjs";
import { compile, openBaker } from "../../../../../../../tools/headless/host.mjs";

const density = 1280,
  repeat = 2,
  rows = density * repeat,
  pad = 8;

export async function serviceMetalProgram(doc, mesh, host, { Document, NodeIO, copyToDocument }, entry = "service-aluminum.material.ts") {
  const appearance = new URL("../materials/", import.meta.url);
  const source = await readFile(new URL(entry, appearance), "utf8");
  const id = entry.replace(/\.material\.ts$/, "");
  const receiver = new Document();
  const copy = copyToDocument(receiver, doc, [mesh]);
  receiver.createScene().addChild(receiver.createNode("skin").setMesh(copy.get(mesh)));
  const project = {
    schemaVersion: 2,
    kind: "material",
    entry,
    modules: { [entry]: source },
    parameters: {},
    seed: 87,
    resolution: 64,
    inspection: "final",
    mapping: "world",
    domain: [1, 1],
    edges: [],
    gravity: [0, 0, -1],
    base: { kind: "swatch" },
    target: { scope: "mesh", enabled: false },
    assets: { "receiver.glb": await new NodeIO().writeBinary(receiver) },
    models: [{ id: "duct", kind: "glb", name: "receiver.glb" }],
    materials: [{ id, entry, parameters: {}, meshes: ["duct/node/0/mesh/0/primitive/0"] }],
    includeScenePacket: false,
  };
  const { metadata } = await compile("shade", project);
  const material = metadata.materials[id];
  return {
    material,
    uniforms: await host.uniforms(material, id === "service-aluminum" ? { formedWidth: 0 } : {}, {}),
  };
}

// ItemSpec's route UVs measure distance around the section and along its centerline, in meters.
function section(primitive) {
  const uv = primitive.getAttribute("TEXCOORD_0"),
    stops = new Set();
  for (let i = 0; i < uv.getCount(); i++) stops.add(uv.getElement(i, [])[0]);
  const sorted = [...stops].sort((a, b) => a - b);
  if (sorted.length !== 5 || sorted[0] !== 0) throw Error("Expected four metric duct faces");
  return sorted;
}

function edgePixels(host, bake, readMaterialEdges) {
  const { gl, renderer } = host;
  const fbo = gl.createFramebuffer();
  const operations = bake.live.operations;
  try {
    const { pixels } = readMaterialEdges({
      gl,
      width: bake.width,
      height: bake.height,
      grazing: operations.find((op) => op.id === "grazingFalloff").arguments,
      specular: operations.find((op) => op.id === "scaleSpecular").arguments,
      texture: (name) => bake.live.textures[name],
      bind(texture) {
        gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
        gl.framebufferTexture2D(
          gl.FRAMEBUFFER,
          gl.COLOR_ATTACHMENT0,
          gl.TEXTURE_2D,
          renderer.properties.get(texture).__webglTexture,
          0,
        );
        return [bake.width, bake.height];
      },
    });
    return pixels;
  } finally {
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.deleteFramebuffer(fbo);
    renderer.resetState();
  }
}

async function sheet(doc, host, shader, stops, { sharp, TextureInfo, readMaterialEdges }) {
  const width = stops[1],
    height = stops[2] - width;
  const corners = [
    [0, 0],
    [width, 0],
    [width, height],
    [0, height],
  ];
  const normals = [
    [0, -1],
    [1, 0],
    [0, 1],
    [-1, 0],
  ];
  const strips = [];
  let columns = 0;
  for (let i = 0; i < 4; i++) {
    const span = stops[i + 1] - stops[i],
      count = Math.ceil(span * density);
    strips.push({ start: columns + pad, count, span });
    columns += count + 2 * pad;
  }
  // The folded lock and the broader hanger strap use separate cross-section profiles.
  for (const [span, count] of [
    [0.018, 32],
    [0.055, 64],
  ]) {
    strips.push({ start: columns + pad, span, count });
    columns += count + pad * 2;
  }
  const maps = Object.fromEntries(
    ["baseColor", "normal", "properties", "edge"].map((key) => [key, Buffer.alloc(columns * rows * 4)]),
  );
  for (let i = 0; i < strips.length; i++) {
    const { start, count, span } = strips[i],
      a = i >= 4 ? [0, 0] : corners[i],
      b = i >= 4 ? [span, 0] : corners[(i + 1) % 4],
      n = i >= 4 ? normals[0] : normals[i];
    const features =
      i >= 4
        ? []
        : [i, (i + 1) % 4].flatMap((j) => {
            const p = corners[j],
              prev = normals[(j + 3) % 4],
              next = normals[j];
            return [
              ...p,
              -1,
              0,
              ...p,
              repeat + 1,
              90,
              (prev[0] + next[0]) * Math.SQRT1_2,
              (prev[1] + next[1]) * Math.SQRT1_2,
              0,
              1,
            ];
          });
    const domain = {
      width: count,
      height: rows,
      gutter: 0,
      origin: [...a, 0],
      u: [b[0] - a[0], b[1] - a[1], 0],
      v: [0, 0, repeat],
      uvOrigin: [0, 0],
      uvU: [1, 0],
      uvV: [0, 1],
      normal: [...n, 0],
      mapping: "world",
      extent: [span, repeat],
      objectOrigin: [0, 0, 0],
      boundsMin: [0, 0, 0],
      boundsMax: [width, height, repeat],
      gravity: [0, 0, -1],
      features,
    };
    const uniforms =
      i < 4 ? shader.uniforms : await host.uniforms(shader.material, { formedWidth: span }, {});
    const bake = await host.baker.bake(shader.material, uniforms, domain);
    if (!bake) throw Error("Interrupted duct finish bake");
    const source = { ...host.baker.readBack(bake), edge: edgePixels(host, bake, readMaterialEdges) };
    for (const [key, dst] of Object.entries(maps)) {
      const src = Buffer.from(source[key].buffer, source[key].byteOffset, source[key].byteLength);
      for (let row = 0; row < rows; row++) {
        const offset = (row * columns + start) * 4,
          from = row * count * 4;
        dst.set(src.subarray(from, from + 4 * count), offset);
        dst.fill(src.subarray(from, from + 4), offset - 4 * pad, offset);
        dst.fill(
          src.subarray(from + 4 * count - 4, from + 4 * count),
          offset + 4 * count,
          offset + 4 * (count + pad),
        );
      }
    }
  }
  const texture = async (key) =>
    doc
      .createTexture("Aluminum duct " + key)
      .setMimeType("image/png")
      .setImage(
        await sharp(maps[key], { raw: { width: columns, height: rows, channels: 4 } })
          .removeAlpha()
          .png()
          .toBuffer(),
      );
  const [base, normal, properties, edge] = await Promise.all(Object.keys(maps).map(texture));
  const material = doc
    .createMaterial("Duct rough-polished aluminum")
    .setBaseColorTexture(base)
    .setNormalTexture(normal)
    .setMetallicRoughnessTexture(properties)
    .setMetallicFactor(1)
    .setRoughnessFactor(1)
    .setOcclusionTexture(edge)
    .setOcclusionStrength(0)
    .setExtras({
      worldSpecDuct: true,
      worldSpecSpecularBands: 0.65,
      materialSpecNormalFrame: true,
      materialSpecEdge: { from: 0.3, to: 0.06 },
    });
  for (const info of [
    material.getBaseColorTextureInfo(),
    material.getNormalTextureInfo(),
    material.getMetallicRoughnessTextureInfo(),
    material.getOcclusionTextureInfo(),
  ]) {
    info.setWrapT(TextureInfo.WrapMode.MIRRORED_REPEAT);
    info.setMinFilter(TextureInfo.MinFilter.LINEAR_MIPMAP_LINEAR);
  }
  return { material, strips, columns };
}

// Native rectangular flange UVs use planar meter coordinates on each annular end
// and perimeter/distance on the narrow rolled sides. Fit both to the formed strip.
function bindDuctHardware(doc, primitive, skin, halfHeight, strips, columns, bracket) {
  const trim = strips[bracket ? 5 : 4];
  // Each native bracket consists of its 48-vertex rectangular annulus, followed by
  // two 24-vertex solid prisms for the support arm and its mounting plate.
  const onStrap = (i) => !bracket || i % 96 < 48;
  const original = primitive.getAttribute("TEXCOORD_0");
  if (bracket && original.getCount() % 96 !== 0)
    throw Error("Expected rectangular hanger ring, arm and plate");
  const mapped = original.clone();
  let halfThickness = 0;
  for (let i = 0; i < original.getCount(); i++) {
    if (!onStrap(i)) continue;
    const distance = Math.abs(original.getElement(i, [])[1] - 1);
    if (distance < halfHeight * 0.9) halfThickness = Math.max(halfThickness, distance);
  }
  if (!halfThickness) throw Error("Expected a rectangular flange with a rolled side");
  for (let i = 0; i < original.getCount(); i++) {
    const [u, v] = original.getElement(i, []);
    if (!onStrap(i)) {
      const plate = strips[0];
      const start = Math.floor(i / 4) * 4;
      const left = Math.min(...[0, 1, 2, 3].map((j) => original.getElement(start + j, [])[0]));
      mapped.setElement(i, [
        (plate.start + (0.11 + Math.min(0.76, (u - left) / plate.span)) * plate.count) / columns,
        v / repeat,
      ]);
      continue;
    }
    const h = v - 1;
    const onEnd = Math.abs(h) >= halfHeight * 0.9;
    const profile = onEnd ? (Math.abs(h) > halfHeight + 0.0001 ? 0.54 : 0.03) : 0.5 + h / (2 * halfThickness);
    mapped.setElement(i, [(trim.start + profile * trim.count) / columns, (u + (onEnd ? h : 0)) / repeat]);
  }
  primitive.setAttribute("TEXCOORD_0", mapped).setMaterial(skin);
  primitive.setAttribute(
    "_MS_ASPECT",
    doc
      .createAccessor()
      .setBuffer(mapped.getBuffer())
      .setType("SCALAR")
      .setArray(new Float32Array(mapped.getCount()).fill(rows / columns)),
  );
}

export async function finishServiceDucts(doc, adapters) {
  const nodes = doc
    .getRoot()
    .listNodes()
    .filter((n) => n.getMesh());
  const bodies = nodes.filter(
    (n) =>
      n.getName().endsWith("/body") &&
      n
        .getMesh()
        .listPrimitives()
        .some((p) => p.getMaterial()?.getName() === "builtin:galvanized-duct"),
  );
  if (!bodies.length) return;
  const host = openBaker(),
    sheets = new Map(),
    hardware = new Map();
  try {
    const program = await serviceMetalProgram(doc, bodies[0].getMesh(), host, adapters);
    for (const body of bodies) {
      const primitive = body.getMesh().listPrimitives()[0],
        stops = section(primitive),
        key = stops.join(",");
      if (!sheets.has(key)) sheets.set(key, await sheet(doc, host, program, stops, adapters));
      const { material, strips, columns } = sheets.get(key);
      const original = primitive.getAttribute("TEXCOORD_0"),
        uv = original.clone(),
        index = primitive.getIndices();
      for (let k = 0; k < (index?.getCount() ?? uv.getCount()); k += 3) {
        const ids = [0, 1, 2].map((j) => (index ? index.getScalar(k + j) : k + j));
        const coords = ids.map((j) => original.getElement(j, []));
        const side = stops.indexOf(Math.min(...coords.map((p) => p[0])));
        if (side < 0 || side >= 4) throw Error("Unrecognized duct UV strip");
        const { start, count, span } = strips[side];
        for (let j = 0; j < 3; j++)
          uv.setElement(ids[j], [
            (start + (count * (coords[j][0] - stops[side])) / span) / columns,
            (1 - coords[j][1]) / repeat,
          ]);
      }
      primitive.setAttribute("TEXCOORD_0", uv).setMaterial(material);
      primitive.setAttribute(
        "_MS_ASPECT",
        doc
          .createAccessor()
          .setBuffer(uv.getBuffer())
          .setType("SCALAR")
          .setArray(new Float32Array(uv.getCount()).fill(rows / columns)),
      );
      const faces = ductPanelFaces(primitive, original);
      const opening = addDuctRegister(doc, body, primitive);
      const branch = addDuctBranch(doc, body, primitive, opening, strips, columns, repeat);
      addDuctPanels(
        doc,
        body,
        primitive,
        faces,
        [opening, branch].filter(Boolean),
        strips[4],
        columns,
        repeat,
      );
      const prefix = body.getName().slice(0, -4);
      for (const node of nodes.filter((n) => n !== body && n.getName().startsWith(prefix))) {
        for (const part of node.getMesh().listPrimitives()) {
          if (node.getName().endsWith("/fitting") || node.getName().endsWith("/bracket")) {
            bindDuctHardware(
              doc,
              part,
              material,
              (stops[2] - stops[1]) / 2,
              strips,
              columns,
              node.getName().endsWith("/bracket"),
            );
            continue;
          }
          const original = part.getMaterial();
          if (!hardware.has(original))
            hardware.set(
              original,
              original
                .clone()
                .setName("Duct brushed aluminum hardware")
                .setBaseColorTexture(null)
                .setBaseColorFactor([0.61, 0.62, 0.63, 1])
                .setMetallicFactor(0.88)
                .setRoughnessFactor(0.47)
                .setExtras({ ...original.getExtras(), worldSpecDuct: true, worldSpecSpecularBands: 0.65 }),
            );
          part.setMaterial(hardware.get(original));
        }
      }
      console.log("Aluminum duct PBR", columns, "×", rows, "at", density, "texels/m:", body.getName());
    }
  } finally {
    host.close();
  }
}
