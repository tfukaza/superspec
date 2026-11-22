/** Static PBR export of TreeSpec's painted lawn. The original tuft sampler and
 * 24-stamp atlas supply the plants. Relief, root heights and pigments are
 * baked in metres once, so the delivered asset needs only stock glTF draws. */
import * as THREE from "three";
import { grassTuftSamples, tuftAtlas } from "../../../../../../../browser/renderer/grass-tuft-source.ts";

const { lerp, smoothstep } = THREE.MathUtils;
const fract = (v) => v - Math.floor(v);
const hash = (x, y, z) => fract(Math.sin(x * 127.1 + y * 311.7 + z * 74.7) * 43758.5453);

function noise(x, y, z) {
  const ix = Math.floor(x),
    iy = Math.floor(y),
    iz = Math.floor(z);
  const u = smoothstep(x - ix, 0, 1),
    v = smoothstep(y - iy, 0, 1),
    w = smoothstep(z - iz, 0, 1);
  const layer = (k) =>
    lerp(lerp(hash(ix, iy, k), hash(ix + 1, iy, k), u), lerp(hash(ix, iy + 1, k), hash(ix + 1, iy + 1, k), u), v);
  return lerp(layer(iz), layer(iz + 1), w);
}
function fractal(x, y, z) {
  return (
    0.5 * noise(x, y, z) +
    0.3 * noise(2.03 * x + 7.1, 2.03 * y + 7.1, 2.03 * z + 7.1) +
    0.2 * noise(4.07 * x + 3.3, 4.07 * y + 3.3, 4.07 * z + 3.3)
  );
}

// The afternoon key adds amber. Leave blue and green in the middle values so
// planted turf keeps a leafy colour where the sun crosses the court.
const palette = ["#365631", "#50743d", "#62834a", "#81974a", "#a2b54f"].map((c) => new THREE.Color(c));

function field(x, y, width, depth) {
  // The RELIEF_ROLLING branch in grass-relief.ts, evaluated before export.
  const qx = x / 0.7,
    qy = y / 0.7;
  const warp = (k) => 1.2 * (fractal(qx * 0.9 + k, qy * 0.9 + k, k) - 0.5);
  const wx = warp(3.1),
    wy = warp(17.4),
    wz = warp(29.8);
  const big = fractal(qx + wx, qy + wy, wz);
  const fine = fractal(x / 0.25 + wx * 0.6 + 41.7, y / 0.25 + wy * 0.6 + 41.7, wz * 0.6 + 41.7);
  const a = smoothstep(big, 0.32, 0.68),
    b = smoothstep(fine, 0.32, 0.68);
  const height = 0.08 * (0.65 * a + 0.35 * b * (0.5 + 0.5 * a));
  // Settle the edge of a finite planter onto its receiving face.
  const rim = smoothstep(Math.min(x, y, width - x, depth - y), 0, 0.1);
  const px = 0.425 * x + 0.6 * wx,
    py = 0.34 * y + 0.6 * wy;
  const hue = fractal(px * 1.6 + 113.2, py * 1.6 + 67.8, 0);
  const dark = smoothstep(fractal(px * 0.9 + 41.2, py * 0.9 + 7.7, 0), 0.7, 0.76);
  const light = smoothstep(fractal(px * 1.17 + 9.4, py * 1.17 + 51.3, 0), 0.67, 0.73);
  const colour = palette[1]
    .clone()
    .lerp(palette[2], smoothstep(hue, 0.32, 0.5))
    .lerp(palette[3], smoothstep(hue, 0.5, 0.68))
    .lerp(palette[0], dark)
    .lerp(palette[4], light * (1 - dark));
  // Carry meadow-scale hue drift and warm relief crowns across both layers.
  const meadow = smoothstep(fractal(x / 9 + 7.3, y / 9 + 2.9, 0), 0.3, 0.7);
  const crown = smoothstep(height / 0.08, 0.45, 1);
  colour.r *= lerp(0.946, 1.054, meadow) * (1 + 0.09 * crown);
  colour.g *= lerp(0.991, 1.018, meadow) * (1 + 0.075 * crown);
  colour.b *= lerp(1.18, 0.865, meadow) * (1 - 0.11 * crown);
  colour.multiplyScalar((0.85 + (0.15 * height) / 0.08) * (1 + 0.4 * (big - 0.5)));
  return { height: 0.0015 + height * rim, colour };
}

