import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { releaseDeclaredLodOwner, retainDeclaredLodOwner } from "./lodOwnership.ts";
import { selectAuthoredLod, type AuthoredLodHint } from "./lodSelection.ts";

const EXTENSION = "MSFT_lod";
type Hint = AuthoredLodHint;
type Choice<T> = { levels: T[]; hint: Hint; level: number };
type NodeChoice = Choice<THREE.Object3D> & { source: THREE.Object3D; localBox: THREE.Box3;
  sourceMatrixInverse: THREE.Matrix4; variantMatrices: THREE.Matrix4[] };
type MaterialChoice = { source: THREE.Mesh; original: THREE.Material | THREE.Material[];
  slots: { slot: number; choice: Choice<THREE.Material> }[]; applied?: THREE.Material | THREE.Material[];
  raycast: THREE.Mesh["raycast"]; wrapper: THREE.Mesh["raycast"] };
type Resources = { geometries: Set<THREE.BufferGeometry>; materials: Set<THREE.Material>; textures: Set<THREE.Texture> };

export type DeclaredLodOptions = {
  /** Relative separation of promotion and demotion boundaries. Default 0.15. */
  hysteresis?: number;
  /** Source-linked failures are isolated to the malformed chain. */
  onWarning?: (message: string, error?: unknown) => void;
};

function resources(roots: Iterable<THREE.Object3D>, extraMaterials: Iterable<THREE.Material> = []): Resources {
  const result: Resources = { geometries: new Set(), materials: new Set(extraMaterials), textures: new Set() };
  for (const root of roots) root.traverse(object => {
    if (!(object instanceof THREE.Mesh)) return;
    result.geometries.add(object.geometry);
    for (const material of Array.isArray(object.material) ? object.material : [object.material]) result.materials.add(material);
  });
  for (const material of result.materials) for (const value of Object.values(material)) {
    if (value instanceof THREE.Texture) result.textures.add(value);
  }
  return result;
}

function attributeStorage(attribute: THREE.BufferAttribute | THREE.InterleavedBufferAttribute): object {
  return attribute instanceof THREE.InterleavedBufferAttribute ? attribute.data : attribute;
}

function disposeGeometry(geometry: THREE.BufferGeometry, retained: Set<object>): void {
  // glTF accessors may share one GPU buffer across otherwise distinct geometries.
  // WebGLGeometries.dispose removes each listed attribute, so hide borrowed
  // storage for that event while still releasing this alternative's index/VAO.
  const attributes = geometry.attributes, index = geometry.index, morph = geometry.morphAttributes;
  geometry.attributes = Object.fromEntries(Object.entries(attributes).filter(([, attribute]) => !retained.has(attributeStorage(attribute))));
  if (index && retained.has(attributeStorage(index))) geometry.setIndex(null);
  geometry.morphAttributes = Object.fromEntries(Object.entries(morph).map(([name, values]) =>
    [name, values?.filter(value => !retained.has(attributeStorage(value))) ?? []]));
  try { geometry.dispose(); }
  finally { geometry.attributes = attributes; geometry.setIndex(index); geometry.morphAttributes = morph; }
}

function hintFor(definition: any, count: number): Hint {
  const authored = definition.extras?.worldSpecLod;
  let distance: Hint | undefined;
  if (authored !== undefined) {
    const values = authored?.distances;
    if (authored.version !== 1 || !Array.isArray(values) || values.length !== count || values[0] !== 0
        || values.some((n: unknown, i: number) => typeof n !== "number" || !Number.isFinite(n) || n < 0 || i > 0 && n <= values[i - 1]))
      throw Error("worldSpecLod distances must start at zero and strictly increase, one value per level");
    distance = { kind: "distance", values };
  }
  const authoredCoverage = definition.extras?.MSFT_screencoverage;
  if (authoredCoverage !== undefined) {
    if (!Array.isArray(authoredCoverage) || authoredCoverage.length !== count
        || authoredCoverage.some((n: unknown, i: number) => typeof n !== "number" || !Number.isFinite(n) || n < 0 || n > 1
          || i < count - 1 && n === 0 || i > 0 && n >= authoredCoverage[i - 1]))
      throw Error("MSFT_screencoverage must strictly decrease, one value per level");
    return { kind: "coverage", values: authoredCoverage };
  }
  if (distance) return distance;
  // Engine policy only when no authored hints exist. These are fractions of a
  // 1080-pixel viewport height; a higher drawing-buffer resolution retains detail.
  return { kind: "coverage", values: Array.from({ length: count }, (_, i) => i === count - 1 ? 0 : .25 * .4 ** i) };
}

