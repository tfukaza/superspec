import { Vector3 } from "three";

const pitch = 0.3048; // transverse beading on 12-inch centers
const margin = 0.014;
const vec = (accessor, i) => new Vector3().fromArray(accessor.getElement(i, []));

export function ductPanelFaces(primitive, metric) {
  const indices = primitive.getIndices().getArray();
  const positions = primitive.getAttribute("POSITION"),
    uv = primitive.getAttribute("TEXCOORD_0");
  const faces = [];
  // Native rectangular routes emit two indexed triangles per face per pair of stations.
  for (let i = 0; i < indices.length; i += 6) {
    const ids = [indices[i], indices[i + 1], indices[i + 2], indices[i + 5]];
    const corners = ids.map((id) => vec(positions, id));
    const [p, q, r, s] = corners;
    const coordinates = ids.map((id) => metric.getElement(id, []));
    const n = new Vector3().crossVectors(q.clone().sub(p), s.clone().sub(p)).normalize();
    const width = coordinates[1][0] - coordinates[0][0],
      length = coordinates[0][1] - coordinates[3][1];
    if (!(width > 0 && length > 0)) throw Error("Unrecognized rectangular duct panel chart");
    const straight =
      s.clone().sub(p).sub(r.clone().sub(q)).lengthSq() < 1e-10 &&
      Math.abs(s.distanceTo(p) - length) < 0.0001;
    faces.push({
      corners,
      uv: ids.map((id) => uv.getElement(id, [])),
      width,
      length,
      n,
      straight,
      lock: coordinates[0][0] === 0,
    });
  }
  return faces;
}

