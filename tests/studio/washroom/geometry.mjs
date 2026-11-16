/** Small mesh-building helpers for the washroom scene: meters, right-handed Z-up. */

export const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
export const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
export const mul = (a, s) => [a[0] * s, a[1] * s, a[2] * s];
export const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
export const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
export const length = a => Math.hypot(a[0], a[1], a[2]);
export const unit = a => mul(a, 1 / length(a));
const X = [1, 0, 0], Y = [0, 1, 0], Z = [0, 0, 1];

/** One named mesh: indexed triangles with per-vertex normals. */
export class Mesh {
  constructor(name, color) {
    this.name = name; this.color = color;
    this.positions = []; this.normals = []; this.indices = [];
  }
  vertex(position, normal) {
    this.positions.push(...position); this.normals.push(...normal);
    return this.positions.length / 3 - 1;
  }
  /** A flat quad whose front faces `outward`, whatever order its corners are given in. */
  quad(a, b, c, d, outward) {
    let normal = unit(cross(sub(b, a), sub(d, a)));
    let corners = [a, b, c, d];
    if (dot(normal, outward) < 0) { normal = mul(normal, -1); corners = [a, d, c, b]; }
    const [i, j, k, l] = corners.map(corner => this.vertex(corner, normal));
    this.indices.push(i, j, k, i, k, l);
  }
  /** A triangle of existing vertices whose front faces `outward`. */
  face(i, j, k, outward) {
    const at = index => this.positions.slice(index * 3, index * 3 + 3);
    const normal = cross(sub(at(j), at(i)), sub(at(k), at(i)));
    if (dot(normal, outward) < 0) this.indices.push(i, k, j); else this.indices.push(i, j, k);
  }
  /** A box with half sizes `half` along the orthonormal `axes`. */
  orientedBox(center, half, axes = [X, Y, Z]) {
    const corner = (x, y, z) => add(center, add(mul(axes[0], x * half[0]), add(mul(axes[1], y * half[1]), mul(axes[2], z * half[2]))));
    for (let axis = 0; axis < 3; axis++) {
      for (const side of [-1, 1]) {
        const at = (u, v) => { const signs = [0, 0, 0]; signs[axis] = side; signs[(axis + 1) % 3] = u; signs[(axis + 2) % 3] = v; return corner(...signs); };
        this.quad(at(-1, -1), at(1, -1), at(1, 1), at(-1, 1), mul(axes[axis], side));
      }
    }
    return this;
  }
  /** An axis-aligned box between two corners. */
  box(low, high) {
    return this.orientedBox(mul(add(low, high), 0.5), mul(sub(high, low), 0.5));
  }
  /** A bar of rectangular section from `start` to `end`; `side` fixes its width direction. */
  bar(start, end, side, width, thickness) {
    const along = unit(sub(end, start));
    const across = unit(sub(side, mul(along, dot(side, along))));
    return this.orientedBox(mul(add(start, end), 0.5), [length(sub(end, start)) / 2, width / 2, thickness / 2], [along, across, cross(along, across)]);
  }
  /** Joins consecutive rings of smooth vertices; `closed` also joins each ring's last vertex to its first. */
  rings(rings, closed = true) {
    const indexed = rings.map(ring => ring.map(([position, normal]) => this.vertex(position, normal)));
    for (let r = 0; r + 1 < rings.length; r++) {
      const count = rings[r].length;
      for (let s = 0; s < (closed ? count : count - 1); s++) {
        const next = (s + 1) % count;
        const outward = add(rings[r][s][1], rings[r + 1][next][1]);
        this.face(indexed[r][s], indexed[r][next], indexed[r + 1][next], outward);
        this.face(indexed[r][s], indexed[r + 1][next], indexed[r + 1][s], outward);
      }
    }
    return this;
  }
  /** A flat cap over a ring of points, facing `outward`. */
  cap(points, outward) {
    const normal = unit(outward);
    const center = mul(points.reduce(add, [0, 0, 0]), 1 / points.length);
    const hub = this.vertex(center, normal);
    const rim = points.map(point => this.vertex(point, normal));
    for (let s = 0; s < rim.length; s++) this.face(hub, rim[s], rim[(s + 1) % rim.length], normal);
    return this;
  }
  /** A round tube along a path with rounded corners of radius `bend`. */
  tube(points, radius, { bend = radius * 2.2, segments = 14, steps = 6, caps = true } = {}) {
    const samples = centerline(points, bend, steps);
    let normal = perpendicular(samples[0].tangent);
    const rings = samples.map(({ point, tangent }) => {
      normal = unit(sub(normal, mul(tangent, dot(normal, tangent))));
      const binormal = cross(tangent, normal);
      return Array.from({ length: segments }, (_, s) => {
        const angle = s / segments * Math.PI * 2;
        const out = add(mul(normal, Math.cos(angle)), mul(binormal, Math.sin(angle)));
        return [add(point, mul(out, radius)), out];
      });
    });
    this.rings(rings);
    if (caps) {
      this.cap(rings[0].map(([position]) => position), mul(samples[0].tangent, -1));
      this.cap(rings.at(-1).map(([position]) => position), samples.at(-1).tangent);
    }
    return this;
  }
  /** A rectangular duct along a path that bends in the plane perpendicular to `lateral`. */
  duct(points, lateral, width, height, { bend, steps = 5, caps = true } = {}) {
    const samples = centerline(points, bend ?? height, steps);
    const sections = samples.map(({ point, tangent }) => {
      const other = unit(cross(tangent, lateral));
      return [[-1, -1], [1, -1], [1, 1], [-1, 1]].map(([u, v]) => add(point, add(mul(lateral, u * width / 2), mul(other, v * height / 2))));
    });
    for (let r = 0; r + 1 < sections.length; r++) {
      const middle = mul(add(samples[r].point, samples[r + 1].point), 0.5);
      for (let s = 0; s < 4; s++) {
        const [a, b, c, d] = [sections[r][s], sections[r][(s + 1) % 4], sections[r + 1][(s + 1) % 4], sections[r + 1][s]];
        this.quad(a, b, c, d, sub(mul(add(add(a, b), add(c, d)), 0.25), middle));
      }
    }
    if (caps) {
      this.quad(...sections[0], mul(samples[0].tangent, -1));
      this.quad(...sections.at(-1), samples.at(-1).tangent);
    }
    return this;
  }
  /** A torus around `axis`. */
  torus(center, axis, major, minor, { segments = 28, sides = 10 } = {}) {
    const u = perpendicular(axis), v = cross(axis, u);
    const rings = Array.from({ length: segments + 1 }, (_, r) => {
      const angle = r / segments * Math.PI * 2;
      const radial = add(mul(u, Math.cos(angle)), mul(v, Math.sin(angle)));
      return Array.from({ length: sides }, (_, s) => {
        const turn = s / sides * Math.PI * 2;
        const out = add(mul(radial, Math.cos(turn)), mul(axis, Math.sin(turn)));
        return [add(center, add(mul(radial, major), mul(out, minor))), out];
      });
    });
    return this.rings(rings);
  }
  /** An open-topped basin: an outer box with a cavity `wall` in from the sides and `floor` above the base. */
  basin(low, high, wall, floor) {
    const [x0, y0, z0] = low, [x1, y1, z1] = high;
    const [a0, b0, a1, b1, c0] = [x0 + wall, y0 + wall, x1 - wall, y1 - wall, z0 + floor];
    const outer = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]], inner = [[a0, b0], [a1, b0], [a1, b1], [a0, b1]];
    const middle = [(x0 + x1) / 2, (y0 + y1) / 2, 0];
    for (let s = 0; s < 4; s++) {
      const [p, q] = [outer[s], outer[(s + 1) % 4]], [m, n] = [inner[s], inner[(s + 1) % 4]];
      const away = sub([(p[0] + q[0]) / 2, (p[1] + q[1]) / 2, 0], middle);
      this.quad([...p, z0], [...q, z0], [...q, z1], [...p, z1], away);
      this.quad([...p, z1], [...q, z1], [...n, z1], [...m, z1], Z);
      this.quad([...m, c0], [...n, c0], [...n, z1], [...m, z1], mul(away, -1));
    }
    this.quad([x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0], mul(Z, -1));
    this.quad([a0, b0, c0], [a1, b0, c0], [a1, b1, c0], [a0, b1, c0], Z);
    return this;
  }
}

