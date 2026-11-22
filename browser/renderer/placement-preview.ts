import { Box3, BufferAttribute, BufferGeometry, Group, InstancedMesh, Matrix4, Mesh, MeshLambertMaterial, Plane, Vector3 } from "three";
import { footprintSide, splitFootprintMesh } from "./footprint-split.ts";
import type { SceneBatch, SceneMesh } from "../model/packet.ts";

export type Footprint = readonly (readonly number[])[];
interface FootprintEdit { from: Footprint; to: Footprint; origins: readonly (readonly number[])[]; heading: number }
export interface PlacementMove { owners: readonly string[]; translation: readonly number[]; footprint?: FootprintEdit }

interface Draw {
  mesh: Mesh;
  matrix: Matrix4;
  position: Vector3;
  autoUpdate: boolean;
  layers: number;
  index?: number;
  split?: 0 | 1 | [Mesh, Mesh];
  splitGeometry?: BufferGeometry;
}
interface Owner {
  draws: Draw[];
  top: number;
  proxy?: Mesh<BufferGeometry, MeshLambertMaterial>;
  diagonal?: Plane;
  detailed?: boolean;
}

function ordinaryDraw(mesh: Mesh): Draw {
  if (mesh.matrixAutoUpdate) mesh.updateMatrix();
  return { mesh, matrix: mesh.matrix.clone(), position: mesh.position.clone(), autoUpdate: mesh.matrixAutoUpdate, layers: mesh.layers.mask };
}

/** Map the three corners on one side of the lot diagonal. Both transforms agree along its entire length. */
function localTransform(from: Footprint, to: Footprint, corner: 0 | 2): Matrix4 {
  const x = from[1][0] - from[corner][0], y = from[1][1] - from[corner][1];
  const u = from[3][0] - from[corner][0], v = from[3][1] - from[corner][1], det = x*v - y*u;
  const dx = to[1][0] - to[corner][0], dy = to[1][1] - to[corner][1];
  const du = to[3][0] - to[corner][0], dv = to[3][1] - to[corner][1];
  const a = (dx*v - du*y)/det, b = (du*x - dx*u)/det;
  const c = (dy*v - dv*y)/det, d = (dv*x - dy*u)/det;
  const tx = to[corner][0] - a*from[corner][0] - b*from[corner][1], ty = to[corner][1] - c*from[corner][0] - d*from[corner][1];
  return new Matrix4().set(a,b,0,tx, c,d,0,ty, 0,0,1,0, 0,0,0,1);
}

function releaseSplit(draw: Draw): void {
  if (Array.isArray(draw.split)) for (const half of draw.split) { half.removeFromParent(); half.geometry.dispose(); }
  draw.split = draw.splitGeometry = undefined;
}

/** Budget only receivers that straddle the diagonal. Receivers wholly on either side need just a draw matrix. */
function detailedTaper(owner: Owner, diagonal: Plane): boolean {
  if (owner.diagonal?.equals(diagonal)) return owner.detailed!;
  for (const draw of owner.draws) releaseSplit(draw);
  owner.diagonal = diagonal;
  let bytes = 0;
  const inverse = new Matrix4();
  return owner.detailed = owner.draws.every(({ mesh, index, matrix }) => {
    if (!mesh.visible) return true;
    const geometry = mesh.geometry;
    if (index !== undefined || Array.isArray(mesh.material) || geometry.drawRange.start !== 0
      || geometry.drawRange.count !== Infinity || geometry.morphAttributes.position) return false;
    if (footprintSide(geometry, diagonal.clone().applyMatrix4(inverse.copy(matrix).invert())) !== undefined) return true;
    const vertices = geometry.getAttribute("position").count, corners = geometry.index?.count ?? vertices;
    const stride = Object.values(geometry.attributes).reduce((sum, attr) => sum + attr.itemSize * 4, 0);
    // At most two cut vertices and three output triangles per source triangle.
    bytes += (vertices + 2 * corners / 3) * stride + corners * 12;
    return bytes <= 64 * 1024 * 1024;
  });
}

