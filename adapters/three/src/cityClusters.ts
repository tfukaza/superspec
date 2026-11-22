import * as THREE from "three";
import { hasDeclaredLodOwner } from "./lodOwnership.ts";
import type { CityLodLevels } from "./cityLods";
import type { CityFarHardware } from "./cityFarHardware";
import type { CityInstanceGroups } from "./cityVisibility";

const TARGET_TRIANGLES = 8000;
const MAX_CELLS = 16;

/** Split large atlas primitives into spatial regions. A wall can then hide the
 * shelves and railings on its far side even when another part of its atlas is visible. */
export function clusterCityGeometry(root: THREE.Object3D, lods: Map<THREE.BufferGeometry, CityLodLevels>, farHardware?: CityFarHardware,
  instanceGroups?: CityInstanceGroups): WeakMap<THREE.Mesh, object> {
  const pairs = new WeakMap<THREE.Mesh, object>();
  const clusters = new Map<THREE.BufferGeometry, THREE.BufferGeometry[]>();
  const originals: THREE.Mesh[] = [];
  root.traverseVisible(mesh => {
    if (!(mesh instanceof THREE.Mesh) || hasDeclaredLodOwner(mesh) || !lods.has(mesh.geometry) || mesh.children.length ||
        mesh instanceof THREE.SkinnedMesh || mesh.morphTargetInfluences?.length ||
        mesh.geometry instanceof THREE.InstancedBufferGeometry || Array.isArray(mesh.material) ||
        mesh.material.transparent || mesh.material instanceof THREE.ShaderMaterial || mesh.material.displacementMap ||
        (mesh instanceof THREE.InstancedMesh && (mesh.morphTexture || mesh.instanceColor)) ||
        Object.values(mesh.geometry.attributes).some(attribute => attribute instanceof THREE.InstancedBufferAttribute ||
          (attribute instanceof THREE.InterleavedBufferAttribute && attribute.data instanceof THREE.InstancedInterleavedBuffer))) return;
    const prototype = mesh instanceof THREE.InstancedMesh ? THREE.InstancedMesh.prototype : THREE.Mesh.prototype;
    if ((["onBeforeRender", "onAfterRender", "onBeforeShadow", "onAfterShadow", "raycast"] as const)
      .some(callback => mesh[callback] !== prototype[callback])) return;
    if ((mesh.geometry.index?.count ?? 0) > 36000) originals.push(mesh);
  });

  for (const mesh of originals) {
    let pieces = clusters.get(mesh.geometry);
    if (!pieces) {
      pieces = splitPrimitive(mesh.geometry, lods);
      clusters.set(mesh.geometry, pieces);
    }
    const provenance = {};
    const children: THREE.Mesh[] = [];
    const owners = instanceGroups?.get(mesh);
    for (const part of pieces) {
      const child = mesh.clone(false);
      child.geometry = part;
      if (owners) instanceGroups!.set(child, owners);
      pairs.set(child, provenance);
      child.customDepthMaterial = mesh.customDepthMaterial;
      child.customDistanceMaterial = mesh.customDistanceMaterial;
      if (child instanceof THREE.InstancedMesh) {
        child.boundingBox = null;
        child.boundingSphere = null;
      }
      mesh.parent!.add(child);
      if (farHardware) children.push(child);
    }
    farHardware?.register(mesh, children, lods.get(mesh.geometry)!);
    mesh.removeFromParent();
    if (mesh instanceof THREE.InstancedMesh) mesh.dispose();
  }
  return pairs;
}

