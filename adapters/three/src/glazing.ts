import * as THREE from "three";
import { compileWorldStage, warmupMesh } from "./worldWarmup.ts";

type Position = THREE.BufferAttribute | THREE.InterleavedBufferAttribute;
type Planes = { position: Position; positionVersion: number; index: THREE.BufferAttribute | null; indexVersion: number; values: Float64Array;
  panes?: { bounds: THREE.Box3; diameter: number } };
const geometryPlanes = new WeakMap<THREE.BufferGeometry, Planes>();
const knownGlazing = new WeakMap<THREE.Material, { shader: THREE.Material["onBeforeCompile"]; render: THREE.Material["onBeforeRender"];
  key: THREE.Material["customProgramCacheKey"]; stableSides: boolean }>();

function sideView(source: THREE.Material, side: THREE.Side): THREE.Material {
  // Read the source's current maps, scalar uniforms, shader hooks and version
  // on every draw. Only the renderer identity, cull mode and disposal are local.
  const identity = new THREE.Material();
  return Object.create(source, {
    id: Object.getOwnPropertyDescriptor(identity, "id")!, uuid: { value: identity.uuid }, side: { value: side },
    forceSinglePass: { value: true }, _listeners: { value: undefined, writable: true },
  });
}

type Draw = THREE.WebGLRenderer["renderBufferDirect"];
type ActivePass = { dispatch: Draw; select(material: THREE.Material, object: THREE.Object3D, single: boolean): void };
const activePasses = new WeakMap<THREE.WebGLRenderer, ActivePass>();

/** Three normally invalidates one material twice per two-sided transparent
 * draw. Retain independent renderer properties for each of its cull modes. */
export class GlazingSides {
  private readonly materials = new Map<THREE.Material, { views: THREE.Material[]; release(): void }>();

  views(material: THREE.Material): THREE.Material[] {
    const cached = this.materials.get(material);
    if (cached) return cached.views;
    const views = [THREE.FrontSide, THREE.BackSide, THREE.DoubleSide].map(side => sideView(material, side));
    const release = () => {
      material.removeEventListener("dispose", release);
      this.materials.delete(material);
      for (const view of views) view.dispose();
    };
    this.materials.set(material, { views, release });
    material.addEventListener("dispose", release);
    return views;
  }

  render(renderer: THREE.WebGLRenderer, draw: () => void): void {
    const original = renderer.renderBufferDirect;
    if (typeof original !== "function") { draw(); return; }
    const outer = activePasses.get(renderer);
    let pending: THREE.Material | undefined, placement: THREE.Object3D, single = false, previousSingle = false;
    function restoreMaterial() {
      if (pending) { pending.forceSinglePass = previousSingle; pending = undefined; }
    }
    const dispatch: Draw = (camera, scene, geometry, material, object, group) => {
      if (pending !== material || placement !== object)
        return original.call(renderer, camera, scene, geometry, material, object, group);
      const one = single;
      restoreMaterial();
      const views = this.views(material);
      if (!one) original.call(renderer, camera, scene, geometry, views[THREE.BackSide], object, group);
      original.call(renderer, camera, scene, geometry, views[one ? THREE.DoubleSide : THREE.FrontSide], object, group);
    };
    activePasses.set(renderer, { dispatch, select(material, object, one) {
      restoreMaterial();
      pending = material; placement = object; single = one; previousSingle = material.forceSinglePass;
      // The single renderer submission occupies the original sorted slot.
      // Dispatch its original face sequence there, before the next render item.
      material.forceSinglePass = true;
    } });
    renderer.renderBufferDirect = dispatch;
    try { draw(); } finally {
      restoreMaterial();
      if (renderer.renderBufferDirect === dispatch) renderer.renderBufferDirect = original;
      if (outer) activePasses.set(renderer, outer); else activePasses.delete(renderer);
    }
  }

  dispose() { for (const entry of this.materials.values()) entry.release(); }
}

/** The normal precompile sees the pass count at the loading camera. Prepare
 * the other side configuration that can be selected when panes turn edge-on. */
