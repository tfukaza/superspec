import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";
import type { SceneMesh, ScenePacket } from "../model/packet.ts";
import type { NavigationMesh, ValidationReport } from "../model/validation.ts";
import { TexturePool, imageIdentity } from "./textures.ts";
import { applyLiveOperations, liveTextures } from "./live.ts";
import { programMaterial, programTexture, setParameters, setSeeds, type MaterialShading, type SceneShading } from "./shade.ts";
import { GpuBaker, matchesMesh, scaledDomain, type BakeDomain, type FieldTexels, type GpuBake } from "./bake.ts";
import { parseView, viewCamera } from "./view.ts";
import { InspectionWalk } from "./walk.ts";

/** Builds a translucent inspection surface from Rust-owned navigation polygons. */
function navigationSurface(data: NavigationMesh, color: number): THREE.Mesh {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(data.vertices.flat(), 3));
  const indices: number[] = [];
  for (const polygon of data.polygons) {
    for (let i = 1; i + 1 < polygon.length; i++) indices.push(polygon[0], polygon[i], polygon[i + 1]);
  }
  geometry.setIndex(indices);
  const surface = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0.45, side: THREE.DoubleSide, depthWrite: false }));
  surface.position.z = 0.025;
  return surface;
}

/** Builds neutral reflection lighting once per viewer; the caller owns the returned GPU target. */
function reflectionEnvironment(renderer: THREE.WebGLRenderer): THREE.WebGLRenderTarget {
  const room = new RoomEnvironment();
  const generator = new THREE.PMREMGenerator(renderer);
  try {
    return generator.fromScene(room);
  } finally {
    room.dispose();
    generator.dispose();
  }
}

/** Compares uploaded attributes against current Rust buffers without allocating replacement geometry. */
function sameGeometry(mesh: THREE.Mesh, packet: ScenePacket, source: SceneMesh): boolean {
  const matches = (current: ArrayLike<number>, incoming: ArrayLike<number>, start: number, count: number): boolean => {
    if (current.length !== count) return false;
    for (let i = 0; i < count; i++) if (current[i] !== incoming[start + i]) return false;
    return true;
  };
  const tangents = packet.manifest.meshTangents?.[source.id];
  const currentTangents = mesh.geometry.getAttribute("tangent");
  if (Boolean(tangents) !== Boolean(currentTangents)) return false;
  if (tangents && currentTangents) {
    if (currentTangents.count !== tangents.length) return false;
    for (let vertex = 0; vertex < tangents.length; vertex++) {
      for (let axis = 0; axis < 4; axis++) {
        if (currentTangents.array[vertex * 4 + axis] !== Math.fround(tangents[vertex][axis])) return false;
      }
    }
  }
  return matches(mesh.geometry.getAttribute("position").array, packet.positions, source.vertexStart * 3, source.vertexCount * 3)
    && matches(mesh.geometry.getAttribute("normal").array, packet.normals, source.vertexStart * 3, source.vertexCount * 3)
    && matches(mesh.geometry.getAttribute("uv").array, packet.uvs, source.vertexStart * 2, source.vertexCount * 2)
    && matches(mesh.geometry.getAttribute("color").array, packet.colors, source.vertexStart * 3, source.vertexCount * 3)
    && matches(mesh.geometry.getIndex()!.array, packet.indices, source.indexStart, source.indexCount);
}

/** Minimal Z-up scene adapter. Owns GPU resources; all model geometry and materials arrive from Rust. */
export class RustScene {
  private readonly renderer = new THREE.WebGLRenderer({ antialias: true });
  private readonly scene = new THREE.Scene();
  private readonly environment: THREE.WebGLRenderTarget;
  private readonly view = parseView(new URLSearchParams(location.search));
  private readonly camera = viewCamera(this.view);
  private readonly controls: OrbitControls;
  private readonly walk: InspectionWalk;
  private walking = false;
  private readonly resize: ResizeObserver;
  private models = new THREE.Group();
  private readonly texturePool = new TexturePool(this.renderer);
  private textureLeases = new Set<string>();
  /** Shaded materials by identity: the uniforms every mesh of the material shares. */
  private shading = new Map<string, { uniforms: Record<string, THREE.IUniform>; material: MaterialShading }>();
  /** Program images by their bytes, per color role; kept while the viewer lives. */
  private readonly programImages = new WeakMap<Uint8Array, Map<boolean, Promise<THREE.Texture>>>();
  private readonly programTextures = new Set<THREE.Texture>();
  private navigation?: THREE.Group;
  private itemGuides?: THREE.Group;
  private collisionIds = new Set<string>();
  private selected?: string;
  private sceneId?: string;
  private frame = 0;
  private closed = false;

