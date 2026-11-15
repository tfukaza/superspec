import * as THREE from "three";
import type { ScenePacket } from "../model/packet.ts";
interface WalkState { feet: number[]; eye: number[]; height: number; radius: number; stance: string; grounded: boolean; verticalSpeed: number }

/** Owns browser input, movement-worker lifetime and camera presentation. */
export class InspectionWalk {
  private packet?: ScenePacket;
  private worker?: Worker;
  private feet?: number[];
  private pending = false;
  private armed = false;
  private frame = 0;
  private last = 0;
  private yaw = 0;
  private pitch = 0;
  private pose?: WalkState;
  private helper = "off";
  private jumpQueued = false;
  private readonly keys = new Set<string>();

  constructor(private readonly canvas: HTMLCanvasElement, private readonly camera: THREE.Camera,
    private readonly render: () => void, private readonly active: (value: boolean) => void,
    private readonly message: (text: string) => void) {
    window.addEventListener("keydown", this.keyDown);
    window.addEventListener("keyup", this.keyUp);
    document.addEventListener("mousemove", this.look);
    document.addEventListener("pointerlockchange", this.lockChanged);
    window.addEventListener("blur", this.stop);
    document.addEventListener("visibilitychange", this.visibilityChanged);
  }

  setPacket(packet: ScenePacket): void { this.stop(); this.packet = packet; }

  toggle(): void {
    if (this.armed || this.feet) { this.stop(); return; }
    if (!(this.camera instanceof THREE.PerspectiveCamera)) { this.message("Walking requires a perspective camera."); return; }
    this.armed = true;
    this.canvas.style.cursor = "crosshair";
    this.message("Click a walkable floor. WASD moves, Space jumps, Ctrl/C crouches, Escape exits.");
  }

  /** The renderer supplies a picked world point without deciding whether it is walkable. */
  pick(point: THREE.Vector3): boolean {
    if (this.feet) {
      if (document.pointerLockElement !== this.canvas) void this.canvas.requestPointerLock().catch(() => this.message("Mouse capture is unavailable. Escape exits walking."));
      return true;
    }
    if (!this.armed || !this.packet) return false;
    this.armed = false;
    this.canvas.style.cursor = "";
    const colliders = new Set(this.packet.manifest.colliders);
    const meshes = this.packet.manifest.meshes.filter(mesh => colliders.has(mesh.id));
    const triangles = new Float32Array(meshes.reduce((sum, mesh) => sum + mesh.indexCount * 3, 0));
    const owners = new Uint32Array(triangles.length / 9);
    const objectIds = new Map<string, number>();
    let offset = 0;
    for (const mesh of meshes) {
      if (!objectIds.has(mesh.objectId)) objectIds.set(mesh.objectId, objectIds.size);
      for (let i = mesh.indexStart; i < mesh.indexStart + mesh.indexCount; i++) {
        const vertex = (mesh.vertexStart + this.packet.indices[i]) * 3;
        triangles.set(this.packet.positions.subarray(vertex, vertex + 3), offset);
        offset += 3;
        owners[Math.floor((offset - 3) / 9)] = objectIds.get(mesh.objectId)!;
      }
    }
    const context = this.packet.manifest.architecture as { metadata?: { kind: string; attributes?: Record<string, unknown> }[] } | undefined;
    const player = context?.metadata?.find(node => node.kind === "player")?.attributes ?? {};
    const agent = [Number(player.radius ?? 0.35), Number(player.height ?? 1.8), Number(player.step ?? 0.45), Number(player.slope ?? 42), Number(player.crouch ?? 0)];
    this.worker = new Worker(new URL("../navigation/walk-worker.ts", import.meta.url), { type: "module" });
    const worker = this.worker;
    worker.onmessage = (event: MessageEvent<WalkState & { error?: string }>) => {
      if (this.worker !== worker) return;
      this.pending = false;
      if (event.data.error || !event.data.feet) { this.stop(); this.message(event.data.error ?? "Walking query failed."); return; }
      const starting = !this.feet;
      this.feet = event.data.feet;
      this.pose = event.data;
      if (starting) {
        const direction = this.camera.getWorldDirection(new THREE.Vector3());
        this.yaw = Math.atan2(direction.y, direction.x);
        this.pitch = Math.asin(direction.z);
        this.active(true);
        void this.canvas.requestPointerLock().catch(() => this.message("Click the scene for mouse capture. WASD moves, Escape exits."));
        this.last = performance.now();
        this.frame = requestAnimationFrame(this.tick);
      }
      this.updateCamera();
    };
    worker.onerror = event => { this.stop(); this.message(event.message); };
    this.pending = true;
    this.message("Preparing inspection controls…");
    worker.postMessage({ triangles, owners, point: point.toArray(), agent }, [triangles.buffer, owners.buffer]);
    return true;
  }