export async function warmGlazingSides(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera,
  map: THREE.Object3D, target: THREE.WebGLRenderTarget | null, sides?: GlazingSides): Promise<void> {
  const stage = new THREE.Group(), materials = new Set<THREE.Material>();
  map.traverse(object => {
    if (!(object instanceof THREE.Mesh)) return;
    for (const material of Array.isArray(object.material) ? object.material : [object.material]) {
      if (!knownGlazing.has(material) || material.side !== THREE.DoubleSide || !batchableGlazing(material)) continue;
      if (sides && knownGlazing.get(material)!.stableSides) {
        stage.add(warmupMesh(object, sides.views(material)));
      } else {
        stage.add(warmupMesh(object, material));
        materials.add(material);
      }
    }
  });
  if (!stage.children.length) return;
  let ready: Promise<unknown>;
  for (const material of materials) material.forceSinglePass = !material.forceSinglePass;
  try { ready = compileWorldStage(renderer, stage, camera, scene, target); }
  finally {
    for (const material of materials) {
      material.forceSinglePass = !material.forceSinglePass;
      material.needsUpdate = true;
    }
  }
  await ready;
}

/** Only ordinary glTF shading or this adapter's admitted glazing hooks may batch. */
export function batchableGlazing(material: THREE.Material): material is THREE.MeshStandardMaterial {
  if (!(material instanceof THREE.MeshStandardMaterial) || !material.transparent
    || material.userData.worldSpecSurface?.recipe !== "architectural-glazing") return false;
  const known = knownGlazing.get(material);
  return known ? material.onBeforeCompile === known.shader && material.onBeforeRender === known.render && material.customProgramCacheKey === known.key
    : material.onBeforeCompile === THREE.Material.prototype.onBeforeCompile
      && material.onBeforeRender === THREE.Material.prototype.onBeforeRender;
}

function planesOf(geometry: THREE.BufferGeometry): Planes {
  const position = geometry.getAttribute("position"), index = geometry.index;
  const version = position instanceof THREE.InterleavedBufferAttribute ? position.data.version : position.version;
  const cached = geometryPlanes.get(geometry);
  if (cached?.position === position && cached.positionVersion === version && cached.index === index && cached.indexVersion === (index?.version ?? 0))
    return cached;
  const values: number[] = [], distinct = new Set<string>();
  const a = new THREE.Vector3(), b = new THREE.Vector3(), c = new THREE.Vector3();
  for (let i = 0, count = index?.count ?? position.count; i + 2 < count; i += 3) {
    a.fromBufferAttribute(position, index?.getX(i) ?? i);
    b.fromBufferAttribute(position, index?.getX(i + 1) ?? i + 1).sub(a);
    c.fromBufferAttribute(position, index?.getX(i + 2) ?? i + 2).sub(a);
    b.cross(c);
    if (b.lengthSq() === 0) continue;
    b.normalize();
    const plane = [b.x, b.y, b.z, b.dot(a)], key = plane.join(",");
    if (!distinct.has(key)) { distinct.add(key); values.push(...plane); }
  }
  const result = { position, positionVersion: version, index, indexVersion: index?.version ?? 0, values: new Float64Array(values) };
  geometryPlanes.set(geometry, result);
  return result;
}

function paneBounds(record: Planes): { bounds: THREE.Box3; diameter: number } {
  if (record.panes) return record.panes;
  const { position, index } = record;
  const parents = new Int32Array(position.count), canonical = new Map<string, number>();
  const point = new THREE.Vector3(), bounds = new THREE.Box3();
  const root = (vertex: number): number => {
    while (parents[vertex] !== vertex) { parents[vertex] = parents[parents[vertex]]; vertex = parents[vertex]; }
    return vertex;
  };
  for (let i = 0; i < position.count; i++) {
    point.fromBufferAttribute(position, i); bounds.expandByPoint(point);
    const key = point.toArray().join(",");
    parents[i] = canonical.get(key) ?? i;
    canonical.set(key, parents[i]);
  }
  for (let i = 0, count = index?.count ?? position.count; i + 2 < count; i += 3) {
    const a = root(index?.getX(i) ?? i);
    parents[root(index?.getX(i + 1) ?? i + 1)] = a;
    parents[root(index?.getX(i + 2) ?? i + 2)] = a;
  }
  const parts = new Map<number, THREE.Box3>();
  for (let i = 0; i < position.count; i++) {
    const id = root(i);
    if (!parts.has(id)) parts.set(id, new THREE.Box3());
    parts.get(id)!.expandByPoint(point.fromBufferAttribute(position, i));
  }
  let diameter = 0;
  for (const box of parts.values()) diameter = Math.max(diameter, box.getSize(point).length());
  return record.panes = { bounds, diameter };
}