  constructor(private readonly host: HTMLElement, private readonly inspect: (mesh?: SceneMesh) => void) {
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.scene.background = new THREE.Color("#d5d7ce");
    this.environment = reflectionEnvironment(this.renderer);
    this.scene.environment = this.environment.texture;
    this.scene.environmentRotation.set(Math.PI / 2, 0, 0);
    this.scene.environmentIntensity = 0.35;
    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.addEventListener("change", this.invalidate);
    this.controls.target.fromArray(this.view.target);
    this.controls.update();
    this.walk = new InspectionWalk(this.renderer.domElement, this.camera, this.invalidate, active => {
      this.walking = active;
      this.controls.enabled = !active;
      if (!active) this.controls.target.copy(this.camera.position).addScaledVector(this.camera.getWorldDirection(new THREE.Vector3()), 3);
    }, text => host.dispatchEvent(new CustomEvent("walkstatus", { detail: text })));
    const ambient = new THREE.HemisphereLight(0xffffff, 0x777768, 0.65);
    ambient.position.set(0, 0, 1);
    const sun = new THREE.DirectionalLight(0xffefdb, 3);
    sun.position.set(-3, -5, 8);
    this.scene.add(ambient, sun, this.models);
    host.append(this.renderer.domElement);
    this.renderer.domElement.addEventListener("click", this.pick);
    this.renderer.domElement.addEventListener("webglcontextrestored", this.invalidate);
    this.resize = new ResizeObserver(() => {
      const width = Math.max(1, host.clientWidth), height = Math.max(1, host.clientHeight);
      this.renderer.setSize(width, height);
      if (this.camera instanceof THREE.PerspectiveCamera) this.camera.aspect = width / height;
      else {
        this.camera.left = -this.view.height * width / height / 2;
        this.camera.right = -this.camera.left;
        this.camera.top = this.view.height / 2;
        this.camera.bottom = -this.camera.top;
      }
      this.camera.updateProjectionMatrix();
      this.invalidate();
    });
    this.resize.observe(host);
    this.render();
  }

