import { readFile } from "node:fs/promises";
import { BoxGeometry, CylinderGeometry, ExtrudeGeometry, Matrix4, Shape, Vector3 } from "three";
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js";

function oxidizedSteel(sharp) {
  const width = 192,
    data = Buffer.alloc(width * width * 3);
  function hash(x, y) {
    let n = Math.imul(x, 374761393) + Math.imul(y, 668265263);
    n = Math.imul(n ^ (n >>> 13), 1274126177);
    return ((n ^ (n >>> 16)) >>> 0) / 4294967296;
  }
  function noise(x, y, steps) {
    const X = (x * steps) / width,
      Y = (y * steps) / width,
      i = Math.floor(X),
      j = Math.floor(Y),
      u = X - i,
      v = Y - j;
    const h = (a, b) => hash((i + a) % steps, (j + b) % steps);
    return (h(0, 0) * (1 - u) + h(1, 0) * u) * (1 - v) + (h(0, 1) * (1 - u) + h(1, 1) * u) * v;
  }
  for (let y = 0; y < width; y++)
    for (let x = 0; x < width; x++) {
      const t = 0.4 * noise(x, y, 6) + 0.38 * noise(x, y, 17) + 0.22 * hash(x, y);
      const wear = Math.max(0, Math.min(1, (t - 0.54) * 10)),
        grain = 13 * (hash(y, x) - 0.5);
      const i = (y * width + x) * 3;
      [0, 1, 2].forEach(
        (k) => (data[i + k] = [109, 118, 116][k] * (1 - wear) + [141, 103, 73][k] * wear + grain),
      );
    }
  return sharp(data, { raw: { width, height: width, channels: 3 } })
    .png()
    .toBuffer();
}

