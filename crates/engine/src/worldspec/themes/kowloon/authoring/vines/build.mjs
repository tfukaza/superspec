import { readFile } from "node:fs/promises";
import { Vector3, BufferGeometry, Float32BufferAttribute } from "three";
const v = (...x) => new Vector3(...x);
const up = v(0, 0, 1);
function random(seed) {
  return () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 4294967296;
  };
}
class Batch {
  positions = [];
  indices = [];
  colors = [];
  uvs = [];
  vertex(p, c, uv = [0, 0]) {
    const i = this.positions.length / 3;
    this.positions.push(...p.toArray());
    this.colors.push(...c);
    this.uvs.push(...uv);
    return i;
  }
  quad(a, b, c, d) {
    this.indices.push(a, b, c, a, c, d);
  }
  primitive(doc, buf, material) {
    const g = new BufferGeometry();
    g.setAttribute("position", new Float32BufferAttribute(this.positions, 3));
    g.setIndex(this.indices);
    g.computeVertexNormals();
    const acc = (type, arr) => doc.createAccessor().setBuffer(buf).setType(type).setArray(arr);
    const p = doc
      .createPrimitive()
      .setMaterial(material)
      .setIndices(acc("SCALAR", g.index.array))
      .setAttribute("POSITION", acc("VEC3", g.attributes.position.array))
      .setAttribute("NORMAL", acc("VEC3", g.attributes.normal.array))
      .setAttribute("COLOR_0", acc("VEC3", new Float32Array(this.colors)));
    if (material.getBaseColorTexture()) p.setAttribute("TEXCOORD_0", acc("VEC2", new Float32Array(this.uvs)));
    return p;
  }
}
function stem(batch, points, radius, end, young = false, drape = false) {
  const segments = 6,
    begin = batch.positions.length / 3;
  let prior;
  for (let i = 0; i < points.length; i++) {
    const tangent = points[Math.min(i + 1, points.length - 1)]
      .clone()
      .sub(points[Math.max(0, i - 1)])
      .normalize();
    const reference = prior ?? (Math.abs(tangent.z) > 0.8 ? v(1, 0, 0) : up);
    const a = reference.clone().addScaledVector(tangent, -reference.dot(tangent)).normalize(),
      b = tangent.clone().cross(a);
    prior = a;
    const r = radius + ((end - radius) * i) / (points.length - 1);
    for (let s = 0; s < segments; s++) {
      const phase = (s * 2 * Math.PI) / segments,
        shade = 0.88 + 0.22 * Math.cos(phase + 0.65 * i);
      const tipMix = drape ? Math.max(0, (i / (points.length - 1) - 0.56) / 0.44) : 0;
      const tone = drape
        ? [0.235 + 0.078 * tipMix, 0.127 - 0.018 * tipMix, 0.069 - 0.013 * tipMix]
        : young
          ? [0.22, 0.19, 0.074]
          : [0.25, 0.143, 0.079];
      batch.vertex(
        points[i]
          .clone()
          .addScaledVector(a, Math.cos(phase) * r)
          .addScaledVector(b, Math.sin(phase) * r),
        tone.map((c) => c * shade),
      );
      if (i) {
        const prev = begin + (i - 1) * segments + s,
          next = begin + (i - 1) * segments + ((s + 1) % segments);
        batch.quad(prev, next, next + segments, prev + segments);
      }
    }
  }
}
function blade(batch, origin, direction, normal, width, len, tile, rng, juvenile = 0, seat) {
  const side = direction.clone().cross(normal).normalize(),
    face = side.clone().cross(direction).normalize();
  const twist = 0.09 * (rng() - 0.5),
    curl = 0.04 + 0.115 * rng();
  const base = batch.positions.length / 3,
    fold = 0.037 + rng() * 0.073 + juvenile * 0.2,
    asym = 0.92 + rng() * 0.15,
    shade = 0.89 + rng() * 0.11;
  const rows = [
    [0.12, 0.58],
    [0.185, 0.89],
    [0.34, 1],
    [0.53, 0.86],
    [0.74, 0.58],
    [0.87, 0.28],
    [0.955, 0.014],
  ];
  for (let i = 0; i < rows.length; i++) {
    const [at, w] = rows[i];
    for (let j = 0; j < 7; j++) {
      const s = (j - 3) / 3;
      const t = i === 0 ? 0.18 - 0.15 * Math.sin(Math.PI * Math.abs(s) * 0.86) : at;
      const across = s * w * (s < 0 ? asym : 1);
      const p = origin
        .clone()
        .addScaledVector(direction, (len * (t - 0.155)) / 0.8)
        .addScaledVector(side, width * 0.5 * across * (1 - 0.25 * juvenile))
        .addScaledVector(
          face,
          len *
            (fold * (1 - s * s) * Math.sin(Math.PI * Math.max(0, t)) +
              0.038 * s * Math.sin(5 * t + s) +
              twist * s * t -
              curl * t * t * t),
        );
      if (seat) seat(p);
      batch.vertex(
        p,
        [shade * (1 + 0.035 * juvenile), shade * (1 - 0.14 * juvenile), shade * (1 - 0.11 * juvenile)],
        [((tile % 2) + 0.5 + 0.466 * across) / 2, (Math.floor(tile / 2) + t) / 2],
      );
      if (i && j) {
        const d = base + i * 7 + j;
        batch.quad(d - 8, d - 7, d, d - 1);
      }
    }
  }
}
// Trailing blades turn outward at the lip and hang from short, curved petioles.
function drapeLeaves(site, path, points, normals, wood, leaves, rng) {
  const outward = v(...site.outward),
    lateral = up.clone().cross(outward),
    axis = site.axis;
  const component = axis === 0 ? "x" : "y",
    sign = site.outward[axis];
  const edge = sign < 0 ? site.coping.lo[axis] : site.coping.hi[axis];
  const face = sign < 0 ? site.body.lo[axis] : site.body.hi[axis];
  const top = site.coping.hi[2],
    bottom = site.coping.lo[2];
  const cumulative = [0];
  points.slice(1).forEach((p, i) => cumulative.push(cumulative.at(-1) + points[i].distanceTo(p)));
  const length = cumulative.at(-1),
    positions = [];
  let at = path.role === "root" ? 0.09 : 0.045;
  while (at < length - 0.065) {
    positions.push(at);
    at += (0.066 + 0.055 * rng()) * path.scale;
  }
  positions.push(length - 0.018);
  let count = 0,
    i = 1;
  for (let j = 0; j < positions.length; j++) {
    const distance = positions[j];
    while (i < cumulative.length - 1 && cumulative[i] < distance) i++;
    const t = (distance - cumulative[i - 1]) / (cumulative[i] - cumulative[i - 1]);
    const node = points[i - 1].clone().lerp(points[i], t);
    const n = normals[i - 1].clone().lerp(normals[i], t).normalize();
    const crown = path.role === "crown";
    if (crown && distance < length * 0.36) continue;
    const inner = n.dot(outward) < -0.3;
    if (inner && j % 2) continue;
    const rim = crown ? 0.82 : Math.min(1, Math.max(0, (node.z - top + 0.085) / 0.085));
    const end = Math.max(0, 1 - (length - distance) / (0.14 * path.scale));
    const faceNormal = (inner ? outward.clone().negate() : outward.clone())
      .multiplyScalar(0.94 - 0.35 * rim)
      .addScaledVector(up, 0.31 + 0.49 * rim)
      .normalize();
    const nearLip = (node[component] - edge) * sign > -0.1;
    const across = (j % 2 ? 1 : -1) * (0.78 + 0.4 * rng());
    const number = crown && end < 0.68 ? 2 : end > 0.65 ? 1 : rng() < 0.42 ? 2 : 1;
    for (let k = 0; k < number; k++) {
      const spread = k ? -across : across;
      const size = (0.09 + 0.057 * rng()) * path.scale * (1 - 0.59 * end) * (k ? 0.76 : 1);
      const angle = spread * (0.43 + 0.53 * rng());
      const surface = inner ? outward.clone().negate() : outward;
      const growAxis = lateral
        .clone()
        .multiplyScalar(angle)
        .addScaledVector(up, -(0.82 - 0.58 * rim))
        .addScaledVector(surface, 0.1 + 0.28 * rim)
        .normalize();
      const base = node
        .clone()
        .addScaledVector(lateral, 0.033 * spread * path.scale)
        .addScaledVector(surface, (0.019 - 0.011 * rim) * path.scale)
        .addScaledVector(up, (0.009 + 0.022 * rim) * path.scale);
      const seat = (p) => {
        if (inner || crown) return;
        const offset = (p[component] - edge) * sign;
        if (rim > 0.65 && offset < 0.003 && p.z > top - 0.065) {
          p.z = Math.max(p.z, top + 0.0025);
        } else if (nearLip && p.z < top && p.z > bottom - 0.001)
          p[component] = edge + sign * Math.max(0.0025, offset);
        else if (nearLip && p.z <= bottom)
          p[component] = face + sign * Math.max(0.003, (p[component] - face) * sign);
      };
      seat(base);
      const bend = node.clone().lerp(base, 0.52).addScaledVector(up, 0.008);
      stem(wood, [node, bend, base], 0.0024 * path.scale, 0.001 * path.scale, true);
      const draw = rng(),
        tile = end > 0.6 ? 3 : draw > 0.77 ? 2 : draw > 0.34 ? 1 : 0;
      blade(leaves, base, growAxis, faceNormal, size * (0.65 + 0.13 * rng()), size, tile, rng, end, seat);
      count++;
    }
  }
  return count;
}