function splitPrimitive(original: THREE.BufferGeometry, lods: Map<THREE.BufferGeometry, CityLodLevels>): THREE.BufferGeometry[] {
  const levels = [original, ...lods.get(original)!];
  original.computeBoundingBox();
  const originalBounds = original.boundingBox!;
  const size = originalBounds.getSize(new THREE.Vector3());
  const cells = new THREE.Vector3(1, 1, 1);
  const triangleCount = original.index!.count / 3;
  const cellCount = Math.min(MAX_CELLS, 2 ** Math.ceil(Math.log2(triangleCount / TARGET_TRIANGLES)));
  for (let count = 1; count < cellCount; count *= 2) {
    const dimensions = size.clone().divide(cells);
    const axis = dimensions.x >= dimensions.y && dimensions.x >= dimensions.z ? "x" : dimensions.y >= dimensions.z ? "y" : "z";
    cells[axis] *= 2;
  }
  const scale = cells.clone().divide(size);
  const position = original.getAttribute("position");
  const componentCells = disconnectedCells(levels, cells, scale, originalBounds);
  const bins = new Map<THREE.BufferGeometry, THREE.BufferGeometry[]>();
  for (const level of new Set(levels)) {
    const groups: number[][] = Array.from({ length: cellCount }, () => []);
    const indices = level.index!.array;
    for (let i = 0; i < indices.length; i += 3) {
      const a = indices[i], b = indices[i + 1], c = indices[i + 2];
      let bucket = componentCells?.vertices[a];
      if (bucket === undefined) {
        const x = cell((position.getX(a) + position.getX(b) + position.getX(c)) / 3, originalBounds.min.x, scale.x, cells.x);
        const y = cell((position.getY(a) + position.getY(b) + position.getY(c)) / 3, originalBounds.min.y, scale.y, cells.y);
        const z = cell((position.getZ(a) + position.getZ(b) + position.getZ(c)) / 3, originalBounds.min.z, scale.z, cells.z);
        bucket = x + cells.x * (y + cells.y * z);
      }
      groups[bucket].push(a, b, c);
    }
    bins.set(level, groups.map(indices => {
      const part = new THREE.BufferGeometry();
      part.userData.cityLodErrorMeters = level.userData.cityLodErrorMeters;
      for (const [name, attribute] of Object.entries(original.attributes)) part.setAttribute(name, attribute);
      part.setIndex(indices);
      return part;
    }));
  }

  const pieces: THREE.BufferGeometry[] = [];
  const point = new THREE.Vector3();
  for (let i = 0; i < cellCount; i++) {
    const [near, ...reduced] = levels.map(level => bins.get(level)![i]);
    const bounds = componentCells?.boxes[i] ?? new THREE.Box3();
    const distinct = new Set([near, ...reduced]);
    if (!componentCells) for (const part of distinct) {
      for (const index of part.index!.array) bounds.expandByPoint(point.fromBufferAttribute(position, index));
    }
    if (bounds.isEmpty()) continue;
    const sphere = bounds.getBoundingSphere(new THREE.Sphere());
    for (const part of distinct) {
      part.boundingBox = bounds;
      part.boundingSphere = sphere;
      part.userData.cityLodIndependent = componentCells !== undefined;
      if (componentCells) {
        part.userData.cityLodComponentDiameter = componentCells.diameters[i];
        part.userData.cityLodComponentArea = componentCells.areas[i];
      }
    }
    // Disconnected components never share a boundary across cells at any LOD.
    // Continuous surfaces keep synchronized levels to avoid cracks at a split.
    if (!componentCells) near.userData.lodDistanceBounds = originalBounds;
    lods.set(near, reduced);
    pieces.push(near);
  }
  return pieces;
}

/** Keep each connected component together across every index stream. Equal
 * positions are welded for connectivity only, including UV and normal seams. */
