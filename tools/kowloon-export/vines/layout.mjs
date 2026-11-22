const station = (p, n) => ({ p, n });
const line = (points, n) => points.map((p) => station(p, n));
function pierGrowth(post, b) {
  const x = post.hi[0] + 0.017,
    y = post.lo[1],
    z = post.lo[2],
    top = b.hi[2] - 0.18,
    bx = b.hi[0] + 0.015;
  const east = [1, 0, 0];
  const climb = [
    [x + 0.087, y + 0.28, z + 0.012],
    [x + 0.018, y + 0.25, z + 0.045],
    [x, y + 0.22, z + 0.12],
  ];
  const ratio = [0.32, 0.38, 0.29, 0.18, 0.22, 0.35, 0.29, 0.12, 0.17, 0.24, 0.18, 0.1, 0.04];
  ratio.forEach((offset, i) =>
    climb.push([
      x + 0.005 * Math.sin(i * 2),
      y + offset,
      z + 0.15 + ((top - z - 0.15) * (i + 1)) / ratio.length,
    ]),
  );
  climb.push([x, y - 0.018, top + 0.015], [bx, y - 0.06, top + 0.025]);
  const reach = Math.min(3.35, y - b.lo[1] - 0.2);
  for (let i = 1; i <= 15; i++)
    climb.push([
      bx,
      y - 0.06 - (reach * i) / 15,
      top + 0.073 * Math.sin(i * 0.64) - 0.026 * Math.sin(i * 1.6),
    ]);
  const fork = (i, values) => [climb[i], ...values];
  return {
    id: "climbing-" + post.id,
    seed: 718,
    paths: [
      { r: 0.017, points: line(climb, east) },
      {
        r: 0.011,
        scale: 0.72,
        points: line(
          [
            climb[0],
            [x + 0.021, y + 0.31, z + 0.038],
            [x, y + 0.34, z + 0.09],
            [x, y + 0.31, z + 0.21],
            [x, y + 0.23, z + 0.3],
            [x, y + 0.14, z + 0.37],
            [x, y + 0.08, z + 0.47],
            [x, y + 0.1, z + 0.56],
            [x, y + 0.055, z + 0.64],
            [x, y + 0.028, z + 0.69],
          ],
          east,
        ),
      },
      {
        r: 0.008,
        scale: 0.48,
        points: line(
          [
            climb[1],
            [x + 0.044, y + 0.18, z + 0.019],
            [x + 0.038, y + 0.1, z + 0.011],
            [x + 0.072, y + 0.02, z + 0.01],
            [x + 0.084, y - 0.02, z + 0.008],
          ],
          [0, 0, 1],
        ),
      },
      {
        r: 0.006,
        scale: 0.55,
        points: line(
          [
            climb[4],
            [x, y + 0.38, z + 0.55],
            [x, y + 0.42, z + 0.62],
            [x, y + 0.4, z + 0.73],
            [x, y + 0.34, z + 0.82],
            [x, y + 0.35, z + 0.9],
          ],
          east,
        ),
      },
      {
        r: 0.008,
        scale: 0.64,
        points: [
          ...line(
            [climb[9], [x, y + 0.17, top - 0.86], [x, y + 0.09, top - 0.73], [x, y - 0.013, top - 0.67]],
            east,
          ),
          ...line(
            [
              [x - 0.055, y - 0.016, top - 0.58],
              [x - 0.14, y - 0.016, top - 0.49],
              [x - 0.16, y - 0.017, top - 0.34],
              [x - 0.21, y - 0.016, top - 0.27],
              [x - 0.29, y - 0.016, top - 0.21],
              [x - 0.31, y - 0.015, top - 0.13],
            ],
            [0, -1, 0],
          ),
        ],
      },
      {
        r: 0.006,
        scale: 0.69,
        points: line(
          [
            climb[10],
            [x, y + 0.1, top - 0.59],
            [x, y + 0.14, top - 0.42],
            [x, y + 0.28, top - 0.34],
            [x, y + 0.32, top - 0.26],
            [x, y + 0.3, top - 0.18],
            [x, y + 0.36, top - 0.1],
            [x, y + 0.42, top - 0.062],
          ],
          east,
        ),
      },
      {
        r: 0.004,
        scale: 0.5,
        points: line(
          [
            climb[17],
            [bx, y - 0.18, top - 0.056],
            [bx, y - 0.27, top - 0.106],
            [bx, y - 0.34, top - 0.17],
            [bx, y - 0.34, top - 0.29],
            [bx + 0.011, y - 0.39, top - 0.39],
            [bx + 0.014, y - 0.37, top - 0.5],
            [bx + 0.023, y - 0.4, top - 0.55],
            [bx + 0.022, y - 0.422, top - 0.58],
          ],
          east,
        ),
      },
      {
        r: 0.004,
        scale: 0.55,
        points: line(
          [
            climb[27],
            [bx, y - 2.53, top - 0.11],
            [bx, y - 2.64, top - 0.19],
            [bx, y - 2.68, top - 0.32],
            [bx + 0.021, y - 2.67, top - 0.45],
            [bx + 0.03, y - 2.72, top - 0.53],
          ],
          east,
        ),
      },
      {
        r: 0.009,
        points: line(
          fork(6, [
            [x, y + 0.14, z + 1.24],
            [x, y + 0.09, z + 1.44],
            [x, y + 0.05, z + 1.62],
            [x, y + 0.09, z + 1.86],
          ]),
          east,
        ),
      },
      {
        r: 0.008,
        points: line(
          fork(19, [
            [bx, y - 0.58, top - 0.12],
            [bx, y - 0.78, top - 0.18],
            [bx, y - 0.98, top - 0.22],
            [bx + 0.036, y - 1.08, top - 0.43],
            [bx + 0.058, y - 1.06, top - 0.65],
          ]),
          east,
        ),
      },
      {
        r: 0.007,
        points: line(
          fork(24, [
            [bx, y - 1.74, top - 0.1],
            [bx, y - 1.92, top - 0.19],
            [bx, y - 2.19, top - 0.17],
            [bx, y - 2.42, top - 0.23],
          ]),
          east,
        ),
      },
    ],
  };
}

