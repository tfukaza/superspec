import * as THREE from "three";
import type { CityTextureBinding } from "./cityTextureLoader.ts";
export type { CityTextureBinding } from "./cityTextureLoader.ts";
type Surface = { material: THREE.MeshStandardMaterial; bindings: CityTextureBinding[]; boxes: THREE.Box3[];
  distance: number; near: boolean; active: boolean; loading: boolean; failed: boolean; high?: THREE.Texture[] };

/** Keep the published coarse mip images resident and promote a material when
 * any of its placements approaches the camera. High maps share normal Three
 * sampling and material code. Full texture uploads are released on departure. */
export class CityTextureResidency {
  private readonly surfaces = new Map<THREE.MeshStandardMaterial, Surface>();
  private readonly counts = new Map<THREE.Texture, number>();
  private readonly released = new WeakSet<THREE.Texture>();
  private readonly requests = new Map<CityTextureBinding["loadHigh"], Promise<THREE.Texture>>();
  private readonly prepared = new WeakSet<THREE.Texture>();
  private readonly uploads: { surface: Surface; textures: THREE.Texture[]; done: () => void; fail: (error: unknown) => void }[] = [];
  private draining = 0;
  private readonly eye = new THREE.Vector3();
  private readonly transform = new THREE.Matrix4();
  private readonly bounds = new THREE.Box3();
  private readonly pending = new Set<Promise<void>>();
  private readonly cameraReceivers: Set<THREE.Mesh>;
  private disposed = false;
  private redraw?: number;

  constructor(private readonly root: THREE.Object3D, private readonly bindings: (m: THREE.MeshStandardMaterial) => CityTextureBinding[],
    private readonly invalidate?: () => void, private readonly nearMeters = 18,
    private readonly prepareTexture?: (texture: THREE.Texture) => void, cameraReceivers?: ReadonlySet<THREE.Mesh>) {
    this.cameraReceivers = new Set(cameraReceivers);
    root.updateWorldMatrix(true, true);
    const inventory = (object: THREE.Object3D, visible: boolean) => {
      visible &&= object.visible;
      if (object instanceof THREE.Mesh) {
        if (!visible) this.cameraReceivers.add(object);
        else if (!this.cameraReceivers.has(object)) this.measurePlacement(object, true);
      }
      for (const child of object.children) inventory(child, visible);
    };
    inventory(root, true);
  }

  private readonly unstreamed = new WeakSet<THREE.Material>();

  private measurePlacement(mesh: THREE.Mesh, fixed: boolean): void {
    if (mesh instanceof THREE.InstancedMesh && !mesh.count) return;
    for (const m of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
      if (!(m instanceof THREE.MeshStandardMaterial) || this.unstreamed.has(m)) continue;
      let surface = this.surfaces.get(m);
      if (!surface) {
        const slots = this.bindings(m);
        if (!slots.length) { this.unstreamed.add(m); continue; }
        surface = { material: m, bindings: slots, boxes: [], distance: Infinity, near: false, active: false, loading: false, failed: false };
        this.surfaces.set(m, surface);
      }
      if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
      if (!fixed) mesh.updateWorldMatrix(true, false);
      const count = mesh instanceof THREE.InstancedMesh ? mesh.count : 1;
      for (let i = 0; i < count; i++) {
        if (mesh instanceof THREE.InstancedMesh) {
          mesh.getMatrixAt(i, this.transform);
          this.transform.premultiply(mesh.matrixWorld);
        } else this.transform.copy(mesh.matrixWorld);
        this.bounds.copy(mesh.geometry.boundingBox!).applyMatrix4(this.transform);
        if (fixed) surface.boxes.push(this.bounds.clone());
        else surface.distance = Math.min(surface.distance, this.bounds.distanceToPoint(this.eye));
      }
    }
  }

  frame(camera: THREE.Camera, viewportHeight: number): void {
    if (this.disposed || (!this.surfaces.size && !this.cameraReceivers.size)) return;
    if (this.redraw !== undefined) { cancelAnimationFrame(this.redraw); this.redraw = undefined; }
    camera.updateWorldMatrix(true, false);
    this.eye.setFromMatrixPosition(camera.matrixWorld);
    // The host's near distance is measured at 1080p and 70° vertical FOV.
    // Raising resolution or zoom increases that range proportionally.
    const perspective = camera instanceof THREE.PerspectiveCamera;
    const radius = perspective ? (this.nearMeters * viewportHeight * Math.abs(camera.projectionMatrix.elements[5]) / 1542.4) : Infinity;
    for (const surface of this.surfaces.values()) {
      let nearest = Infinity;
      for (const box of surface.boxes) nearest = Math.min(nearest, box.distanceToPoint(this.eye));
      surface.distance = nearest;
    }
    for (const mesh of this.cameraReceivers) {
      let node: THREE.Object3D | null = mesh;
      while (node && node.visible && node !== this.root) node = node.parent;
      if (node === this.root && node.visible) this.measurePlacement(mesh, false);
    }
    for (const surface of this.surfaces.values()) {
      surface.near = Number.isFinite(surface.distance) && surface.distance <= radius * (surface.active || surface.near ? 1.28 : 1);
      if (!surface.near) surface.failed = false;
      if (!surface.near && surface.active) this.demote(surface);
    }
    this.pump();
    this.upload(false);
  }

