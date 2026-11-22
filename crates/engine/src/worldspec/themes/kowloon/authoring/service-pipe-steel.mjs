import { serviceMetalProgram } from "./service-duct-finish.mjs";
import { openBaker } from "../../../../../../../tools/headless/host.mjs";

// Route UVs already measure developed length in metres. Unroll the cylinder for
// the MaterialSpec receiver; the original smooth mesh still supplies its form.
const density = 1280;

async function bakeStrip(doc, host, sharp, shader, span, length, collar, seats = [], normalOnly = false) {
  const route = seats.length > 0;
  const width = Math.max(16, Math.ceil(span * (route ? 512 : density)));
  const height = Math.min(4096, Math.max(16, Math.ceil(length * density)));
  const edges = collar
    ? [
        [0, -1],
        [length, 1],
      ].flatMap(([z, sign]) => [
        -0.1,
        0,
        z,
        0,
        span + 0.1,
        0,
        z,
        90,
        0,
        -Math.SQRT1_2,
        sign * Math.SQRT1_2,
        1,
      ])
    : seats.flatMap((z) => [-0.1, 0, z, 4, span + 0.1, 0, z, 90, 0, 0, 0, 0]);
  const domain = {
    width,
    height,
    gutter: 0,
    origin: [0, 0, 0],
    u: [span, 0, 0],
    v: [0, 0, length],
    uvOrigin: [0, 0],
    uvU: [1, 0],
    uvV: [0, 1],
    normal: [0, -1, 0],
    mapping: "world",
    extent: [span, length],
    objectOrigin: [0, 0, 0],
    boundsMin: [0, 0, 0],
    boundsMax: [span, 0, length],
    gravity: [0, 0, -1],
    features: edges,
  };
  const roles = normalOnly
    ? ["normal"]
    : route
      ? ["baseColor", "properties"]
      : ["baseColor", "normal", "properties"];
  const bake = await host.baker.bake(shader.material, shader.uniforms, domain);
  if (!bake) throw Error("Interrupted oxidized steel pipe bake");
  const channels = host.baker.readBack(bake);
  const textures = await Promise.all(
    roles.map(async (role) => {
      const pixels = Buffer.from(channels[role]);
      // The native readback starts at the last longitudinal station. glTF PNG
      // row zero is V=0, the start of the pipe. Restore that order before export.
      const rowSize = width * 4;
      for (let row = 0; row < height >> 1; row++) {
        const top = pixels.subarray(row * rowSize, (row + 1) * rowSize);
        const bottom = pixels.subarray((height - 1 - row) * rowSize, (height - row) * rowSize);
        const scratch = Buffer.from(top);
        bottom.copy(top);
        scratch.copy(bottom);
      }
      // Match the last few millimetres to the first column around the UV seam.
      const seam = Math.min(10, Math.floor(width / 8));
      for (let y = 0; y < height; y++)
        for (let dx = 0; dx < seam; dx++) {
          const from = y * width * 4,
            index = from + (width - seam + dx) * 4,
            t = (dx + 1) / seam;
          for (let k = 0; k < 3; k++)
            pixels[index + k] = Math.round(pixels[index + k] * (1 - t) + pixels[from + k] * t);
        }
      const bytes = await sharp(pixels, { raw: { width, height, channels: 4 } })
        .removeAlpha()
        .png()
        .toBuffer();
      return doc
        .createTexture("Service pipe steel " + role)
        .setMimeType("image/png")
        .setImage(bytes);
    }),
  );
  return {
    textures: Object.fromEntries(roles.map((role, i) => [role, textures[i]])),
    width,
    height,
    span,
    length,
  };
}

// The sleeve has annular end faces (planar UVs) and rounded sides (developed
// cylindrical UVs). Only the latter carry the two constant longitudinal stops.
function sleeveSides(prim) {
  const normal = prim.getAttribute("NORMAL"),
    indices = prim.getIndices(),
    sides = new Set();
  for (let i = 0; i < (indices?.getCount() ?? normal.getCount()); i += 3) {
    const ids = [0, 1, 2].map((k) => (indices ? indices.getScalar(i + k) : i + k));
    const n = normal.getElement(ids[0], []);
    if (
      ids
        .slice(1)
        .some((id) => normal.getElement(id, []).some((v, k) => Math.abs(n[k] - v) > 0.0001))
    ) {
      for (const id of ids) sides.add(id);
    }
  }
  if (!sides.size) throw Error("Expected round coupling side walls");
  return sides;
}

