import { Vector3 } from "three";

// One outlet per long horizontal run. The wall-side vertical runs and curved
// elbows have no eligible rectangular flank. Work in the native Z-up frame.
export function registerFaces(primitive, minLength = 2.5) {
  const positions = primitive.getAttribute("POSITION"),
    normal = primitive.getAttribute("NORMAL");
  const indices = primitive.getIndices(),
    groups = new Map();
  for (let i = 0; i < (indices?.getCount() ?? positions.getCount()); i += 3) {
    const ids = [0, 1, 2].map((j) => (indices ? indices.getScalar(i + j) : i + j));
    const n = normal.getElement(ids[0], []);
    const side = n.findIndex((v, axis) => axis < 2 && v > 0.9999);
    if (side < 0) continue;
    const axis = 1 - side,
      points = ids.map((id) => positions.getElement(id, []));
    const lo = Math.min(...points.map((p) => p[axis])),
      hi = Math.max(...points.map((p) => p[axis]));
    const low = Math.min(...points.map((p) => p[2])),
      high = Math.max(...points.map((p) => p[2]));
    if (
      hi - lo < minLength ||
      high - low < 0.13 ||
      high - low > 1 ||
      points.some((p) => Math.abs(p[side] - points[0][side]) > 0.0001)
    )
      continue;
    const plane = points[0][side],
      key = [side, plane, lo, hi, low, high].join("/");
    if (!groups.has(key))
      groups.set(key, { axis, side, plane, lo, hi, low, high, triangles: [], corners: new Map() });
    const face = groups.get(key);
    face.triangles.push(i);
    for (const [j, p] of points.entries())
      face.corners.set(Number(p[axis] === hi) + 2 * Number(p[2] === high), ids[j]);
  }
  return [...groups.values()]
    .filter((face) => face.triangles.length === 2 && face.corners.size === 4)
    .sort((a, b) => b.hi - b.lo - (a.hi - a.lo));
}

export function registerStation(doc, body, face, width, fraction = 0.82) {
  const { lo, hi, axis, side, plane, low, high } = face,
    obstacles = [];
  const prefix = body.getName().slice(0, -4);
  for (const node of doc.getRoot().listNodes()) {
    if (!node.getName().startsWith(prefix) || node === body || !node.getMesh()) continue;
    for (const prim of node.getMesh().listPrimitives()) {
      const positions = prim.getAttribute("POSITION");
      for (let i = 0; i < positions.getCount(); i++) {
        const p = positions.getElement(i, []);
        if (Math.abs(p[side] - plane) < 0.25 && p[2] > low - 0.04 && p[2] < high + 0.04)
          obstacles.push(p[axis]);
      }
    }
  }
  const target = lo + fraction * (hi - lo),
    clearance = width / 2 + 0.055;
  for (let d = 0; d < hi - lo; d += 0.05)
    for (const station of [target - d, target + d]) {
      if (
        station - lo > clearance &&
        hi - station > clearance &&
        obstacles.every((v) => Math.abs(v - station) > clearance)
      )
        return station;
    }
}

// Replace the original two sheet triangles with four strips around a true
// opening. Interpolate each existing attribute so the adjacent sheet texture and
// shader inputs still follow exactly the same metric mapping across the sheet.
export function openSheet(doc, mesh, skin, face, left, right, bottom, top) {
  const attributes = Object.fromEntries(skin.listSemantics().map((s) => [s, []]));
  const retainedIndices = [],
    old = skin.getIndices(),
    indexCount = old?.getCount() ?? skin.getAttribute("POSITION").getCount();
  for (let i = 0; i < indexCount; i++)
    if (!face.triangles.includes(i - (i % 3))) retainedIndices.push(old ? old.getScalar(i) : i);
  const buffer = skin.getAttribute("POSITION").getBuffer();
  skin.setIndices(
    doc.createAccessor().setBuffer(buffer).setType("SCALAR").setArray(Uint32Array.from(retainedIndices)),
  );
  for (const [l, r, b, t] of [
    [face.lo, left, face.low, face.high],
    [right, face.hi, face.low, face.high],
    [left, right, face.low, bottom],
    [left, right, top, face.high],
  ]) {
    const corners = [
      [l, b],
      [r, b],
      [r, t],
      [l, t],
    ];
    for (const k of face.axis === 1 ? [0, 1, 2, 0, 2, 3] : [0, 2, 1, 0, 3, 2]) {
      const [x, z] = corners[k],
        u = (x - face.lo) / (face.hi - face.lo),
        v = (z - face.low) / (face.high - face.low);
      for (const [key, values] of Object.entries(attributes)) {
        const a = skin.getAttribute(key),
          weights = [(1 - u) * (1 - v), u * (1 - v), (1 - u) * v, u * v];
        const data = weights.map((_, j) => a.getElement(face.corners.get(j), []));
        values.push(...data[0].map((_, c) => weights.reduce((s, w, j) => s + w * data[j][c], 0)));
      }
    }
  }
  const patch = doc.createPrimitive().setMaterial(skin.getMaterial());
  for (const [key, values] of Object.entries(attributes))
    patch.setAttribute(
      key,
      doc
        .createAccessor()
        .setBuffer(buffer)
        .setType(skin.getAttribute(key).getType())
        .setArray(Float32Array.from(values)),
    );
  mesh.addPrimitive(patch);
}