function planterGrowth(body, cap, bed, axis) {
  const side = (body.lo[axis] + body.hi[axis]) / 2 < (bed.lo[axis] + bed.hi[axis]) / 2 ? -1 : 1;
  const outer = side < 0 ? cap.lo[axis] : cap.hi[axis],
    inner = side < 0 ? cap.hi[axis] : cap.lo[axis];
  const soil = side < 0 ? bed.lo[axis] : bed.hi[axis],
    other = 1 - axis,
    span = cap.hi[other] - cap.lo[other];
  if (
    (inner - soil) * side < -0.035 ||
    (inner - soil) * side > 0.33 ||
    cap.lo[2] - bed.lo[2] > 0.72 ||
    cap.lo[2] <= bed.lo[2] ||
    Math.abs(body.hi[2] - cap.lo[2]) > 0.01
  )
    return;
  const n = [0, 0, 0];
  n[axis] = side;
  const path = (fraction, d, length) => {
    const t = cap.lo[other] + span * fraction,
      h = cap.hi[2],
      base = bed.lo[2];
    if (t < bed.lo[other] + 0.2 || t > bed.hi[other] - 0.2) return;
    const point = (a, c, z, norm = n) => {
      const p = [0, 0, z];
      p[axis] = a;
      p[other] = t + c * d;
      return station(p, norm);
    };
    const up = [0, 0, 1],
      inside = n.map((x) => -x);
    const bodyFace = side < 0 ? body.lo[axis] : body.hi[axis];
    const face = (across, sweep, below, normal = n) => point(across, sweep, h - below, normal);
    const trunk = [
      point(soil - 0.15 * side, -0.12, base + 0.004, up),
      point(soil - 0.035 * side, -0.1, base + 0.025, inside),
      point(soil - 0.012 * side, -0.06, base + 0.14, inside),
      point(soil + 0.006 * side, -0.05, base + 0.147, up),
      point(inner - 0.012 * side, 0.045, base + 0.147, inside),
      face(inner - 0.01 * side, 0.1, 0.12, inside),
      face(inner - 0.011 * side, 0.135, 0.007, inside),
      face(inner - 0.006 * side, 0.14, -0.006, up),
      face(inner + 0.014 * side, 0.15, -0.006, up),
      face((inner + outer) / 2, 0.245, -0.0065, up),
      face(outer - 0.045 * side, 0.3, -0.006, up),
      face(outer - 0.001 * side, 0.308, -0.006, up),
      face(outer + 0.007 * side, 0.312, 0.002, n),
      face(outer + 0.008 * side, 0.315, 0.025),
      face(outer + 0.006 * side, 0.3, h - cap.lo[2] - 0.002),
      face(outer + 0.007 * side, 0.296, h - cap.lo[2] + 0.012),
      face(bodyFace + 0.012 * side, 0.26, h - cap.lo[2] + 0.025),
      face(bodyFace + 0.01 * side, 0.236, 0.155),
      face(bodyFace + 0.008 * side, 0.215, 0.235),
      face(bodyFace + 0.012 * side, 0.224, 0.3),
      face(bodyFace + 0.016 * side, 0.27, length * 0.74),
      face(bodyFace + 0.025 * side, 0.29, length * 0.86),
      face(bodyFace + 0.033 * side, 0.262, length),
    ];
    // Round the bends while retaining tangent points at the two coping edges.
    const round = (points) => {
      const result = [points[0]];
      const mix = (a, b, t) =>
        station(
          a.p.map((v, i) => v + (b.p[i] - v) * t),
          a.n.map((v, i) => v + (b.n[i] - v) * t),
        );
      for (let i = 1; i < points.length - 1; i++) {
        const prev = points[i - 1],
          at = points[i],
          after = points[i + 1];
        const a = mix(at, prev, 0.24),
          b = mix(at, after, 0.24);
        result.push(a, mix(mix(a, at, 0.5), mix(at, b, 0.5), 0.5), b);
      }
      return [...result, points.at(-1)];
    };
    const main = { r: 0.007, scale: 0.95, role: "root", points: round(trunk) };
    const anchor = (i) => main.points[1 + (i - 1) * 3 + 1];
    const lateral = {
      r: 0.0044,
      scale: 0.75,
      role: "shoot",
      points: round([
        anchor(17),
        face(bodyFace + 0.012 * side, 0.13, 0.164),
        face(bodyFace + 0.01 * side, 0.046, 0.186),
        face(bodyFace + 0.011 * side, -0.018, 0.238),
        face(bodyFace + 0.014 * side, -0.032, 0.3),
        face(bodyFace + 0.02 * side, -0.066, 0.352),
        face(bodyFace + 0.027 * side, -0.051, 0.406),
      ]),
    };
    const lip = {
      r: 0.0041,
      scale: 0.68,
      role: "shoot",
      points: round([
        anchor(10),
        face(outer - 0.035 * side, 0.42, -0.005, up),
        face(outer - 0.006 * side, 0.51, -0.005, up),
        face(outer + 0.007 * side, 0.538, -0.004, up),
        face(outer + 0.009 * side, 0.551, 0.022),
        face(outer + 0.008 * side, 0.572, h - cap.lo[2] + 0.009),
        face(bodyFace + 0.012 * side, 0.577, h - cap.lo[2] + 0.027),
        face(bodyFace + 0.018 * side, 0.594, 0.17),
        face(bodyFace + 0.024 * side, 0.574, 0.238),
      ]),
    };
    const crown = (offset, height, reach) => {
      // Basal shoots share this root and reach the skylight from inside the bed.
      const soilPoint = (inset, sweep, z) => point(soil - side * inset, sweep, z, n);
      const tip = h + height,
        z0 = base + 0.004;
      return {
        r: 0.0058,
        scale: 0.84,
        role: "crown",
        points: round([
          main.points[0],
          soilPoint(0.148, -0.12 + offset * 0.12, z0 + 0.08),
          soilPoint(0.11, -0.12 + offset * 0.36, z0 + (tip - z0) * 0.4),
          soilPoint(0.083, -0.12 + offset * 0.61, z0 + (tip - z0) * 0.67),
          soilPoint(reach * 0.7, -0.12 + offset * 0.83, tip - 0.055),
          soilPoint(reach, -0.12 + offset, tip),
        ]),
      };
    };
    return [
      main,
      lateral,
      lip,
      crown(-0.19, 0.084, 0.15),
      crown(0.125, 0.116, 0.028),
      crown(0.285, 0.045, 0.085),
    ];
  };
  const paths = (
    span > 2
      ? [
          [0.26, 1, 0.59],
          [0.69, -1, 0.47],
        ]
      : [[0.4, 1, 0.46]]
  ).flatMap((a) => path(...a) ?? []);
  return paths.length
    ? { id: "trailing-" + body.id, outward: n, coping: cap, body, axis, seed: axis ? 293 : 145, paths }
    : null;
}