  /** Atomically replaces a scene and preserves selection; explicit new-file opens may request camera framing. */
  async apply(packet: ScenePacket, current: () => boolean = () => true, fit = false, decorate?: (meshes: THREE.Mesh[]) => void): Promise<boolean> {
    const next = new THREE.Group();
    const textures = new Map<string, THREE.Texture>();
    const leases = new Set<string>();
    const identities = packet.assetIdentities ??= new Map<string, Promise<string>>();
    const previousMeshes = new Map(this.models.children.map(object => [object.name, object as THREE.Mesh]));
    const reused: { mesh: THREE.Mesh; source: SceneMesh }[] = [];
    const ordered: THREE.Mesh[] = [];
    let committed = false;
    const assets = new Map(packet.manifest.assets.map(asset => [asset.id, asset]));
    const materials = new Map(packet.manifest.materials.map(material => [material.id, material]));
    const checkCurrent = (): void => {
      if (!current() || this.closed) throw new Error("Scene replacement cancelled.");
    };
    /** Decodes a supplied image once per color-space role; URLs never outlive their load. */
    const texture = async (id: string | null, color: boolean): Promise<THREE.Texture | null> => {
      if (id === null) return null;
      const key = `${id}:${color}`;
      const cached = textures.get(key);
      if (cached) return cached;
      const asset = assets.get(id)!;
      const bytes = packet.assets.slice(asset.byteStart, asset.byteStart + asset.byteLength);
      let identity = identities.get(id);
      if (!identity) {
        identity = imageIdentity(bytes);
        identities.set(id, identity);
      }
      const content = await identity;
      checkCurrent();
      const value = await this.texturePool.acquire(content + ":" + color, bytes, asset.mime, color, leases);
      textures.set(key, value);
      return value;
    };
    try {
      const requests = new Map<string, { id: string; color: boolean }>();
      for (const descriptor of packet.manifest.meshes) {
        const definition = descriptor.visible && descriptor.material !== null ? materials.get(descriptor.material) : undefined;
        if (!definition) continue;
        for (const [id, color] of [[definition.texture, true], [definition.normalTexture, false], [definition.metallicRoughnessTexture, false], [definition.emissiveTexture ?? null, true], [definition.occlusionTexture ?? null, false]] as const) {
          if (id !== null) requests.set(`${id}:${color}`, { id, color });
        }
        for (const id of liveTextures(definition.live)) requests.set(`${id}:false`, { id, color: false });
      }
      const pending = [...requests.values()];
      let nextTexture = 0;
      const prepared = await Promise.allSettled(Array.from({ length: Math.min(8, pending.length) }, async () => {
        while (nextTexture < pending.length) {
          checkCurrent();
          const request = pending[nextTexture++];
          await texture(request.id, request.color);
        }
      }));
      const failed = prepared.find(result => result.status === "rejected");
      if (failed?.status === "rejected") throw failed.reason;
      checkCurrent();
      for (const descriptor of packet.manifest.meshes) {
        checkCurrent();
        const definition = descriptor.material === null ? undefined : materials.get(descriptor.material);
        const signature = JSON.stringify(definition ? {
          ...definition, id: undefined,
          texture: definition.texture === null ? null : textures.get(definition.texture + ":true")?.uuid,
          normalTexture: definition.normalTexture === null ? null : textures.get(definition.normalTexture + ":false")?.uuid,
          metallicRoughnessTexture: definition.metallicRoughnessTexture === null ? null : textures.get(definition.metallicRoughnessTexture + ":false")?.uuid,
          emissiveTexture: definition.emissiveTexture == null ? null : textures.get(definition.emissiveTexture + ":true")?.uuid,
          occlusionTexture: definition.occlusionTexture == null ? null : textures.get(definition.occlusionTexture + ":false")?.uuid,
        } : null);
        const previous = previousMeshes.get(descriptor.id);
        if (previous && previous.visible === descriptor.visible && previous.userData.materialSignature === signature && sameGeometry(previous, packet, descriptor)) {
          reused.push({ mesh: previous, source: descriptor });
          ordered.push(previous);
          continue;
        }
        const geometry = new THREE.BufferGeometry();
        const material = new THREE.MeshStandardMaterial();
        const mesh = new THREE.Mesh(geometry, material);
        ordered.push(mesh);
        next.add(mesh);
        geometry.setAttribute("position", new THREE.BufferAttribute(packet.positions.slice(descriptor.vertexStart * 3, (descriptor.vertexStart + descriptor.vertexCount) * 3), 3));
        geometry.setAttribute("normal", new THREE.BufferAttribute(packet.normals.slice(descriptor.vertexStart * 3, (descriptor.vertexStart + descriptor.vertexCount) * 3), 3));
        geometry.setAttribute("uv", new THREE.BufferAttribute(packet.uvs.slice(descriptor.vertexStart * 2, (descriptor.vertexStart + descriptor.vertexCount) * 2), 2));
        geometry.setAttribute("color", new THREE.BufferAttribute(packet.colors.slice(descriptor.vertexStart * 3, (descriptor.vertexStart + descriptor.vertexCount) * 3), 3));
        const tangents = packet.manifest.meshTangents?.[descriptor.id];
        if (tangents) geometry.setAttribute("tangent", new THREE.Float32BufferAttribute(tangents.flat(), 4));
        material.vertexColors = true;
        material.wireframe = this.view.wireframe;
        geometry.setIndex(new THREE.BufferAttribute(packet.indices.slice(descriptor.indexStart, descriptor.indexStart + descriptor.indexCount), 1));
        geometry.computeBoundingBox();
        geometry.computeBoundingSphere();
        if (definition?.baseColorFactor) material.color.fromArray(definition.baseColorFactor);
        else material.color.set(definition?.color ?? "#b9c7d1");
        material.roughness = definition?.roughness ?? 0.8;
        material.metalness = definition?.metalness ?? 0;
        if (definition && descriptor.visible) {
          if (definition.emissiveFactor) material.emissive.fromArray(definition.emissiveFactor);
          else material.emissive.set(definition.emissive ?? "#000000");
          material.emissiveIntensity = definition.emissiveStrength ?? 1;
          material.opacity = definition.alphaMode === "OPAQUE" ? 1 : definition.opacity ?? 1;
          material.transparent = definition.alphaMode == null ? material.opacity < 1 : definition.alphaMode === "BLEND";
          material.depthWrite = !material.transparent;
          material.side = definition.doubleSided == null ? (material.transparent ? THREE.FrontSide : THREE.DoubleSide)
            : definition.doubleSided ? THREE.DoubleSide : THREE.FrontSide;
          material.alphaTest = definition.alphaMode == null ? definition.alphaCutoff ?? 0
            : definition.alphaMode === "MASK" ? definition.alphaCutoff ?? 0.5 : 0;
          material.alphaToCoverage = material.alphaTest > 0;
          material.polygonOffset = material.alphaTest > 0;
          material.polygonOffsetFactor = -1;
          material.polygonOffsetUnits = -2;
          material.map = await texture(definition.texture, true);
          material.normalMap = await texture(definition.normalTexture, false);
          material.normalScale.setScalar(definition.normalScale ?? 1);
          material.roughnessMap = material.metalnessMap = await texture(definition.metallicRoughnessTexture, false);
          material.emissiveMap = await texture(definition.emissiveTexture ?? null, true);
          material.aoMap = await texture(definition.occlusionTexture ?? null, false);
          material.aoMapIntensity = definition.occlusionStrength ?? 1;
          // Live renderer operations execute in this material's shader on top of the baked maps.
          mesh.userData.liveOperations = applyLiveOperations(material, definition.live ?? [], id => textures.get(`${id}:false`));
        }
        mesh.name = descriptor.id;
        mesh.visible = descriptor.visible;
        mesh.userData.source = descriptor;
        mesh.userData.materialSignature = signature;
      }
      if (!current() || this.closed) { this.release(next); this.texturePool.release(leases); return false; }
      for (const item of reused) {
        item.mesh.userData.source = item.source;
        // A reused mesh may still show a GPU preview; the build's own maps replace it.
        this.restoreBaked(item.mesh);
      }
      delete this.host.dataset.gpuPreviewKey;
      decorate?.(ordered);
      for (const mesh of ordered) next.add(mesh);
      this.scene.add(next);
      const previous = this.models;
      this.scene.remove(previous);
      this.models = next;
      committed = true;
      this.texturePool.release(this.textureLeases);
      this.textureLeases = leases;
      this.host.dataset.reusedMeshes = String(reused.length);
      // Counts of uploaded material maps by kind: "compressed" (KTX2) or "image" (PNG, JPEG).
      const kinds: Record<string, number> = {};
      for (const mesh of ordered) {
        const material = mesh.material as THREE.MeshStandardMaterial;
        for (const map of [material.map, material.normalMap, material.roughnessMap, material.emissiveMap, material.aoMap]) {
          if (!map) continue;
          const kind = (map as THREE.CompressedTexture).isCompressedTexture ? "compressed" : "image";
          kinds[kind] = (kinds[kind] ?? 0) + 1;
        }
      }
      this.host.dataset.textureKinds = JSON.stringify(kinds);
      this.host.dataset.liveOperations = JSON.stringify(Object.fromEntries(ordered
        .filter(mesh => (mesh.userData.liveOperations as string[] | undefined)?.length)
        .map(mesh => [mesh.name, mesh.userData.liveOperations])));
      this.clearNavigation();
      this.release(previous);
      const sceneBounds = new THREE.Box3().setFromObject(next);
      if (!sceneBounds.isEmpty()) {
        const extent = sceneBounds.getSize(new THREE.Vector3()).length();
        const distance = this.camera.position.distanceTo(sceneBounds.getCenter(new THREE.Vector3()));
        this.camera.far = Math.max(1000, distance + extent * 10);
        this.controls.maxDistance = this.camera.far * 0.4;
        this.camera.updateProjectionMatrix();
      }
      if ((fit || this.sceneId !== packet.manifest.id) && next.children.length && !this.view.fixed) {
        const bounds = new THREE.Box3().setFromObject(next);
        const center = bounds.getCenter(new THREE.Vector3());
        const radius = Math.max(1, bounds.getSize(new THREE.Vector3()).length());
        this.controls.target.copy(center);
        this.camera.position.copy(center).add(new THREE.Vector3(0.8, -1.2, 0.9).multiplyScalar(radius));
        this.controls.update();
        this.sceneId = packet.manifest.id;
      }
      this.host.dataset.camera = JSON.stringify({ position: this.camera.position.toArray(), target: this.controls.target.toArray(), projection: this.view.projection, fov: this.view.fov, viewHeight: this.view.height, roll: this.view.roll });
      const selection = packet.manifest.meshes.find(mesh => mesh.id === this.selected);
      this.selected = selection?.id;
      this.inspect(selection);
      this.renderer.render(this.scene, this.camera);
      // Public inspection data reflects uploaded geometry, not author-supplied manifest bounds.
      this.host.dataset.meshes = JSON.stringify(next.children.map(object => {
        const mesh = object as THREE.Mesh;
        return { id: mesh.name, bounds: [mesh.geometry.boundingBox!.min.toArray(), mesh.geometry.boundingBox!.max.toArray()], vertices: mesh.geometry.getAttribute("position").count };
      }));
      this.host.dataset.revision = String(packet.manifest.revision);
      this.collisionIds = new Set(packet.manifest.colliders);
      this.clearItemGuides();
      this.walk.setPacket(packet);
      return true;
    } catch (error) {
      if (this.models !== next) { this.scene.remove(next); this.release(next); }
      if (!committed) this.texturePool.release(leases);
      if (!current() || this.closed) return false;
      throw error;
    }
  }

