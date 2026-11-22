/** Bake miniature painted leaf clusters for the Kowloon weed ItemSpec.
 * node tools/weed-paint.mjs <output.glb> */
import { writeFile } from "node:fs/promises";
import { PNG } from "pngjs";
import { BufferGeometry, Float32BufferAttribute } from "three";
import { leafCard, melted } from "../browser/renderer/leaf-cards.ts";

const output = process.argv[2];
if (!output) throw Error("Supply the destination GLB.");
const cell = 128,
  width = cell * 2,
  height = cell * 2;
const pigment = new PNG({ width, height }),
  normals = new PNG({ width, height });
for (let tile = 0; tile < 4; tile++) {
  if (tile >= 2) {
    // A broad crevice weed leaf, tapered into an attached petiole. Opposite
    // scalloped lobes and a softer rounder sister leaf break up the rosette.
    for (let y = 0; y < cell; y++)
      for (let x = 0; x < cell; x++) {
        const v = (y + 0.5) / cell,
          u = (x + 0.5) / cell;
        const t = Math.max(0, Math.min(1, (v - 0.025) / 0.95));
        const centre = 0.49 + 0.045 * Math.sin(Math.PI * t) * (tile === 2 ? 1 : -1);
        const side = u < centre ? -1 : 1;
        const skew = 0.14 * side + 0.46;
        const lobe = 0.5 - 0.5 * Math.cos(2 * Math.PI * (4.7 * t + skew));
        const tooth = (tile === 2 ? 0.52 : 0.26) * Math.pow(lobe, 2.2) * Math.sin(Math.PI * t);
        const leafWidth =
          (0.034 * (1 - t) + 0.39 * Math.pow(Math.sin(Math.PI * Math.pow(t, 1.05)), 0.84)) * (1 - tooth);
        const lateral = (u - centre) / Math.max(0.013, leafWidth);
        const distance = Math.min(leafWidth - Math.abs(u - centre), v - 0.018, 0.988 - v);
        const ramp = Math.max(0, Math.min(1, 0.5 + (distance * cell) / 2.9));
        const alpha = ramp * ramp * (3 - 2 * ramp);
        const ink =
          0.84 +
          0.13 * t +
          0.07 * Math.sin(2 * Math.PI * (0.75 * t + 0.14 * lateral)) -
          0.07 * lobe * Math.min(1, Math.abs(lateral));
        const o = 4 * ((Math.floor(tile / 2) * cell + cell - 1 - y) * width + (tile % 2) * cell + x);
        pigment.data.set(
          [Math.round(142 * ink), Math.round(151 * ink), Math.round(112 * ink), Math.round(255 * alpha)],
          o,
        );
        const nx = Math.max(-0.7, Math.min(0.7, 0.7 * lateral + 0.08 * Math.sin(22 * t + tile)));
        const ny = 0.19 * Math.sin(Math.PI * (t - 0.2)) + 0.08 * lobe;
        normals.data.set(
          [nx, -ny, Math.sqrt(Math.max(0, 1 - nx * nx - ny * ny))]
            .map((n) => Math.round(127.5 * (1 + n)))
            .concat(255),
          o,
        );
      }
    continue;
  }
  const card = leafCard({
    size: cell,
    seed: [27, 103][tile],
    shape: tile ? "lopsided" : "round",
    leaf: 0.095,
    density: 1.45,
    round: 0.48,
    width: 0.68,
    offshoots: 0,
  });
  const relief = melted(card, 1);
  const silhouette = melted(card, 2);
  for (let y = 0; y < cell; y++)
    for (let x = 0; x < cell; x++) {
      const i = y * cell + x;
      const o = 4 * (((tile >> 1) * cell + cell - 1 - y) * width + (tile % 2) * cell + x);
      // Feather alpha across nearby leaf tips; retain the relief and pigment inside.
      let coverage = 0,
        weights = 0;
      for (let k = -3; k <= 3; k++)
        for (let j = -3; j <= 3; j++) {
          const weight = Math.exp(-(k * k + j * j) / 5.5);
          const xx = x + k,
            yy = y + j;
          if (xx >= 0 && yy >= 0 && xx < cell && yy < cell)
            coverage += silhouette.coverage[yy * cell + xx] * weight;
          weights += weight;
        }
      // A wash behind the smaller leaf strokes keeps the core continuous.
      // The last few leaf tips alone define the feathered perimeter.
      const px = (x + 0.5) / cell - 0.5,
        py = (y + 0.5) / cell - 0.48;
      const angle = Math.atan2(py, px);
      const radius = Math.hypot(px / 0.36, py / 0.38) * (1 + 0.05 * Math.sin(3 * angle + tile));
      const t = Math.min(1, Math.max(0, (1.1 - radius) / 0.42));
      const wash = 0.88 * t * t * (3 - 2 * t);
      const stroke = Math.min(1, coverage / weights);
      const alpha = wash + (1 - wash) * stroke;
      const leaf = relief.coverage[i] ? Math.min(1, relief.tone[i]) : 0.77;
      const hue = relief.hue[i] || 0.5;
      const ink = 0.76 + 0.3 * leaf;
      pigment.data.set(
        [
          Math.round((128 + 24 * hue) * ink),
          Math.round((142 + 13 * hue) * ink),
          Math.round((117 - 13 * hue) * ink),
          Math.round(255 * alpha),
        ],
        o,
      );
      // +Y in the brush is up the card. glTF's authored V and bitangent run down.
      const nx = relief.nx[i] * 0.88,
        ny = -relief.ny[i] * 0.88;
      const nz = Math.sqrt(Math.max(0, 1 - nx * nx - ny * ny));
      normals.data.set([nx, ny, nz].map((v) => Math.round(127.5 * (1 + v))).concat(255), o);
    }
}