let sharedAtlas;
function atlasImage(sharp) {
  if (sharedAtlas) return sharedAtlas;
  const atlas = tuftAtlas(11, 16);
  const { data, width, height } = atlas.image;
  const bytes = new Uint8Array(data.length),
    cell = width / 4,
    tint = new THREE.Color();
  for (let y = 0; y < height; y++)
    for (let x = 0; x < width; x++) {
      const i = 4 * (y * width + x),
        row = Math.floor(y / cell);
      const tone = data[i] / 255,
        tip = data[i + 1] / 255,
        alpha = data[i + 3] / 255;
      // White at every rooted stroke: the ground's linear vertex pigment is
      // the complete albedo there. Give only the upper blade its painted hue.
      const cardY = ((y % cell) + 0.5) / cell;
      const lifted = smoothstep(cardY, 0.32, 0.82) * smoothstep(tip, 0.15, 0.68);
      // The live shader's stroke-to-stroke range, in pigment space. Upper
      // vertices supply the bright end so the sRGB image stays representable.
      const stroke = row === 4 ? lerp(0.94, 1.06, tone) / 1.06 : lerp(0.92, 1.08, tone) / 1.08;
      const value = lerp(1, stroke, lifted);
      tint.setRGB(value, value, value).convertLinearToSRGB();
      bytes[i] = Math.round(255 * tint.r);
      bytes[i + 1] = Math.round(255 * tint.g);
      bytes[i + 2] = Math.round(255 * tint.b);
      const inset = Math.min(x % cell, y % cell, cell - 1 - (x % cell), cell - 1 - (y % cell));
      bytes[i + 3] = Math.round(
        255 *
          smoothstep(inset, 0, 3) *
          (row === 4
            ? smoothstep(alpha, 0.3, 0.6)
            : row === 5
              ? smoothstep(alpha, 0.02, 0.6)
              : 0.90 * Math.pow(alpha, 0.72)),
      );
    }
  atlas.dispose();
  sharedAtlas = sharp(bytes, { raw: { width, height, channels: 4 } })
    .png({ compressionLevel: 9 })
    .toBuffer();
  return sharedAtlas;
}

const streams = () => ({ positions: [], normals: [], colors: [], indices: [], uvs: [] });

function lawnField(width, depth) {
  const rows = Math.ceil(depth / 0.063),
    cols = Math.ceil(width / 0.063),
    stride = cols + 1;
  const dx = width / cols,
    dy = depth / rows,
    ground = streams();
  for (let j = 0; j <= rows; j++)
    for (let i = 0; i <= cols; i++) {
      const x = i * dx,
        y = j * dy,
        surface = field(x, y, width, depth);
      ground.positions.push(x, y, surface.height);
      ground.colors.push(surface.colour.r, surface.colour.g, surface.colour.b);
      if (i < cols && j < rows) {
        const k = j * stride + i;
        ground.indices.push(k, k + 1, k + stride, k + 1, k + stride + 1, k + stride);
      }
    }
  const pz = (k) => ground.positions[3 * k + 2];
  const n = new THREE.Vector3();
  for (let j = 0; j <= rows; j++)
    for (let i = 0; i <= cols; i++) {
      const k = j * stride + i;
      const left = i ? k - 1 : k,
        right = i < cols ? k + 1 : k,
        below = j ? k - stride : k,
        above = j < rows ? k + stride : k;
      n.set(
        -(pz(right) - pz(left)) / ((right - left) * dx),
        -(pz(above) - pz(below)) / (((above - below) / stride) * dy),
        1,
      ).normalize();
      ground.normals.push(n.x, n.y, n.z);
    }
  return sampleReceiver(width, depth, ground);
}

function sampleReceiver(width, depth, ground) {
  const cols = Math.ceil(width / 0.063), rows = Math.ceil(depth / 0.063);
  const stride = cols + 1, dx = width / cols, dy = depth / rows;
  if (ground.positions.length !== 3 * stride * (rows+1) ||
      ground.normals.length !== ground.positions.length || ground.colors.length !== ground.positions.length)
    throw Error("Painted lawn template requires its authored relief grid");
  const pz = k => ground.positions[3*k+2];
  // Barycentric interpolation on the very triangles that are drawn keeps
  // every root attached even where a clump drops steeply to the planter rim.
  const normal = new THREE.Vector3(),
    foot = new THREE.Vector3(),
    colour = new THREE.Color();
  function sample(x, y) {
    const i = Math.min(cols - 1, Math.floor(x / dx)),
      j = Math.min(rows - 1, Math.floor(y / dy)),
      k = j * stride + i;
    const u = x / dx - i,
      v = y / dy - j;
    const ids = u + v <= 1 ? [k, k + 1, k + stride] : [k + stride + 1, k + stride, k + 1];
    const weights = u + v <= 1 ? [1 - u - v, u, v] : [u + v - 1, 1 - u, 1 - v];
    foot.set(x, y, 0);
    normal.set(0, 0, 0);
    colour.setRGB(0, 0, 0);
    for (let c = 0; c < 3; c++) {
      const id = ids[c],
        p = 3 * id,
        w = weights[c];
      foot.z += pz(id) * w;
      normal.x += ground.normals[p] * w;
      normal.y += ground.normals[p + 1] * w;
      normal.z += ground.normals[p + 2] * w;
      colour.r += ground.colors[p] * w;
      colour.g += ground.colors[p + 1] * w;
      colour.b += ground.colors[p + 2] * w;
    }
    normal.normalize();
  }
  const plane = new THREE.PlaneGeometry(width, depth).translate(width / 2, depth / 2, 0);
  const { roots, data } = grassTuftSamples(new THREE.Mesh(plane));
  plane.dispose();
  return { ground, dx, dy, roots, data, sample, normal, foot, colour };
}