  /**
   * Prepares materials that evaluate their MaterialSpec program per pixel (see shade.ts) and
   * returns the step that puts them on a scene's meshes, for `apply`. `values` are parameter
   * overrides by material; `image` supplies the bytes of each image a program samples.
   */
  async prepareShading(shading: SceneShading, values: Record<string, Record<string, unknown>>,
    image: (name: string) => Promise<Uint8Array>): Promise<(meshes: THREE.Mesh[]) => void> {
    const prepared = new Map<string, { uniforms: Record<string, THREE.IUniform>; material: MaterialShading }>();
    for (const [id, material] of Object.entries(shading.materials)) {
      prepared.set(id, { uniforms: await this.programUniforms(material, values[id] ?? {}, image), material });
    }
    return meshes => {
      this.shading = prepared;
      for (const mesh of meshes) {
        const spec = shading.meshes[mesh.name];
        const entry = spec && prepared.get(spec.material);
        if (!spec || !entry) continue;
        (mesh.material as THREE.Material).dispose();
        const material = programMaterial(entry.material, spec, shading.gravity, entry.uniforms);
        material.wireframe = this.view.wireframe;
        material.side = THREE.DoubleSide;
        mesh.material = material;
        mesh.userData.liveOperations = material.userData.liveOperations;
        mesh.userData.shaded = spec.material;
      }
    };
  }

