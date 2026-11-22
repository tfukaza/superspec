import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { selectAuthoredLod } from "./lodSelection.ts";

type Record = { id: string; proxy: number; sources: number[]; distance: number; bounds: { min: number[]; max: number[] } };
type Block = {
  record: Record; sections: THREE.Group[]; proxy: THREE.Object3D; bounds: THREE.Box3; worldBounds: THREE.Box3;
  active: boolean; submitted: boolean; distance: number;
};

const vec3 = (v: unknown): v is number[] => Array.isArray(v) && v.length === 3 && v.every(Number.isFinite);

function includeSourceBounds(bounds: THREE.Box3, scene: THREE.Object3D, roots: THREE.Object3D[]): void {
  scene.updateWorldMatrix(true, false);
  const inverse = scene.matrixWorld.clone().invert(), matrix = new THREE.Matrix4(), local = new THREE.Box3();
  for (const root of roots) {
    root.updateWorldMatrix(false, true);
    root.traverse(node => {
      if (!(node instanceof THREE.Mesh)) return;
      const geometry = node instanceof THREE.InstancedMesh ? node : node.geometry;
      if (!geometry.boundingBox) geometry.computeBoundingBox();
      bounds.union(local.copy(geometry.boundingBox!).applyMatrix4(matrix.multiplyMatrices(inverse, node.matrixWorld)));
    });
  }
}

/** Optional static, root-level blocks in a published glTF. Meshes stay in their
 * original coordinate system under identity groups. Group boundaries survive
 * ordinary city instancing, clustering and per-placement LOD selection. */
export class BlockLods {
  private readonly blocks: Block[] = [];
  private readonly detailGroups = new WeakMap<THREE.Object3D, THREE.Object3D>();
  readonly hidden = new Set<THREE.Object3D>();
  private enabled = true;
  private readonly eye = new THREE.Vector3();

  private constructor(private readonly scene: THREE.Object3D) {}

  static async load(gltf: GLTF): Promise<BlockLods | undefined> {
    const manifest = gltf.scene.userData.worldSpecBlockLods;
    if (manifest?.version !== 1 || !Array.isArray(manifest.blocks)) return;
    const result = new BlockLods(gltf.scene);
    const { parser } = gltf, json = parser.json;
    const active = new Map<number, THREE.Object3D>();
    for (const source of gltf.scene.children) {
      const index = parser.associations.get(source)?.nodes;
      if (index !== undefined) active.set(index, source);
    }
    const referenced = new Set<number>((json.scenes ?? []).flatMap((s: {nodes?: number[]}) => s.nodes ?? []));
    for (const node of json.nodes) {
      for (const child of node.children ?? []) referenced.add(child);
      for (const lower of node.extensions?.MSFT_lod?.ids ?? []) referenced.add(lower);
    }
    const animated = new Set<number>((json.animations ?? []).flatMap((a: {channels: {target: {node?: number}}[]}) =>
      a.channels.map(c => c.target.node)));
    const claimed = new Set<number>();
    const staticMesh = (index: number) => {
      const node = json.nodes[index], mesh = json.meshes?.[node?.mesh];
      return Number.isSafeInteger(index) && index >= 0 && mesh?.primitives?.length && !node.children?.length
        && node.skin === undefined && node.camera === undefined && !node.extensions?.KHR_lights_punctual
        && !node.extensions?.MSFT_lod && !animated.has(index)
        && !mesh.primitives.some((p: {targets?: unknown[]}) => p.targets?.length);
    };
    for (const record of manifest.blocks as Record[]) {
      try {
        if (!record || typeof record.id !== "string" || !record.id || !Array.isArray(record.sources) || !record.sources.length
            || !(record.distance > 0) || !Number.isFinite(record.distance) || !vec3(record.bounds?.min) || !vec3(record.bounds.max)
            || record.bounds.min.some((v, i) => v > record.bounds.max[i])
            || new Set([...record.sources, record.proxy]).size !== record.sources.length + 1
            || [record.proxy, ...record.sources].some(id => claimed.has(id) || !staticMesh(id))
            || referenced.has(record.proxy) || record.sources.some(id => !active.has(id)))
          throw Error("Expected distinct static source roots and a detached proxy with metric bounds");
        const proxy = await parser.getDependency("node", record.proxy) as THREE.Object3D;
        if (proxy.parent) throw Error("Proxy already has a parent");
        const sources = record.sources.map(id => active.get(id)!);
        for (const source of sources) source.traverse(node => {
          if (!node.visible || node instanceof THREE.Mesh
            && (Array.isArray(node.material) ? node.material : [node.material]).some(material => !material.visible))
            throw Error("A hidden source must retain its authored visibility");
        });
        const meshes: THREE.Mesh[] = [];
        proxy.traverse(node => { if (node instanceof THREE.Mesh) meshes.push(node); });
        const primitives = json.meshes[json.nodes[record.proxy].mesh].primitives;
        if (!meshes.length || meshes.length !== primitives.length || meshes.some((mesh, i) => {
          const m = mesh.material as THREE.MeshStandardMaterial;
          const definition = json.materials?.[primitives[i].material];
          return !mesh.geometry.attributes.position || mesh instanceof THREE.SkinnedMesh || Array.isArray(m)
            || definition?.pbrMetallicRoughness?.baseColorTexture && !m.map
            || definition?.pbrMetallicRoughness?.metallicRoughnessTexture && !(m.roughnessMap && m.metalnessMap)
            || definition?.normalTexture && !m.normalMap || definition?.emissiveTexture && !m.emissiveMap
            || definition?.occlusionTexture && !m.aoMap;
        }))
          throw Error("Incomplete proxy mesh or atlas");
        const bounds = new THREE.Box3(new THREE.Vector3(...record.bounds.min), new THREE.Vector3(...record.bounds.max));
        includeSourceBounds(bounds, gltf.scene, sources);
        if (![...bounds.min, ...bounds.max].every(Number.isFinite)) throw Error("Invalid source mesh bounds");
        // Preserve traversal order: batching, raster sort ties and allocation of
        // instance indices must agree with the core scene while detail is active.
        const sections = sources.map(source => {
          const group = new THREE.Group();
          group.name = record.id + " / detailed";
          group.matrixAutoUpdate = false;
          const index = gltf.scene.children.indexOf(source);
          group.add(source);
          gltf.scene.add(group);
          gltf.scene.children.splice(index, 0, gltf.scene.children.pop()!);
          return group;
        });
        const block = { record, sections, proxy, bounds, worldBounds: bounds.clone(), active: false, submitted: false, distance: 0 };
        for (const mesh of meshes) {
          mesh.raycast = () => false;
          mesh.onAfterRender = (_renderer, scene, _camera, _geometry, material) => {
            if (!scene.overrideMaterial && material.colorWrite) block.submitted = true;
          };
        }
        proxy.visible = false;
        gltf.scene.add(proxy);
        sections.forEach(group => result.detailGroups.set(group, sections[0]));
        result.blocks.push(block);
        for (const index of [...record.sources, record.proxy]) claimed.add(index);
      } catch (error) { console.warn(`Keeping full detail for WorldSpec block ${record?.id ?? "(unnamed)"}`, error); }
    }
    return result.blocks.length ? result : undefined;
  }

