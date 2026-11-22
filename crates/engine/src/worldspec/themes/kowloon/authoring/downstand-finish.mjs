import { fileURLToPath } from "node:url";

const tileWidth = 2.4,
  tileHeight = 0.6;
const columns = 1200,
  rows = 300;
const grainSource = new URL(
  "../../../../materialspec/program/examples/textures/concrete-plain.png",
  import.meta.url,
);
const clamp = (x, a, b) => Math.max(a, Math.min(b, x));
const mix = (a, b, t) => a + (b - a) * t;
const fraction = (x) => x - Math.floor(x);
const smooth = (x) => x * x * (3 - 2 * x);
function hash(x, y, seed) {
  let h = Math.imul(x + 77, 0x9e3779b1) ^ Math.imul(y + 137, 0x85ebca77) ^ seed;
  h = Math.imul(h ^ (h >>> 16), 0x21f0aaad);
  h = Math.imul(h ^ (h >>> 15), 0x735a2d97);
  return ((h ^ (h >>> 15)) >>> 0) / 4294967296;
}
function field(u, v, n, m, seed) {
  const x = fraction(u) * n,
    y = fraction(v) * m,
    i = Math.floor(x),
    j = Math.floor(y);
  const h = (a, b) => hash(a % n, b % m, seed);
  return mix(
    mix(h(i, j), h(i + 1, j), smooth(fraction(x))),
    mix(h(i, j + 1), h(i + 1, j + 1), smooth(fraction(x))),
    smooth(fraction(y)),
  );
}
const cyclicDistance = (v, center) =>
  Math.abs(fraction(v - center + 0.5) - 0.5);

async function textures(doc, sharp) {
  const sand = await sharp(fileURLToPath(grainSource))
    .resize(columns, rows, { fit: "cover" })
    .greyscale()
    .raw()
    .toBuffer();
  const low = await sharp(sand, {
    raw: { width: columns, height: rows, channels: 1 },
  })
    .blur(4)
    .greyscale()
    .raw()
    .toBuffer();
  const albedo = Buffer.alloc(columns * rows * 3),
    normal = Buffer.alloc(albedo.length),
    properties = Buffer.alloc(albedo.length);
  const displacement = new Float32Array(columns * rows);
  for (let y = 0; y < rows; y++)
    for (let x = 0; x < columns; x++) {
      const i = y * columns + x,
        u = (x + 0.5) / columns,
        v = (y + 0.5) / rows;
      const stain = field(u, v, 6, 4, 81) - 0.5,
        grit = field(u, v, 90, 46, 17),
        grain = sand[i] - low[i];
      const boards = fraction(v) * 4,
        board = Math.floor(boards);
      const drift = (field(u, v, 11, 4, 54) - 0.5) * 0.02;
      const wood =
        (field(u + drift, v, 8, 111, 95) - 0.5) *
          smooth(field(u, v, 22, 9, 177)) +
        0.45 * (field(u, v, 29, 79, 58) - 0.5);
      const joint = Math.min(
        (cyclicDistance(boards + drift, 0) * tileHeight) / 4,
        cyclicDistance(u, board % 2 ? 0.72 : 0.18) * tileWidth,
      );
      const jointMask = 1 - smooth(clamp(joint / 0.0032, 0, 1));
      const nearTie = Math.min(
        ...[
          [0.23, 0.4],
          [0.73, 0.6],
        ].map(([tx, ty]) =>
          Math.hypot(
            cyclicDistance(u, tx) * tileWidth,
            cyclicDistance(v, ty) * tileHeight,
          ),
        ),
      );
      const tieRadius = nearTie + (grit - 0.5) * 0.0035 + grain * 0.000025;
      const tie = 1 - smooth(clamp((tieRadius - 0.006) / 0.009, 0, 1));
      const rim =
        Math.exp(-(((tieRadius - 0.014) / 0.0027) ** 2)) * (0.32 + 1.45 * grit);
      const skin =
        3 * stain +
        4.5 * wood +
        1.8 * (hash(board, 0, 8) - 0.5) +
        grain * 0.75 +
        4.5 * (grit - 0.5);
      const edge = clamp((v - 0.932) / 0.068, 0, 1);
      const chip =
        edge *
        smooth(
          clamp(
            (0.64 * field(u, v, 82, 13, 104) + 0.36 * grit - 0.56) / 0.19,
            0,
            1,
          ),
        );
      // A few shell-shaped chips on each struck arris. A rough grit boundary
      // separates the lighter cement interior from the narrow dark lip.
      let spall = 0,
        fracture = 0;
      if (v > 0.9)
        for (const [along, reach, height] of [
          [0.062, 0.015, 0.035],
          [0.274, 0.029, 0.061],
          [0.551, 0.011, 0.028],
          [0.813, 0.024, 0.052],
        ]) {
          const dist =
            Math.hypot(cyclicDistance(u, along) / reach, (1 - v) / height) +
            (grit - 0.5) * 0.32 +
            grain * 0.002;
          spall = Math.max(
            spall,
            1 - smooth(clamp((dist - 0.78) / 0.28, 0, 1)),
          );
          fracture = Math.max(
            fracture,
            Math.exp(-(((dist - 0.96) / 0.14) ** 2)),
          );
        }
      const variation =
        skin -
        6 * jointMask -
        27 * tie +
        13 * rim +
        5 * edge -
        11 * chip +
        30 * spall -
        15 * fracture;
      [183, 177, 164].forEach(
        (base, c) =>
          (albedo[i * 3 + c] = clamp(
            base + variation + (c === 0 ? 2 * stain : 0),
            0,
            255,
          )),
      );
      displacement[i] =
        -0.001 * jointMask -
        0.0041 * tie +
        0.00065 * rim +
        0.0001 * wood +
        0.00023 * (grit - 0.5) +
        grain * 0.000008 -
        0.0011 * chip -
        0.0034 * spall;
      properties.set(
        [255, clamp(220 + 12 * tie + 8 * stain + 4 * wood, 0, 255), 0],
        i * 3,
      );
    }
  for (let y = 0; y < rows; y++)
    for (let x = 0; x < columns; x++) {
      const at = (a, b) =>
        displacement[((b + rows) % rows) * columns + ((a + columns) % columns)];
      const dx = ((at(x + 1, y) - at(x - 1, y)) * columns) / (2 * tileWidth);
      const dy = ((at(x, y + 1) - at(x, y - 1)) * rows) / (2 * tileHeight);
      const len = Math.hypot(dx, dy, 1);
      normal.set(
        [(-0.5 * dx) / len + 0.5, (0.5 * dy) / len + 0.5, 0.5 / len + 0.5].map(
          (c) => Math.round(255 * c),
        ),
        (y * columns + x) * 3,
      );
    }
  return Promise.all(
    [
      [albedo, "pigment"],
      [normal, "relief"],
      [properties, "roughness"],
    ].map(async ([data, name]) => {
      const png = await sharp(data, {
        raw: { width: columns, height: rows, channels: 3 },
      })
        .png()
        .toBuffer();
      return doc
        .createTexture("Board-formed concrete " + name)
        .setImage(png)
        .setMimeType("image/png");
    }),
  );
}