  /** A program's parameter, seed and image uniforms; images decode once per bytes and color role. */
  private async programUniforms(material: MaterialShading, values: Record<string, unknown>,
    image: (name: string) => Promise<Uint8Array>): Promise<Record<string, THREE.IUniform>> {
    const uniforms: Record<string, THREE.IUniform> = {};
    setParameters(uniforms, material, values);
    setSeeds(uniforms, material);
    for (const entry of material.shader.images) {
      const bytes = await image(entry.name);
      let roles = this.programImages.get(bytes);
      if (!roles) this.programImages.set(bytes, roles = new Map());
      let texture = roles.get(entry.linear);
      if (!texture) {
        texture = programTexture(bytes, entry.linear).then(loaded => { this.programTextures.add(loaded); return loaded; });
        roles.set(entry.linear, texture);
      }
      uniforms[entry.uniform] = { value: await texture };
    }
    return uniforms;
  }

  private bakerInstance?: GpuBaker;
  private gpuUniforms?: { material: MaterialShading; uniforms: Record<string, THREE.IUniform> };

  /** Why GPU preview baking is not available in this viewer, or null. */
  gpuPreviewUnavailable(): string | null {
    return (this.bakerInstance ??= this.newBaker()).unavailable();
  }

  private newBaker(): GpuBaker {
    const baker = new GpuBaker(this.renderer);
    baker.onLost = () => { this.gpuUniforms = undefined; this.clearBakedPreview(); };
    return baker;
  }

