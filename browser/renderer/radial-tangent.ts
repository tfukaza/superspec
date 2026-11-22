import * as THREE from "three";

/** Recover each groove's signed tangent from the mesh's centred polar UV chart.
 * UV and position seams share chords. Surface normals can split at a turned
 * shoulder while the direction around the same groove remains continuous. */
export function prepareRadialTangents(geometry: THREE.BufferGeometry): void {
  if (geometry.hasAttribute("radialTangent")) return;
  const position = geometry.getAttribute("position");
  if (!position) return;
  const uv = geometry.getAttribute("uv");
  const values = new THREE.Float32BufferAttribute(new Float32Array(position.count * 3), 3);
  geometry.setAttribute("radialTangent", values);
  if (!uv || uv.count !== position.count) return;

  const vertices: { point: THREE.Vector3; u: number; v: number; radius: number; neighbors: Set<number> }[] = [];
  const welded = new Uint32Array(position.count);
  const keys = new Map<string, number>();
  for (let i = 0; i < position.count; i++) {
    const x = position.getX(i), y = position.getY(i), z = position.getZ(i);
    const u = uv.getX(i) - 0.5, v = uv.getY(i) - 0.5;
    const key = [x, y, z, u, v].map(n => Math.round(n * 1e5)).join(",");
    let id = keys.get(key);
    if (id === undefined) {
      id = vertices.length;
      keys.set(key, id);
      vertices.push({ point: new THREE.Vector3(x, y, z), u, v, radius: Math.hypot(u, v), neighbors: new Set() });
    }
    welded[i] = id;
  }

  const indices = geometry.index;
  const count = indices?.count ?? position.count;
  const vertexAt = (n: number) => welded[indices ? indices.getX(n) : n];
  for (let i = 0; i + 2 < count; i += 3) {
    for (let edge = 0; edge < 3; edge++) {
      const a = vertexAt(i + edge), b = vertexAt(i + (edge + 1) % 3), c = vertexAt(i + (edge + 2) % 3);
      const p = vertices[a], q = vertices[b], other = vertices[c];
      // A chart triangle spans one ring and one meridian. Reject accidental
      // equal-radius edges on an ordinary planar/rectangular UV island.
      if (p.radius < 1e-4 || Math.abs(p.radius - q.radius) > 3e-5 ||
          Math.abs(p.radius - other.radius) < 6e-5) continue;
      const onMeridian = (s: typeof p) => s.u * other.u + s.v * other.v > 0 &&
        Math.abs(s.u * other.v - s.v * other.u) < 0.001 * s.radius * other.radius;
      if (!onMeridian(p) && !onMeridian(q)) continue;
      p.neighbors.add(b);
      q.neighbors.add(a);
    }
  }

  const tangents = vertices.map(p => {
    const tangent = new THREE.Vector3();
    if (p.neighbors.size !== 2) return tangent;
    let balance = 0;
    for (const id of p.neighbors) {
      const q = vertices[id];
      const sine = (p.u * q.v - p.v * q.u) / (p.radius * q.radius);
      if (Math.abs(sine) < 1e-5) return tangent.set(0, 0, 0);
      balance += Math.sign(sine);
      // Chord / sin(delta) estimates d(position)/d(angle). Keeping chord
      // lengths accounts for the two different radii of an elliptical turn.
      tangent.addScaledVector(new THREE.Vector3().subVectors(q.point, p.point), 1 / sine);
    }
    return balance === 0 ? tangent.normalize() : tangent.set(0, 0, 0);
  });
  for (let i = 0; i < position.count; i++) {
    const { x, y, z } = tangents[welded[i]];
    values.setXYZ(i, x, y, z);
  }
}