  private updateCamera(): void {
    if (!this.feet || !this.pose) return;
    this.camera.position.fromArray(this.pose.eye);
    const direction = new THREE.Vector3(Math.cos(this.yaw) * Math.cos(this.pitch), Math.sin(this.yaw) * Math.cos(this.pitch), Math.sin(this.pitch));
    this.camera.lookAt(this.camera.position.clone().add(direction));
    this.canvas.dataset.walkFeet = JSON.stringify(this.feet);
    this.canvas.dataset.walk = JSON.stringify({ ...this.pose, helper: this.helper });
    this.render();
  }

  private readonly tick = (time: number): void => {
    if (!this.feet) return;
    const seconds = Math.min(0.05, (time - this.last) / 1000);
    this.last = time;
    const forward = Number(this.keys.has("KeyW") || this.keys.has("ArrowUp")) - Number(this.keys.has("KeyS") || this.keys.has("ArrowDown"));
    const right = Number(this.keys.has("KeyD") || this.keys.has("ArrowRight")) - Number(this.keys.has("KeyA") || this.keys.has("ArrowLeft"));
    if (!this.pending) {
      this.pending = true;
      this.worker!.postMessage({ forward, right, yaw: this.yaw, seconds, jump: this.jumpQueued || this.keys.has("Space"), crouch: this.keys.has("ControlLeft") || this.keys.has("ControlRight") || this.keys.has("KeyC"), helper: this.helper });
      this.jumpQueued = false;
    }
    this.frame = requestAnimationFrame(this.tick);
  };
  private readonly keyDown = (event: KeyboardEvent): void => {
    if (event.code === "Escape") this.stop();
    if (this.feet && event.code === "Space" && !event.repeat) this.jumpQueued = true;
    if (this.feet && ["KeyW", "KeyA", "KeyS", "KeyD", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Space", "ControlLeft", "ControlRight", "KeyC"].includes(event.code)) { event.preventDefault(); this.keys.add(event.code); }
  };
  private readonly keyUp = (event: KeyboardEvent): void => { this.keys.delete(event.code); };
  private readonly look = (event: MouseEvent): void => {
    if (document.pointerLockElement !== this.canvas || !this.feet) return;
    this.yaw -= event.movementX * 0.002;
    this.pitch = THREE.MathUtils.clamp(this.pitch - event.movementY * 0.002, -1.396, 1.396);
    this.updateCamera();
  };
  private readonly lockChanged = (): void => { if (this.feet && document.pointerLockElement !== this.canvas) this.stop(); };
  private readonly visibilityChanged = (): void => { if (document.hidden) this.stop(); };
  setHelper(helper: string): void { this.helper = helper; }
  readonly stop = (): void => {
    const wasWalking = this.feet !== undefined;
    this.worker?.terminate(); this.worker = undefined;
    this.feet = undefined; this.armed = false; this.pending = false;
    this.pose = undefined; this.helper = "off";
    this.jumpQueued = false;
    this.keys.clear(); cancelAnimationFrame(this.frame);
    this.canvas.style.cursor = "";
    delete this.canvas.dataset.walkFeet;
    delete this.canvas.dataset.walk;
    this.canvas.dispatchEvent(new CustomEvent("walkstop", { bubbles: true }));
    if (document.pointerLockElement === this.canvas) document.exitPointerLock();
    if (wasWalking) this.active(false);
  };
  dispose(): void {
    this.stop();
    window.removeEventListener("keydown", this.keyDown);
    window.removeEventListener("keyup", this.keyUp);
    document.removeEventListener("mousemove", this.look);
    document.removeEventListener("pointerlockchange", this.lockChanged);
    document.removeEventListener("visibilitychange", this.visibilityChanged);
    window.removeEventListener("blur", this.stop);
  }
}