  /** Call after material binding, which sets castShadow on regular glTF meshes. */
  keepCanonicalShadows(): void {
    for (const { proxy } of this.blocks) proxy.traverse(node => { node.castShadow = false; });
  }

  readonly groupFor = (object: THREE.Object3D): THREE.Object3D | undefined => {
    for (let node: THREE.Object3D | null = object; node && node !== this.scene; node = node.parent) {
      const group = this.detailGroups.get(node);
      if (group) return group;
    }
  };

  setEnabled(enabled: boolean): void { this.enabled = enabled; }
  beginFrame(): void { for (const block of this.blocks) block.submitted = false; }

  select(camera: THREE.Camera, height: number): boolean {
    this.scene.updateWorldMatrix(true, false);
    camera.getWorldPosition(this.eye);
    let changed = false;
    for (const b of this.blocks) {
      b.worldBounds.copy(b.bounds).applyMatrix4(this.scene.matrixWorld);
      b.distance = b.worldBounds.distanceToPoint(this.eye);
      // CityVisibility selects mixed instance batches only for perspective views.
      const active = this.enabled && camera instanceof THREE.PerspectiveCamera && b.distance >= b.record.distance
        && selectAuthoredLod(b.worldBounds, camera, height, { kind: "distance", values: [0, b.record.distance] }, +b.active, .08) === 1;
      if (active !== b.active) {
        changed = true; b.active = active;
        if (active) this.hidden.add(b.sections[0]); else this.hidden.delete(b.sections[0]);
      }
      for (const group of b.sections) group.visible = !active;
      b.proxy.visible = active;
    }
    return changed;
  }

  cull(occluded: (bounds: THREE.Box3) => boolean): void {
    for (const b of this.blocks) if (b.active && occluded(b.worldBounds)) b.proxy.visible = false;
  }
  restore(): void {
    for (const b of this.blocks) { for (const group of b.sections) group.visible = true; b.proxy.visible = false; }
  }
  stats() {
    const blocks = this.blocks.map(b => ({ id: b.record.id, active: b.active, submitted: b.submitted,
      distance: b.distance, minDistance: b.record.distance, sources: b.record.sources.length,
      bounds: { min: b.worldBounds.min.toArray(), max: b.worldBounds.max.toArray() } }));
    return { ...blocks[0], count: blocks.length, enabled: this.enabled, blocks };
  }
  /** The glTF owner releases the returned proxy assets alongside the source
   * scene, deduplicating any buffers, textures and materials shared by glTF. */
  dispose(): THREE.Object3D[] {
    this.restore();
    const proxies = this.blocks.map(({ proxy, sections }) => {
      for (const group of sections) {
        const index = this.scene.children.indexOf(group), children = group.children.slice();
        this.scene.add(...children); group.removeFromParent(); this.detailGroups.delete(group);
        if (children.length) this.scene.children.splice(index, 0, ...this.scene.children.splice(-children.length));
      }
      proxy.removeFromParent();
      return proxy;
    });
    this.hidden.clear(); this.blocks.length = 0;
    return proxies;
  }
}