// Select the bearing and the outward face in a local frame. The same runner
// can follow either end or side of an axial member.
function climbingSite(boxes, beam) {
  const along = beam.hi[1] - beam.lo[1] > beam.hi[0] - beam.lo[0] ? 1 : 0,
    across = 1 - along;
  for (const side of [1, -1])
    for (const end of [1, -1]) {
      const local = (p) => [p[across] * side, p[along] * end, p[2]];
      const world = (p) => {
        const q = [0, 0, p[2]];
        q[across] = p[0] * side;
        q[along] = p[1] * end;
        return q;
      };
      const box = (q) => {
        const a = local(q.lo),
          b = local(q.hi);
        return { ...q, lo: a.map((x, i) => Math.min(x, b[i])), hi: a.map((x, i) => Math.max(x, b[i])) };
      };
      const b = box(beam),
        surroundings = boxes.map(box);
      if (b.hi[1] - b.lo[1] < 3.9 || b.hi[2] - b.lo[2] < 0.4) continue;
      const post = surroundings.find(
        (p) =>
          p.hi[0] - p.lo[0] < 0.8 &&
          p.hi[0] - p.lo[0] > 0.38 &&
          p.hi[1] - p.lo[1] < 0.8 &&
          p.hi[1] - p.lo[1] > 0.4 &&
          b.hi[1] >= p.lo[1] &&
          b.hi[1] <= p.hi[1] &&
          b.lo[0] >= p.lo[0] - 0.01 &&
          Math.abs(p.hi[0] - b.hi[0]) < 0.065 &&
          Math.abs(p.hi[2] - b.hi[2]) < 0.05 &&
          p.hi[2] - p.lo[2] > 2.55,
      );
      if (
        !post ||
        !surroundings.some(
          (f) =>
            Math.abs(f.hi[2] - post.lo[2]) < 0.035 &&
            f.lo[0] <= post.hi[0] + 0.08 &&
            f.hi[0] >= post.hi[0] + 0.08 &&
            f.lo[1] <= post.lo[1] + 0.28 &&
            f.hi[1] >= post.lo[1] + 0.28,
        )
      )
        continue;
      const site = pierGrowth(post, b);
      const blocked = site.paths.some((path) =>
        path.points.some(({ p }) =>
          surroundings.some(
            (q) =>
              q.id !== post.id &&
              q.lo[0] < p[0] + 0.13 &&
              q.hi[0] > p[0] &&
              q.lo[1] < p[1] + 0.065 &&
              q.hi[1] > p[1] - 0.065 &&
              q.lo[2] < p[2] + 0.025 &&
              q.hi[2] > p[2] + 0.005,
          ),
        ),
      );
      if (blocked) continue;
      for (const path of site.paths)
        for (const st of path.points) {
          st.p = world(st.p);
          st.n = world(st.n);
        }
      return site;
    }
}

export function growthSites({ boxes = [], beds = [], beams = [] } = {}) {
  const sites = [];
  for (const b of beams) {
    const site = climbingSite(boxes, b);
    if (site) sites.push(site);
  }
  for (const body of boxes.filter((b) => b.id?.endsWith("-body-solid") && b.id.includes("planter"))) {
    const cap = boxes.find((c) => c.id === body.id.replace("-body-solid", "-coping-solid"));
    if (!cap) continue;
    const axis = body.hi[0] - body.lo[0] < body.hi[1] - body.lo[1] ? 0 : 1;
    for (const bed of beds) {
      const site = planterGrowth(body, cap, bed, axis);
      if (site) {
        sites.push(site);
        break;
      }
    }
  }
  return sites;
}