export function addDuctPanels(doc, body, primitive, faces, openings, trim, columns, repeat) {
  const pos = [],
    normals = [],
    texcoords = [],
    indices = [];
  const color = [],
    aspect = [];
  const chartAspect = primitive.getAttribute("_MS_ASPECT").getScalar(0);
  const skinColor = primitive.getAttribute("COLOR_0")?.getElement(0, []) ?? [1, 1, 1];
  const fittings = doc
    .getRoot()
    .listNodes()
    .filter(
      (n) => n.getName().startsWith(body.getName().slice(0, -4)) && /\/(fitting|bracket)$/.test(n.getName()),
    )
    .flatMap((n) => n.getMesh().listPrimitives());
  const metalPoints = [];
  for (const fitting of fittings) {
    const positions = fitting.getAttribute("POSITION");
    for (let i = 0; i < positions.getCount(); i++) metalPoints.push(vec(positions, i));
  }

  for (const face of faces) {
    const {
      width,
      length,
      n,
      uv,
      corners: [a, b, c, d],
    } = face;
    function vertex(u, v, h, trimAcross, station) {
      const x = u / width,
        y = v / length;
      const p = a.clone().lerp(b, x).lerp(d.clone().lerp(c, x), y).addScaledVector(n, h);
      return { p, uv: [(trim.start + trimAcross * trim.count) / columns, station / repeat] };
    }
    function quad(points) {
      const [p, q, , s] = points.map((v) => v.p);
      const normal = new Vector3().crossVectors(q.clone().sub(p), s.clone().sub(p)).normalize();
      if (normal.dot(n) < 0) {
        points.reverse();
        normal.negate();
      }
      const i = pos.length / 3;
      for (const v of points) {
        pos.push(...v.p.toArray());
        normals.push(...normal.toArray());
        texcoords.push(...v.uv);
        color.push(...skinColor);
        aspect.push(chartAspect);
      }
      indices.push(i, i + 1, i + 2, i, i + 2, i + 3);
    }
    // Asymmetric folded pocket down one longitudinal corner of the wraparound sheet.
    // The buried foot avoids coplanar fragments where the lip meets the skin.
    if (face.lock) {
      const fold = [
        [0.001, -0.0003],
        [0.0025, 0.0047],
        [0.009, 0.005],
        [0.0105, 0.002],
        [0.012, -0.0003],
      ];
      for (let j = 1; j < fold.length; j++) {
        const [u0, h0] = fold[j - 1],
          [u1, h1] = fold[j];
        quad([
          vertex(u0, 0, h0, (u0 - 0.001) / 0.011, uv[0][1] * repeat),
          vertex(u1, 0, h1, (u1 - 0.001) / 0.011, uv[0][1] * repeat),
          vertex(u1, length, h1, (u1 - 0.001) / 0.011, uv[3][1] * repeat),
          vertex(u0, length, h0, (u0 - 0.001) / 0.011, uv[3][1] * repeat),
        ]);
      }
    }
    if (!face.straight || length < pitch) continue;
    const axis = d.clone().sub(a).normalize(),
      mid = a.clone().lerp(b, 0.5);
    // Transverse flanges divide the straight into plates. Recover their real axial
    // positions instead of assuming the repeat on short routes or through elbows.
    const contacts = [];
    for (const p of metalPoints) {
      const delta = p.clone().sub(mid),
        station = axis.dot(delta);
      if (station < 0 || station > length) continue;
      delta.addScaledVector(axis, -station);
      if (delta.length() < width / 2 + 0.09) contacts.push(station);
    }
    contacts.sort((a, b) => a - b);
    const intervals = [];
    for (const t of contacts) {
      const last = intervals.at(-1);
      if (last && t - last[1] < 0.065) last[1] = t;
      else intervals.push([t, t]);
    }
    let at = 0;
    for (const [next, end] of [...intervals, [length, length]]) {
      // The first bead sits half a pitch from its joint. Clear the final flange lip.
      for (let v = at + pitch / 2; v < next - 0.065; v += pitch) {
        const middle = vertex(width / 2, v, 0, 0, 0).p;
        if (
          openings.some(
            (opening) =>
              n.dot(new Vector3(...opening.normal)) > 0.95 &&
              ["x", "y", "z"].every(
                (k, i) => middle[k] >= opening.min[i] - 0.015 && middle[k] <= opening.max[i] + 0.015,
              ),
          )
        )
          continue;
        const profile = [
          [-0.009, -0.00025],
          [-0.004, 0.0038],
          [0.002, 0.0048],
          [0.008, -0.00025],
        ];
        const ends = [
          [margin, 0],
          [margin + 0.011, 1],
          [width - margin - 0.011, 1],
          [width - margin, 0],
        ];
        for (let j = 1; j < profile.length; j++)
          for (let k = 1; k < ends.length; k++) {
            const [v0, h0] = profile[j - 1],
              [v1, h1] = profile[j];
            const [u0, f0] = ends[k - 1],
              [u1, f1] = ends[k];
            quad([
              vertex(u0, v + v0, h0 * f0, (v0 + 0.009) / 0.017, u0 + v),
              vertex(u1, v + v0, h0 * f1, (v0 + 0.009) / 0.017, u1 + v),
              vertex(u1, v + v1, h1 * f1, (v1 + 0.009) / 0.017, u1 + v),
              vertex(u0, v + v1, h1 * f0, (v1 + 0.009) / 0.017, u0 + v),
            ]);
          }
      }
      at = end;
    }
  }
  if (!indices.length) return;
  const buffer = primitive.getAttribute("POSITION").getBuffer();
  const accessor = (type, data) => doc.createAccessor().setBuffer(buffer).setType(type).setArray(data);
  const detail = doc
    .createPrimitive()
    .setMaterial(primitive.getMaterial())
    .setIndices(accessor("SCALAR", new Uint32Array(indices)));
  for (const [name, type, values] of [
    ["POSITION", "VEC3", pos],
    ["NORMAL", "VEC3", normals],
    ["TEXCOORD_0", "VEC2", texcoords],
    ["COLOR_0", skinColor.length === 3 ? "VEC3" : "VEC4", color],
    ["_MS_ASPECT", "SCALAR", aspect],
  ])
    detail.setAttribute(name, accessor(type, new Float32Array(values)));
  body.getMesh().addPrimitive(detail);
}