  /** Whether `shading`'s shader is compiled for baking and its uniforms are bound, so only parameters need updating. */
  gpuPreviewReady(shading: MaterialShading): boolean {
    return this.bakerInstance?.compiledFor(shading) === true && this.gpuUniforms?.material.shader.source === shading.shader.source
      && this.gpuUniforms.material.seedValues.join() === shading.seedValues.join();
  }

  /**
   * Bakes a material over `domain` on the GPU and shows it on `meshId` as a PREVIEW: the textures
   * replace the mesh's baked maps until the next `apply` or `clearBakedPreview`, which restore them.
   * `shading` may be omitted to rebake the last program with new `values` (uniform updates only).
   * Never produces anything that can be saved or exported. Throws when the GPU cannot bake; the
   * caller then keeps the CPU path.
   */
  async bakePreview(meshId: string, domain: BakeDomain, values: Record<string, unknown>, shading?: MaterialShading,
    image?: (name: string) => Promise<Uint8Array>, measure = false, current: () => boolean = () => true,
    onShown?: (bake: GpuBake, stage: "low" | "full", imagesMs: number) => void, fields?: Record<string, FieldTexels>): Promise<GpuBake | null> {
    const baker = this.bakerInstance ??= this.newBaker();
    if (!current()) return null;
    const mesh = this.models.children.find(object => object.name === meshId) as THREE.Mesh | undefined;
    if (!mesh) throw new Error("GPU preview target mesh is not displayed.");
    let imagesMs = 0;
    if (shading) {
      if (!image) throw new Error("GPU preview needs image bytes.");
      const started = performance.now();
      this.gpuUniforms = { material: shading, uniforms: await this.programUniforms(shading, values, image) };
      imagesMs = performance.now() - started;
    }
    const bound = this.gpuUniforms;
    if (!bound) throw new Error("GPU preview has no program to bake.");
    setParameters(bound.uniforms, bound.material, values);
    if (!matchesMesh(domain, mesh.geometry.getAttribute("position").array, mesh.geometry.getAttribute("uv").array)) {
      throw new Error("GPU preview layout does not match the displayed mesh.");
    }
    // The caller may have moved on (a newer edit, or the CPU result for this one) while shaders compile or bands draw.
    const alive = () => current() && this.models.children.includes(mesh);
    let shown: GpuBake | null = null;
    // Large targets show a quarter-size bake first, then refine to the requested size; a newer edit cancels the refinement.
    if (domain.width >= 256) {
      const low = await baker.bake(bound.material, bound.uniforms, scaledDomain(domain, 4), { measure, current: alive, fields });
      if (!low || !alive()) return null;
      this.showBaked(mesh, low); onShown?.(low, "low", imagesMs); shown = low;
      await new Promise(done => setTimeout(done, 0));
      if (!alive()) return shown;
    }
    const bake = await baker.bake(bound.material, bound.uniforms, domain, { measure, current: alive, fields });
    if (!bake || !alive()) return shown;
    this.showBaked(mesh, bake); onShown?.(bake, "full", imagesMs);
    return bake;
  }

  /** Reads the last GPU preview's texels (tests and diagnostics only). */
  readBakedPreview(bake: GpuBake): ReturnType<GpuBaker["readBack"]> {
    return this.bakerInstance!.readBack(bake);
  }

  private showBaked(mesh: THREE.Mesh, bake: GpuBake): void {
    const material = mesh.material as THREE.MeshStandardMaterial;
    if (!mesh.userData.bakedOriginal) {
      mesh.userData.bakedOriginal = {
        map: material.map, normalMap: material.normalMap, roughnessMap: material.roughnessMap, metalnessMap: material.metalnessMap,
        emissiveMap: material.emissiveMap, emissive: material.emissive.clone(), emissiveIntensity: material.emissiveIntensity,
        roughness: material.roughness, metalness: material.metalness, normalScale: material.normalScale.clone(),
      };
    }
    const shown = mesh.userData.bakedOriginal;
    const changed = material.map !== bake.baseColor;
    material.map = bake.baseColor;
    material.normalMap = bake.normal;
    material.roughnessMap = material.metalnessMap = bake.properties;
    material.emissiveMap = bake.emissive;
    material.emissive.set(0xffffff);
    material.emissiveIntensity = 1;
    material.roughness = 1; material.metalness = 1;
    material.normalScale.setScalar(1);
    if (changed) material.needsUpdate = true;
    mesh.userData.bakedPreview = { key: bake.key, profile: bake.profile, original: shown };
    this.host.dataset.gpuPreviewKey = bake.key;
    // Draw now, as `apply` does, without the frame loop's control update (which can nudge the camera by a rounding step).
    this.renderer.render(this.scene, this.camera);
  }