/** Draw a non-affine provisional lot with exactly the requested silhouette, using the current building's height. */
function updateHull(owner: Owner, edit: FootprintEdit, index: number, move: PlacementMove,
  group: Group, material: MeshLambertMaterial): void {
  const origin = edit.origins[index], height = owner.top - origin[2];
  if (!owner.proxy) {
    const geometry = new BufferGeometry();
    geometry.setAttribute("position", new BufferAttribute(new Float32Array(8 * 3), 3));
    geometry.setIndex([4,5,6, 4,6,7, 0,2,1, 0,3,2,
      0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7]);
    owner.proxy = new Mesh(geometry, material);
    owner.proxy.castShadow = owner.draws.some(draw => draw.mesh.castShadow);
    group.add(owner.proxy);
  }
  const mesh = owner.proxy, positions = mesh.geometry.getAttribute("position");
  edit.to.forEach(([x, y], i) => {
    positions.setXYZ(i, x, y, 0);
    positions.setXYZ(i + 4, x, y, height);
  });
  positions.needsUpdate = true;
  mesh.geometry.computeVertexNormals();
  mesh.geometry.computeBoundingSphere();
  mesh.position.set(origin[0] + move.translation[0], origin[1] + move.translation[1], origin[2] + move.translation[2]);
  mesh.rotation.z = edit.heading * Math.PI / 180;
  mesh.visible = true;
}

/** Temporary draw transforms. Each drag sample starts from its accepted matrices, including shared batch instances. */
export class PlacementTransforms {
  readonly hulls = new Group();
  readonly pieces = new Group();
  private material = new MeshLambertMaterial({ color: 0xa29c8e, flatShading: true });
  private owners = new Map<string, Owner>();
  private originals = new Map<InstancedMesh, { box: InstancedMesh["boundingBox"]; sphere: InstancedMesh["boundingSphere"]; cull: boolean }>();
  private meshes?: Mesh[];
  private batches?: Record<string, string[]>;
  private wholeTranslation = false;
  private footprint = false;
  private readonly scratch = new Matrix4();

  clear(): boolean {
    const changed = this.owners.size > 0;
    for (const owner of this.owners.values()) {
      owner.proxy?.geometry.dispose();
      for (const draw of owner.draws) releaseSplit(draw);
      for (const { mesh, index, matrix, position, autoUpdate, layers } of owner.draws) {
        if (index === undefined) {
          mesh.layers.mask = layers;
          mesh.matrix.copy(matrix);
          mesh.position.copy(position);
          mesh.matrixAutoUpdate = autoUpdate;
          mesh.matrixWorldNeedsUpdate = true;
        } else this.setInstance(mesh as InstancedMesh, index, matrix);
      }
    }
    for (const [mesh, { box, sphere, cull }] of this.originals) {
      mesh.boundingBox = box;
      mesh.boundingSphere = sphere;
      mesh.frustumCulled = cull;
    }
    this.hulls.clear();
    this.pieces.visible = true;
    this.originals.clear();
    this.owners.clear();
    this.meshes = undefined;
    this.batches = undefined;
    this.footprint = false;
    return changed;
  }

  dispose(): void { this.clear(); this.material.dispose(); }

  footprintOwners(): Set<string> | undefined {
    return this.footprint ? new Set(this.owners.keys()) : undefined;
  }