// Each lobe has a continuous rounded body under the overlapping, feathered
// leaf dabs. Faces around its foot and crown share positions and normals.
const mound = [
  [-0.165, -0.002, 0.087, 0.035, 0.062, -0.26],
  [-0.066, 0.009, 0.113, 0.061, 0.151, 0.1],
  [0.028, -0.012, 0.111, 0.056, 0.1, -0.17],
  [0.114, 0.013, 0.092, 0.046, 0.085, 0.22],
  [0.189, -0.004, 0.047, 0.027, 0.039, -0.16],
];
const seam = [
  [-0.174, 0.001, 0.027, 0.015, 0.024, -0.19],
  [-0.056, -0.003, 0.057, 0.022, 0.061, 0.09],
  [0.012, 0.002, 0.036, 0.018, 0.032, -0.17],
  [0.142, 0.002, 0.039, 0.019, 0.044, 0.16],
];
function paintMesh(lobes) {
  const positions = [],
    directions = [],
    uvs = [],
    colors = [],
    indices = [];
  for (const [tile, [cx, cy, rx, ry, tall, angle]] of lobes.entries()) {
    const ca = Math.cos(angle),
      sa = Math.sin(angle);
    function vertex(x, y, z, nx, ny, nz, u, v, chart = tile % 2, fresh = false) {
      const len = Math.hypot(nx, ny, nz);
      positions.push(cx + ca * x - sa * y, z, -(cy + sa * x + ca * y));
      directions.push((ca * nx - sa * ny) / len, nz / len, -(sa * nx + ca * ny) / len);
      uvs.push(((chart % 2) + u) / 2, ((chart >> 1) + 1 - v) / 2);
      // Older leaves at the rooted interior have a cooler blue-green pigment.
      // New exposed leaflets are olive. Surface light still determines which
      // side of either pigment is bright when the plant rotates.
      const age = Math.max(
        0,
        Math.min(1, (fresh ? 0.22 : -0.08) + (0.86 * z) / tall + 0.07 * Math.sin(tile * 4)),
      );
      colors.push(0.49 + 0.51 * age, 0.6 + 0.4 * age, 0.9 - 0.22 * age);
    }
    function profile(theta, phi) {
      const c = Math.cos(theta),
        s = Math.sin(theta),
        cp = Math.cos(phi),
        sp = Math.sin(phi);
      const wobble = 1 + 0.055 * Math.sin(3 * theta + tile) + 0.023 * Math.sin(5 * theta - 0.7 * tile);
      const radius = cp * (0.72 + 0.32 * Math.sin(2 * phi)) * wobble;
      // The broad lobe normal stays stable even where its small rim wobbles.
      return {
        x: 0.86 * rx * (radius * c + sp * sp * 0.22 * Math.cos(tile + 0.4)),
        y: 0.85 * ry * (radius * s + sp * sp * 0.19 * Math.sin(tile + 1)),
        z: 0.82 * tall * sp,
        nx: cp * c,
        ny: cp * s,
        nz: 0.22 + 0.92 * sp,
      };
    }
    // Four mirrored patches use the core of the same chart. Its tangent frame
    // turns with the surface so the light-facing side is determined by the sun.
    const cols = 3,
      rows = 4;
    for (let sector = 0; sector < 4; sector++) {
      const start = positions.length / 3;
      for (let j = 0; j <= rows; j++)
        for (let i = 0; i <= cols; i++) {
          if (j === rows && i === 0) continue;
          const u = i / cols,
            v = j / rows,
            theta = ((sector + u) * Math.PI) / 2;
          const q = profile(theta, (v * Math.PI) / 2);
          vertex(q.x, q.y, q.z, q.nx, q.ny, q.nz, 0.3 + 0.4 * (sector % 2 ? 1 - u : u), 0.3 + 0.4 * v);
        }
      for (let j = 0; j < rows; j++)
        for (let i = 0; i < cols; i++) {
          const a = start + j * (cols + 1) + i,
            b = a + 1,
            d = a + cols + 1,
            c = d + 1;
          indices.push(a, b, j + 1 === rows ? d : c);
          if (j + 1 < rows) indices.push(a, c, d);
        }
    }
    // Real curved cards grow from each lobe's central crown. The leaf stalk,
    // arch and tip remain one connected stroke. Wide leaves spill over the
    // cool underpainting and finish on or just above the pavement.
    const count = lobes === seam ? 6 : 11;
    for (let k = 0; k < count; k++) {
      const rank = k % 3,
        phase = (k * 0.61803398875 + tile * 0.279) % 1;
      const theta = ((k + 0.32 * tile + 0.22 * Math.sin(k * 7 + tile)) * 2 * Math.PI) / count;
      const cs = Math.cos(theta),
        sn = Math.sin(theta);
      const sideLength = Math.hypot(rx * sn, ry * cs);
      const tx = (-rx * sn) / sideLength,
        ty = (ry * cs) / sideLength;
      const low = rank === 0;
      const root = low ? 0.64 : 0.12 + 0.15 * phase;
      const end = low ? 1.38 + 0.27 * phase : 0.95 + 0.33 * phase;
      const rootZ = (low ? 0.31 : 0.66 + 0.1 * phase) * tall;
      const tipZ = (low ? 0.018 + 0.045 * phase : 0.18 + 0.16 * phase) * tall;
      const arch = (low ? 0.14 : 0.51 + 0.07 * phase) * tall;
      const width = (low ? 0.32 + 0.12 * phase : 0.32 + 0.24 * phase) * rx;
      const start = positions.length / 3,
        cols = 2,
        rows = 4;
      for (let j = 0; j <= rows; j++)
        for (let i = 0; i <= cols; i++) {
          const u = i / cols,
            v = j / rows,
            lateral = 2 * u - 1;
          const r = root + (end - root) * v;
          const reach = Math.hypot(rx * cs, ry * sn) * (end - root);
          const dz = tipZ - rootZ + Math.PI * arch * Math.cos(Math.PI * v);
          const tilt = Math.max(-0.68, Math.min(0.91, -dz / Math.hypot(reach, dz)));
          const nz = Math.sqrt(Math.max(0, 1 - tilt * tilt));
          const twist = width * 0.42 * Math.sin(Math.PI * v) * (phase - 0.5);
          const fold = 0.11 * tall * lateral * lateral * Math.sin(Math.PI * v);
          const z = Math.max(0.001, rootZ + (tipZ - rootZ) * v + arch * Math.sin(Math.PI * v) - fold);
          vertex(
            rx * cs * r + tx * (width * lateral + twist),
            ry * sn * r + ty * (width * lateral + twist),
            z,
            cs * tilt + tx * lateral * 0.26,
            sn * tilt + ty * lateral * 0.26,
            nz,
            u,
            v,
            2 + (k % 2),
            !low,
          );
        }
      for (let j = 0; j < rows; j++)
        for (let i = 0; i < cols; i++) {
          const a = start + j * (cols + 1) + i,
            b = a + 1,
            d = a + cols + 1,
            c = d + 1;
          indices.push(a, c, b, a, d, c);
        }
    }
  }
  return { positions, directions, uvs, colors, indices };
}
const chunks = [],
  views = [],
  accessors = [];
