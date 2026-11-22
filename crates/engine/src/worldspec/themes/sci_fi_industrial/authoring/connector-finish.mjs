import { fileURLToPath } from "node:url";
import { Vector3 } from "three";

const catalog = new URL("../../../../../../../assets/materials/", import.meta.url);
const cable = "Power cable | orange jacket";
const rubber = new Set(["Strain relief | five convolutions", cable]);
const octagons = new Set(["Housing | armored casting", "Cable clamp | broad octagonal base", "Cable clamp | stepped octagonal cap"]);
// Linear RGB and blend weight for the soft blue fill on the shaded side.
const connectorShade = [0.94, 0.96, 1.52, 0.60];
const polishedRings = new Map([
  ["Cable gland | compression ring", { roughness: 0.40, intensity: 0.70, tint: [0.91, 1.01, 1.10] }],
  ["Coupling | receiver-side retaining band", { roughness: 0.34, metalness: 0.85, intensity: 1.15, broadReflectionBands: true, tint: [0.95, 1.02, 1.08] }],
  ["Socket | flared receiver", { roughness: 0.36, metalness: 0.80, intensity: 1.00, broadReflectionBands: true, tint: [0.96, 1.01, 1.05] }],
]);
const socketWhite = new Map([
  ["Socket | ivory casting", [0.83, 0.84, 0.85, 1]],
  ["Socket | warm porcelain enamel", [0.88, 0.89, 0.90, 1]],
]);

/** Use the brushed hardware and molded stool maps as fine surface detail. */
export async function connectorFinishes(doc, sharp) {
  const finish = async (name, prefix, crop) => {
    const source = (suffix = "") => {
      const image = sharp(fileURLToPath(new URL(prefix + suffix + ".png", catalog)));
      return crop ? image.extract(crop) : image;
    };
    const texture = (channel, image) => doc.createTexture(name + " " + channel).setMimeType("image/png").setImage(image);
    const { data, info } = await source().greyscale().raw().toBuffer({ resolveWithObject: true });
    const raw = { width: info.width, height: info.height, channels: 1 };
    const soft = await sharp(data, { raw }).blur(4).greyscale().raw().toBuffer();
    const rgb = { ...raw, channels: 3 };
    const pigment = Buffer.from(data.map((value, i) => Math.min(255, 248 + 0.32 * (value - soft[i]))));
    const [color, normal, roughness] = await Promise.all([
      sharp(pigment, { raw }).png().toBuffer(),
      source("-normal").png().toBuffer(),
      source("-properties").extractChannel("green").raw().toBuffer(),
    ]);
    const packed = Buffer.alloc(roughness.length * 3, 255);
    for (let i = 0; i < roughness.length; i++) packed[i * 3 + 1] = 204 + 0.2 * roughness[i];
    const properties = await sharp(packed, { raw: rgb }).png().toBuffer();
    return { color: texture("grain", color), normal: texture("normal", normal), properties: texture("roughness", properties) };
  };
  const [metal, elastomer] = await Promise.all([
    finish("Clean brushed metal", "brushed-metal"),
    finish("Molded rubber", "market-hero-props", { left: 516, top: 4, width: 504, height: 504 }),
  ]);
  metal.edge = doc.createTexture("Octagon bevel mask").setMimeType("image/png").setImage(await sharp(
    Buffer.from([255, 0, 0, 255, 255, 0]), { raw: { width: 2, height: 1, channels: 3 } },
  ).png().toBuffer());
  return { metal, elastomer };
}