function grow(site) {
  const wood = new Batch(),
    leaves = new Batch(),
    rng = random(site.seed);
  let count = 0;
  for (const path of site.paths) {
    const points = path.points.map((s) => v(...s.p)),
      normals = path.points.map((s) => v(...s.n));
    stem(wood, points, path.r, path.r * 0.14, false, Boolean(site.outward));
    if (site.outward) {
      count += drapeLeaves(site, path, points, normals, wood, leaves, rng);
      continue;
    }
    let travel = 0,
      next = 0.1,
      cluster = 0;
    for (let i = 1; i < points.length; i++) {
      const distance = points[i - 1].distanceTo(points[i]);
      if (!distance) continue;
      while (next < travel + distance) {
        const t = (next - travel) / distance,
          point = points[i - 1].clone().lerp(points[i], t);
        let n = normals[i - 1].clone().lerp(normals[i], t).normalize();
        if (n.lengthSq() < 0.9) n = normals[i];
        const right = (Math.abs(n.z) > 0.8 ? v(1, 0, 0) : up.clone().cross(n)).normalize();
        const down = right.clone().cross(n).normalize(),
          sign = cluster % 2 ? -1 : 1;
        const number = rng() < 0.72 ? 3 : 2;
        for (let j = 0; j < number; j++) {
          const size = (0.077 + 0.094 * rng()) * (j === 2 ? 0.63 : 1) * (path.scale ?? 1);
          const angle = sign * (j === 1 ? -1 : 1) * (0.32 + 1.26 * rng());
          const direction = down
            .clone()
            .multiplyScalar(Math.cos(angle))
            .addScaledVector(right, Math.sin(angle))
            .addScaledVector(n, 0.19 + 0.28 * rng())
            .normalize();
          const base = point
            .clone()
            .addScaledVector(right, Math.sin(angle) * ((0.047 + 0.044 * rng()) * (path.scale ?? 1)))
            .addScaledVector(down, -0.012 + 0.043 * rng())
            .addScaledVector(n, 0.032 + 0.022 * rng());
          const joint = point.clone().lerp(base, 0.58).addScaledVector(down, -0.012);
          stem(wood, [point, joint, base], 0.0028, 0.0013, true);
          const draw = rng(),
            tile = j === 2 && draw > 0.62 ? 3 : draw > 0.66 ? 2 : draw > 0.36 ? 1 : 0;
          blade(leaves, base, direction, n, size * (0.64 + 0.19 * rng()), size, tile, rng);
          count++;
        }
        if (n.z < 0.2 && rng() < 0.42) {
          const hold = point
            .clone()
            .addScaledVector(right, 0.025 + sign * 0.054)
            .addScaledVector(down, 0.054)
            .addScaledVector(n, -0.01);
          stem(wood, [point, hold, hold.clone().addScaledVector(down, 0.023)], 0.0031, 0.0007);
        }
        cluster++;
        next += (cluster % 4 === 0 ? 0.26 + 0.2 * rng() : 0.093 + 0.112 * rng()) * (path.scale ?? 1);
      }
      travel += distance;
    }
  }
  return { wood, leaves, count };
}
export async function buildVines(sites, Document) {
  const doc = new Document(),
    scene = doc.createScene(),
    buf = doc.createBuffer();
  const root = doc.createNode("vines-native-Z-up").setRotation([-Math.SQRT1_2, 0, 0, Math.SQRT1_2]);
  scene.addChild(root);
  const tex = async (name) =>
    doc
      .createTexture(name)
      .setImage(await readFile(new URL(name, import.meta.url)))
      .setMimeType("image/png");
  const color = await tex("leaf-color.png"),
    packed = await tex("leaf-properties.png");
  const foliage = doc
    .createMaterial("painted creeping fig leaf")
    .setBaseColorTexture(color)
    .setMetallicRoughnessTexture(packed)
    .setMetallicFactor(0)
    .setRoughnessFactor(1)
    .setDoubleSided(true)
    .setEmissiveTexture(color)
    .setEmissiveFactor([0.13, 0.14, 0.12])
    .setExtras({ worldSpecOutline: false });
  const bark = doc
    .createMaterial("creeping fig woody runners and petioles")
    .setMetallicFactor(0)
    .setRoughnessFactor(0.93)
    .setEmissiveFactor([0.02, 0.012, 0.005])
    .setExtras({ worldSpecOutline: false });
  for (const site of sites) {
    const { wood, leaves, count } = grow(site);
    const mesh = doc
      .createMesh(site.id)
      .addPrimitive(wood.primitive(doc, buf, bark))
      .addPrimitive(leaves.primitive(doc, buf, foliage));
    root.addChild(doc.createNode(site.id).setMesh(mesh).setExtras({ leafCount: count, host: site.id }));
  }
  return doc;
}
