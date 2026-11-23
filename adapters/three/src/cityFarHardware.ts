import * as THREE from "three";
import type { CityLodLevels } from "./cityLods.ts";
import { copyInstanceTint, uploadInstanceTints } from "./instanceTint.ts";

type Member = { bounds: THREE.Box3; level: number; keep: boolean };
type Assembly = { source: THREE.Mesh; parent: THREE.Object3D; levels: CityLodLevels; members: Member[];
  draws: Map<number, { mesh: THREE.Mesh; indices: number[] }> };

/** Spatial cells help a pedestrian see through doors and around corners. In a
 * wide view, submit a distant hardware atlas once per LOD for all its visible
 * buildings. Reuse the published index stream and the original surface shader. */
export class CityFarHardware {
  private readonly assemblies: Assembly[] = [];
  private readonly children = new WeakMap<THREE.Mesh, Member[]>();
  private readonly active: THREE.Mesh[] = [];
  private readonly matrix = new THREE.Matrix4();
  private readonly eye = new THREE.Vector3();

  register(source: THREE.Mesh, pieces: THREE.Mesh[], levels: CityLodLevels): void {
    const material = source.material;
    if (!source.parent || pieces.length < 2 || Array.isArray(material) || !(material instanceof THREE.MeshStandardMaterial)
      || material.transparent || material.alphaTest || material.alphaHash || material.emissiveMap
      || (material.emissiveIntensity > 0 && material.emissive.getHex() !== 0) || material.userData.cityDetailKeep) return;
    const finishes = material.userData.sourceFinishes;
    if (finishes !== undefined && (!Array.isArray(finishes) || finishes.some(finish => typeof finish !== "string"
      || /\b(?:sign|signage|lettering|poster|posters|tube-light|lens|emissive)\b/i.test(finish)))) return;
    const surface = material.userData.worldSpecSurface;
    if (surface?.family !== "Building atlases" || surface.surface !== "Mixed surfaces with metal"
      || !["weathered-steel", "stock-hardware"].includes(surface.recipe)) return;
    const members: Member[] = [];
    source.updateWorldMatrix(true, false);
    if (!source.geometry.boundingBox) source.geometry.computeBoundingBox();
    const count = source instanceof THREE.InstancedMesh ? source.count : 1;
    for (let i = 0; i < count; i++) {
      if (source instanceof THREE.InstancedMesh) {
        source.getMatrixAt(i, this.matrix);
        this.matrix.premultiply(source.matrixWorld);
      } else this.matrix.copy(source.matrixWorld);
      members.push({ bounds: source.geometry.boundingBox!.clone().applyMatrix4(this.matrix), level: 0, keep: false });
    }
    this.assemblies.push({ source, parent: source.parent, levels, members, draws: new Map() });
    for (const piece of pieces) this.children.set(piece, members);
  }

  member(mesh: THREE.Mesh, index: number): Member | undefined { return this.children.get(mesh)?.[index]; }

  select(camera: THREE.PerspectiveCamera, height: number): void {
    this.eye.setFromMatrixPosition(camera.matrixWorld);
    const m = camera.matrixWorldInverse.elements;
    const focal = height * Math.abs(camera.projectionMatrix.elements[5]) / 2;
    for (const { levels, members } of this.assemblies) for (const member of members) {
      const { bounds: b, level: previous } = member;
      member.keep = false;
      member.level = 0;
      if (b.distanceToPoint(this.eye) < (previous ? 16 : 20)) continue;
      // Minimum view-axis depth of the entire atlas, including off-center buildings.
      const depth = -m[14] - m[2] * (m[2] >= 0 ? b.max.x : b.min.x)
        - m[6] * (m[6] >= 0 ? b.max.y : b.min.y) - m[10] * (m[10] >= 0 ? b.max.z : b.min.z);
      if (depth < Math.max(12, camera.near)) continue;
      const budget = Math.min(32, 10 + Math.max(0, depth - 18) / 2);
      for (let i = levels.length - 1; i >= 1; i--) {
        const error = levels[i].userData.cityLodErrorMeters;
        if (Number.isFinite(error) && error * focal <= depth * budget * (previous > i ? 1.12 : 1)) {
          member.level = i + 1;
          break;
        }
      }
    }
  }

  finish(): void {
    this.active.length = 0;
    for (const { source, parent, levels, members, draws } of this.assemblies) {
      // An entirely hidden building never schedules a far draw. A visible cell
      // admits the whole coarse primitive, trading some overdraw for fewer calls.
      for (let level = 1; level <= levels.length; level++) {
        const geometry = levels[level - 1];
        if (!geometry.index?.count) continue;
        let draw = draws.get(level), count = 0, dirty = false;
        for (let i = 0; i < members.length; i++) {
          if (!members[i].keep || members[i].level !== level) continue;
          if (!draw) {
            const mesh = source.clone(false);
            mesh.geometry = geometry;
            mesh.castShadow = false;
            mesh.raycast = () => {};
            mesh.visible = false;
            if (mesh instanceof THREE.InstancedMesh) mesh.boundingBox = mesh.boundingSphere = null;
            parent.add(mesh);
            draws.set(level, draw = { mesh, indices: [] });
          }
          if (source instanceof THREE.InstancedMesh && draw.mesh instanceof THREE.InstancedMesh && draw.indices[count] !== i) {
            source.getMatrixAt(i, this.matrix);
            draw.mesh.setMatrixAt(count, this.matrix);
            copyInstanceTint(source, i, draw.mesh, count);
            draw.indices[count] = i;
            dirty = true;
          }
          count++;
        }
        if (draw && count) {
          if (draw.mesh instanceof THREE.InstancedMesh) {
            draw.mesh.count = count;
            if (dirty) {
              draw.mesh.instanceMatrix.clearUpdateRanges();
              draw.mesh.instanceMatrix.addUpdateRange(0, count * 16);
              draw.mesh.instanceMatrix.needsUpdate = true;
              uploadInstanceTints(draw.mesh, count);
            }
            // Per-cell camera tests admitted these placements; a sphere computed
            // for a previous subset would incorrectly reject a later subset.
            draw.mesh.frustumCulled = false;
          }
          this.active.push(draw.mesh);
        }
      }
    }
  }

  restore(): void { for (const mesh of this.active) mesh.visible = false; }
  show(): void { for (const mesh of this.active) mesh.visible = true; }
  dispose(): void {
    for (const assembly of this.assemblies) for (const { mesh } of assembly.draws.values()) {
      mesh.removeFromParent();
      if (mesh instanceof THREE.InstancedMesh) mesh.dispose();
    }
    this.active.length = this.assemblies.length = 0;
  }
}