function idsFor(definitions: any[], index: number): number[] | undefined {
  const extension = definitions[index]?.extensions?.[EXTENSION];
  if (extension === undefined) return;
  const ids = extension?.ids;
  if (!Array.isArray(ids) || !ids.length || ids.length > 16 || new Set(ids).size !== ids.length
      || ids.some(id => !Number.isSafeInteger(id) || id < 0 || id >= definitions.length || id === index))
    throw Error("MSFT_lod ids must name one to sixteen distinct lower levels");
  // Reject recursive/chained declarations rather than accidentally importing a
  // cycle or applying ambiguous quality ordering. Each chain names its levels.
  if (ids.some(id => definitions[id].extensions?.[EXTENSION] !== undefined))
    throw Error("MSFT_lod lower levels must not define another LOD chain");
  return ids;
}

function detachedNodeTrees(definitions: any[], ids: number[], reachable: Set<number>): void {
  const visited = new Set<number>();
  const visit = (index: number) => {
    if (!Number.isSafeInteger(index) || index < 0 || index >= definitions.length || reachable.has(index) || visited.has(index))
      throw Error("Node alternatives must have detached, disjoint, acyclic subtrees");
    visited.add(index);
    const children = definitions[index].children ?? [];
    if (!Array.isArray(children)) throw Error("Node children must be an array");
    children.forEach(visit);
  };
  ids.forEach(visit);
}

function visible(object: THREE.Object3D): boolean {
  for (let node: THREE.Object3D | null = object; node; node = node.parent) if (!node.visible) return false;
  return true;
}

function staticNode(object: THREE.Object3D, animated: Set<string>): boolean {
  let supported = true;
  object.traverse(node => {
    if (node instanceof THREE.SkinnedMesh || node instanceof THREE.Mesh && node.morphTargetInfluences?.length
        || animated.has(node.name) || animated.has(node.uuid) || node instanceof THREE.Light || node instanceof THREE.Camera) supported = false;
  });
  return supported;
}

function localBounds(source: THREE.Object3D): THREE.Box3 {
  const result = new THREE.Box3(), inverse = source.matrixWorld.clone().invert(), matrix = new THREE.Matrix4();
  source.traverse(object => {
    if (!(object instanceof THREE.Mesh)) return;
    if (!object.geometry.boundingBox) object.geometry.computeBoundingBox();
    const count = object instanceof THREE.InstancedMesh ? object.count : 1;
    for (let i = 0; i < count; i++) {
      if (object instanceof THREE.InstancedMesh) { object.getMatrixAt(i, matrix); matrix.premultiply(object.matrixWorld); }
      else matrix.copy(object.matrixWorld);
      matrix.premultiply(inverse);
      result.union(object.geometry.boundingBox!.clone().applyMatrix4(matrix));
    }
  });
  return result;
}

function meshBounds(source: THREE.Mesh, out: THREE.Box3): THREE.Box3 {
  if (!source.geometry.boundingBox) source.geometry.computeBoundingBox();
  if (source instanceof THREE.InstancedMesh) {
    source.computeBoundingBox();
    return out.copy(source.boundingBox!).applyMatrix4(source.matrixWorld);
  }
  return out.copy(source.geometry.boundingBox!).applyMatrix4(source.matrixWorld);
}

