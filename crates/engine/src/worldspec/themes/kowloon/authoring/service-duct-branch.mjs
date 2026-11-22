import { Vector3 } from "three";
import { buildRegisterGrille, openSheet, registerFaces, registerStation } from "./service-duct-register.mjs";

export function addDuctBranch(doc, body, skin, register, strips, columns, repeat) {
  if (!register) return;
  const main = body.getMesh();
  const patch = main.listPrimitives().find((p) => p !== skin && p.getMaterial() === skin.getMaterial());
  const face = patch && registerFaces(patch, 1.05).find((f) => f.lo >= register.max[f.axis] - 0.02);
  if (!face) return;
  const inletWidth = 0.255,
    inletHeight = Math.min(0.16, face.high - face.low - 0.025);
  const station = registerStation(doc, body, face, inletWidth + 0.035, 0.49);
  if (station === undefined) return;
  const z = (face.low + face.high) / 2,
    origin = new Vector3();
  origin.setComponent(face.axis, station).setComponent(face.side, face.plane).setZ(z);
  const out = new Vector3().setComponent(face.side, 1),
    along = new Vector3().setComponent(face.axis, 1),
    up = new Vector3(0, 0, 1);
  const forward = out.clone().addScaledVector(along, -0.32).normalize(),
    lateral = along.clone().addScaledVector(out, 0.32).normalize();
  openSheet(
    doc,
    main,
    patch,
    face,
    station - inletWidth / 2,
    station + inletWidth / 2,
    z - inletHeight / 2,
    z + inletHeight / 2,
  );
  const buffer = skin.getAttribute("POSITION").getBuffer(),
    mesh = doc.createMesh("Ventilation branch and terminal");
  const branch = doc
    .createNode(body.getName().replace(/body$/, "branch"))
    .setMesh(mesh)
    .setMatrix(body.getMatrix());
  (body.getParentNode() ?? doc.getRoot().listScenes()[0]).addChild(branch);
  const positions = [],
    normals = [],
    coords = [];
  const atlas = (strip, x, s) => [(strip.start + x * strip.count) / columns, s / repeat];
  function quad(p, uv, outward) {
    const n = p[1].clone().sub(p[0]).cross(p[2].clone().sub(p[0])).normalize();
    const winding = n.dot(outward) > 0 ? [0, 1, 2, 0, 2, 3] : [0, 2, 1, 0, 3, 2];
    if (n.dot(outward) < 0) n.negate();
    for (const i of winding) {
      positions.push(...p[i].toArray());
      normals.push(...n.toArray());
      coords.push(...uv[i]);
    }
  }
  function ring(c, w, h, right, vertical) {
    return {
      center: c,
      corners: [
        [-1, -1],
        [1, -1],
        [1, 1],
        [-1, 1],
      ].map(([x, y]) =>
        c
          .clone()
          .addScaledVector(right, (x * w) / 2)
          .addScaledVector(vertical, (y * h) / 2),
      ),
    };
  }
  // Sheet V follows the branch centerline. The trim chart crosses the section of each rolled lip.
  function span(a, b, station0, trim = false) {
    const len = a.center.distanceTo(b.center);
    for (let i = 0; i < 4; i++) {
      const j = (i + 1) % 4,
        strip = strips[trim ? 4 : i],
        p = [a.corners[i], a.corners[j], b.corners[j], b.corners[i]];
      const outward = p[0].clone().lerp(p[1], 0.5).sub(a.center);
      const u0 = 0.06,
        u1 = u0 + Math.min(0.9, p[0].distanceTo(p[1]) / strip.span);
      const uv = trim
        ? [
            atlas(strip, 0, 0),
            atlas(strip, 0, p[0].distanceTo(p[1])),
            atlas(strip, 1, p[2].distanceTo(p[3])),
            atlas(strip, 1, 0),
          ]
        : [
            atlas(strip, u0, station0),
            atlas(strip, u1, station0),
            atlas(strip, u1, station0 + len),
            atlas(strip, u0, station0 + len),
          ];
      quad(p, uv, outward);
    }
    return station0 + len;
  }
  function collar(c, axis, right, vertical, w, h, thickness) {
    const a = c.clone().addScaledVector(axis, -thickness / 2),
      b = c.clone().addScaledVector(axis, thickness / 2);
    const profile = [
      [a, w, h],
      [a.clone().addScaledVector(axis, 0.002), w + 0.017, h + 0.017],
      [b.clone().addScaledVector(axis, -0.002), w + 0.017, h + 0.017],
      [b, w, h],
    ];
    let prev;
    for (const [center, ww, hh] of profile) {
      const r = ring(center, ww, hh, right, vertical);
      if (prev) span(prev, r, 0, true);
      prev = r;
    }
  }
  const w = 0.184,
    h = 0.126,
    neck = origin.clone().addScaledVector(out, 0.245).addScaledVector(along, -0.038);
  let prev = ring(neck, w, h, lateral, up),
    phase = span(ring(origin, inletWidth, inletHeight, along, up), prev, 0);
  collar(origin.clone().addScaledVector(out, 0.012), out, along, up, inletWidth, inletHeight, 0.024);
  collar(neck, forward, lateral, up, w, h, 0.02);
  const bend = neck.clone().addScaledVector(forward, 0.83),
    straight = ring(bend, w, h, lateral, up);
  phase = span(prev, straight, phase);
  prev = straight;
  const radius = 0.185,
    angle = (72 * Math.PI) / 180;
  let end, vertical;
  for (let i = 1; i <= 6; i++) {
    const a = (angle * i) / 6;
    end = bend
      .clone()
      .addScaledVector(forward, radius * Math.sin(a))
      .addScaledVector(up, radius * (Math.cos(a) - 1));
    vertical = up.clone().multiplyScalar(Math.cos(a)).addScaledVector(forward, Math.sin(a));
    const next = ring(end, w, h, lateral, vertical);
    phase = span(prev, next, phase);
    prev = next;
  }
  const outletNormal = forward.clone().multiplyScalar(Math.cos(angle)).addScaledVector(up, -Math.sin(angle));
  const mouth = end.clone().addScaledVector(outletNormal, 0.12),
    width = 0.3,
    height = 0.19;
  span(prev, ring(mouth, width, height, lateral, vertical), phase);
  buildRegisterGrille(doc, mesh, buffer, width, height, ([x, y, d]) =>
    mouth.clone().addScaledVector(lateral, x).addScaledVector(vertical, y).addScaledVector(outletNormal, d),
  );
  // Read the soffit height from hangers on the selected main. The new drop has its own two-rod saddle.
  const hang = neck.clone().addScaledVector(forward, 0.57);
  const bracket = doc
    .getRoot()
    .listNodes()
    .find((n) => n.getName() === body.getName().replace(/body$/, "bracket"));
  const top = bracket?.getMesh()?.listPrimitives()[0].getAttribute("POSITION").getMax([])[2];
  collar(hang, forward, lateral, up, w, h, 0.025);
  if (top > hang.z + h / 2 && top - hang.z < 1.25) {
    for (const sign of [-1, 1]) {
      const base = hang
        .clone()
        .addScaledVector(lateral, sign * (w / 2 + 0.003))
        .addScaledVector(up, h / 2);
      const ceiling = base.clone().setZ(top);
      span(ring(base, 0.012, 0.017, lateral, forward), ring(ceiling, 0.012, 0.017, lateral, forward), 0);
      span(
        ring(ceiling.clone().addScaledVector(up, -0.008), 0.036, 0.048, lateral, forward),
        ring(ceiling, 0.036, 0.048, lateral, forward),
        0,
        true,
      );
    }
  }
  const accessor = (type, data) =>
    doc.createAccessor().setBuffer(buffer).setType(type).setArray(Float32Array.from(data));
  mesh.addPrimitive(
    doc
      .createPrimitive()
      .setMaterial(skin.getMaterial())
      .setAttribute("POSITION", accessor("VEC3", positions))
      .setAttribute("NORMAL", accessor("VEC3", normals))
      .setAttribute("TEXCOORD_0", accessor("VEC2", coords))
      .setAttribute(
        "_MS_ASPECT",
        accessor(
          "SCALAR",
          new Float32Array(positions.length / 3).fill(skin.getAttribute("_MS_ASPECT").getScalar(0)),
        ),
      ),
  );
  const min = origin.toArray(),
    max = origin.toArray();
  min[face.axis] -= inletWidth / 2 + 0.014;
  max[face.axis] += inletWidth / 2 + 0.014;
  min[2] -= inletHeight / 2 + 0.014;
  max[2] += inletHeight / 2 + 0.014;
  return { normal: out.toArray(), min, max };
}
