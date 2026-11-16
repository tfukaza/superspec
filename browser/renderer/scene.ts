import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";
import type { SceneMesh, ScenePacket } from "../model/packet.ts";
import type { NavigationMesh, ValidationReport } from "../model/validation.ts";
import { TexturePool, imageIdentity } from "./textures.ts";
import { applyLiveOperations, liveTextures } from "./live.ts";
import { hasPixelRelief, programMaterial, programTexture, setParameters, setSeeds, type MaterialShading, type MeshShading, type SceneShading } from "./shade.ts";
import { GpuBaker, gpuBakeUnsupported, matchesMesh, scaledDomain, type BakeDomain, type FieldTexels, type GpuBake } from "./bake.ts";
import { atlasLayouts, type ProjectLayouts } from "./atlas.ts";
import { SceneBaker, type FieldSource, type PreparedScene, type SceneBakeTarget } from "./scene-bake.ts";
import type { ShadowStyle } from "./shadow-style.ts";
import type { AmbientOcclusion } from "./ambient-occlusion.ts";
import type { Lighting } from "./lighting.ts";
import { SceneLighting } from "./rendering.ts";
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
  // A mesh showing GPU-baked maps is drawn with its atlas's geometry; the packet's is kept beside it (scene-bake.ts).
  const geometry = (mesh.userData.sourceGeometry as THREE.BufferGeometry | undefined) ?? mesh.geometry;
  const tangents = packet.manifest.meshTangents?.[source.id];
  const currentTangents = geometry.getAttribute("tangent");
  if (Boolean(tangents) !== Boolean(currentTangents)) return false;
  if (tangents && currentTangents) {
    if (currentTangents.count !== tangents.length) return false;
    for (let vertex = 0; vertex < tangents.length; vertex++) {
      for (let axis = 0; axis < 4; axis++) {
        if (currentTangents.array[vertex * 4 + axis] !== Math.fround(tangents[vertex][axis])) return false;
      }
    }
  }
  return matches(geometry.getAttribute("position").array, packet.positions, source.vertexStart * 3, source.vertexCount * 3)
    && matches(geometry.getAttribute("normal").array, packet.normals, source.vertexStart * 3, source.vertexCount * 3)
    && matches(geometry.getAttribute("uv").array, packet.uvs, source.vertexStart * 2, source.vertexCount * 2)
    && matches(geometry.getAttribute("color").array, packet.colors, source.vertexStart * 3, source.vertexCount * 3)
    && matches(geometry.getIndex()!.array, packet.indices, source.indexStart, source.indexCount);
}

/** A shaded material as the viewer holds it: the uniforms every mesh of it shares, and its plan's when it bakes from one. */
interface ShadedMaterial {
  uniforms: Record<string, THREE.IUniform>;
  material: MaterialShading;
  /** The parameter values last applied, to tell a change from a repeat. */
  values: string;
  plan?: { uniforms: Record<string, THREE.IUniform>; material: MaterialShading };
}
/** A material's shading with the hybrid plan the engine adds for programs with CPU-only parts (`shade_project`). */
type PlannedShading = MaterialShading & { plan?: { shader: MaterialShading["shader"]; seedValues: number[] } };
/**
 * Asks `prepareShading` for GPU-baked maps once the first look is shown: the receivers' atlas layouts from the engine
 * (`layout_project`) and, for programs with CPU-only parts, a source of their CPU-baked fields.
 */
export interface SceneBakeRequest { layouts: ProjectLayouts; fields?: FieldSource }
/** Materials prepared for a scene's meshes while the scene before it is still displayed: put on them, or given up. */
export interface SceneLook { commit(): void; discard(): void }
/** The uniforms of a program material that describe its mesh, not its program (see `programMaterial`). */
const RECEIVER_UNIFORMS = ["ms_features", "ms_feature_count", "ms_gravity", "ms_bounds_min", "ms_bounds_max", "msObjectFromWorld", "msMapping"];

/**
 * How long a newly shaded scene waits for its baked maps alone before its per-pixel looks start compiling beside them
 * (whichever is ready first is then shown first).
 */
const FIRST_LOOK_HEDGE_MS = 2000;

/** The geometry a scene's packet gave a mesh, whichever look the mesh shows (scene-bake.ts keeps it beside a baked look's). */
function sourceGeometry(mesh: THREE.Mesh): THREE.BufferGeometry {
  return (mesh.userData.sourceGeometry as THREE.BufferGeometry | undefined) ?? mesh.geometry;
}

