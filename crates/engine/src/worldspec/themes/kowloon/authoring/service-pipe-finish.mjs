import { Box3, Ray, Triangle, Vector3 } from "three";
import { finishPipeSteel } from "./service-pipe-steel.mjs";

const reach = 0.14; // meters, just beyond one pipe diameter
const ray = new Ray(),
  hit = new Vector3(),
  end = new Vector3();
const tangent = new Vector3(),
  bitangent = new Vector3(),
  up = new Vector3(0, 0, 1);
const directions = Array.from({ length: 48 }, (_, i) => {
  const radius = Math.sqrt((i + 0.5) / 48),
    angle = i * Math.PI * (3 - Math.sqrt(5));
  return [radius * Math.cos(angle), radius * Math.sin(angle), Math.sqrt(1 - radius * radius)];
});

function triangleTree(faces) {
  const bounds = new Box3();
  for (const face of faces) for (const p of [face.a, face.b, face.c]) bounds.expandByPoint(p);
  if (faces.length <= 12) return { bounds, faces };
  const size = bounds.getSize(new Vector3());
  const axis = size.x > size.y && size.x > size.z ? "x" : size.y > size.z ? "y" : "z";
  faces.sort((a, b) => a.a[axis] + a.b[axis] + a.c[axis] - b.a[axis] - b.b[axis] - b.c[axis]);
  const middle = faces.length >> 1;
  return { bounds, left: triangleTree(faces.slice(0, middle)), right: triangleTree(faces.slice(middle)) };
}

function blocked(tree) {
  if (
    !tree.bounds.containsPoint(ray.origin) &&
    (!ray.intersectBox(tree.bounds, end) || ray.origin.distanceToSquared(end) > reach * reach)
  )
    return false;
  if (!tree.faces) return blocked(tree.left) || blocked(tree.right);
  return tree.faces.some(
    (t) =>
      ray.intersectTriangle(t.a, t.b, t.c, false, hit) && hit.distanceToSquared(ray.origin) <= reach * reach,
  );
}

function visibility(position, normal, tree) {
  if (tree.bounds.distanceToPoint(position) > reach) return 255;
  tangent.set(1, 0, 0);
  if (Math.abs(normal.z) < 0.85) tangent.copy(up);
  tangent.cross(normal).normalize();
  bitangent.crossVectors(normal, tangent);
  ray.origin.copy(position).addScaledVector(normal, 0.00065);
  let clear = 0;
  for (const [x, y, z] of directions) {
    ray.direction.copy(normal).multiplyScalar(z).addScaledVector(tangent, x).addScaledVector(bitangent, y);
    if (!blocked(tree)) clear++;
  }
  // Deepen contact in the shadowed arcade while a fully open hemisphere stays white.
  return Math.round(255 * Math.max(0.13, Math.pow(clear / directions.length, 2.2)));
}

function facesOf(primitive) {
  const position = primitive.getAttribute("POSITION"),
    index = primitive.getIndices();
  const points = Array.from({ length: position.getCount() }, (_, i) =>
    new Vector3().fromArray(position.getElement(i, [])),
  );
  const pointsAt = (i) => points[index ? index.getScalar(i) : i];
  const faces = [];
  for (let i = 0; i < (index?.getCount() ?? points.length); i += 3)
    faces.push(new Triangle(pointsAt(i), pointsAt(i + 1), pointsAt(i + 2)));
  return faces;
}

// The pipe builder supplies one metric UV ring at each station, including the two
// copies of its angular seam. Interpolating adjacent rings follows the actual elbows.
function pipeRings(primitive) {
  const uv = primitive.getAttribute("TEXCOORD_0"),
    pos = primitive.getAttribute("POSITION");
  const byStation = new Map();
  for (let i = 0; i < uv.getCount(); i++) {
    const [u, v] = uv.getElement(i, []),
      station = 1 - v;
    if (!byStation.has(station)) byStation.set(station, new Map());
    byStation.get(station).set(u, new Vector3().fromArray(pos.getElement(i, [])));
  }
  return [...byStation]
    .sort(([a], [b]) => a - b)
    .map(([station, ring]) => {
      const points = [...ring].sort(([a], [b]) => a - b).map(([, p]) => p),
        center = new Vector3();
      for (const p of points.slice(0, -1)) center.add(p);
      center.divideScalar(points.length - 1);
      return { station, points, center };
    });
}

async function grayTexture(doc, sharp, name, pixels, width, height) {
  const image = await sharp(pixels, { raw: { width, height, channels: 1 } })
    .png()
    .toBuffer();
  return doc.createTexture(name).setMimeType("image/png").setImage(image);
}

function assignPipeMaterial(primitive, name, paintedBarrel = false) {
  const source = primitive.getMaterial();
  const material = source
    .clone()
    .setName(name)
    .setExtras({
      ...source.getExtras(),
      worldSpecPipe: true,
      worldSpecSpecularBands: 0.65,
      ...(paintedBarrel ? { worldSpecPaintedDiffuse: 0.72, worldSpecSatin: 0.30 } : {}),
    });
  primitive.setMaterial(material);
  return material;
}

function bind(doc, TextureInfo, primitive, texture, coordinates, name, paintedBarrel = false) {
  const material = assignPipeMaterial(primitive, name, paintedBarrel).setOcclusionTexture(texture).setOcclusionStrength(1);
  material.getOcclusionTextureInfo().setTexCoord(1).setWrapS(TextureInfo.WrapMode.CLAMP_TO_EDGE);
  const uv1 = doc
    .createAccessor()
    .setBuffer(primitive.getAttribute("POSITION").getBuffer())
    .setType("VEC2")
    .setArray(coordinates);
  primitive.setAttribute("TEXCOORD_1", uv1);
}