// Each octagonal loft face is a convex polygon. Recover its physical boundary
// from the unwelded triangles so both halves of a quad see the same four edges.
function creaseDistances(position) {
  const planes = new Map(), adjacentNormals = new Map(), triangles = [];
  const key = values => values.map(v => Math.round(v * 100000)).join(",");
  for (let i = 0; i < position.getCount(); i += 3) {
    const points = [0, 1, 2].map(j => new Vector3().fromArray(position.getElement(i + j, [])));
    const [a,b,c] = points;
    const normal = b.clone().sub(a).cross(c.clone().sub(a)).normalize();
    const plane = key([...normal.toArray(), normal.dot(a)]);
    if (!planes.has(plane)) planes.set(plane, new Map());
    const edges = planes.get(plane);
    for (let j = 0; j < 3; j++) {
      const start = points[j], end = points[(j + 1) % 3];
      const id = [key(start.toArray()), key(end.toArray())].sort().join("|");
      if (!adjacentNormals.has(id)) adjacentNormals.set(id, []);
      adjacentNormals.get(id).push(normal);
      if (edges.has(id)) edges.delete(id);
      else edges.set(id, { start, inward: normal.clone().cross(end.clone().sub(start)).normalize() });
    }
    triangles.push({ points, edges });
  }
  // Slightly warped quad faces can triangulate into two different planes.
  // A shallow change across that diagonal is not a machined crease.
  const creaseCosine = Math.cos(8 * Math.PI / 180);
  for (const edges of planes.values()) {
    for (const id of edges.keys()) {
      const adjacent = adjacentNormals.get(id);
      if (adjacent.length === 2 && adjacent[0].dot(adjacent[1]) > creaseCosine) edges.delete(id);
    }
  }
  const values = new Float32Array(position.getCount() * 4).fill(1);
  triangles.forEach(({ points, edges }, triangle) => {
    const center = points.reduce((sum, p) => sum.add(p), new Vector3()).multiplyScalar(1/3);
    const distance = (edge, p) => edge.inward.dot(p) - edge.inward.dot(edge.start);
    const nearest = [...edges.values()].sort((a,b) => Math.abs(distance(a, center)) - Math.abs(distance(b, center))).slice(0,4);
    points.forEach((p, j) => nearest.forEach((e, k) => { values[(3*triangle+j)*4+k] = distance(e,p); }));
  });
  return values;
}