/** MSFT_lod is optional: ordinary glTF clients keep the original highest level.
 * This controller changes only the camera view. Call restore() inside a shadow
 * capture before drawing casters, and frame() again afterward. Picking continues
 * to use the original meshes/material sides even while a camera variant is shown.
 * Scene edits rebuild the controller; moving the owning node/parent is supported.
 */
export class DeclaredLods {
  readonly managedNodes = new Set<THREE.Object3D>();
  private readonly managedMaterials = new WeakSet<THREE.Mesh>();
  private readonly ownership = new Set<THREE.Object3D>();
  private readonly displayedAlternates = new Map<THREE.Object3D, THREE.Object3D>();
  private readonly changedMaterials: MaterialChoice[] = [];
  private readonly box = new THREE.Box3();
  private readonly animated = new Set<string>();
  private readonly nodes: NodeChoice[] = [];
  private readonly materials: MaterialChoice[] = [];
  private readonly holders: THREE.Mesh[] = [];
  private readonly loadedRoots: THREE.Object3D[] = [];
  private readonly loadedMaterials = new Set<THREE.Material>();
  private readonly originalResources: Resources;
  private readonly hysteresis: number;
  private disposed = false;
  private binding = false;

  constructor(private readonly gltf: GLTF, private readonly options: DeclaredLodOptions = {}) {
    this.originalResources = resources(gltf.scenes);
    this.hysteresis = Math.min(.45, Math.max(0, Number.isFinite(options.hysteresis) ? options.hysteresis! : .15));
    for (const clip of gltf.animations) for (const track of clip.tracks) {
      this.animated.add(THREE.PropertyBinding.parseTrackName(track.name).nodeName ?? "");
    }
  }

  /** Whether an existing geometry optimizer should leave this authored node
   * chain intact. Material-only chains can coexist with index-buffer LOD. */
  ownsNode(object: THREE.Object3D): boolean {
    for (let at: THREE.Object3D | null = object; at; at = at.parent) if (this.managedNodes.has(at)) return true;
    return false;
  }

  /** Mesh-replacing/merging optimizers must preserve these source identities. */
  ownsMesh(object: THREE.Object3D): boolean {
    return this.ownsNode(object) || object instanceof THREE.Mesh && this.managedMaterials.has(object);
  }

  /** Placements whose attachment, transform or material can change with the camera. */
  textureReceivers(): Set<THREE.Mesh> {
    const receivers = new Set(this.materials.map(choice => choice.source));
    for (const { levels } of this.nodes) for (const root of levels) root.traverse(object => {
      if (object instanceof THREE.Mesh) receivers.add(object);
    });
    return receivers;
  }

  /** On-demand diagnostics; no counters or scene traversal are added to frame().
   * byTier is the last camera selection, while visibleByTier includes only
   * currently displayed receivers (and excludes detached node alternatives). */
  stats() {
    const nodes = { total: this.nodes.length, byTier: [] as number[], visibleByTier: [] as number[] };
    const materials = { meshes: this.materials.length, slots: 0, byTier: [] as number[], visibleByTier: [] as number[] };
    const increment = (counts: number[], level: number) => {
      while (counts.length <= level) counts.push(0);
      counts[level]++;
    };
    const displayed = (object: THREE.Object3D) => {
      for (let node: THREE.Object3D | null = object; node; node = node.parent) {
        if (!node.visible) return false;
        if (node === this.gltf.scene) return true;
      }
      return false;
    };
    for (const entry of this.nodes) {
      increment(nodes.byTier, entry.level);
      const shown = entry.levels.findIndex(displayed);
      if (shown >= 0) increment(nodes.visibleByTier, shown);
    }
    for (const entry of this.materials) {
      const current = Array.isArray(entry.source.material) ? entry.source.material : [entry.source.material];
      const shown = displayed(entry.source);
      for (const { slot, choice } of entry.slots) {
        materials.slots++;
        increment(materials.byTier, choice.level);
        const level = choice.levels.indexOf(current[slot]);
        if (shown && level >= 0) increment(materials.visibleByTier, level);
      }
    }
    return { nodes, materials };
  }