/** Largest connected pane, retaining UV-seam vertex joins. The returned bounds are read-only. */
export function glazingPaneBounds(geometry: THREE.BufferGeometry): { bounds: THREE.Box3; diameter: number } {
  return paneBounds(planesOf(geometry));
}

function smallDistantPanes(record: Planes, object: THREE.Mesh, camera: THREE.Camera, renderer: THREE.WebGLRenderer,
  pixels: number, matrix: THREE.Matrix4, instance: THREE.Matrix4, viewport: THREE.Vector4): boolean {
  if (!(pixels > 0) || !(camera as THREE.PerspectiveCamera).isPerspectiveCamera) return false;
  const { bounds: box, diameter } = paneBounds(record);
  renderer.getCurrentViewport(viewport);
  const p = camera.projectionMatrix.elements;
  const focal = 0.5 * Math.max(viewport.z * Math.abs(p[0]), viewport.w * Math.abs(p[5]));
  const count = object instanceof THREE.InstancedMesh ? object.count : 1;
  for (let i = 0; i < count; i++) {
    matrix.multiplyMatrices(camera.matrixWorldInverse, object.matrixWorld);
    if (object instanceof THREE.InstancedMesh) { object.getMatrixAt(i, instance); matrix.multiply(instance); }
    const m = matrix.elements;
    const center = -m[14] - 0.5 * (m[2] * (box.min.x + box.max.x) + m[6] * (box.min.y + box.max.y) + m[10] * (box.min.z + box.max.z));
    const radius = 0.5 * (Math.abs(m[2]) * (box.max.x - box.min.x) + Math.abs(m[6]) * (box.max.y - box.min.y) + Math.abs(m[10]) * (box.max.z - box.min.z));
    const nearest = center - radius;
    const maxX = Math.abs(m[12] + 0.5 * (m[0] * (box.min.x + box.max.x) + m[4] * (box.min.y + box.max.y) + m[8] * (box.min.z + box.max.z)))
      + 0.5 * (Math.abs(m[0]) * (box.max.x - box.min.x) + Math.abs(m[4]) * (box.max.y - box.min.y) + Math.abs(m[8]) * (box.max.z - box.min.z));
    const maxY = Math.abs(m[13] + 0.5 * (m[1] * (box.min.x + box.max.x) + m[5] * (box.min.y + box.max.y) + m[9] * (box.min.z + box.max.z)))
      + 0.5 * (Math.abs(m[1]) * (box.max.x - box.min.x) + Math.abs(m[5]) * (box.max.y - box.min.y) + Math.abs(m[9]) * (box.max.z - box.min.z));
    const projectionScale = Math.sqrt(1 + (maxX * maxX + maxY * maxY) / (nearest * nearest));
    // sqrt(norm1*normInf) bounds scaling even when hierarchy transforms shear.
    const one = Math.max(Math.abs(m[0]) + Math.abs(m[1]) + Math.abs(m[2]), Math.abs(m[4]) + Math.abs(m[5]) + Math.abs(m[6]), Math.abs(m[8]) + Math.abs(m[9]) + Math.abs(m[10]));
    const infinity = Math.max(Math.abs(m[0]) + Math.abs(m[4]) + Math.abs(m[8]), Math.abs(m[1]) + Math.abs(m[5]) + Math.abs(m[9]), Math.abs(m[2]) + Math.abs(m[6]) + Math.abs(m[10]));
    if (!(nearest >= 20 && diameter * Math.sqrt(one * infinity) * focal * projectionScale / nearest <= pixels)) return false;
  }
  return count > 0;
}