export function addDuctRegister(doc, body, skin) {
  const face = registerFaces(skin)[0];
  if (!face) return null;
  const width = 0.56,
    height = Math.min(0.19, face.high - face.low - 0.022);
  const station = registerStation(doc, body, face, width);
  if (station === undefined) return null;
  const centerHeight = (face.low + face.high) / 2,
    openingHalfWidth = width / 2 - 0.016,
    openingHalfHeight = height / 2 - 0.016;
  const mesh = body.getMesh();
  openSheet(
    doc,
    mesh,
    skin,
    face,
    station - openingHalfWidth,
    station + openingHalfWidth,
    centerHeight - openingHalfHeight,
    centerHeight + openingHalfHeight,
  );
  buildRegisterGrille(doc, mesh, skin.getAttribute("POSITION").getBuffer(), width, height, ([u, v, d]) => {
    const p = [0, 0, centerHeight + v];
    p[face.axis] = station + u;
    p[face.side] = face.plane + d;
    return new Vector3(...p);
  });
  const normal = [0, 0, 0],
    min = [0, 0, face.low],
    max = [0, 0, face.high];
  normal[face.side] = 1;
  min[face.axis] = station - width / 2;
  max[face.axis] = station + width / 2;
  min[face.side] = face.plane - 0.042;
  max[face.side] = face.plane + 0.012;
  return { normal, min, max };
}

export function buildRegisterGrille(doc, mesh, buffer, width, height, toWorld) {
  const openingHalfWidth = width / 2 - 0.016,
    openingHalfHeight = height / 2 - 0.016;
  const parts = new Map();
  // The turned steel border and angled aluminium blades follow the construction
  // of a double-deflection supply grille. Titus 272R submittal, 2013.
  function quad(part, coords) {
    if (!parts.has(part)) parts.set(part, { positions: [], normals: [] });
    const data = parts.get(part);
    const points = coords.map(toWorld);
    const n = points[1].clone().sub(points[0]).cross(points[2].clone().sub(points[0])).normalize();
    for (const k of [0, 1, 2, 0, 2, 3]) {
      data.positions.push(...points[k].toArray());
      data.normals.push(...n.toArray());
    }
  }
  const loop = (u, v, d) => [
    [-u, -v, d],
    [u, -v, d],
    [u, v, d],
    [-u, v, d],
  ];
  const ring = (part, a, b) => {
    for (let i = 0; i < 4; i++) {
      const j = (i + 1) % 4;
      quad(part, [a[i], a[j], b[j], b[i]]);
    }
  };
  ring("Border", loop(width / 2, height / 2, 0.001), loop(width / 2 - 0.003, height / 2 - 0.003, 0.012));
  ring(
    "Border",
    loop(width / 2 - 0.003, height / 2 - 0.003, 0.012),
    loop(openingHalfWidth, openingHalfHeight, 0.008),
  );
  ring(
    "Throat",
    loop(openingHalfWidth, openingHalfHeight, 0.008),
    loop(openingHalfWidth, openingHalfHeight, -0.042),
  );
  quad("Throat", loop(openingHalfWidth, openingHalfHeight, -0.042));
  // Shallow four-sided blade sections show an illuminated leading edge, a
  // tilted vane, and the recessed return bank through the spaces between.
  for (let y = -openingHalfHeight + 0.021; y < openingHalfHeight - 0.005; y += 0.03) {
    quad("Rear vanes", [
      [-openingHalfWidth, y, -0.033],
      [openingHalfWidth, y, -0.033],
      [openingHalfWidth, y + 0.008, -0.018],
      [-openingHalfWidth, y + 0.008, -0.018],
    ]);
  }
  const count = Math.floor((2 * openingHalfWidth) / 0.01905);
  for (let i = 0; i < count; i++) {
    const x = -openingHalfWidth + (2 * openingHalfWidth * (i + 0.5)) / count;
    const profile = [
      [x - 0.006, -0.018],
      [x + 0.002, 0.004],
      [x + 0.005, 0.005],
      [x + 0.005, -0.003],
    ];
    for (let j = 0; j < 4; j++) {
      const [a, ad] = profile[j],
        [b, bd] = profile[(j + 1) % 4];
      quad("Blades", [
        [a, -openingHalfHeight, ad],
        [b, -openingHalfHeight, bd],
        [b, openingHalfHeight, bd],
        [a, openingHalfHeight, ad],
      ]);
    }
  }
  const finishes = {
    Border: [[0.55, 0.56, 0.57], 0.45, 0.87],
    Blades: [[0.44, 0.45, 0.46], 0.5, 0.83],
    "Rear vanes": [[0.16, 0.17, 0.18], 0.59, 0.72],
    Throat: [[0.055, 0.063, 0.061], 0.84, 0.12],
  };
  for (const [name, { positions, normals }] of parts) {
    const [rgb, rough, metal] = finishes[name];
    const material =
      doc
        .getRoot()
        .listMaterials()
        .find((m) => m.getName() === "Duct register " + name) ??
      doc
        .createMaterial("Duct register " + name)
        .setDoubleSided(true)
        .setBaseColorFactor([...rgb, 1])
        .setRoughnessFactor(rough)
        .setMetallicFactor(metal)
        .setExtras({ worldSpecDuct: true, worldSpecSpecularBands: 0.65 });
    const attrib = (array) =>
      doc.createAccessor().setBuffer(buffer).setType("VEC3").setArray(Float32Array.from(array));
    mesh.addPrimitive(
      doc
        .createPrimitive()
        .setMaterial(material)
        .setAttribute("POSITION", attrib(positions))
        .setAttribute("NORMAL", attrib(normals)),
    );
  }
}