  private warn(kind: string, index: number, error: unknown): void {
    (this.options.onWarning ?? console.warn)(`Keeping full detail for ${kind}[${index}] MSFT_lod`, error);
  }

  private own(object: THREE.Object3D): void {
    if (!this.ownership.has(object)) { this.ownership.add(object); retainDeclaredLodOwner(object); }
  }

  async load(): Promise<this> {
    const { parser, scene } = this.gltf, json = parser.json;
    const sourceNodes: [THREE.Object3D, number][] = [];
    const reachable = new Set<number>();
    for (const root of this.gltf.scenes) root.traverse(node => {
      const index = parser.associations.get(node)?.nodes;
      if (index !== undefined) { reachable.add(index); if (root === scene) sourceNodes.push([node, index]); }
    });
    scene.updateWorldMatrix(true, true);
    for (const [source, index] of sourceNodes) {
      try {
        const ids = idsFor(json.nodes ?? [], index);
        if (!ids) continue;
        const hint = hintFor(json.nodes[index], ids.length + 1);
        if (!staticNode(source, this.animated) || ids.some(id => reachable.has(id)))
          throw Error("Node alternatives must be static and detached from core scenes");
        // Validate before getDependency: GLTFLoader reparents referenced child
        // objects, so a malformed alternate must never steal a canonical child.
        detachedNodeTrees(json.nodes, ids, reachable);
        const results = await Promise.allSettled(ids.map(id => parser.getDependency("node", id).then((node: THREE.Object3D) => {
          this.loadedRoots.push(node); return node;
        })));
        const failed = results.find(result => result.status === "rejected");
        if (failed?.status === "rejected") throw failed.reason;
        const loaded: THREE.Object3D[] = results.flatMap(result => result.status === "fulfilled" ? [result.value] : []);
        if (loaded.some(node => !staticNode(node, this.animated))) throw Error("Animated, skinned or attached light/camera alternatives retain full detail");
        const alternatives = loaded.map(node => {
          node.updateWorldMatrix(false, true);
          const clone = node.clone(true);
          const copyAssociations = (a: THREE.Object3D, b: THREE.Object3D) => {
            const association = parser.associations.get(a);
            if (association) parser.associations.set(b, association);
            a.children.forEach((child, i) => copyAssociations(child, b.children[i]));
          };
          copyAssociations(node, clone);
          // Returning false also prevents the Raycaster from visiting children.
          clone.raycast = () => false;
          return clone;
        });
        this.loadedRoots.push(...alternatives);
        const localBox = localBounds(source);
        if (localBox.isEmpty() || ![...localBox.min, ...localBox.max].every(Number.isFinite)) throw Error("Node has no finite mesh bounds");
        this.nodes.push({ source, levels: [source, ...alternatives], hint, level: 0, localBox,
          sourceMatrixInverse: source.matrix.clone().invert(), variantMatrices: alternatives.map(node => node.matrix.clone()) });
        this.managedNodes.add(source);
        this.own(source); alternatives.forEach(node => this.own(node));
      } catch (error) { this.warn("nodes", index, error); }
    }
    // Each node level owns its own material chains. A material is selected per
    // receiving mesh; shared near materials are never modified in place.
    const meshes = new Set<THREE.Mesh>();
    for (const root of [scene, ...this.nodes.flatMap(node => node.levels.slice(1))]) root.traverse(node => {
      if (node instanceof THREE.Mesh) meshes.add(node);
    });
    for (const mesh of meshes) {
      const original = mesh.material, sourceMaterials = Array.isArray(original) ? original : [original];
      const slots: MaterialChoice["slots"] = [];
      for (let slot = 0; slot < sourceMaterials.length; slot++) {
        const index = parser.associations.get(sourceMaterials[slot])?.materials;
        if (index === undefined) continue;
        try {
          const ids = idsFor(json.materials ?? [], index);
          if (!ids) continue;
          if (mesh instanceof THREE.SkinnedMesh || mesh.morphTargetInfluences?.length)
            throw Error("Deforming meshes retain their original material until conservative animated bounds are supplied");
          const hint = hintFor(json.materials[index], ids.length + 1);
          const results = await Promise.allSettled(ids.map(id => parser.getDependency("material", id).then((material: THREE.Material) => {
            this.loadedMaterials.add(material); return material;
          })));
          const failed = results.find(result => result.status === "rejected");
          if (failed?.status === "rejected") throw failed.reason;
          const loaded: THREE.Material[] = results.flatMap(result => result.status === "fulfilled" ? [result.value] : []);
          const levels = loaded.map(material => {
            const holder = mesh instanceof THREE.InstancedMesh
              ? new THREE.InstancedMesh(mesh.geometry, material, 0) : new THREE.Mesh(mesh.geometry, material);
            if (holder instanceof THREE.InstancedMesh && mesh instanceof THREE.InstancedMesh) {
              // Compilation needs the same vertex variant as its receiving mesh.
              // Hidden holders never draw or own the source's instance resources.
              holder.instanceColor = mesh.instanceColor;
              holder.morphTexture = mesh.morphTexture;
            }
            holder.layers.mask = mesh.layers.mask;
            holder.userData.worldSpecOutline = mesh.userData.worldSpecOutline;
            parser.assignFinalMaterial(holder);
            holder.visible = false; holder.raycast = () => false;
            this.holders.push(holder);
            this.loadedMaterials.add(holder.material);
            return holder.material;
          });
          slots.push({ slot, choice: { levels: [sourceMaterials[slot], ...levels], hint, level: 0 } });
        } catch (error) { this.warn("materials", index, error); }
      }
      if (!slots.length) continue;
      const raycast = mesh.raycast;
      const wrapper: THREE.Mesh["raycast"] = function (raycaster, hits) {
        const current = mesh.material;
        mesh.material = original;
        try { return raycast.call(mesh, raycaster, hits); } finally { mesh.material = current; }
      };
      mesh.raycast = wrapper;
      this.materials.push({ source: mesh, original, slots, raycast, wrapper });
      this.managedMaterials.add(mesh);
      this.own(mesh);
    }
    return this;
  }

