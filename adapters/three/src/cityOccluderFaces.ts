import * as THREE from "three";

type Faces = THREE.Vector3[][];
type SelectFaces = (geometry: THREE.BufferGeometry) => Faces;
type Triangle = { first: number; normal: THREE.Vector3 };

/** Both the front and the rear draw must obey ordinary nearest-depth ordering.
 * A rear draw with a stencil operation can matter even if every depth test fails. */
export function usesCityDepthTest(material: THREE.Material): boolean {
  return material.depthTest && !material.stencilWrite && !material.polygonOffset
    && (material.depthFunc === THREE.LessEqualDepth || material.depthFunc === THREE.LessDepth);
}

/** Only called on ordinary depth-tested receivers. These conditions establish
 * that a filled triangle can hide the color behind it at every covered sample. */
export function writesSolidCitySurface(material: THREE.Material | THREE.Material[]): material is THREE.Material {
  return !Array.isArray(material) && material.visible && material.depthWrite && material.colorWrite
    && !material.transparent && !material.alphaTest && !material.alphaHash && !material.alphaToCoverage
    && (material.blending === THREE.NormalBlending || material.blending === THREE.NoBlending)
    && !((material as THREE.MeshPhysicalMaterial).transmission > 0)
    && !(material as THREE.MeshBasicMaterial).wireframe && !material.clippingPlanes?.length
    && material.side !== THREE.BackSide;
}

/** The cross product of two transformed edges is the cofactor matrix applied
 * to their original cross product. Its length is twice the triangle area. */
function faceTransform(matrix: THREE.Matrix4): THREE.Matrix3 {
  const x = new THREE.Vector3().setFromMatrixColumn(matrix, 0);
  const y = new THREE.Vector3().setFromMatrixColumn(matrix, 1);
  const z = new THREE.Vector3().setFromMatrixColumn(matrix, 2);
  const a = new THREE.Vector3().crossVectors(y, z);
  const b = new THREE.Vector3().crossVectors(z, x);
  const c = new THREE.Vector3().crossVectors(x, y);
  return new THREE.Matrix3().set(a.x, b.x, c.x, a.y, b.y, c.y, a.z, b.z, c.z);
}

/** Share decoded candidates across placements. World area, wall orientation and
 * the static/instanced face budgets are resolved for each distinct linear transform. */
export class CityOccluderFaces {
  private readonly linear = new THREE.Matrix3();
  private readonly selectors = new Map<string, SelectFaces>();
  private readonly candidates = new Map<THREE.BufferGeometry, { cutoff: number; triangles: Triangle[] }>();

  forPlacement(transform: THREE.Matrix4, instanced: boolean): SelectFaces {
    this.linear.setFromMatrix4(transform);
    const key = (instanced ? "instance:" : "mesh:") + this.linear.elements.join(",");
    const existing = this.selectors.get(key);
    if (existing) return existing;
    const worldNormal = faceTransform(transform);
    const minTwiceArea = instanced ? 10 : 2;
    // The Frobenius norm upper-bounds normal expansion, including nonuniform
    // scale and shear. Round down the cutoff so similarly sized placements can
    // reuse the decoded candidate set even when their transforms differ.
    const threshold = minTwiceArea / Math.hypot(...worldNormal.elements);
    const cutoff = 2 ** Math.floor(Math.log2(threshold * (1 - 1e-12)));
    const selected = new Map<THREE.BufferGeometry, Faces>();
    const select: SelectFaces = geometry => {
      let faces = selected.get(geometry);
      if (!faces) {
        let candidates = this.candidates.get(geometry);
        if (!candidates || candidates.cutoff > cutoff) {
          candidates = { cutoff, triangles: largeTriangles(geometry, cutoff) };
          this.candidates.set(geometry, candidates);
        }
        faces = selectFaces(geometry, candidates.triangles, worldNormal, instanced ? 22 : 100, minTwiceArea);
        selected.set(geometry, faces);
      }
      return faces;
    };
    this.selectors.set(key, select);
    return select;
  }
}