  /** A regenerate can finish ahead of its materials. Hide only the affected provisional draws. */
  showRegenerated(retained: ReadonlySet<Mesh>): void {
    this.pieces.visible = false;
    this.scratch.makeScale(0, 0, 0);
    for (const owner of this.owners.values()) {
      if (owner.proxy) owner.proxy.visible = false;
      for (const { mesh, index, matrix, position, autoUpdate, layers } of owner.draws) {
        if (index === undefined && retained.has(mesh)) {
          // This receiver's world triangles are already correct. Keep its original chart and material in the scene.
          mesh.layers.mask = layers;
          mesh.position.copy(position);
          mesh.matrix.copy(matrix);
          mesh.matrixAutoUpdate = autoUpdate;
          mesh.matrixWorldNeedsUpdate = true;
        } else if (index === undefined) mesh.layers.disableAll();
        else this.setInstance(mesh as InstancedMesh, index, this.scratch);
      }
    }
  }

  private setInstance(mesh: InstancedMesh, index: number, matrix: Matrix4): void {
    mesh.setMatrixAt(index, matrix);
    mesh.instanceMatrix.addUpdateRange(index * 16, 16);
    mesh.instanceMatrix.needsUpdate = true;
  }

  private bind(meshes: Mesh[], batches: Record<string, string[]>, ids: Set<string>, wholeTranslation: boolean): boolean {
    if (this.meshes === meshes && this.batches === batches && this.wholeTranslation === wholeTranslation && this.owners.size === ids.size
      && [...ids].every(id => this.owners.has(id))) return true;
    this.clear();
    const owners = new Map<string, Owner>([...ids].map(id => [id, { draws: [], top: -Infinity }]));
    const found = new Set<string>();
    const bounds = new Box3();
    for (const mesh of meshes) {
      const source = mesh.userData.source as SceneMesh | SceneBatch;
      if (mesh instanceof InstancedMesh && "template" in source && source.template) {
        // Without a placement owner for each instance we cannot account for every draw of the moved object.
        const listed = batches[source.objectId];
        if (!listed || listed.length !== mesh.count) return false;
        if (wholeTranslation && listed.length && listed.every(id => owners.has(id))) {
          // A row moved as one only changes each batch's draw transform. No per-instance writes, copies or uploads.
          owners.get(listed[0])!.draws.push(ordinaryDraw(mesh));
          for (const id of listed) found.add(id);
          continue;
        }
        for (let index = 0; index < listed.length; index++) {
          const owner = owners.get(listed[index]);
          if (!owner) continue;
          found.add(listed[index]);
          const matrix = new Matrix4();
          mesh.getMatrixAt(index, matrix);
          owner.draws.push({ mesh, index, matrix, position: mesh.position, autoUpdate: mesh.matrixAutoUpdate, layers: mesh.layers.mask });
          if (source.localBounds) {
            bounds.min.fromArray(source.localBounds[0]); bounds.max.fromArray(source.localBounds[1]);
            owner.top = Math.max(owner.top, bounds.applyMatrix4(matrix).max.z);
          }
        }
      } else {
        const owner = owners.get(source.objectId);
        if (!owner) continue;
        found.add(source.objectId);
        owner.draws.push(ordinaryDraw(mesh));
        if (source.bounds) owner.top = Math.max(owner.top, source.bounds[1][2]);
      }
    }
    if (found.size !== ids.size) return false;
    for (const owner of owners.values()) for (const { mesh, index } of owner.draws) {
      if (index === undefined) continue;
      const batch = mesh as InstancedMesh;
      if (!this.originals.has(batch)) this.originals.set(batch, { box: batch.boundingBox, sphere: batch.boundingSphere, cull: batch.frustumCulled });
      batch.frustumCulled = false;
      batch.boundingBox = batch.boundingSphere = null;
    }
    this.meshes = meshes;
    this.batches = batches;
    this.wholeTranslation = wholeTranslation;
    this.owners = owners;
    return true;
  }