/** A small content hash of numbers (FNV-1a over their float bits), for cache keys. */
function hashNumbers(values: number[]): string {
  const bits = new Uint32Array(new Float32Array(values).buffer);
  let value = 0x811c9dc5;
  for (let index = 0; index < bits.length; index++) value = Math.imul(value ^ bits[index], 0x01000193);
  return (value >>> 0).toString(16);
}

/** One shaded mesh waiting for its per-pixel relief program: the first-look material it shows and how to make the other. */
interface ReliefRefinement { mesh: THREE.Mesh; first: THREE.Material; source: string; make: () => THREE.MeshStandardMaterial }

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
  private shading = new Map<string, ShadedMaterial>();
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
  /**
   * How this view lights and renders its scene, whatever the materials: the lights, shadows and their look, ambient
   * occlusion, tone mapping. Built from the scene's lighting record (`setLighting`; the shipped look until one is
   * given), with the page's parameters on top as session overrides.
   */
  private readonly lights: SceneLighting;
  /** Whether the viewer casts and receives shadows (the record's sun casts them, unless `?shadows=off`). */
  get shadows(): boolean { return this.lights.shadows; }
  /** The shadows' look, patched into every scene material; it draws three.js's plain shadows while the style is `plain`. */
  private get shadowStyle(): ShadowStyle { return this.lights.shadowStyle; }
  /** Ambient occlusion (on unless the record or `?ao=off` turns it off, or the renderer cannot composite); frames then go through it. */
  private get ambientOcclusion(): AmbientOcclusion | undefined { return this.lights.ambientOcclusion; }

  /** Draws one frame now: through the ambient occlusion when it is on, else straight to the canvas. */
  private draw(): void {
    if (this.ambientOcclusion) this.ambientOcclusion.render();
    else this.renderer.render(this.scene, this.camera);
  }

  /** Fits the sun's shadow camera to the models' bounds and asks for one new shadow map; the light direction is unchanged. */
  private fitShadows(): void {
    this.lights.fit(this.models);
  }

  /**
   * Lights and renders the view by a scene's lighting record (see `lighting.ts`); the page's parameters and the
   * exposure control stay on top of it. Values change in place; only turning shadows, their style or ambient occlusion
   * on or off recompiles programs.
   */
  setLighting(record: Lighting): void {
    this.lights.set(record);
    for (const mesh of this.models.children) mesh.castShadow = mesh.receiveShadow = this.shadows;
    this.fitShadows();
    this.stateLighting();
    this.invalidate();
  }

  /** States on the host what is drawn (`data-rendering`) and the record in effect with the session's overrides (`data-lighting`), for inspection and tests. */
  private stateLighting(): void {
    this.host.dataset.rendering = JSON.stringify({ shadows: this.shadows, shadowStyle: this.lights.styledShadows, ambientOcclusion: this.ambientOcclusion !== undefined });
    this.host.dataset.lighting = JSON.stringify(this.lights.lighting);
  }

  constructor(private readonly host: HTMLElement, private readonly inspect: (mesh?: SceneMesh) => void) {
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.scene.background = new THREE.Color("#d5d7ce");
    this.environment = reflectionEnvironment(this.renderer);
    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.addEventListener("change", this.invalidate);
    this.controls.target.fromArray(this.view.target);
    this.controls.update();
    this.walk = new InspectionWalk(this.renderer.domElement, this.camera, this.invalidate, active => {
      this.walking = active;
      this.controls.enabled = !active;
      if (!active) this.controls.target.copy(this.camera.position).addScaledVector(this.camera.getWorldDirection(new THREE.Vector3()), 3);
    }, text => host.dispatchEvent(new CustomEvent("walkstatus", { detail: text })));
    // The lights, shadows and ambient occlusion of the shipped look, until a scene brings its own record (`setLighting`).
    // Inspection overlays are drawn in the image but cast and receive no occlusion.
    this.lights = new SceneLighting(this.renderer, this.scene, this.camera, this.environment.texture, new URLSearchParams(location.search),
      () => [this.navigation, this.itemGuides].filter((group): group is THREE.Group => group !== undefined));
    this.scene.add(this.models);
    this.stateLighting();
    host.append(this.renderer.domElement);
    this.renderer.domElement.addEventListener("click", this.pick);
    this.renderer.domElement.addEventListener("webglcontextrestored", this.contextRestored);
    this.resize = new ResizeObserver(() => {
      const width = Math.max(1, host.clientWidth), height = Math.max(1, host.clientHeight);
      this.renderer.setSize(width, height);
      this.ambientOcclusion?.setSize(width, height, this.renderer.getPixelRatio());
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
  async apply(packet: ScenePacket, current: () => boolean = () => true, fit = false, decorate?: (meshes: THREE.Mesh[]) => Promise<SceneLook>): Promise<boolean> {
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
      // A scene without shaded materials replaces one that may show GPU-baked looks: its meshes get their own back first.
      if (!decorate) this.sceneBaker?.reset();
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
        // The shadow style goes on first; live operations and GPU previews chain onto it.
        this.shadowStyle?.apply(material);
        const mesh = new THREE.Mesh(geometry, material);
        // Every displayed mesh casts and receives; textures, GPU previews and live operations only change its material.
        mesh.castShadow = mesh.receiveShadow = this.shadows;
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
          const lookup = (id: string) => textures.get(`${id}:false`);
          mesh.userData.liveOperations = applyLiveOperations(material, definition.live ?? [], lookup);
          // What a GPU preview needs to put these operations back.
          mesh.userData.liveSource = { operations: definition.live ?? [], lookup };
        }
        mesh.name = descriptor.id;
        mesh.visible = descriptor.visible;
        mesh.userData.source = descriptor;
        mesh.userData.materialSignature = signature;
      }
      // Shaded materials are made and compiled while the scene before this one stays on screen, untouched.
      const look = await decorate?.(ordered);
      if (!current() || this.closed) { look?.discard(); this.release(next); this.texturePool.release(leases); return false; }
      for (const item of reused) {
        item.mesh.userData.source = item.source;
        // A reused mesh may still show a GPU preview; the build's own maps replace it.
        this.restoreBaked(item.mesh);
      }
      delete this.host.dataset.gpuPreviewKey; delete this.host.dataset.gpuLiveOperations;
      // The build's own maps are shown from here on, on reused and new meshes alike.
      this.bakerInstance?.display(null);
      look?.commit();
      for (const mesh of ordered) next.add(mesh);
      this.scene.add(next);
      const previous = this.models;
      this.scene.remove(previous);
      this.models = next;
      committed = true;
      this.fitShadows();
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
      this.draw();
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
    image: (name: string) => Promise<Uint8Array>, bake?: SceneBakeRequest): Promise<(meshes: THREE.Mesh[]) => Promise<SceneLook>> {
    const prepared = new Map<string, ShadedMaterial>();
    const layouts = bake ? atlasLayouts(bake.layouts) : undefined;
    for (const [id, material] of Object.entries(shading.materials)) {
      const entry: ShadedMaterial = { uniforms: await this.programUniforms(material, values[id] ?? {}, image), material, values: JSON.stringify(values[id] ?? {}) };
      // A program with CPU-only parts is baked from its plan: a shader that reads those parts as CPU-baked fields.
      const plan = (material as PlannedShading).plan;
      if (bake && plan && gpuBakeUnsupported(material, true)) {
        const planned: MaterialShading = { ...material, shader: plan.shader, seedValues: plan.seedValues };
        entry.plan = { material: planned, uniforms: await this.programUniforms(planned, values[id] ?? {}, image) };
      }
      prepared.set(id, entry);
    }
    const make = (entry: { uniforms: Record<string, THREE.IUniform>; material: MaterialShading }, spec: MeshShading, mode: "pixel" | "quad") => {
      const material = programMaterial(entry.material, spec, shading.gravity, entry.uniforms, mode);
      this.shadowStyle?.apply(material);
      material.wireframe = this.view.wireframe;
      material.side = THREE.DoubleSide;
      return material;
    };
    return async meshes => {
      // The first look uses the relief that compiles fast. With baking, GPU-baked maps follow (scene-bake.ts); without
      // it the per-pixel relief programs do, in the background (`refineRelief`).
      const refine: ReliefRefinement[] = [];
      const targets: SceneBakeTarget[] = [];
      const made: { mesh: THREE.Mesh; material: THREE.MeshStandardMaterial; shaded: string }[] = [];
      // The programs the bakes will draw with start compiling at once (the driver compiles them off the page's thread).
      if (bake) {
        const baker = this.bakerInstance ??= this.newBaker();
        for (const entry of prepared.values()) baker.precompileAtlas((entry.plan ?? entry).material);
      }
      for (const mesh of meshes) {
        const spec = shading.meshes[mesh.name];
        const entry = spec && prepared.get(spec.material);
        if (!spec || !entry) continue;
        const material = make(entry, spec, "quad");
        made.push({ mesh, material, shaded: spec.material });
        if (this.pixelRelief && hasPixelRelief(entry.material)) {
          refine.push({ mesh, first: material, source: entry.material.shader.source, make: () => make(entry, spec, "pixel") });
        }
        const layout = layouts?.get(mesh.name);
        if (layout) {
          const use = entry.plan ?? entry;
          const own = material.userData.programUniforms as Record<string, THREE.IUniform>;
          // The program's shared uniforms (the plan's, when it bakes from one) with this receiver's own.
          const receiver = Object.fromEntries(RECEIVER_UNIFORMS.map(name => [name, own[name]]));
          targets.push({ mesh, material: spec.material, shader: material, shading: use.material, uniforms: { ...use.uniforms, ...receiver }, layout,
            receiver: `${spec.features.length}:${hashNumbers(spec.features)}`, extent: (shading.domain as [number, number] | undefined) ?? [1, 1] });
        }
      }
      // The per-pixel looks, compiled as the meshes that will show them and without blocking the page: drawn
      // uncompiled, each program would stall a frame for as long as it compiles (seconds for a heavy one).
      const looks = made.map(item => ({ mesh: item.mesh, geometry: sourceGeometry(item.mesh), material: item.material as THREE.Material }));
      let shaders: Promise<void> | undefined, discarded = false;
      const compileShaders = () => shaders ??= discarded ? Promise.resolve() : this.compileLooks(looks).catch(() => { /* compiled at the first draw instead */ });
      delete this.host.dataset.firstLook;
      const started = performance.now();
      let shaderReady = false;
      // The baked looks are prepared from here on, while the scene before this one stays on screen.
      const baked = bake ? (this.sceneBaker ??= this.newSceneBaker()).prepare(targets, bake.fields) : undefined;
      if (!baked) { await compileShaders(); shaderReady = true; }
      else if (this.shaderFirst) {
        // `?firstlook=shader`: the per-pixel look always comes first and the maps follow it.
        await compileShaders(); shaderReady = true;
      } else {
        // Whichever look is ready first is the first on screen. The maps usually are (their programs are the lighter
        // ones), and they are what a settled scene shows anyway; then the per-pixel looks compile behind them, for
        // edits. Those start compiling beside the bake only when the maps take long, so a light scene never has the
        // two sets of programs compete for the driver.
        const hedge = new Promise<void>(done => setTimeout(done, FIRST_LOOK_HEDGE_MS)).then(compileShaders).then(() => "shader" as const);
        const first = await Promise.race([baked.ready.then(() => "baked" as const), hedge]);
        if (first === "shader") shaderReady = true;
        else {
          // Meshes that get no maps (or have no atlas) show their per-pixel look from the start.
          const withMaps = new Set(targets.map(target => target.mesh));
          for (const mesh of baked.unprepared()) withMaps.delete(mesh);
          const unbaked = looks.filter(look => !withMaps.has(look.mesh));
          if (unbaked.length) await this.compileLooks(unbaked).catch(() => {});
        }
        this.host.dataset.firstLook = first;
      }
      this.host.dataset.firstLookMs = String(Math.round(performance.now() - started));
      return {
        discard: () => { discarded = true; baked?.discard(); for (const item of made) item.material.dispose(); },
        commit: () => {
          const run = this.commitShading(prepared, made, refine, baked, shaderReady);
          if (!shaderReady) void compileShaders().then(() => { if (run === this.reliefRun && !this.closed) this.sceneBaker?.shadersReady(); });
        },
      };
    };
  }

  /** The per-pixel look first, whatever is ready first (`?firstlook=shader`). */
  private readonly shaderFirst = new URLSearchParams(location.search).get("firstlook") === "shader";

  /** Compiles materials as the meshes that will show them, with the displayed scene's lights, shadows and environment, without blocking the page. */
  private async compileLooks(items: { mesh: THREE.Mesh; geometry: THREE.BufferGeometry; material: THREE.Material }[]): Promise<void> {
    if (!items.length) return;
    const stage = new THREE.Group();
    for (const { mesh, geometry, material } of items) {
      const twin = new THREE.Mesh(geometry, material);
      twin.castShadow = mesh.castShadow; twin.receiveShadow = mesh.receiveShadow;
      stage.add(twin);
    }
    // (For the target the scene is drawn into: with ambient occlusion that is its scene target, not the canvas.)
    const compile = () => this.renderer.compileAsync(stage, this.camera, this.scene);
    await (this.ambientOcclusion ? this.ambientOcclusion.compiling(compile) : compile());
  }

  /** Puts prepared per-pixel materials on their meshes and starts what follows them: baked maps, or the per-pixel relief. Returns the run it started. */
  private commitShading(prepared: Map<string, ShadedMaterial>, made: { mesh: THREE.Mesh; material: THREE.MeshStandardMaterial; shaded: string }[],
    refine: ReliefRefinement[], baked: PreparedScene | undefined, shaderReady: boolean): number {
    {
      // Meshes kept from the scene before may show baked looks; they get their per-pixel ones back before those are replaced.
      this.sceneBaker?.reset();
      this.shading = prepared;
      for (const { mesh, material, shaded } of made) {
        (mesh.material as THREE.Material).dispose();
        mesh.material = material;
        mesh.userData.liveOperations = material.userData.liveOperations;
        mesh.userData.shaded = shaded;
      }
      const run = ++this.reliefRun;
      delete this.host.dataset.reliefMs;
      if (baked) {
        delete this.host.dataset.relief;
        // Meshes that cannot be baked keep the per-pixel look and get its better relief once the others show their maps.
        this.unbakedRefinement = () => {
          this.unbakedRefinement = undefined;
          const unbaked = new Set(this.sceneBaker?.unbaked());
          const left = refine.filter(item => unbaked.has(item.mesh));
          if (left.length && run === this.reliefRun) void this.refineRelief(left, run);
        };
        // The scene prepared for these meshes takes the place of the one before it: maps that are ready show at once.
        baked.commit(shaderReady);
        return run;
      }
      delete this.host.dataset.sceneBake;
      this.host.dataset.relief = refine.length ? "quad" : this.pixelRelief ? "pixel" : "quad";
      // After `apply` has drawn the first look.
      if (refine.length) setTimeout(() => void this.refineRelief(refine, run), 0);
      return run;
    }
  }

  private sceneBaker?: SceneBaker;
  private unbakedRefinement?: () => void;

  private newSceneBaker(): SceneBaker {
    return new SceneBaker({
      baker: () => this.bakerInstance ??= this.newBaker(),
      style: material => { this.shadowStyle?.apply(material); material.wireframe = this.view.wireframe; },
      compile: items => this.compileLooks(items),
      displayed: mesh => !this.closed && this.models.children.includes(mesh),
      draw: () => this.invalidate(),
      report: status => {
        this.host.dataset.sceneBake = JSON.stringify(status);
        if (status.state === "baked") this.unbakedRefinement?.();
      },
    });
  }

  /** Per-pixel relief in the shader preview unless the page asks for the cheaper one alone (`?relief=quad`). */
  private readonly pixelRelief = new URLSearchParams(location.search).get("relief") !== "quad";
  private reliefRun = 0;

  /**
   * Swaps shaded meshes from the first-look relief to the per-pixel one, a program at a time and off the critical
   * path: each program is compiled without blocking, then put on its meshes and drawn once, which is where the
   * driver finishes compiling it. A newer scene abandons the run. `data-relief` turns to `pixel` when the GPU has
   * drawn the last of them, and `data-relief-ms` says how long that took after the first look.
   */
  private async refineRelief(items: ReliefRefinement[], run: number): Promise<void> {
    const started = performance.now();
    const current = () => run === this.reliefRun && !this.closed && !this.renderer.getContext().isContextLost();
    // The first look must reach the screen before anything competes with it: its own programs finish compiling at their first draw.
    await this.gpuFinished();
    await new Promise(done => requestAnimationFrame(done));
    const programs = new Map<string, ReliefRefinement[]>();
    for (const item of items) programs.set(item.source, [...programs.get(item.source) ?? [], item]);
    for (const group of programs.values()) {
      if (!current()) return;
      const made = group.map(item => ({ item, material: item.make() }));
      const discard = () => { for (const entry of made) if (entry.item.mesh.material !== entry.material) entry.material.dispose(); };
      // Compiled as the meshes that will show them, with the displayed scene's lights, shadows and environment.
      const stage = new THREE.Group();
      for (const { item, material } of made) {
        const twin = new THREE.Mesh(item.mesh.geometry, material);
        twin.castShadow = item.mesh.castShadow; twin.receiveShadow = item.mesh.receiveShadow;
        stage.add(twin);
      }
      // (For the target the scene is drawn into: with ambient occlusion that is its scene target, not the canvas.)
      const compile = () => this.renderer.compileAsync(stage, this.camera, this.scene);
      try { await (this.ambientOcclusion ? this.ambientOcclusion.compiling(compile) : compile()); } catch { discard(); continue; }
      if (!current()) { discard(); return; }
      for (const { item, material } of made) {
        // A mesh that left the scene or was given another material keeps what it has.
        if (item.mesh.material !== item.first || !this.models.children.includes(item.mesh)) continue;
        item.first.dispose();
        item.mesh.material = material;
        item.mesh.userData.liveOperations = material.userData.liveOperations;
      }
      discard();
      this.draw();
      await this.gpuFinished();
    }
    if (!current()) return;
    this.host.dataset.relief = "pixel";
    this.host.dataset.reliefMs = String(Math.round(performance.now() - started));
  }

  /** Resolves when the GPU has executed everything issued so far, without blocking the page (a fence, polled). */
  private gpuFinished(): Promise<void> {
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    const sync = typeof gl.fenceSync === "function" ? gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0) : null;
    if (!sync) return Promise.resolve();
    gl.flush();
    return new Promise(done => {
      const poll = (): void => {
        if (!this.closed && !gl.isContextLost() && gl.clientWaitSync(sync, 0, 0) === gl.TIMEOUT_EXPIRED) { setTimeout(poll, 4); return; }
        gl.deleteSync(sync);
        done();
      };
      poll();
    });
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

  /** The shader source pass 1 compiles for the last GPU preview program (diagnostics: `?gpuprobe`). */
  gpuPreviewSource(): string | undefined {
    const material = this.gpuUniforms?.material;
    return material ? GpuBaker.pass1Source(material.shader.source) : undefined;
  }

  private bakerInstance?: GpuBaker;
  /** The program the GPU preview last bound, with what names its images' contents (the caller's identity for them). */
  private gpuUniforms?: { material: MaterialShading; uniforms: Record<string, THREE.IUniform>; images: string };

  /** Why GPU preview baking is not available in this viewer, or null. */
  gpuPreviewUnavailable(): string | null {
    return (this.bakerInstance ??= this.newBaker()).unavailable();
  }

  private newBaker(): GpuBaker {
    const baker = new GpuBaker(this.renderer);
    baker.onLost = () => { this.gpuUniforms = undefined; this.sceneBaker?.lost(); this.clearBakedPreview(); };
    return baker;
  }

  /**
   * Whether `shading`'s shader is compiled for baking and its uniforms are bound, so only parameters need updating.
   * `images` identifies the contents of the images the program samples: the same source with another image binds again.
   */
  gpuPreviewReady(shading: MaterialShading, images = ""): boolean {
    return this.bakerInstance?.compiledFor(shading) === true && this.gpuUniforms?.material.shader.source === shading.shader.source
      && this.gpuUniforms.material.seedValues.join() === shading.seedValues.join() && this.gpuUniforms.images === images;
  }

  /**
   * Texture units the material of `mesh` will use while it shows a GPU preview, besides live argument textures: three.js
   * binds one unit per sampler, so the packed properties texture counts for roughness and for metalness.
   */
  private previewUnits(mesh: THREE.Mesh): number {
    const material = mesh.material as THREE.MeshStandardMaterial;
    let units = 5;
    for (const map of [material.aoMap, material.lightMap, material.alphaMap, material.bumpMap, material.displacementMap]) if (map) units++;
    if (material.envMap ?? this.scene.environment) units++;
    if (this.shadows && mesh.receiveShadow) units++;
    return units;
  }

  /** Texture units the program a material was last drawn with binds: its active sampler uniforms. */
  private boundUnits(material: THREE.Material): number | undefined {
    const program = (this.renderer.properties.get(material) as { currentProgram?: { program?: WebGLProgram } }).currentProgram?.program;
    if (!program) return undefined;
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    const samplers = new Set<number>([gl.SAMPLER_2D, gl.SAMPLER_CUBE, gl.SAMPLER_3D, gl.SAMPLER_2D_ARRAY, gl.SAMPLER_2D_SHADOW, gl.SAMPLER_CUBE_SHADOW,
      gl.SAMPLER_2D_ARRAY_SHADOW, gl.INT_SAMPLER_2D, gl.INT_SAMPLER_3D, gl.INT_SAMPLER_CUBE, gl.INT_SAMPLER_2D_ARRAY,
      gl.UNSIGNED_INT_SAMPLER_2D, gl.UNSIGNED_INT_SAMPLER_3D, gl.UNSIGNED_INT_SAMPLER_CUBE, gl.UNSIGNED_INT_SAMPLER_2D_ARRAY]);
    let units = 0;
    for (let index = 0, count = gl.getProgramParameter(program, gl.ACTIVE_UNIFORMS) as number; index < count; index++) {
      const uniform = gl.getActiveUniform(program, index);
      if (uniform && samplers.has(uniform.type)) units += uniform.size;
    }
    return units;
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
    onShown?: (bake: GpuBake, stage: "low" | "full", imagesMs: number) => void, fields?: Record<string, FieldTexels>, images = ""): Promise<GpuBake | null> {
    const baker = this.bakerInstance ??= this.newBaker();
    if (!current()) return null;
    const mesh = this.models.children.find(object => object.name === meshId) as THREE.Mesh | undefined;
    if (!mesh) throw new Error("GPU preview target mesh is not displayed.");
    let imagesMs = 0;
    if (shading) {
      if (!image) throw new Error("GPU preview needs image bytes.");
      const started = performance.now();
      const uniforms = await this.programUniforms(shading, values, image);
      // A newer edit may have bound its own program while the images decoded; this one must not replace it.
      if (!current()) return null;
      this.gpuUniforms = { material: shading, uniforms, images };
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
    // A bake that gave way (the CPU result of its edit landed first) still leaves its program compiled and drawn once, at a
    // tiny size and off the critical path, so the next edit's preview is instant instead of paying the driver's first-draw compile.
    const warm = (): void => {
      if (baker.hasDrawn(bound.material) || (bound.material.shader.cpuFields?.length && !fields)) return;
      setTimeout(() => void baker.warm(bound.material, bound.uniforms, domain, fields), 0);
    };
    // Large targets show a quarter-size bake first, then refine to the requested size; a newer edit cancels the refinement.
    const viewerUnits = this.previewUnits(mesh);
    if (domain.width >= 256) {
      const low = await baker.bake(bound.material, bound.uniforms, scaledDomain(domain, 4), { measure, current: alive, fields, viewerUnits });
      if (!low || !alive()) { warm(); return null; }
      this.showBaked(mesh, low); onShown?.(low, "low", imagesMs); shown = low;
      await new Promise(done => setTimeout(done, 0));
      if (!alive()) return shown;
    }
    const bake = await baker.bake(bound.material, bound.uniforms, domain, { measure, current: alive, fields, viewerUnits });
    if (!bake || !alive()) { if (!shown) warm(); return shown; }
    this.showBaked(mesh, bake); onShown?.(bake, "full", imagesMs);
    return bake;
  }

  /** Reads the last GPU preview's texels (tests and diagnostics only). */
  readBakedLive(bake: GpuBake): ReturnType<GpuBaker["readLive"]> {
    return this.bakerInstance!.readLive(bake);
  }

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
    this.showLive(mesh, material, bake);
    mesh.userData.bakedPreview = { key: bake.key, profile: bake.profile, original: shown };
    this.host.dataset.gpuPreviewKey = bake.key;
    this.host.dataset.gpuLiveOperations = JSON.stringify(mesh.userData.liveOperations ?? []);
    this.bakerInstance?.display(bake);
    // Draw now, as `apply` does, without the frame loop's control update (which can nudge the camera by a rounding step).
    this.draw();
    // What the material's program really binds, now that it has been drawn; the bake's own figure was the estimate.
    const units = this.boundUnits(material);
    if (units !== undefined) {
      bake.units.viewer = units;
      if (units > bake.units.limit) {
        this.restoreBaked(mesh);
        delete this.host.dataset.gpuPreviewKey; delete this.host.dataset.gpuLiveOperations;
        this.draw();
        throw new Error(`GPU preview needs ${units} texture units to show its maps and lighting arguments; this device has ${bake.units.limit}.`);
      }
    }
  }

  /**
   * Points the material's live renderer operations at the preview's own argument textures, so lighting effects follow the
   * edit. The same operations and arguments only swap the textures in place; a different set replaces the operations (one
   * shader recompile). Without live arguments in the bake, the build's own operations are put back.
   */
  private showLive(mesh: THREE.Mesh, material: THREE.MeshStandardMaterial, bake: GpuBake): void {
    const live = bake.live;
    mesh.userData.bakedLiveSource = live;
    if (!live) { this.restoreLive(mesh, material); return; }
    const textures = new Map(Object.entries(live.textures));
    if (mesh.userData.bakedLive?.key === live.key && material.userData.liveUniforms) {
      const uniforms = material.userData.liveUniforms as Record<string, THREE.IUniform>;
      for (const [id, slot] of material.userData.liveTextureSlots as Map<string, number>) {
        const uniform = uniforms[`msLiveTex${slot}Map`];
        if (uniform) uniform.value = textures.get(id);
      }
      return;
    }
    mesh.userData.liveOriginal ??= { onBeforeCompile: material.onBeforeCompile, customProgramCacheKey: material.customProgramCacheKey,
      uniforms: material.userData.liveUniforms, slots: material.userData.liveTextureSlots, ids: mesh.userData.liveOperations };
    // A material with no operations has the prototype's hooks; the operations chain onto whatever is installed.
    material.onBeforeCompile = THREE.Material.prototype.onBeforeCompile;
    material.customProgramCacheKey = THREE.Material.prototype.customProgramCacheKey;
    this.shadowStyle?.apply(material);
    delete material.userData.liveUniforms; delete material.userData.liveTextureSlots;
    mesh.userData.liveOperations = applyLiveOperations(material, live.operations as never, id => textures.get(id));
    mesh.userData.bakedLive = { key: live.key };
  }

  /** Puts the CPU build's live operations back on a mesh whose GPU preview replaced them. */
  private restoreLive(mesh: THREE.Mesh, material: THREE.MeshStandardMaterial): void {
    const original = mesh.userData.liveOriginal;
    if (!original) return;
    material.onBeforeCompile = original.onBeforeCompile;
    material.customProgramCacheKey = original.customProgramCacheKey;
    if (original.uniforms) { material.userData.liveUniforms = original.uniforms; material.userData.liveTextureSlots = original.slots; }
    else { delete material.userData.liveUniforms; delete material.userData.liveTextureSlots; }
    mesh.userData.liveOperations = original.ids;
    material.needsUpdate = true;
    delete mesh.userData.liveOriginal; delete mesh.userData.bakedLive;
  }

  /** Puts a mesh's CPU-baked maps back after a GPU preview. */
  private restoreBaked(mesh: THREE.Mesh): void {
    const original = mesh.userData.bakedOriginal;
    if (!original) return;
    const material = mesh.material as THREE.MeshStandardMaterial;
    this.restoreLive(mesh, material);
    material.map = original.map; material.normalMap = original.normalMap;
    material.roughnessMap = original.roughnessMap; material.metalnessMap = original.metalnessMap;
    material.emissiveMap = original.emissiveMap; material.emissive.copy(original.emissive); material.emissiveIntensity = original.emissiveIntensity;
    material.roughness = original.roughness; material.metalness = original.metalness; material.normalScale.copy(original.normalScale);
    material.needsUpdate = true;
    delete mesh.userData.bakedOriginal; delete mesh.userData.bakedPreview;
    // Nothing shows the preview's targets any more (a swatch has one mesh).
    this.bakerInstance?.display(null);
  }

  /** Removes any GPU preview from the displayed meshes, restoring their CPU-baked maps. */
  clearBakedPreview(): void {
    for (const object of this.models.children) this.restoreBaked(object as THREE.Mesh);
    delete this.host.dataset.gpuPreviewKey; delete this.host.dataset.gpuLiveOperations;
    this.invalidate();
  }

  /** Updates a shaded material's parameters in place: no build and no new shader. */
  setShadingParameters(material: string, values: Record<string, unknown>): boolean {
    const entry = this.shading.get(material);
    if (!entry) return false;
    const next = JSON.stringify(values);
    if (next === entry.values) return true;
    entry.values = next;
    setParameters(entry.uniforms, entry.material, values);
    if (entry.plan) setParameters(entry.plan.uniforms, entry.plan.material, values);
    // Meshes showing baked maps of this material go back to the per-pixel look until the change settles.
    this.sceneBaker?.invalidate(material);
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
    this.lights.setExposure(value);
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

  /** A restored context has an empty shadow map, and the map is only drawn on request. */
  private readonly contextRestored = (): void => {
    if (this.shadows) this.renderer.shadowMap.needsUpdate = true;
    // The baked maps went with the old context. Baked again once every listener has run (the GPU baker's own comes later).
    setTimeout(() => { if (!this.closed) this.sceneBaker?.rebake(); }, 0);
    this.invalidate();
  };

  /** Schedules a frame only when scene, viewport or camera state changes. */
  private readonly invalidate = (): void => {
    if (!this.closed && this.frame === 0) this.frame = requestAnimationFrame(this.render);
  };

  /** Static procedural scenes remain idle between interaction frames. */
  private render = (): void => {
    if (this.closed) return;
    if (!this.walking) this.controls.update();
    this.draw();
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
    this.renderer.domElement.removeEventListener("webglcontextrestored", this.contextRestored);
    this.sceneBaker?.dispose();
    this.release(this.models);
    this.texturePool.dispose();
    this.bakerInstance?.dispose();
    for (const texture of this.programTextures) texture.dispose();
    this.lights.dispose();
    this.scene.environment = null;
    this.environment.dispose();
    this.renderer.dispose(); this.renderer.domElement.remove();
  }
}
