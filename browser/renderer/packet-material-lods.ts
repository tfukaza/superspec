import * as THREE from "three";
import type { SceneMaterial } from "../model/packet.ts";
import { selectAuthoredLod } from "../../adapters/three/src/lodSelection.ts";
import { applyLiveOperations } from "./live.ts";

export type PacketMaterialContext = {
  batch: boolean; visible: boolean; wireframe: boolean;
  texture(id: string | null, color: boolean): Promise<THREE.Texture | null>;
  lookup(id: string): THREE.Texture | undefined;
};

/** The same delivered-channel binding is used for both original and LOD looks. */
export async function configurePacketMaterial(material: THREE.MeshStandardMaterial, definition: SceneMaterial | undefined,
  context: PacketMaterialContext): Promise<string[]> {
  material.vertexColors = true; material.wireframe = context.wireframe;
  if (definition?.baseColorFactor) material.color.fromArray(definition.baseColorFactor);
  else material.color.set(definition?.color ?? (context.batch ? "#ffffff" : "#b9c7d1"));
  material.roughness = definition?.roughness ?? (context.batch ? .9 : .8);
  material.metalness = definition?.metalness ?? 0;
  if (!definition || !context.visible) return [];
  if (definition.emissiveFactor) material.emissive.fromArray(definition.emissiveFactor);
  else material.emissive.set(definition.emissive ?? "#000000");
  material.emissiveIntensity = definition.emissiveStrength ?? 1;
  material.opacity = definition.alphaMode === "OPAQUE" ? 1 : definition.opacity ?? 1;
  material.transparent = definition.alphaMode == null ? material.opacity < 1 : definition.alphaMode === "BLEND";
  material.depthWrite = !material.transparent;
  material.side = definition.doubleSided == null ? (material.transparent ? THREE.FrontSide : THREE.DoubleSide)
    : definition.doubleSided ? THREE.DoubleSide : THREE.FrontSide;
  material.alphaTest = definition.alphaMode == null ? definition.alphaCutoff ?? 0
    : definition.alphaMode === "MASK" ? definition.alphaCutoff ?? .5 : 0;
  material.alphaToCoverage = material.alphaTest > 0; material.polygonOffset = material.alphaTest > 0;
  material.polygonOffsetFactor = -1; material.polygonOffsetUnits = -2;
  material.map = await context.texture(definition.texture, true);
  material.normalMap = await context.texture(definition.normalTexture, false);
  material.normalScale.setScalar(definition.normalScale ?? 1);
  material.roughnessMap = material.metalnessMap = await context.texture(definition.metallicRoughnessTexture, false);
  material.emissiveMap = await context.texture(definition.emissiveTexture ?? null, true);
  material.aoMap = await context.texture(definition.occlusionTexture ?? null, false);
  material.aoMapIntensity = definition.occlusionStrength ?? 1;
  return applyLiveOperations(material, definition.live ?? [], context.lookup);
}

export type PreparedPacketMaterialLod = { mesh: THREE.Mesh; levels: THREE.Material[]; screenCoverage: number[] };
type Entry = PreparedPacketMaterialLod & { near: THREE.Material; geometry: THREE.BufferGeometry; level: number;
  hint: { kind: "coverage"; values: number[] };
  applied?: THREE.Material; raycast: THREE.Mesh["raycast"]; wrapper: THREE.Mesh["raycast"] };

/** Source texture leases remain owned by RustScene's existing pool. This owns
 * only alternate material objects; geometry, near looks and picking stay intact. */
export class PacketMaterialLods {
  private readonly entries: Entry[] = [];
  private readonly bounds = new THREE.Box3();
  private disposed = false;

  constructor(prepared: readonly PreparedPacketMaterialLod[]) {
    for (const source of prepared) {
      if (!source.levels.length || source.levels.length !== source.screenCoverage.length || Array.isArray(source.mesh.material)) continue;
      const entry = { ...source, near: source.mesh.material, geometry: source.mesh.geometry,
        level: 0, hint: { kind: "coverage", values: [...source.screenCoverage, 0] }, raycast: source.mesh.raycast } as Entry;
      entry.wrapper = (raycaster, hits) => {
        const material = entry.mesh.material; entry.mesh.material = entry.near;
        try { return entry.raycast.call(entry.mesh, raycaster, hits); } finally { entry.mesh.material = material; }
      };
      entry.mesh.raycast = entry.wrapper; this.entries.push(entry);
    }
  }

  get size(): number { return this.entries.length; }
  get fieldReads(): string[] { return this.entries.flatMap(entry => entry.levels.flatMap(material => material.userData.sceneFields ?? [])); }

  restore(): void {
    for (const entry of this.entries) {
      if (entry.applied && entry.mesh.material === entry.applied) entry.mesh.material = entry.near;
      else if (!Array.isArray(entry.mesh.material)) entry.near = entry.mesh.material;
      entry.applied = undefined;
    }
  }

  frame(camera: THREE.Camera, height: number, editing: boolean): void {
    this.restore();
    if (this.disposed || editing || !this.entries.length) return;
    camera.updateWorldMatrix(true, false);
    for (const entry of this.entries) {
      if (!entry.mesh.visible || entry.mesh.geometry !== entry.geometry || entry.mesh.material !== entry.near) continue;
      entry.mesh.updateWorldMatrix(true, false);
      if (entry.mesh instanceof THREE.InstancedMesh) {
        if (!entry.mesh.boundingBox) entry.mesh.computeBoundingBox();
        this.bounds.copy(entry.mesh.boundingBox!);
      } else {
        if (!entry.geometry.boundingBox) entry.geometry.computeBoundingBox();
        this.bounds.copy(entry.geometry.boundingBox!);
      }
      this.bounds.applyMatrix4(entry.mesh.matrixWorld);
      entry.level = selectAuthoredLod(this.bounds, camera, height,
        entry.hint, entry.level);
      if (entry.level) entry.mesh.material = entry.applied = entry.levels[entry.level - 1];
    }
  }

  dispose(): void {
    if (this.disposed) return;
    this.restore(); this.disposed = true;
    const retained = new Set(this.entries.map(entry => entry.near));
    for (const entry of this.entries) if (entry.mesh.raycast === entry.wrapper) entry.mesh.raycast = entry.raycast;
    for (const material of new Set(this.entries.flatMap(entry => entry.levels))) if (!retained.has(material)) material.dispose();
    this.entries.length = 0;
  }
}
