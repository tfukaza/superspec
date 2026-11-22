import { BufferAttribute, BufferGeometry, Mesh, Plane } from "three";

/** Which side of the lot diagonal contains the entire draw, if its bounds prove that. */
export function footprintSide(geometry: BufferGeometry, plane: Plane): 0 | 1 | undefined {
  if (!geometry.boundingBox) geometry.computeBoundingBox();
  const { min, max } = geometry.boundingBox!, { x, y, z } = plane.normal;
  const lo = Math.min(x * min.x, x * max.x) + Math.min(y * min.y, y * max.y) + Math.min(z * min.z, z * max.z) + plane.constant;
  const hi = Math.max(x * min.x, x * max.x) + Math.max(y * min.y, y * max.y) + Math.max(z * min.z, z * max.z) + plane.constant;
  if (hi <= 1e-7) return 0;
  if (lo >= -1e-7) return 1;
}

/** Clip at the lot's diagonal once. Both draws retain their original material, UV charts and vertex colors. */
export function splitFootprintMesh(mesh: Mesh, plane: Plane): 0 | 1 | [Mesh, Mesh] {
  const geometry = mesh.geometry, bounded = footprintSide(geometry, plane);
  if (bounded !== undefined) return bounded;
  const position = geometry.getAttribute("position"), count = position.count, { x, y, z } = plane.normal;
  const distances = new Float64Array(count);
  let negative = false, positive = false;
  for (let i = 0; i < count; i++) {
    const d = x * position.getX(i) + y * position.getY(i) + z * position.getZ(i) + plane.constant;
    distances[i] = Math.abs(d) < 1e-7 ? 0 : d;
    negative ||= distances[i] < 0; positive ||= distances[i] > 0;
  }
  if (!negative || !positive) return positive ? 1 : 0;
  const sections: [number[], number[]] = [[], []], cut = new Map<number, number>();
  const crossings: { a: number; b: number; weight: number }[] = [];
  const intersect = (a: number, b: number) => {
    const key = Math.min(a, b) * count + Math.max(a, b);
    const cached = cut.get(key);
    if (cached !== undefined) return cached;
    const point = count + crossings.length;
    crossings.push({ a, b, weight: distances[a] / (distances[a] - distances[b]) });
    cut.set(key, point);
    return point;
  };
  const index = geometry.index, end = index?.count ?? count;
  for (let i = 0; i < end; i += 3) {
    const a = index ? index.getX(i) : i, b = index ? index.getX(i + 1) : i + 1, c = index ? index.getX(i + 2) : i + 2;
    const da = distances[a], db = distances[b], dc = distances[c];
    if (da <= 0 && db <= 0 && dc <= 0) sections[0].push(a, b, c);
    else if (da >= 0 && db >= 0 && dc >= 0) sections[1].push(a, b, c);
    else {
      const triangle = [a, b, c], sides = [[], []] as number[][];
      for (let edge = 0; edge < 3; edge++) {
        const from = triangle[edge], to = triangle[(edge + 1) % 3];
        if (distances[from] <= 0) sides[0].push(from);
        if (distances[from] >= 0) sides[1].push(from);
        if (distances[from] * distances[to] < 0) {
          const point = intersect(from, to);
          sides[0].push(point); sides[1].push(point);
        }
      }
      for (let side = 0; side < 2; side++) {
        const vertices = sides[side];
        for (let j = 1; j < vertices.length - 1; j++) sections[side].push(vertices[0], vertices[j], vertices[j + 1]);
      }
    }
  }
  const halves = [new BufferGeometry(), new BufferGeometry()];
  for (const [name, attribute] of Object.entries(geometry.attributes)) {
    const stride = attribute.itemSize;
    const data = new Float32Array((count + crossings.length) * stride);
    if (attribute instanceof BufferAttribute && !attribute.normalized) data.set(attribute.array);
    else for (let i = 0; i < count; i++) for (let j = 0; j < stride; j++) data[i*stride+j] = attribute.getComponent(i, j);
    for (const [i, { a, b, weight }] of crossings.entries()) {
      for (let j = 0; j < stride; j++) {
        const v = data[a*stride+j];
        data[(count+i)*stride+j] = v + weight * (data[b*stride+j] - v);
      }
    }
    const buffer = new BufferAttribute(data, stride);
    for (const half of halves) half.setAttribute(name, buffer);
  }
  return halves.map((half, side) => {
    half.setIndex(sections[side]);
    half.boundingBox = geometry.boundingBox;
    half.boundingSphere = geometry.boundingSphere;
    const part = new Mesh(half, mesh.material);
    part.matrixAutoUpdate = false;
    part.receiveShadow = mesh.receiveShadow;
    part.castShadow = mesh.castShadow;
    part.renderOrder = mesh.renderOrder;
    return part;
  }) as [Mesh, Mesh];
}