function largeTriangles(geometry: THREE.BufferGeometry, cutoff: number): Triangle[] {
  const positions = geometry.getAttribute("position"), index = geometry.index;
  if (!positions?.count || index?.count === 0) return [];
  const end = Math.min(index?.count ?? positions.count, geometry.drawRange.start + geometry.drawRange.count);
  const a = new THREE.Vector3(), b = new THREE.Vector3(), c = new THREE.Vector3();
  const triangles: Triangle[] = [];
  for (let i = geometry.drawRange.start; i + 2 < end; i += 3) {
    a.fromBufferAttribute(positions, index?.getX(i) ?? i);
    b.fromBufferAttribute(positions, index?.getX(i + 1) ?? i + 1).sub(a);
    c.fromBufferAttribute(positions, index?.getX(i + 2) ?? i + 2).sub(a);
    b.cross(c);
    if (b.lengthSq() >= cutoff * cutoff) triangles.push({ first: i, normal: b.clone() });
  }
  return triangles;
}

function selectFaces(geometry: THREE.BufferGeometry, triangles: Triangle[], worldNormal: THREE.Matrix3,
  limit: number, minTwiceArea: number): Faces {
  const walls: { first: number; area: number }[] = [], horizontal: typeof walls = [];
  const normal = new THREE.Vector3();
  for (const triangle of triangles) {
    const area = normal.copy(triangle.normal).applyMatrix3(worldNormal).length();
    if (!(area > minTwiceArea) || !Number.isFinite(area)) continue;
    (Math.abs(normal.y) < .5 * area ? walls : horizontal).push({ first: triangle.first, area });
  }
  // Reserve the original wall budget while also letting roofs and floors hide
  // enclosed stock and fixtures from elevated or low viewpoints.
  const largest = (faces: typeof walls, count: number) => faces.sort((a, b) => b.area - a.area).slice(0, count);
  const positions = geometry.getAttribute("position"), index = geometry.index;
  return joinFacePairs([...largest(walls, limit), ...largest(horizontal, Math.max(4, Math.floor(limit / 3)))]
    .map(({ first }) => [first, first + 1, first + 2].map(i => new THREE.Vector3().fromBufferAttribute(positions, index?.getX(i) ?? i))));
}

/** An exact shared edge can be removed only when both opaque triangles form one convex face. */
function joinFacePairs(triangles: THREE.Vector3[][]): THREE.Vector3[][] {
  const points = triangles.map(t => t.map(v => v.toArray().join(",")));
  const neighbors = new Map<string, { index: number; edge: number }>();
  const joined = new Set<number>();
  const faces = triangles.slice();
  const ab = new THREE.Vector3(), bc = new THREE.Vector3(), n = new THREE.Vector3();
  for (let i = 0; i < triangles.length; i++) {
    const tri = triangles[i], keys = points[i];
    for (let e = 0; e < 3; e++) {
      const end = (e + 1) % 3;
      const other = neighbors.get(keys[end] + "|" + keys[e]);
      if (!other || joined.has(other.index)) continue;
      const outside = triangles[other.index][(other.edge + 2) % 3];
      const quad = [tri[end], tri[(e + 2) % 3], tri[e], outside];
      n.crossVectors(ab.subVectors(quad[1], quad[0]), bc.subVectors(quad[2], quad[1])).normalize();
      const diagonal = ab.subVectors(quad[2], quad[0]).length();
      if (Math.abs(ab.subVectors(outside, quad[0]).dot(n)) > diagonal * 1e-8) continue;
      if (!quad.every((v, k) => {
        const next = quad[(k + 1) % 4], after = quad[(k + 2) % 4];
        return ab.subVectors(next, v).cross(bc.subVectors(after, next)).dot(n) > 0;
      })) continue;
      faces[other.index] = quad;
      joined.add(other.index);
      joined.add(i);
      faces[i] = [];
      break;
    }
    if (!joined.has(i))
      for (let e = 0; e < 3; e++) neighbors.set(keys[e] + "|" + keys[(e + 1) % 3], { index: i, edge: e });
  }
  return faces.filter(face => face.length);
}
