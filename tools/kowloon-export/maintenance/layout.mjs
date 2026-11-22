// All measurements use the authored AML frame. Marks sit on real wall planes
// beside straight service risers; cabinetry and apertures exclude occupied spans.
const dist = (a, b) => Math.hypot(...a.map((x, i) => x - b[i]));
const dot = (a, b) => a.reduce((s, x, i) => s + x * b[i], 0);
const add = (a, b, k = 1) => a.map((x, i) => x + k * b[i]);

export function maintenanceStations({ walls, pipes, cabinets }) {
  const candidates = [];
  for (const pipe of pipes) {
    const bend = pipe.radius * 2.5;
    let station = 0;
    for (let i = 0; i < pipe.supports.length; i++) {
      const a = pipe.points[i],
        b = pipe.points[i + 1],
        delta = add(b, a, -1),
        length = dist(a, b);
      const trims = [i > 0 ? bend : 0, i + 1 < pipe.points.length - 1 ? bend : 0];
      const straight = length - trims[0] - trims[1],
        start = station + trims[0];
      station += length - (i + 1 < pipe.points.length - 1 ? (2 - Math.PI / 2) * bend : 0);
      const offset = pipe.supports[i],
        reach = Math.hypot(...offset);
      if (
        Math.abs(delta[2]) < 1.25 ||
        Math.hypot(delta[0], delta[1]) > 0.001 ||
        reach < 0.04 ||
        Math.abs(offset[2]) > 0.001
      )
        continue;
      const n = offset.map((v) => -v / reach),
        right = [-n[1], n[0], 0],
        axis = Math.abs(n[0]) > 0.999 ? 0 : 1,
        along = 1 - axis;
      const wallPoint = add(a, offset);
      const host = walls.find(
        (w) =>
          Math.abs((n[axis] > 0 ? w.high : w.low)[axis] - wallPoint[axis]) < 0.002 &&
          wallPoint[along] > w.low[along] &&
          wallPoint[along] < w.high[along] &&
          Math.min(a[2], b[2]) >= w.low[2] - 0.002 &&
          Math.max(a[2], b[2]) <= w.high[2] + 0.002,
      );
      if (!host) continue;
      // The stock ItemSpec spaces sleeves by developed length and centers each wall bracket on its straight.
      const mounts = Math.ceil(straight / 1.8);
      const hardware = Array.from({ length: mounts }, (_, k) => (straight * (k + 0.5)) / mounts);
      for (let k = Math.ceil(start / 1.35); k * 1.35 <= start + straight; k++)
        hardware.push(k * 1.35 - start);
      const options = [0.33, 0.65, 0.24, 0.74, 0.43].map((t) => t * straight);
      const seat = options.find(
        (t) => hardware.every((f) => Math.abs(t - f) > 0.16) && t > 0.18 && t < straight - 0.18,
      );
      if (seat === undefined) continue;
      const center = [a[0], a[1], a[2] + Math.sign(delta[2]) * (seat + trims[0])],
        surface = add(center, offset);
      for (const side of [1, -1]) {
        const ink = add(surface, right, side * 0.43);
        ink[2] += 0.1;
        if (
          ink[along] - 0.32 < host.low[along] + 0.035 ||
          ink[along] + 0.32 > host.high[along] - 0.035 ||
          ink[2] - 0.28 < host.low[2] ||
          ink[2] + 0.18 > host.high[2]
        )
          continue;
        const blocked = cabinets.some(
          (c) =>
            dot(n, c.normal) > 0.99 &&
            Math.abs(dot(add(c.position, surface, -1), n)) < 0.04 &&
            Math.abs(c.position[along] - ink[along]) < 0.87 &&
            ink[2] - 0.28 < c.position[2] + 1.09 &&
            ink[2] + 0.18 > c.position[2] - 0.48,
        );
        if (blocked) continue;
        candidates.push({
          host: host.id,
          pipe: pipe.id,
          center,
          ink,
          normal: n,
          right,
          reach,
          radius: pipe.radius,
        });
        break;
      }
    }
  }
  // Two workstations are enough for this room. Prefer the long west-side riser,
  // then an accessible route on another face, avoiding neighboring duplicate IDs.
  candidates.sort(
    (a, b) => b.normal[0] - a.normal[0] || b.center[1] - a.center[1] || a.pipe.localeCompare(b.pipe),
  );
  const result = [];
  for (const c of candidates) {
    if (
      result.some(
        (p) => p.pipe === c.pipe || Math.abs(dot(p.normal, c.normal)) > 0.95 || dist(p.center, c.center) < 3,
      )
    )
      continue;
    result.push(c);
    if (result.length === 2) break;
  }
  return result;
}