function perpendicular(direction) {
  const other = Math.abs(direction[2]) < 0.9 ? Z : X;
  return unit(cross(direction, other));
}

/** Points and tangents along a polyline whose corners are rounded with radius `bend`. */
function centerline(points, bend, steps) {
  const samples = [];
  const first = unit(sub(points[1], points[0]));
  samples.push({ point: points[0], tangent: first });
  for (let i = 1; i + 1 < points.length; i++) {
    const before = unit(sub(points[i], points[i - 1])), after = unit(sub(points[i + 1], points[i]));
    const angle = Math.acos(Math.max(-1, Math.min(1, dot(before, after))));
    if (angle < 1e-6) continue;
    const reach = bend * Math.tan(angle / 2);
    const toward = unit(sub(after, mul(before, dot(before, after))));
    const start = sub(points[i], mul(before, reach));
    const center = add(start, mul(toward, bend));
    for (let k = 0; k <= steps; k++) {
      const turn = angle * k / steps;
      samples.push({
        point: add(center, add(mul(toward, -bend * Math.cos(turn)), mul(before, bend * Math.sin(turn)))),
        tangent: unit(add(mul(before, Math.cos(turn)), mul(toward, Math.sin(turn)))),
      });
    }
  }
  samples.push({ point: points.at(-1), tangent: unit(sub(points.at(-1), points.at(-2))) });
  return samples;
}