function disconnectedCells(levels: THREE.BufferGeometry[], cells: THREE.Vector3, scale: THREE.Vector3,
  bounds: THREE.Box3): { vertices: Uint32Array; diameters: Float64Array; areas: Float64Array; boxes: THREE.Box3[] } | undefined {
  const position = levels[0].getAttribute("position");
  const parents = new Uint32Array(position.count);
  for (let i = 0; i < parents.length; i++) parents[i] = i;
  const find = (index: number): number => {
    let root = index;
    while (parents[root] !== root) root = parents[root];
    while (parents[index] !== index) {
      const next = parents[index];
      parents[index] = root;
      index = next;
    }
    return root;
  };
  const join = (a: number, b: number): void => {
    a = find(a);
    b = find(b);
    if (a !== b) parents[Math.max(a, b)] = Math.min(a, b);
  };
  const array = position.array;
  const interleaved = position instanceof THREE.InterleavedBufferAttribute;
  const stride = interleaved ? position.data.stride : position.itemSize;
  const offset = interleaved ? position.offset : 0;
  const prototype = interleaved ? THREE.InterleavedBufferAttribute.prototype : THREE.BufferAttribute.prototype;
  if (array.constructor === Int16Array && position.getX === prototype.getX && position.getY === prototype.getY
    && position.getZ === prototype.getZ && position.itemSize >= 3 && Number.isInteger(position.count)
    && Number.isInteger(stride) && Number.isInteger(offset) && stride >= offset + 3 && offset >= 0
    && (position.count - 1) * stride + offset + 2 < array.length) {
    // Three decodes -32768 and -32767 to -1 for normalized Int16.
    const minimum = position.normalized ? -32767 : -32768;
    const welded = new Map<number, number>();
    for (let i = 0, at = offset; i < position.count; i++, at += stride) {
      const x = Math.max(minimum, array[at]) - minimum;
      const y = Math.max(minimum, array[at + 1]) - minimum;
      const z = Math.max(minimum, array[at + 2]) - minimum;
      const key = (x * 65536 + y) * 65536 + z;
      const previous = welded.get(key);
      if (previous === undefined) welded.set(key, i);
      else join(previous, i);
    }
  } else {
    const welded = new Map<string, number>();
    for (let i = 0; i < position.count; i++) {
      const key = `${position.getX(i)},${position.getY(i)},${position.getZ(i)}`;
      const previous = welded.get(key);
      if (previous === undefined) welded.set(key, i);
      else join(previous, i);
    }
  }
  const streams = new Set(levels.map(level => level.index!.array));
  for (const indices of streams) {
    for (let i = 0; i < indices.length; i += 3) {
      join(indices[i], indices[i + 1]);
      join(indices[i], indices[i + 2]);
    }
  }
  const components = new Map<number, { box: THREE.Box3; bucket: number; triangles: number }>();
  const point = new THREE.Vector3();
  const near = levels[0].index!.array;
  const visited = new Uint8Array(position.count);
  for (const indices of streams) {
    const countTriangles = indices === near;
    for (let i = 0; i < indices.length; i++) {
      const index = indices[i];
      const triangle = countTriangles && i % 3 === 0;
      if (visited[index] && !triangle) continue;
      const root = find(index);
      let component = components.get(root);
      if (!component) {
        component = { box: new THREE.Box3(), bucket: 0, triangles: 0 };
        components.set(root, component);
      }
      if (!visited[index]) {
        visited[index] = 1;
        component.box.expandByPoint(point.fromBufferAttribute(position, index));
      }
      if (triangle) component.triangles++;
    }
  }
  const cellCount = cells.x * cells.y * cells.z;
  if (components.size < cellCount || [...components.values()].some(component => component.triangles > near.length / 6)) return;
  const diameters = new Float64Array(cellCount);
  const areas = new Float64Array(cellCount);
  const boxes = Array.from({ length: cellCount }, () => new THREE.Box3());
  for (const component of components.values()) {
    component.box.getCenter(point);
    const x = cell(point.x, bounds.min.x, scale.x, cells.x);
    const y = cell(point.y, bounds.min.y, scale.y, cells.y);
    const z = cell(point.z, bounds.min.z, scale.z, cells.z);
    component.bucket = x + cells.x * (y + cells.y * z);
    boxes[component.bucket].union(component.box);
    const diameter = component.box.getSize(point).length();
    diameters[component.bucket] = Math.max(diameters[component.bucket], diameter);
    areas[component.bucket] += Math.PI / 4 * diameter * diameter;
  }
  // Keep the output separate from the union-find parent array.
  const vertices = new Uint32Array(parents.length);
  for (let i = 0; i < vertices.length; i++) vertices[i] = components.get(find(i))?.bucket ?? 0;
  return { vertices, diameters, areas, boxes };
}

function cell(value: number, min: number, scale: number, count: number): number {
  return count === 1 ? 0 : Math.max(0, Math.min(count - 1, Math.floor((value - min) * scale)));
}