async function finishSkin(doc, primitive, hardware, { sharp, TextureInfo }) {
  const rings = pipeRings(primitive),
    last = rings.at(-1),
    length = last.station,
    sides = last.points.length - 1;
  const width = Math.min(4096, Math.ceil(length * 200)),
    height = sides * 2;
  const pixels = Buffer.alloc(width * height, 255),
    normal = new Vector3(),
    point = new Vector3();
  const alongA = new Vector3(),
    alongB = new Vector3(),
    center = new Vector3();
  let segment = 0;
  for (let x = 0; x < width; x++) {
    const s = ((x + 0.5) * length) / width;
    while (segment < rings.length - 2 && s > rings[segment + 1].station) segment++;
    const a = rings[segment],
      b = rings[segment + 1],
      t = (s - a.station) / (b.station - a.station);
    center.lerpVectors(a.center, b.center, t);
    for (let y = 0; y < height; y++) {
      const u = ((y + 0.5) * sides) / height,
        k = Math.floor(u),
        f = u - k;
      alongA.lerpVectors(a.points[k], a.points[k + 1], f);
      alongB.lerpVectors(b.points[k], b.points[k + 1], f);
      point.lerpVectors(alongA, alongB, t);
      normal.subVectors(point, center).normalize();
      pixels[y * width + x] = visibility(point, normal, hardware);
    }
  }
  const uv = primitive.getAttribute("TEXCOORD_0"),
    perimeter = uv.getMax([])[0],
    coords = new Float32Array(uv.getCount() * 2);
  for (let i = 0; i < uv.getCount(); i++) {
    const [u, v] = uv.getElement(i, []);
    coords.set([(1 - v) / length, u / perimeter], i * 2);
  }
  bind(
    doc,
    TextureInfo,
    primitive,
    await grayTexture(doc, sharp, "Round pipe contact AO", pixels, width, height),
    coords,
    "Service pipe barrel",
    true,
  );
  // A deeply occluded ring locates the actual sleeve or clamp seat. Keep a
  // single station per continuous seat, including one that ends at the cap.
  const seats = [];
  let start = -1;
  for (let x = 0; x <= width; x++) {
    let sum = 0;
    for (let y = 0; y < height && x < width; y++) sum += pixels[y * width + x];
    const seated = x < width && sum / height < 145;
    if (seated && start < 0) start = x;
    if (!seated && start >= 0) {
      seats.push((start + x) * length / (2 * width));
      start = -1;
    }
  }
  return seats;
}

export async function finishServicePipes(doc, adapters, { bakeWeathering = true } = {}) {
  const { sharp, TextureInfo } = adapters;
  const nodes = doc
    .getRoot()
    .listNodes()
    .filter((n) => n.getMesh());
  const bodies = nodes.filter(
    (n) =>
      n.getName().endsWith("/part/run/run/body") &&
      n
        .getMesh()
        .listPrimitives()
        .some((p) => p.getMaterial()?.getName() === "builtin:wall-rust"),
  );
  if (!bodies.length) return;
  let ramp;
  const seatsByBody = new Map();
  for (const body of bodies) {
    const prefix = body.getName().slice(0, -4);
    const hardware = nodes
      .filter((n) => n.getName().startsWith(prefix) && /\/(bracket|fitting|bolt)$/.test(n.getName()))
      .flatMap((n) => n.getMesh().listPrimitives());
    const barrel = body.getMesh().listPrimitives()[0];
    const end = nodes.find((n) => n.getName() === prefix + "end");
    for (const cap of end?.getMesh().listPrimitives() ?? []) assignPipeMaterial(cap, "Service pipe end");
    if (!bakeWeathering) {
      // The bench already has an authored rust atlas. Keep its maps and UVs.
      assignPipeMaterial(barrel, "Service pipe barrel", true);
      for (const part of hardware) assignPipeMaterial(part, "Service pipe hardware");
      continue;
    }
    if (!hardware.length) {
      assignPipeMaterial(barrel, "Service pipe barrel", true);
      continue;
    }
    ramp ??= await grayTexture(
      doc,
      sharp,
      "Pipe fitting ambient visibility",
      Buffer.from(Array.from({ length: 256 }, (_, i) => i)),
      256,
      1,
    );
    const triangles = hardware.flatMap(facesOf);
    seatsByBody.set(body, await finishSkin(doc, barrel, triangleTree(triangles), adapters));
    const receivers = triangleTree([...triangles, ...facesOf(barrel)]);
    for (const part of hardware) {
      const positions = part.getAttribute("POSITION"),
        normals = part.getAttribute("NORMAL");
      const coords = new Float32Array(positions.getCount() * 2),
        pos = new Vector3(),
        n = new Vector3();
      for (let i = 0; i < positions.getCount(); i++) {
        pos.fromArray(positions.getElement(i, []));
        n.fromArray(normals.getElement(i, [])).normalize();
        coords.set([(visibility(pos, n, receivers) + 0.5) / 256, 0.5], i * 2);
      }
      bind(doc, TextureInfo, part, ramp, coords, "Service pipe hardware");
    }
  }
  if (bakeWeathering) await finishPipeSteel(doc, bodies, nodes, seatsByBody, adapters);
}