let bytes = 0;
function view(data, target) {
  const index = views.length;
  views.push({ buffer: 0, byteOffset: bytes, byteLength: data.length, target });
  const pad = Buffer.alloc((4 - (data.length % 4)) % 4);
  chunks.push(data, pad);
  bytes += data.length + pad.length;
  return index;
}
function attribute(flat, size, indices = false, bounds = false) {
  const array = indices ? new Uint16Array(flat) : new Float32Array(flat);
  const accessor = {
    bufferView: view(Buffer.from(array.buffer), indices ? 34963 : 34962),
    componentType: indices ? 5123 : 5126,
    count: flat.length / size,
    type: size === 1 ? "SCALAR" : "VEC" + size,
  };
  if (bounds) {
    const axes = [0, 1, 2].map((axis) => [...array].filter((_, i) => i % 3 === axis));
    accessor.min = axes.map((a) => Math.min(...a));
    accessor.max = axes.map((a) => Math.max(...a));
  }
  accessors.push(accessor);
  return accessors.length - 1;
}
const meshes = [mound, seam].map((lobes) => {
  const { positions, directions, uvs, colors, indices } = paintMesh(lobes);
  const geometry = new BufferGeometry();
  geometry.setAttribute("position", new Float32BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new Float32BufferAttribute(directions, 3));
  geometry.setAttribute("uv", new Float32BufferAttribute(uvs, 2));
  geometry.setIndex(indices);
  geometry.computeTangents();
  const attributes = {
    POSITION: attribute(positions, 3, false, true),
    NORMAL: attribute(directions, 3),
    TEXCOORD_0: attribute(uvs, 2),
    TANGENT: attribute(geometry.attributes.tangent.array, 4),
    COLOR_0: attribute(colors, 3),
  };
  return { primitives: [{ attributes, indices: attribute(indices, 1, true), material: 0 }] };
});
const images = [pigment, normals].map((image, i) => ({
  name: i ? "Leaf-pattern normals" : "Feathered leaf pigment",
  mimeType: "image/png",
  bufferView: view(PNG.sync.write(image)),
}));
const document = {
  asset: { version: "2.0", generator: "Kowloon miniature leaf mounds" },
  buffers: [{ byteLength: bytes }],
  scene: 0,
  scenes: [
    { name: "Mound", nodes: [0] },
    { name: "Seam", nodes: [1] },
  ],
  nodes: [{ mesh: 0 }, { mesh: 1 }],
  meshes,
  images,
  textures: [{ source: 0 }, { source: 1 }],
  materials: [
    {
      name: "Painted weed undergrowth",
      alphaMode: "BLEND",
      doubleSided: false,
      pbrMetallicRoughness: { metallicFactor: 0, roughnessFactor: 1, baseColorTexture: { index: 0 } },
      normalTexture: { index: 1, scale: 0.56 },
    },
  ],
  bufferViews: views,
  accessors,
};
const json = Buffer.from(JSON.stringify(document));
const jsonChunk = Buffer.concat([json, Buffer.alloc((4 - (json.length % 4)) % 4, 32)]);
const header = Buffer.alloc(20),
  binHeader = Buffer.alloc(8);
header.write("glTF");
header.writeUInt32LE(2, 4);
header.writeUInt32LE(28 + jsonChunk.length + bytes, 8);
header.writeUInt32LE(jsonChunk.length, 12);
header.write("JSON", 16);
binHeader.writeUInt32LE(bytes);
binHeader.write("BIN\0", 4);
await writeFile(output, Buffer.concat([header, jsonChunk, binHeader, ...chunks]));