  /** Puts a mesh's CPU-baked maps back after a GPU preview. */
  private restoreBaked(mesh: THREE.Mesh): void {
    const original = mesh.userData.bakedOriginal;
    if (!original) return;
    const material = mesh.material as THREE.MeshStandardMaterial;
    material.map = original.map; material.normalMap = original.normalMap;
    material.roughnessMap = original.roughnessMap; material.metalnessMap = original.metalnessMap;
    material.emissiveMap = original.emissiveMap; material.emissive.copy(original.emissive); material.emissiveIntensity = original.emissiveIntensity;
    material.roughness = original.roughness; material.metalness = original.metalness; material.normalScale.copy(original.normalScale);
    material.needsUpdate = true;
    delete mesh.userData.bakedOriginal; delete mesh.userData.bakedPreview;
  }

  /** Removes any GPU preview from the displayed meshes, restoring their CPU-baked maps. */
  clearBakedPreview(): void {
    for (const object of this.models.children) this.restoreBaked(object as THREE.Mesh);
    delete this.host.dataset.gpuPreviewKey;
    this.invalidate();
  }

  /** Updates a shaded material's parameters in place: no build and no new shader. */
  setShadingParameters(material: string, values: Record<string, unknown>): boolean {
    const entry = this.shading.get(material);
    if (!entry) return false;
    setParameters(entry.uniforms, entry.material, values);
    this.invalidate();
    return true;
  }

  /** Selects the same stable mesh identity used by pointer picking, without moving the camera. */
  select(id?: string): void {
    const mesh = this.models.children.find(object => object.name === id && object.visible);
    this.selected = mesh?.name;
    this.inspect(mesh?.userData.source);
  }

  /** Adjusts preview lighting exposure without changing model data, textures, camera or authored material values. */
  setExposure(value: number): void {
    if (!Number.isFinite(value) || value < 0.25 || value > 3) throw new Error("Preview exposure must be between 0.25 and 3.");
    this.renderer.toneMappingExposure = value;
    this.invalidate();
  }

  /** Displays authored attachment frames and canonical collision independently of render materials. */
  showItemGuides(attachments: { name: string; position: number[]; direction?: number[] }[], collision: boolean): void {
    this.clearItemGuides();
    const group = new THREE.Group();
    for (const attachment of attachments) {
      if (attachment.position.length !== 3 || !attachment.position.every(Number.isFinite)) continue;
      const position = new THREE.Vector3(...attachment.position);
      const direction = new THREE.Vector3(...(attachment.direction ?? [0, 0, 1])).normalize();
      if (direction.lengthSq() === 0) direction.set(0, 0, 1);
      const marker = new THREE.ArrowHelper(direction, position, 0.3, 0xe2a65f, 0.08, 0.05);
      marker.name = attachment.name;
      group.add(marker);
    }
    if (collision) for (const object of this.models.children) {
      if (!this.collisionIds.has(object.name)) continue;
      const original = object as THREE.Mesh;
      const mesh = new THREE.Mesh(original.geometry.clone(), new THREE.MeshBasicMaterial({
        color: 0x40d6c5, wireframe: true, transparent: true, opacity: 0.6, depthTest: false,
      }));
      group.add(mesh);
    }
    this.itemGuides = group;
    this.scene.add(group);
    this.host.dataset.attachments = JSON.stringify(attachments);
    this.host.dataset.collisionVisible = String(collision);
    this.invalidate();
  }

  /** Releases only resources created for item inspection, never the accepted scene geometry. */
  private clearItemGuides(): void {
    if (!this.itemGuides) return;
    this.scene.remove(this.itemGuides);
    this.itemGuides.traverse(object => {
      const drawable = object as THREE.Mesh;
      drawable.geometry?.dispose();
      const materials = Array.isArray(drawable.material) ? drawable.material : [drawable.material];
      for (const material of materials) material?.dispose();
    });
    this.itemGuides = undefined;
  }