  /** Bind shader adapters to every level before constructing spatial optimizers.
   * Hidden temporary holders expose material variants to ordinary scene traversal.
   * They are detached again even when binding fails; default scene records stay intact. */
  async withVariants<T>(bind: () => T | Promise<T>): Promise<T> {
    if (this.disposed || this.binding) throw Error("Declared LOD binding is unavailable");
    this.restore(); this.binding = true;
    const temporary = new THREE.Group(); temporary.name = "Declared LOD binding"; temporary.visible = false;
    for (const entry of this.nodes) for (const node of entry.levels.slice(1)) temporary.add(node);
    for (const holder of this.holders) temporary.add(holder);
    this.gltf.scene.add(temporary);
    try { return await bind(); }
    finally { temporary.clear(); temporary.removeFromParent(); this.binding = false; }
  }

  private select<T>(choice: Choice<T>, camera: THREE.Camera, height: number): number {
    choice.level = selectAuthoredLod(this.box, camera, height, choice.hint, choice.level, this.hysteresis);
    return choice.level;
  }

  /** Each effective pixel height combines the drawing-buffer height with the host's quality bias. */
  frame(camera: THREE.Camera, geometryViewportHeight: number, materialViewportHeight = geometryViewportHeight): void {
    if (!this.nodes.length && !this.materials.length) return;
    this.restore();
    if (this.disposed || this.binding || !(geometryViewportHeight > 0) || !Number.isFinite(geometryViewportHeight)
        || !(materialViewportHeight > 0) || !Number.isFinite(materialViewportHeight)
        || !(camera instanceof THREE.PerspectiveCamera || camera instanceof THREE.OrthographicCamera)) return;
    camera.updateWorldMatrix(true, false);
    for (const entry of this.nodes) {
      if (!entry.source.parent || !visible(entry.source)) continue;
      entry.source.updateWorldMatrix(true, false);
      this.box.copy(entry.localBox).applyMatrix4(entry.source.matrixWorld);
      const selected = this.select(entry, camera, geometryViewportHeight);
      if (!selected) continue;
      const alternate = entry.levels[selected];
      alternate.matrixAutoUpdate = false;
      alternate.matrix.copy(entry.source.matrix).multiply(entry.sourceMatrixInverse).multiply(entry.variantMatrices[selected - 1]);
      alternate.matrixWorldNeedsUpdate = true;
      alternate.traverse(object => { object.castShadow = false; });
      this.displayedAlternates.set(entry.source, alternate);
      entry.source.visible = false;
      entry.source.parent.add(alternate);
      // A shadow refresh can reattach a whole tier after Three's scene walk.
      // Queued color draws and outline depth then read descendant matrixWorld.
      alternate.updateWorldMatrix(false, true);
    }
    for (const entry of this.materials) {
      if (!visible(entry.source) || entry.source.material !== entry.original) continue;
      entry.source.updateWorldMatrix(true, false);
      meshBounds(entry.source, this.box);
      const materials = Array.isArray(entry.original) ? [...entry.original] : [entry.original];
      let changed = false;
      for (const { slot, choice } of entry.slots) {
        const selected = this.select(choice, camera, materialViewportHeight);
        materials[slot] = choice.levels[selected]; changed ||= selected !== 0;
      }
      if (changed) {
        entry.source.material = entry.applied = Array.isArray(entry.original) ? materials : materials[0];
        this.changedMaterials.push(entry);
      }
    }
  }