/** Finish just the authored downstand. Native Z-up vertex coordinates precede
 * the glTF scene axis conversion; the same coordinates drive physical collision. */
export async function finishDownstands(doc, sharp) {
  const nodes = doc
    .getRoot()
    .listNodes()
    .filter(
      (n) =>
        /^masonry-(downstand|bearing)-/.test(n.getExtras().objectId ?? n.getName()) && n.getMesh(),
    );
  if (!nodes.length) return;
  const [color, normal, roughness] = await textures(doc, sharp);
  const material = doc
    .createMaterial("Board-formed arcade downstand")
    .setBaseColorTexture(color)
    .setNormalTexture(normal)
    .setMetallicRoughnessTexture(roughness)
    .setMetallicFactor(0)
    .setRoughnessFactor(1);
  for (const node of nodes)
    for (const primitive of node.getMesh().listPrimitives()) {
      const pos = primitive.getAttribute("POSITION"),
        norm = primitive.getAttribute("NORMAL"),
        lo = pos.getMin([]),
        hi = pos.getMax([]);
      const haunch = (node.getExtras().objectId ?? node.getName()).startsWith("masonry-bearing-");
      const axis = hi[0] - lo[0] > hi[1] - lo[1] ? 0 : 1,
        cross = 1 - axis;
      const uv = new Float32Array(pos.getCount() * 2),
        tint = new Float32Array(pos.getCount() * 3);
      for (let i = 0; i < pos.getCount(); i++) {
        const p = pos.getElement(i, []),
          n = norm.getElement(i, []);
        // A tapered bearing has a genuine inclined soffit as well as a flat heel.
        const underside = n[2] < -0.78 && Math.abs(n[cross]) < 0.25,
          bevel = n[2] < -0.15 && !underside;
        const end = Math.abs(n[axis]) > 0.9;
        const u =
          (p[end ? cross : axis] - lo[end ? cross : axis]) / tileWidth +
          (n[cross] < 0 ? 0.41 : 0) + (haunch ? 0.355 : 0);
        const v = underside
          ? 0.11 + (p[cross] - lo[cross]) / tileHeight
          : 1 - (p[2] - lo[2]) / tileHeight;
        uv.set([u, v], 2 * i);
        const shade = bevel ? 1 : underside ? 0.76 : 0.92;
        tint.set(
          [
            shade,
            shade * (underside ? 1.025 : 1),
            shade * (underside ? 1.07 : 1),
          ].map((x) => Math.min(1, x)),
          3 * i,
        );
      }
      const accessor = (name, type, a) =>
        doc
          .createAccessor(name)
          .setType(type)
          .setArray(a)
          .setBuffer(pos.getBuffer());
      primitive.setAttribute(
        "TEXCOORD_0",
        accessor("Form boards in metres", "VEC2", uv),
      );
      primitive
        .setAttribute("COLOR_0", accessor("Soffit and chamfer", "VEC3", tint))
        .setMaterial(material);
    }
}