  apply(meshes: Mesh[], batches: Record<string, string[]>, moves: readonly PlacementMove[]): boolean {
    if (!moves.length) { this.clear(); return false; }
    if (!this.bind(meshes, batches, new Set(moves.flatMap(move => [...move.owners])), moves.length === 1 && !moves[0].footprint)) return false;
    this.footprint = moves.every(move => !!move.footprint);
    this.pieces.visible = true;
    for (const move of moves) {
      const edit = move.footprint, first = edit && localTransform(edit.from, edit.to, 0);
      const bend = edit && first && new Vector3(edit.from[2][0], edit.from[2][1], 0).applyMatrix4(first)
        .distanceToSquared(new Vector3(edit.to[2][0], edit.to[2][1], 0)) > 1e-14;
      const second = bend && edit ? localTransform(edit.from, edit.to, 2) : undefined;
      const delta = move.translation;
      for (const [i, id] of move.owners.entries()) {
        const owner = this.owners.get(id)!;
        const transform = new Matrix4().makeTranslation(delta[0], delta[1], delta[2]);
        let opposite: Matrix4 | undefined, diagonal: Plane | undefined;
        if (edit && first) {
          const origin = edit.origins[i], radians = edit.heading * Math.PI / 180;
          const lot = new Matrix4().makeRotationZ(radians).setPosition(origin[0], origin[1], origin[2]);
          const inverse = lot.clone().invert();
          transform.multiply(lot);
          if (second) {
            opposite = transform.clone().multiply(second).multiply(inverse);
            const [a, b] = [edit.from[1], edit.from[3]];
            const normal = new Vector3(b[1] - a[1], a[0] - b[0], 0).normalize();
            diagonal = new Plane(normal, -normal.x * a[0] - normal.y * a[1]).applyMatrix4(lot);
          }
          transform.multiply(first).multiply(inverse);
        }
        // A partially selected instance cannot be split without duplicating its shared prototype.
        const hull = diagonal && !detailedTaper(owner, diagonal);
        if (edit && hull) {
          if (!Number.isFinite(owner.top) || owner.top <= edit.origins[i][2]) { this.clear(); return false; }
          updateHull(owner, edit, i, move, this.hulls, this.material);
          transform.makeScale(0, 0, 0);
        } else if (owner.proxy) owner.proxy.visible = false;
        for (const draw of owner.draws) {
          const { mesh, index, matrix, position, autoUpdate, layers } = draw;
          let placed = transform;
          if (!hull && opposite && diagonal && mesh.visible) {
            if (draw.splitGeometry !== mesh.geometry) {
              releaseSplit(draw);
              draw.splitGeometry = mesh.geometry;
              draw.split = splitFootprintMesh(mesh, diagonal.clone().applyMatrix4(this.scratch.copy(matrix).invert()));
              if (Array.isArray(draw.split)) this.pieces.add(...draw.split);
            }
            if (draw.split === 1) placed = opposite;
          }
          if (Array.isArray(draw.split)) {
            for (const [side, part] of draw.split.entries()) {
              part.visible = !hull && !!opposite;
              if (part.visible) {
                part.material = mesh.material;
                part.layers.mask = layers;
                part.matrix.multiplyMatrices(side ? opposite! : placed, matrix);
                part.matrixWorldNeedsUpdate = true;
              }
            }
          }
          if (index === undefined) mesh.layers.mask = hull || (opposite && Array.isArray(draw.split)) ? 0 : layers;
          if (index !== undefined) this.setInstance(mesh as InstancedMesh, index, this.scratch.multiplyMatrices(transform, matrix));
          else if (!edit && autoUpdate) {
            mesh.matrixAutoUpdate = true;
            mesh.position.set(position.x + delta[0], position.y + delta[1], position.z + delta[2]);
          } else {
            // An oriented footprint scale can shear a rotated mesh. Keep the matrix intact without TRS decomposition.
            mesh.matrixAutoUpdate = false;
            mesh.matrix.multiplyMatrices(placed, matrix);
            mesh.matrixWorldNeedsUpdate = true;
          }
        }
      }
    }
    // Three may have cached bounds while sorting a previous sample. Partially moved batches need fresh sort centers.
    for (const batch of this.originals.keys()) batch.boundingBox = batch.boundingSphere = null;
    return true;
  }
}