/** Writes meshes as one GLB: a node per mesh, with glTF's Y-up axes and a plain material per mesh. */
export function writeGlb(meshes) {
  const chunks = [], accessors = [], views = [];
  let offset = 0;
  const view = (array, target) => {
    const bytes = Buffer.from(array.buffer, array.byteOffset, array.byteLength);
    views.push({ buffer: 0, byteOffset: offset, byteLength: bytes.length, target });
    chunks.push(bytes); offset += bytes.length;
    const padding = (4 - offset % 4) % 4;
    if (padding) { chunks.push(Buffer.alloc(padding)); offset += padding; }
    return views.length - 1;
  };
  const document = {
    asset: { version: "2.0", generator: "superspec washroom scene" }, scene: 0,
    scenes: [{ nodes: meshes.map((_, index) => index) }], nodes: [], meshes: [], materials: [],
  };
  meshes.forEach((mesh, index) => {
    const count = mesh.positions.length / 3;
    const upright = values => { const out = new Float32Array(values.length); for (let i = 0; i < values.length; i += 3) { out[i] = values[i]; out[i + 1] = values[i + 2]; out[i + 2] = -values[i + 1]; } return out; };
    const positions = upright(mesh.positions);
    const low = [Infinity, Infinity, Infinity], high = [-Infinity, -Infinity, -Infinity];
    for (let i = 0; i < positions.length; i++) { low[i % 3] = Math.min(low[i % 3], positions[i]); high[i % 3] = Math.max(high[i % 3], positions[i]); }
    const base = accessors.length;
    accessors.push({ bufferView: view(positions, 34962), componentType: 5126, count, type: "VEC3", min: low, max: high });
    accessors.push({ bufferView: view(upright(mesh.normals), 34962), componentType: 5126, count, type: "VEC3" });
    accessors.push({ bufferView: view(new Float32Array(count * 2), 34962), componentType: 5126, count, type: "VEC2" });
    accessors.push({ bufferView: view(new Uint32Array(mesh.indices), 34963), componentType: 5125, count: mesh.indices.length, type: "SCALAR" });
    document.materials.push({ name: mesh.name, pbrMetallicRoughness: { baseColorFactor: [...mesh.color, 1], metallicFactor: 0, roughnessFactor: 0.8 } });
    document.meshes.push({ name: mesh.name, primitives: [{ attributes: { POSITION: base, NORMAL: base + 1, TEXCOORD_0: base + 2 }, indices: base + 3, material: index }] });
    document.nodes.push({ name: mesh.name, mesh: index });
  });
  const binary = Buffer.concat(chunks);
  document.buffers = [{ byteLength: binary.length }];
  document.bufferViews = views; document.accessors = accessors;
  let json = Buffer.from(JSON.stringify(document));
  json = Buffer.concat([json, Buffer.alloc((4 - json.length % 4) % 4, 0x20)]);
  const header = Buffer.alloc(12);
  header.writeUInt32LE(0x46546c67, 0); header.writeUInt32LE(2, 4); header.writeUInt32LE(12 + 8 + json.length + 8 + binary.length, 8);
  const chunk = (type, data) => { const head = Buffer.alloc(8); head.writeUInt32LE(data.length, 0); head.writeUInt32LE(type, 4); return [head, data]; };
  return Buffer.concat([header, ...chunk(0x4e4f534a, json), ...chunk(0x004e4942, binary)]);
}
