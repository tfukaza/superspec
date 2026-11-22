import { readFile } from "node:fs/promises";
import { BoxGeometry, ExtrudeGeometry, Path, Shape } from "three";

export async function buildDrain({
  origin,
  count,
  width,
  floor,
  module: length,
}, Document) {
  const doc = new Document(),
    buf = doc.createBuffer();
  const root = doc
    .createNode("Passage drain on " + floor)
    .setRotation([-Math.SQRT1_2, 0, 0, Math.SQRT1_2]);
  doc.createScene("Lower arcade drain").addChild(root);
  const maps = await Promise.all(
    ["base", "normal", "properties"].map(async (role) =>
      doc
        .createTexture("Cast iron " + role)
        .setMimeType("image/png")
        .setImage(
          await readFile(new URL("drain-" + role + ".png", import.meta.url)),
        ),
    ),
  );
  const iron = doc
    .createMaterial("Slot drain aged cast iron")
    .setBaseColorTexture(maps[0])
    .setNormalTexture(maps[1])
    .setNormalScale(0.75)
    .setMetallicRoughnessTexture(maps[2])
    .setEmissiveTexture(maps[0])
    .setEmissiveFactor([0.145, 0.14, 0.13])
    .setMetallicFactor(1)
    .setRoughnessFactor(1);
  const worn = iron
    .clone()
    .setName("Slot drain worn casting lips")
    .setRoughnessFactor(0.85)
    .setEmissiveFactor([0.265, 0.26, 0.248]);
  const mortar = doc
    .createMaterial("Slot drain bedding mortar")
    .setBaseColorFactor([0.071, 0.064, 0.052, 1])
    .setMetallicFactor(0)
    .setRoughnessFactor(0.98);
  const throat = doc
    .createMaterial("Slot drain trough shadow")
    .setBaseColorFactor([0.018, 0.016, 0.014, 1])
    .setMetallicFactor(0.04)
    .setRoughnessFactor(0.99);
  const frame = doc
    .createMaterial("Slot drain dark iron frame")
    .setBaseColorFactor([0.12, 0.115, 0.103, 1])
    .setMetallicFactor(0.41)
    .setRoughnessFactor(0.65)
    .setEmissiveFactor([0.012, 0.012, 0.011]);
  const batches = new Map();
  function append(g, x, y, z, material, bevel, plateIndex = 0) {
    const p = g.getAttribute("position"),
      n = g.getAttribute("normal");
    for (let k = 0; k < (g.index?.count ?? p.count); k += 3) {
      const ids = [k, k + 1, k + 2].map((i) => (g.index ? g.index.getX(i) : i));
      const nz = ids.reduce((a, i) => a + n.getZ(i), 0) / 3;
      if (nz < -0.85) continue;
      const mat = bevel && nz > 0.12 && nz < 0.92 ? bevel : material;
      if (!batches.has(mat)) batches.set(mat, { p: [], n: [], uv: [] });
      const b = batches.get(mat);
      for (const i of ids) {
        const px = x + p.getX(i),
          py = y + p.getY(i);
        b.p.push(px + origin[0], py + origin[1], z + p.getZ(i) + origin[2]);
        b.n.push(n.getX(i), n.getY(i), n.getZ(i));
        // A half-turn and an offset change the wearing patterns between castings
        // while keeping the relief, oxide and roughness maps in register.
        const orientation = plateIndex % 2 === 0 ? 1 : -1;
        b.uv.push(
          (orientation * px) / 0.4 + plateIndex * 0.371,
          (orientation * py) / 0.4 + plateIndex * 0.273,
        );
      }
    }
    g.dispose();
  }
  function bar(x0, x1, y0, y1, bottom, top, m) {
    append(
      new BoxGeometry(x1 - x0, y1 - y0, top - bottom),
      (x0 + x1) / 2,
      (y0 + y1) / 2,
      (top + bottom) / 2,
      m,
    );
  }
  const total = length * count;
  const hw = width / 2;
  // A low, bevelled grout apron bites into the original floor. Its inner part
  // closes the visual channel under every slot without cutting the walkable tile.
  function extrude(shape, depth, edge) {
    return new ExtrudeGeometry(shape, {
      depth,
      steps: 1,
      bevelEnabled: true,
      bevelSegments: 1,
      bevelThickness: edge,
      bevelSize: edge,
      curveSegments: 5,
    });
  }
  function rim(s, w, h, r) {
    const a = w / 2,
      b = h / 2;
    s.moveTo(-a + r, -b);
    s.lineTo(a - r, -b);
    s.quadraticCurveTo(a, -b, a, -b + r);
    s.lineTo(a, b - r);
    s.quadraticCurveTo(a, b, a - r, b);
    s.lineTo(-a + r, b);
    s.quadraticCurveTo(-a, b, -a, b - r);
    s.lineTo(-a, -b + r);
    s.quadraticCurveTo(-a, -b, -a + r, -b);
    return s;
  }
  append(
    extrude(
      rim(new Shape(), total + 0.022, width - 0.003, 0.004),
      0.0019,
      0.0014,
    ),
    total / 2,
    0,
    0.0016,
    mortar,
  );
  bar(0, total, -0.102, 0.102, 0.0039, 0.00535, throat);
  for (const s of [-1, 1]) {
    const y = s * 0.101;
    bar(-0.003, total + 0.003, y - 0.003, y + 0.003, 0.0034, 0.009, frame);
  }
  for (const x of [0, total])
    bar(x - 0.003, x + 0.003, -0.098, 0.098, 0.0034, 0.009, frame);

  for (let j = 0; j < count; j++) {
    const plate = rim(new Shape(), length - 0.01, 0.193, 0.004);
    for (let row = 0; row < 4; row++) {
      const cy = (row - 1.5) * 0.043;
      const start = -length / 2 + (row === 0 ? 0.089 : 0.046);
      const end = length / 2 - (row === 3 ? 0.089 : 0.046);
      const radius = 0.0105;
      const hole = new Path();
      hole.moveTo(start, cy + radius);
      hole.lineTo(end, cy + radius);
      hole.absarc(end, cy, radius, Math.PI / 2, -Math.PI / 2, true);
      hole.lineTo(start, cy - radius);
      hole.absarc(start, cy, radius, -Math.PI / 2, Math.PI / 2, true);
      plate.holes.push(hole);
    }
    for (const side of [-1, 1]) {
      const hole = new Path();
      hole.absarc(
        side * (length / 2 - 0.039),
        side * 0.065,
        0.009,
        0,
        -2 * Math.PI,
        true,
      );
      plate.holes.push(hole);
    }
    append(
      extrude(plate, 0.0018, 0.0011),
      (j + 0.5) * length,
      0,
      0.0071,
      iron,
      worn,
      j,
    );
  }

  const mesh = doc.createMesh("Three lift-out slotted castings");
  const attr = (type, data) =>
    doc
      .createAccessor()
      .setBuffer(buf)
      .setType(type)
      .setArray(new Float32Array(data));
  for (const [mat, b] of batches) {
    const prim = doc
      .createPrimitive()
      .setMaterial(mat)
      .setAttribute("POSITION", attr("VEC3", b.p))
      .setAttribute("NORMAL", attr("VEC3", b.n));
    if (mat.getBaseColorTexture())
      prim.setAttribute("TEXCOORD_0", attr("VEC2", b.uv));
    mesh.addPrimitive(prim);
  }
  root.addChild(doc.createNode("Wall-foot drain").setMesh(mesh));
  return doc;
}