/** Front-facing panes make Three's transparent back pass empty. Keep its two-pass
 * path for back-facing normal maps and whenever both sides can contribute. */
export function enableSinglePassGlazing(material: THREE.MeshStandardMaterial, options: { distantPixels?: number; stableSides?: boolean } = {}): void {
  if (material.userData.worldSpecSurface?.recipe !== "architectural-glazing" || !material.transparent
    || material.side !== THREE.DoubleSide || material.forceSinglePass) return;
  const previous = material.onBeforeRender;
  const shader = material.onBeforeCompile, key = material.customProgramCacheKey;
  const stableSides = options.stableSides ?? (shader === THREE.Material.prototype.onBeforeCompile
    && previous === THREE.Material.prototype.onBeforeRender && key === THREE.Material.prototype.customProgramCacheKey);
  const eye = new THREE.Vector3(), local = new THREE.Vector3();
  const inverse = new THREE.Matrix4(), instance = new THREE.Matrix4();
  const viewTransform = new THREE.Matrix4(), viewport = new THREE.Vector4();
  const classify: THREE.Material["onBeforeRender"] = function(this: THREE.Material, renderer, scene, camera, geometry, object, group) {
    previous.call(this, renderer, scene, camera, geometry, object, group);
    let single = false;
    if (object instanceof THREE.Mesh && !(object instanceof THREE.SkinnedMesh) && !(object as THREE.BatchedMesh).isBatchedMesh
      && !geometry.morphAttributes.position?.length && !(geometry instanceof THREE.InstancedBufferGeometry)
      && material.onBeforeCompile === shader && camera.matrixWorld.determinant() > 0
      && !material.displacementMap && geometry.getAttribute("position") && material.side === THREE.DoubleSide) {
      const record = planesOf(geometry), planes = record.values;
      const perspective = (camera as THREE.PerspectiveCamera).isPerspectiveCamera;
      inverse.copy(object.matrixWorld).invert();
      if (perspective) camera.getWorldPosition(eye).applyMatrix4(inverse);
      else camera.getWorldDirection(eye).transformDirection(inverse).negate();
      let facing = 0;
      single = planes.length > 0;
      const count = object instanceof THREE.InstancedMesh ? object.count : 1;
      for (let placement = 0; single && placement < count; placement++) {
        local.copy(eye);
        if (object instanceof THREE.InstancedMesh) {
          object.getMatrixAt(placement, instance);
          if (instance.determinant() <= 0) { single = false; break; }
          instance.invert();
          if (perspective) local.applyMatrix4(instance);
          else local.transformDirection(instance);
        }
        for (let i = 0; i < planes.length; i += 4) {
          const side = local.x * planes[i] + local.y * planes[i + 1] + local.z * planes[i + 2]
            - (perspective ? planes[i + 3] : 0);
          if (!Number.isFinite(side) || Math.abs(side) < 1e-7) { single = false; break; }
          const sign = Math.sign(side);
          if (facing && sign !== facing) { single = false; break; }
          facing = sign;
        }
      }
      // The DoubleSide and FrontSide shaders agree on front-facing fragments.
      // BackSide uses a different normal-map tangent convention, so retain it.
      single &&= facing > 0;
      if (!single) single = smallDistantPanes(record, object, camera, renderer, options.distantPixels ?? 0, viewTransform, instance, viewport);
    }
    const pass = stableSides ? activePasses.get(renderer) : undefined;
    if (pass && renderer.renderBufferDirect === pass.dispatch && material.onBeforeRender === classify
      && material.onBeforeCompile === shader && material.customProgramCacheKey === key
      && material.transparent && material.side === THREE.DoubleSide) {
      pass.select(material, object, single);
    } else if (material.forceSinglePass !== single) {
      material.forceSinglePass = single;
      material.needsUpdate = true;
    }
  };
  material.onBeforeRender = classify;
  knownGlazing.set(material, { shader, render: classify, key, stableSides });
}