// Shared metre-space tuft templates for renderers that support TreeSpec's
// camera-facing strokes. Static glTF geometry remains the portable fallback.
export function paintedTuftLayers(width, depth, ground) {
  const { roots, data, sample, normal, foot, colour } = sampleReceiver(width, depth, ground);
  const layers = [[], [], []];
  for (let i = 0; i < data.length / 4; i++) {
    const [x, y] = roots.subarray(i * 3, i * 3 + 2);
    const [height, span, cell, rank] = data.subarray(i * 4, i * 4 + 4);
    sample(x, y);
    const edge = lerp(0.48, 1, smoothstep(Math.min(x, y, width-x, depth-y), 0, 0.09));
    const part = cell < 16 ? 0 : cell < 20 ? 2 : 1;
    layers[part].push(x, foot.z, -y, normal.x, normal.z, -normal.y,
      colour.r, colour.g, colour.b, height * edge, span * edge, cell, rank);
  }
  return layers.map(values => new Float32Array(values));
}

export async function tuftPaintImage(sharp) {
  const atlas = tuftAtlas();
  try {
    const { data, width, height } = atlas.image;
    return await sharp(data, { raw: { width, height, channels: 4 } }).png().toBuffer();
  } finally { atlas.dispose(); }
}

/** The authored rectangle spans [0,width] × [0,depth] in local AML metres. */
export async function bakeGrassField(width, depth, name, { Document, sharp }) {
  if (![width, depth].every((v) => Number.isFinite(v) && v > 0))
    throw Error("A grass field requires positive finite dimensions");
  const doc = new Document(),
    buffer = doc.createBuffer(),
    mesh = doc.createMesh(name);
  const mat = (id) => doc.createMaterial(id).setMetallicFactor(0).setRoughnessFactor(0.96);
  function add(stream, material) {
    if (!stream.indices.length) return;
    const acc = (data, type) => doc.createAccessor().setType(type).setBuffer(buffer).setArray(data);
    const Index = stream.positions.length / 3 <= 65535 ? Uint16Array : Uint32Array;
    const p = doc
      .createPrimitive()
      .setMaterial(material)
      .setIndices(acc(Index.from(stream.indices), "SCALAR"));
    for (const [id, key, n] of [
      ["POSITION", "positions", 3],
      ["NORMAL", "normals", 3],
      ["COLOR_0", "colors", 3],
      ["TEXCOORD_0", "uvs", 2],
    ]) {
      if (stream[key].length) p.setAttribute(id, acc(Float32Array.from(stream[key]), "VEC" + n));
    }
    mesh.addPrimitive(p);
  }
  const { ground, dx, dy, roots, data, sample, normal, foot, colour } = lawnField(width, depth);
  add(ground, mat("Rolling olive turf"));
  const soft = streams(),
    blades = streams();
  const side = new THREE.Vector3(),
    grow = new THREE.Vector3(),
    vertex = new THREE.Vector3(),
    pn = new THREE.Vector3();
  // An atlas stroke starts at 5% of its cell height. Put that stroke on
  // the soil; the otherwise empty strip beneath it is unnecessary geometry.
  const atlasFoot = 0.05,
    neutralHeight = 0.28;
  const clamp = THREE.MathUtils.clamp;
  for (let i = 0; i < data.length / 4; i++) {
    const x = roots[3 * i],
      y = roots[3 * i + 1],
      cell = data[4 * i + 2];
    const rank = fract(data[4 * i + 3]),
      tall = cell >= 16 && cell < 20,
      accent = cell >= 20;
    // A narrow pen stroke above a broader soft wash, with both tapering
    // into the soil along the inside of a planter lip.
    const border = smoothstep(Math.min(x, y, width-x, depth-y), 0, 0.16);
    const h = data[4*i] * (tall ? 0.70 : 0.86) * lerp(0.36, 1, border);
    const span = data[4*i+1] * (tall ? 0.64 : 1.38) * lerp(0.32, 1, border);
    sample(x, y);
    if (tall) {
      const luminance = colour.r * 0.2126 + colour.g * 0.7152 + colour.b * 0.0722;
      if (rank > lerp(0.18, 0.66, smoothstep(luminance, 0.09, 0.24))) continue;
    }
    // Keep the near density of the milestone, with its stable half share of
    // ordinary chubby tufts in quiet areas and more along changes of relief.
    if (!tall && data[4 * i + 3] < 1 && rank > 0.5 + 0.5 * smoothstep(1 - normal.z, 0.005, 0.12)) continue;
    const angle = 2 * Math.PI * fract(rank * 31.73 + 0.19),
      mirror = fract(rank * 91.7 + 0.37) < 0.5 ? -1 : 1;
    side.set(Math.cos(angle), Math.sin(angle), 0);
    const tilt = (fract(rank * 17.31 + 0.5) - 0.5) * (accent ? 1.6 : tall ? 0.9 : 0.35);
    grow.copy(normal).multiplyScalar(0.4);
    grow.z += 0.6;
    grow.normalize().addScaledVector(side, tilt).normalize();
    const dest = tall ? blades : soft,
      base = dest.positions.length / 3;
    // Original milestone's upper strokes. Chubby tufts take the ground tone.
    // 70% of tall blades have subtle tips; the rest catch a stronger glint.
    // Short clusters are the sparse warm dashes scattered through that paint.
    const hue = fract(rank * 53.7 + 0.21),
      pick = fract(hue * 7.13 + 0.3);
    const tallLift = pick < 0.7 ? lerp(1.1, 1.35, pick / 0.7) : lerp(1.7, 2.3, (pick - 0.7) / 0.3);
    const tipTone = tall ? tallLift * 1.06 : 1.08;
    // A few millimetres of rooted stems splay into the painted crown. Keep
    // the whole root chord in its actual relief triangle (the three edge
    // families in the regular mesh are x, y and x/dx+y/dy integers).
    let rootRadius = Math.min(0.008, 0.08 * span);
    const du = side.x / dx,
      dv = side.y / dy;
    for (const [coordinate, slope] of [
      [x / dx, du],
      [y / dy, dv],
      [x / dx + y / dy, du + dv],
    ]) {
      if (Math.abs(slope) > 1e-7) {
        const fraction = fract(coordinate);
        rootRadius = Math.min(rootRadius, Math.min(fraction, 1 - fraction) / Math.abs(slope));
      }
    }
    for (const u of [-0.5, 0.5]) {
      const rootAcross = u * 2 * rootRadius * mirror;
      sample(clamp(x + side.x * rootAcross, 0, width), clamp(y + side.y * rootAcross, 0, depth));
      for (const v of [0, neutralHeight, 1]) {
        const across = v === 0 ? 0 : u * span * mirror - rootAcross;
        vertex
          .copy(foot)
          .addScaledVector(side, across)
          .addScaledVector(grow, v * h);
        dest.positions.push(vertex.x, vertex.y, vertex.z);
        pn.copy(normal);
        if (v === 1) pn.addScaledVector(side, u * 0.35).normalize();
        dest.normals.push(pn.x, pn.y, pn.z);
        const gain = v === 1 ? tipTone : 1,
          dash = accent && v === 1;
        dest.colors.push(
          Math.min(1, colour.r * gain * (dash ? 3.1 : 1)),
          Math.min(1, colour.g * gain * (dash ? 3.0 : 1)),
          Math.min(1, colour.b * gain * (dash ? 1.3 : 1)),
        );
        dest.uvs.push(((cell % 4) + u + 0.5) / 4, (Math.floor(cell / 4) + lerp(atlasFoot, 1, v)) / 6);
      }
    }
    // Front and back have the same growth-facing normal. Winding both sides
    // explicitly keeps this true in ordinary glTF renderers under moving lights.
    for (const left of [base, base + 1]) {
      const right = left + 3;
      dest.indices.push(
        left,
        right,
        right + 1,
        left,
        right + 1,
        left + 1,
        right + 1,
        right,
        left,
        left + 1,
        right + 1,
        left,
      );
    }
  }
  const tex = doc
    .createTexture("Painted lawn tufts")
    .setMimeType("image/png")
    .setImage(await atlasImage(sharp));
  const undergrowth = mat("Soft olive grass")
    .setBaseColorTexture(tex)
    .setAlphaMode("BLEND")
    .setExtras({ castsShadow: false });
  const tips = mat("Tall grass tips").setBaseColorTexture(tex).setAlphaMode("MASK").setAlphaCutoff(0.4);
  add(soft, undergrowth);
  add(blades, tips);
  const scene = doc.createScene().setExtras({ treeSpecPaintedLawn: { version: 1, width, depth } });
  doc.getRoot().setDefaultScene(scene);
  scene.addChild(doc.createNode(name).setMesh(mesh).setRotation([-Math.SQRT1_2, 0, 0, Math.SQRT1_2]));
  return doc;
}