function applyStrip(doc, TextureInfo, prim, chart, fitting, sides, normalChart = chart) {
  const uv = prim.getAttribute("TEXCOORD_0"),
    coords = uv.clone();
  for (let i = 0; i < coords.getCount(); i++) {
    const [u, v] = uv.getElement(i, []);
    // Place annular end lands on the polished edge of the coupling strip.
    const t = sides
      ? sides.has(i)
        ? (1 - v) / chart.length + 0.5
        : 0.5 / chart.height
      : (1 - v) / chart.length;
    coords.setElement(i, [u / chart.span, t]);
  }
  if (normalChart !== chart) {
    const normalUv = uv.clone();
    for (let i = 0; i < normalUv.getCount(); i++) {
      const [u, v] = uv.getElement(i, []);
      normalUv.setElement(i, [u / normalChart.span, (1 - v) / normalChart.length]);
    }
    prim.setAttribute("TEXCOORD_2", normalUv);
  }
  prim.setAttribute("TEXCOORD_0", coords);
  prim.setAttribute(
    "_MS_ASPECT",
    doc
      .createAccessor()
      .setBuffer(coords.getBuffer())
      .setType("SCALAR")
      .setArray(new Float32Array(coords.getCount()).fill(normalChart.height / normalChart.width)),
  );
  const material = prim.getMaterial();
  const { baseColor, properties } = chart.textures;
  material
    .setBaseColorTexture(baseColor)
    .setNormalTexture(normalChart.textures.normal)
    .setMetallicRoughnessTexture(properties)
    .setBaseColorFactor(fitting ? [0.79, 0.81, 0.82, 1] : [1, 1, 1, 1])
    .setNormalScale(1)
    .setMetallicFactor(1)
    .setRoughnessFactor(fitting ? 0.92 : 1)
    .setExtras({ ...material.getExtras(), worldSpecPipe: true, materialSpecNormalFrame: true });
  material.getNormalTextureInfo().setTexCoord(normalChart === chart ? 0 : 2);
  for (const info of [
    material.getBaseColorTextureInfo(),
    material.getNormalTextureInfo(),
    material.getMetallicRoughnessTextureInfo(),
  ]) {
    info.setWrapS(TextureInfo.WrapMode.REPEAT);
    info.setWrapT(
      sides ? TextureInfo.WrapMode.CLAMP_TO_EDGE : TextureInfo.WrapMode.MIRRORED_REPEAT,
    );
    info.setMinFilter(TextureInfo.MinFilter.LINEAR_MIPMAP_LINEAR);
  }
}

export async function finishPipeSteel(doc, bodies, nodes, seatsByBody, adapters) {
  const { sharp, TextureInfo } = adapters;
  const host = openBaker(),
    charts = new Map();
  const source = bodies[0].getMesh().listPrimitives()[0];
  const receiver = doc.createPrimitive().setIndices(source.getIndices());
  for (const key of ["POSITION", "NORMAL", "TEXCOORD_0"])
    receiver.setAttribute(key, source.getAttribute(key));
  const mesh = doc.createMesh("Steel recipe receiver").addPrimitive(receiver);
  try {
    const shader = await serviceMetalProgram(doc, mesh, host, adapters, "service-pipe-steel.material.ts");
    const sleeveShader = {
      material: shader.material,
      uniforms: await host.uniforms(shader.material, { sleeve: 1 }, {}),
    };
    const chartFor = async (
      span,
      length,
      collar,
      galvanized = collar,
      seats = [],
      normalOnly = false,
    ) => {
      const key = [span, length, collar, galvanized, normalOnly, ...seats].join("/");
      if (!charts.has(key))
        charts.set(
          key,
          await bakeStrip(
            doc,
            host,
            sharp,
            galvanized ? sleeveShader : shader,
            span,
            length,
            collar,
            seats,
            normalOnly,
          ),
        );
      return charts.get(key);
    };
    for (const body of bodies) {
      const barrel = body.getMesh().listPrimitives()[0];
      const circumference = barrel.getAttribute("TEXCOORD_0").getMax([])[0];
      const seats = seatsByBody.get(body) ?? [];
      const length = 1 - barrel.getAttribute("TEXCOORD_0").getMin([])[1];
      const stock = await chartFor(circumference, 2, false, false, [], seats.length > 0);
      const weathered = seats.length
        ? await chartFor(circumference, length, false, false, seats)
        : stock;
      const prefix = body.getName().slice(0, -4);
      for (const node of nodes.filter((n) => n.getName().startsWith(prefix))) {
        const role = node.getName().slice(prefix.length);
        for (const part of node.getMesh().listPrimitives()) {
          if (role === "fitting") {
            const sides = sleeveSides(part),
              uv = part.getAttribute("TEXCOORD_0");
            let min = Infinity,
              max = -Infinity,
              perimeter = 0;
            for (const id of sides) {
              const [u, v] = uv.getElement(id, []);
              min = Math.min(min, v);
              max = Math.max(max, v);
              perimeter = Math.max(perimeter, u);
            }
            const collar = await chartFor(perimeter, max - min, true);
            part.getMaterial().setName("Service pipe steel collar");
            applyStrip(doc, TextureInfo, part, collar, true, sides);
          } else {
            const finish =
              role === "body" ? weathered : await chartFor(circumference, 0.3, false, true);
            applyStrip(
              doc,
              TextureInfo,
              part,
              finish,
              role !== "body",
              undefined,
              role === "body" ? stock : finish,
            );
          }
        }
      }
    }
  } finally {
    mesh.dispose();
    receiver.dispose();
    host.close();
  }
}