  restore(): void {
    for (const source of this.displayedAlternates.keys()) source.visible = true;
    for (const alternate of this.displayedAlternates.values()) alternate.removeFromParent();
    this.displayedAlternates.clear();
    for (const entry of this.changedMaterials) {
      if (entry.applied && entry.source.material === entry.applied) entry.source.material = entry.original;
      entry.applied = undefined;
    }
    this.changedMaterials.length = 0;
  }

  dispose(): void {
    if (this.disposed) return;
    this.restore(); this.disposed = true;
    for (const entry of this.materials) if (entry.source.raycast === entry.wrapper) entry.source.raycast = entry.raycast;
    const retained = resources(this.gltf.scenes), owned = resources([...this.loadedRoots, ...this.holders], this.loadedMaterials);
    const retainedAttributes = new Set<object>();
    for (const geometry of [...retained.geometries, ...this.originalResources.geometries]) {
      for (const attribute of [...Object.values(geometry.attributes), ...Object.values(geometry.morphAttributes).flat()]) {
        if (attribute) retainedAttributes.add(attributeStorage(attribute));
      }
      if (geometry.index) retainedAttributes.add(attributeStorage(geometry.index));
    }
    for (const geometry of owned.geometries) {
      if (!retained.geometries.has(geometry) && !this.originalResources.geometries.has(geometry)) disposeGeometry(geometry, retainedAttributes);
    }
    for (const key of ["materials", "textures"] as const) {
      for (const resource of owned[key]) if (!retained[key].has(resource as never) && !this.originalResources[key].has(resource as never)) resource.dispose();
    }
    const instances = new Set<THREE.InstancedMesh>();
    for (const root of this.loadedRoots) root.traverse(node => { if (node instanceof THREE.InstancedMesh) instances.add(node); });
    for (const instance of instances) instance.dispose();
    this.nodes.length = 0; this.materials.length = 0; this.holders.length = 0; this.loadedRoots.length = 0;
    this.loadedMaterials.clear(); this.managedNodes.clear();
    for (const source of this.ownership) releaseDeclaredLodOwner(source);
    this.ownership.clear();
  }
}

export async function loadDeclaredLods(gltf: GLTF, options?: DeclaredLodOptions): Promise<DeclaredLods> {
  return new DeclaredLods(gltf, options).load();
}