export async function buildMaintenance(stations, { Document, sharp }) {
  const doc = new Document(),
    buf = doc.createBuffer(),
    scene = doc.createScene("Stoa maintenance");
  const root = doc.createNode("AML metres").setRotation([-Math.SQRT1_2, 0, 0, Math.SQRT1_2]);
  scene.addChild(root);
  const atlas = doc
    .createTexture("Service stencil and docket atlas")
    .setMimeType("image/png")
    .setImage(await readFile(new URL("markings.png", import.meta.url)));
  const ink = doc
    .createMaterial("Weathered maintenance stencil")
    .setEmissiveTexture(atlas)
    .setEmissiveFactor([0.2, 0.205, 0.22])
    .setBaseColorTexture(atlas)
    .setAlphaMode("MASK")
    .setAlphaCutoff(0.39)
    .setMetallicFactor(0)
    .setRoughnessFactor(0.97)
    .setExtras({ worldSpecOutline: false, castsShadow: false });
  const paper = doc
    .createMaterial("Stamped inspection docket")
    .setEmissiveTexture(atlas)
    .setEmissiveFactor([0.17, 0.18, 0.19])
    .setBaseColorTexture(atlas)
    .setAlphaMode("MASK")
    .setAlphaCutoff(0.45)
    .setDoubleSided(true)
    .setMetallicFactor(0)
    .setRoughnessFactor(0.89)
    .setExtras({ worldSpecOutline: false, castsShadow: false });
  const metal = doc
    .createMaterial("Maintenance clamp weathered zinc")
    .setBaseColorTexture(
      doc
        .createTexture("Scuffed zinc with oxide freckles")
        .setImage(await oxidizedSteel(sharp))
        .setMimeType("image/png"),
    )
    .setMetallicFactor(0.48)
    .setRoughnessFactor(0.53)
    .setExtras({ worldSpecPipe: true });
  const hardware = doc
    .createMaterial("Clamp galvanized nuts and washers")
    .setBaseColorFactor([0.49, 0.48, 0.42, 1])
    .setMetallicFactor(0.72)
    .setRoughnessFactor(0.39)
    .setExtras({ worldSpecPipe: true });
  const gasket = doc
    .createMaterial("Split clamp rubber seat")
    .setBaseColorFactor([0.038, 0.043, 0.04, 1])
    .setMetallicFactor(0.02)
    .setRoughnessFactor(0.84);
  const vec = (p) => new Vector3(...p);
  const accessor = (type, array) =>
    doc.createAccessor().setBuffer(buf).setType(type).setArray(new Float32Array(array));
  for (const [variant, st] of stations.entries()) {
    const frame = new Matrix4()
      .makeBasis(vec(st.right), vec(st.normal).negate(), new Vector3(0, 0, 1))
      .setPosition(vec(st.center));
    const batches = new Map();
    function triangle(mat, points, ns, uv) {
      if (!batches.has(mat)) batches.set(mat, { pos: [], n: [], uv: [] });
      const b = batches.get(mat);
      for (let i = 0; i < 3; i++) {
        b.pos.push(...points[i]);
        b.n.push(...ns[i]);
        b.uv.push(...uv[i]);
      }
    }
    function solid(mat, g, x = 0, y = 0, z = 0) {
      g.translate(x, y, z);
      const p = g.attributes.position,
        n = g.attributes.normal;
      for (let i = 0; i < (g.index?.count ?? p.count); i += 3) {
        const points = [],
          normals = [],
          uv = [];
        for (let k = 0; k < 3; k++) {
          const j = g.index ? g.index.getX(i + k) : i + k;
          const point = new Vector3().fromBufferAttribute(p, j),
            normal = new Vector3().fromBufferAttribute(n, j);
          uv.push(
            Math.abs(normal.z) > 0.6
              ? [point.x * 3, point.y * 3]
              : [Math.abs(normal.x) > 0.6 ? point.y * 3 : point.x * 3, point.z * 3],
          );
          points.push(point.applyMatrix4(frame).toArray());
          normals.push(normal.transformDirection(frame).toArray());
        }
        triangle(mat, points, normals, uv);
      }
      g.dispose();
    }
    function arc(mat, inner, outer, start, end, height) {
      const section = new Shape();
      section.absarc(0, 0, outer, start, end, false);
      section.absarc(0, 0, inner, end, start, true);
      section.closePath();
      const g = new ExtrudeGeometry(section, {
        depth: height,
        bevelEnabled: true,
        bevelSegments: 1,
        steps: 1,
        bevelSize: 0.001,
        bevelThickness: 0.001,
        curveSegments: 16,
      });
      solid(mat, g, 0, 0, -height / 2);
    }
    const r = st.radius;
    for (const turn of [0, Math.PI]) {
      arc(gasket, r - 0.0006, r + 0.003, turn + 0.065, turn + Math.PI - 0.065, 0.088);
      arc(metal, r + 0.002, r + 0.01, turn + 0.08, turn + Math.PI - 0.08, 0.073);
    }
    const lug = r + 0.024;
    for (const sign of [-1, 1]) {
      for (const y of [-0.011, 0.011])
        solid(metal, new RoundedBoxGeometry(0.047, 0.015, 0.046, 1, 0.003), sign * lug, y, 0);
      solid(hardware, new CylinderGeometry(0.009, 0.009, 0.06, 10), sign * lug, -0.002, 0);
      solid(hardware, new CylinderGeometry(0.017, 0.017, 0.004, 16), sign * lug, -0.02, 0);
      solid(hardware, new CylinderGeometry(0.014, 0.014, 0.012, 6), sign * lug, -0.028, 0);
      solid(hardware, new CylinderGeometry(0.013, 0.013, 0.01, 6), sign * lug, 0.024, 0);
    }
    solid(metal, new BoxGeometry(0.029, st.reach - r - 0.005, 0.042), 0, (st.reach + r) / 2, 0);
    solid(metal, new RoundedBoxGeometry(0.13, 0.007, 0.075, 1, 0.003), 0, st.reach - 0.0035, 0);
    for (const x of [-0.046, 0.046])
      solid(hardware, new CylinderGeometry(0.009, 0.009, 0.006, 6), x, st.reach - 0.009, 0);

    function mark(mat, width, height, level, paperTag) {
      const pos = vec(st.ink);
      pos.z += level;
      const right = vec(st.right),
        out = vec(st.normal),
        up = new Vector3(0, 0, 1),
        grid = paperTag ? 5 : 1;
      const point = (u, v) => {
        let x = (u - 0.5) * width,
          z = (0.5 - v) * height;
        const tilt = (variant ? -0.045 : 0.056) * (paperTag ? 1 : 0.13);
        const curl = paperTag ? 0.01 * Math.pow(Math.max(0, (u - 0.55) / 0.45), 2) * Math.pow(v, 3) : 0;
        return pos
          .clone()
          .addScaledVector(right, x - z * tilt)
          .addScaledVector(up, z + x * tilt)
          .addScaledVector(out, 0.0022 + curl)
          .toArray();
      };
      for (let j = 0; j < grid; j++)
        for (let i = 0; i < grid; i++) {
          const a = [i / grid, j / grid],
            b = [(i + 1) / grid, j / grid],
            c = [(i + 1) / grid, (j + 1) / grid],
            d = [i / grid, (j + 1) / grid];
          for (const coords of [
            [a, d, c],
            [a, c, b],
          ]) {
            const pts = coords.map(([u, v]) => point(u, v));
            const n = vec(pts[1])
              .sub(vec(pts[0]))
              .cross(vec(pts[2]).sub(vec(pts[0])))
              .normalize()
              .toArray();
            triangle(
              mat,
              pts,
              [n, n, n],
              coords.map(([u, v]) => [(variant + u) / 2, ((paperTag ? 1 : 0) + v) / 2]),
            );
          }
        }
    }
    mark(ink, 0.68, 0.34, 0, false);
    mark(paper, 0.26, 0.15, -0.225, true);
    for (const [mat, b] of batches) {
      const p = doc
        .createPrimitive()
        .setMaterial(mat)
        .setAttribute("POSITION", accessor("VEC3", b.pos))
        .setAttribute("NORMAL", accessor("VEC3", b.n));
      if (mat.getBaseColorTexture()) p.setAttribute("TEXCOORD_0", accessor("VEC2", b.uv));
      const name = st.pipe + "/maintenance/" + mat.getName();
      root.addChild(doc.createNode(name).setMesh(doc.createMesh(name).addPrimitive(p)));
    }
  }
  return doc;
}