  private pump(): void {
    if (this.disposed) return;
    // Limit burst decoding and prioritize the nearest material after a teleport.
    while (this.pending.size < 4) {
      let next: Surface | undefined;
      for (const surface of this.surfaces.values()) {
        if (surface.near && !surface.failed && !surface.loading && !surface.active && (!next || surface.distance < next.distance))
          next = surface;
      }
      if (!next) return;
      const surface = next;
      surface.loading = true;
      const job = this.promote(surface).finally(() => {
        this.pending.delete(job);
        surface.loading = false;
        this.pump();
      });
      this.pending.add(job);
    }
  }

  private async promote(surface: Surface): Promise<void> {
    try {
      const textures = surface.high ?? await this.load(surface.bindings);
      this.syncAnisotropy(surface, textures);
      if (this.prepareTexture && surface.near && !this.disposed && textures.some(t => !this.prepared.has(t))) {
        await new Promise<void>((done, fail) => {
          this.uploads.push({ surface, textures, done, fail });
          if (this.draining) this.upload(true);
          else this.requestFrame();
        });
      }
      if (this.disposed) { for (const tex of textures) this.releaseIdle(tex); return; }
      surface.high = textures;
      if (!surface.near) { for (const tex of textures) this.releaseIdle(tex); return; }
      for (let i = 0; i < textures.length; i++) {
        const binding = surface.bindings[i], texture = textures[i];
        surface.material[binding.slot] = texture;
        this.released.delete(texture);
        this.counts.set(texture, (this.counts.get(texture) ?? 0) + 1);
      }
      surface.active = true;
      this.requestFrame();
    } catch (error) {
      surface.failed = true;
      console.warn("Using the published distant texture for", surface.material.name, error);
    }
  }

  private requestFrame(): void {
    if (!this.disposed && this.invalidate && this.redraw === undefined) this.redraw = requestAnimationFrame(() => {
      this.redraw = undefined;
      if (!this.disposed) this.invalidate?.();
    });
  }

  private upload(all: boolean): void {
    let uploaded = false;
    for (let i = 0; i < this.uploads.length;) {
      const job = this.uploads[i];
      const next = !this.disposed && job.surface.near && job.textures.find(tex => !this.prepared.has(tex));
      if (!next) {
        this.uploads.splice(i, 1);
        job.done();
      } else if (all || !uploaded) {
        try {
          this.released.delete(next);
          this.prepareTexture!(next);
          this.prepared.add(next);
        } catch (error) {
          this.uploads.splice(i, 1);
          for (const texture of job.textures) this.releaseIdle(texture);
          job.fail(error);
        }
        uploaded = true;
      } else i++;
    }
    if (this.uploads.length) this.requestFrame();
  }

  private async load(bindings: CityTextureBinding[]): Promise<THREE.Texture[]> {
    const results = await Promise.allSettled(bindings.map(binding => {
      let request = this.requests.get(binding.loadHigh);
      if (!request) {
        request = Promise.resolve().then(() => binding.loadHigh()).catch(error => {
          this.requests.delete(binding.loadHigh);
          throw error;
        });
        this.requests.set(binding.loadHigh, request);
      }
      return request;
    }));
    const textures = results.flatMap(result => result.status === "fulfilled" ? [result.value] : []);
    const failed = results.find(result => result.status === "rejected");
    if (failed) {
      for (const tex of textures) this.releaseIdle(tex);
      throw failed.reason;
    }
    return textures;
  }

  private releaseIdle(texture: THREE.Texture): void {
    if (this.counts.has(texture) || this.released.has(texture)) return;
    texture.dispose();
    this.prepared.delete(texture);
    this.released.add(texture);
  }

  private demote(surface: Surface): void {
    for (let i = 0; i < surface.bindings.length; i++) {
      const binding = surface.bindings[i], high = surface.high![i];
      surface.material[binding.slot] = binding.low;
      const count = this.counts.get(high)! - 1;
      if (count) this.counts.set(high, count);
      else { this.counts.delete(high); this.releaseIdle(high); }
    }
    surface.active = false;
  }

  /** After a camera cut, screenshots can wait for all visible near detail. */
  async settle(): Promise<void> {
    this.draining++;
    try {
      this.upload(true);
      while (this.pending.size) await Promise.all(this.pending);
    } finally { this.draining--; }
  }

  stats(): { materials: number; high: number; pending: number; failed: number } {
    let high = 0, failed = 0;
    for (const surface of this.surfaces.values()) {
      if (surface.active) high++;
      if (surface.failed) failed++;
    }
    return { materials: this.surfaces.size, high, pending: this.pending.size, failed };
  }

  private readonly adjustedAnisotropy = new WeakSet<THREE.Texture>();

  /** Propagate an explicit sampler override from affected coarse maps to full-resolution maps. */
  updateAnisotropy(updated: ReadonlySet<THREE.Texture>): void {
    if (!updated.size) return;
    for (const texture of updated) this.adjustedAnisotropy.add(texture);
    for (const surface of this.surfaces.values()) if (surface.high) this.syncAnisotropy(surface, surface.high);
    for (const job of this.uploads) this.syncAnisotropy(job.surface, job.textures);
  }

  private syncAnisotropy(surface: Surface, textures: THREE.Texture[]): void {
    for (const [i, texture] of textures.entries()) {
      const { low } = surface.bindings[i];
      if (!this.adjustedAnisotropy.has(low)) continue;
      const samples = low.anisotropy;
      if (texture.anisotropy === samples) continue;
      texture.anisotropy = samples;
      texture.needsUpdate = true;
    }
  }

  dispose(): void {
    this.disposed = true;
    this.upload(true);
    if (this.redraw !== undefined) cancelAnimationFrame(this.redraw);
    for (const surface of this.surfaces.values()) if (surface.active) this.demote(surface);
    this.surfaces.clear();
    this.cameraReceivers.clear();
    this.requests.clear();
  }
}