  /** Displays the Rust navigation surface and witnessed route polylines in Z-up coordinates. */
  showNavigation(report: ValidationReport): void {
    this.clearNavigation();
    this.clearItemGuides();
    if (!report.navigation) return;
    const group = new THREE.Group();
    group.add(navigationSurface(report.navigation, 0x1bc5db));
    if (report.water) group.add(navigationSurface(report.water.navigation, 0x247cff));
    for (const route of report.routes ?? []) {
      if (!route.path.length) continue;
      const lineGeometry = new THREE.BufferGeometry().setFromPoints(route.path.map(p => new THREE.Vector3(p[0], p[1], p[2] + 0.08)));
      group.add(new THREE.Line(lineGeometry, new THREE.LineBasicMaterial({ color: 0xffca28 })));
    }
    this.navigation = group;
    this.scene.add(group);
    this.invalidate();
  }

  /** Centers the inspection camera on the bounds attached to a Rust finding. */
  focusBounds(bounds: number[][]): void {
    const box = new THREE.Box3(new THREE.Vector3(...bounds[0]), new THREE.Vector3(...bounds[1]));
    const center = box.getCenter(new THREE.Vector3());
    const direction = this.camera.position.clone().sub(this.controls.target).normalize();
    this.controls.target.copy(center);
    this.camera.position.copy(center).addScaledVector(direction, Math.max(3, box.getSize(new THREE.Vector3()).length() * 1.5));
    this.controls.update();
  }

  /** Releases temporary validation overlays when the compiled source changes. */
  private clearNavigation(): void {
    if (!this.navigation) return;
    this.scene.remove(this.navigation);
    for (const object of this.navigation.children) {
      const drawable = object as THREE.Mesh<THREE.BufferGeometry, THREE.Material>;
      drawable.geometry.dispose();
      drawable.material.dispose();
    }
    this.navigation = undefined;
  }

  /** Resolves a screen pick to the stable source identity supplied by Rust. */
  private readonly pick = (event: MouseEvent): void => {
    const bounds = this.renderer.domElement.getBoundingClientRect();
    const pointer = new THREE.Vector2((event.clientX - bounds.left) / bounds.width * 2 - 1, 1 - (event.clientY - bounds.top) / bounds.height * 2);
    const ray = new THREE.Raycaster();
    ray.setFromCamera(pointer, this.camera);
    const hit = ray.intersectObjects(this.models.children).find(hit => hit.object.visible);
    if (hit && this.walk.pick(hit.point)) return;
    const mesh: SceneMesh | undefined = hit?.object.userData.source;
    this.selected = mesh?.id;
    this.inspect(mesh);
  };

  toggleWalk(): void { this.walk.toggle(); }
  setWalkHelper(helper: string): void { this.walk.setHelper(helper); }

  /** Schedules a frame only when scene, viewport or camera state changes. */
  private readonly invalidate = (): void => {
    if (!this.closed && this.frame === 0) this.frame = requestAnimationFrame(this.render);
  };

  /** Static procedural scenes remain idle between interaction frames. */
  private render = (): void => {
    if (this.closed) return;
    if (!this.walking) this.controls.update();
    this.renderer.render(this.scene, this.camera);
    this.frame = 0;
  };

  /** Releases a group's unique geometry and material resources. */
  private release(group: THREE.Group): void {
    for (const child of group.children) {
      const mesh = child as THREE.Mesh<THREE.BufferGeometry, THREE.MeshStandardMaterial>;
      mesh.geometry.dispose(); mesh.material.dispose();
    }
    group.clear();
  }

  /** Ends animation, listeners, controls and GPU ownership when the editor leaves the page. */
  dispose(): void {
    this.walk.dispose();
    this.closed = true;
    this.clearNavigation();
    this.clearItemGuides();
    cancelAnimationFrame(this.frame);
    this.resize.disconnect(); this.controls.removeEventListener("change", this.invalidate); this.controls.dispose();
    this.renderer.domElement.removeEventListener("click", this.pick);
    this.renderer.domElement.removeEventListener("webglcontextrestored", this.invalidate);
    this.release(this.models);
    this.texturePool.dispose();
    this.bakerInstance?.dispose();
    for (const texture of this.programTextures) texture.dispose();
    this.scene.environment = null;
    this.environment.dispose();
    this.renderer.dispose(); this.renderer.domElement.remove();
  }
}