export function finishConnector(asset, maps, { brightenPlug = false } = {}) {
  const metalMaterials = new Map(), rubberMaterials = new Map();
  const octagonFinish = asset.createMaterial("Bright gray | polished octagons")
    .setBaseColorFactor([0.59, 0.60, 0.61, 1]).setMetallicFactor(0.82).setRoughnessFactor(0.48)
    .setBaseColorTexture(maps.metal.color).setNormalTexture(maps.metal.normal).setNormalScale(0.16)
    .setMetallicRoughnessTexture(maps.metal.properties).setOcclusionTexture(maps.metal.edge).setOcclusionStrength(0)
    .setExtras({ materialSpecEdge: { from: 0.9, to: 0.2, gain: 1.7, polish: 0.31, width: 0.004, creases: true } });
  octagonFinish.getOcclusionTextureInfo().setTexCoord(1);
  for (const mesh of asset.getRoot().listMeshes()) {
    const ring = polishedRings.get(mesh.getName());
    for (const primitive of mesh.listPrimitives()) {
      const original = primitive.getMaterial();
      const isOctagon = octagons.has(mesh.getName());
      const isRubber = rubber.has(mesh.getName());
      const materials = isRubber ? rubberMaterials : metalMaterials;
      const { color, normal, properties } = isRubber ? maps.elastomer : maps.metal;
      if (!materials.has(original)) {
        const finish = original.clone().setBaseColorTexture(color).setNormalTexture(normal)
          .setMetallicRoughnessTexture(properties).setNormalScale(isRubber ? 0.09 : 0.22)
          .setRoughnessFactor(isRubber ? 0.36 : Math.min(0.60, Math.max(0.39, original.getRoughnessFactor() + 0.07)))
          .setMetallicFactor(isRubber ? 0 : Math.max(0.52, original.getMetallicFactor()));
        const white = socketWhite.get(original.getName());
        if (white) finish.setBaseColorFactor(white);
        materials.set(original, finish);
      }
      let finish = isOctagon ? octagonFinish : materials.get(original);
      if (ring) {
        const { roughness, metalness, intensity, tint: [red, green, blue] } = ring;
        const arguments_ = Object.fromEntries(Object.entries({ intensity, red, green, blue })
          .map(([key, value]) => [key, { value }]));
        finish = finish.clone().setRoughnessFactor(roughness).setMetallicFactor(metalness ?? finish.getMetallicFactor()).setNormalScale(0.06)
          .setExtras({ materialSpec: { broadReflectionBands: ring.broadReflectionBands,
            live: [{ id: "virtualSky", arguments: arguments_ }] } });
      }
      finish.setExtras({ ...finish.getExtras(), worldSpecShadeColor: connectorShade,
        worldSpecPaintedReflection: isOctagon || !!ring, worldSpecSpecularBands: mesh.getName() === cable,
        worldSpecPaintedDiffuse: isRubber });
      primitive.setMaterial(finish);

      // Project each triangle in the asset's local space at a fixed physical scale.
      const position = primitive.getAttribute("POSITION"), normals = primitive.getAttribute("NORMAL");
      const uv = new Float32Array(position.getCount() * 2);
      const edge = isOctagon ? new Float32Array(uv.length) : undefined;
      const minX = isOctagon ? position.getMin([])[0] : 0;
      const maxX = isOctagon ? position.getMax([])[0] : 0;
      const wrapBrush = ring || mesh.getName() === "Strain relief | five convolutions";
      const repeats = isRubber ? 7 : 5;
      const turn = 2 * Math.PI;
      const repeatsAround = wrapBrush ? Math.max(1, Math.round(turn * position.getMax([])[1] * repeats)) : 0;
      for (let i = 0; i < position.getCount(); i += 3) {
        const n = normals.getElement(i, []).map((v, axis) => v + normals.getElement(i + 1, [])[axis] + normals.getElement(i + 2, [])[axis]);
        const axis = n.map(Math.abs).indexOf(Math.max(...n.map(Math.abs)));
        const u = (axis + 1) % 3, v = (axis + 2) % 3;
        const first = position.getElement(i, []);
        const startAngle = wrapBrush ? Math.atan2(first[2], first[1]) : 0;
        for (let j = i; j < i + 3; j++) {
          const point = position.getElement(j, []);
          if (wrapBrush) {
            const angle = Math.atan2(point[2], point[1]);
            const continuous = angle - turn * Math.round((angle - startAngle) / turn);
            uv[2 * j] = continuous * repeatsAround / turn;
            uv[2 * j + 1] = point[0] * repeats;
          } else {
            uv[2 * j] = point[u] * repeats;
            uv[2 * j + 1] = point[v] * repeats * (n[axis] < 0 ? -1 : 1);
          }
          if (edge) {
            edge[2 * j] = point[0] - minX;
            edge[2 * j + 1] = maxX - point[0];
          }
        }
      }
      primitive.setAttribute("TEXCOORD_0", asset.createAccessor().setType("VEC2").setArray(uv).setBuffer(position.getBuffer()));
      if (edge) primitive.setAttribute("TEXCOORD_1", asset.createAccessor().setType("VEC2").setArray(edge).setBuffer(position.getBuffer()));
      if (edge) primitive.setAttribute("_MS_CREASE", asset.createAccessor().setType("VEC4")
        .setArray(creaseDistances(position)).setBuffer(position.getBuffer()));
    }
  }
  if (brightenPlug) {
    const materials = new Set(asset.getRoot().listMeshes()
      .flatMap(mesh => mesh.listPrimitives().map(primitive => primitive.getMaterial())));
    for (const material of materials) {
      const [r, g, b, alpha] = material.getBaseColorFactor();
      // Lift dark and middle values gently and roll off toward white.
      const lift = value => value + (1 - value) * (0.004 + 0.19 * value);
      material.setBaseColorFactor([lift(r), lift(g), lift(b), alpha]);
    }
  }
}
