import { PhysicalWalk, type Player, type V3 } from "./physical-walk.ts";

export interface MotionInput { forward: number; right: number; yaw: number; jump: boolean; crouch: boolean }
export type WalkHelper = "off" | "jump" | "crouch";

/** Browser-owned inspection motion over the compiler's immutable collision triangles. */
export class WalkMotion {
  private readonly physical: PhysicalWalk;
  private readonly player: Player;
  private support: V3;
  private lift = 0;
  private velocity = 0;
  private jumpHeld = false;
  private crouched = false;

  constructor(physical: PhysicalWalk, player: Player, requested: V3) {
    this.physical = physical;
    this.player = player;
    if ([...requested, ...Object.values(player)].some(value => !Number.isFinite(value))
      || player.radius <= 0 || player.height < player.radius * 2 || player.crouch < player.radius * 2
      || player.crouch > player.height || player.step < 0 || player.slope < 0 || player.slope >= 89) throw new Error("Invalid inspection position or player dimensions.");
    const feet = physical.resolve(requested, requested, player);
    if (!feet || Math.abs(feet[2] - requested[2]) > 0.1) throw new Error("Select a clear, supported floor to start walking.");
    this.support = feet;
  }

  private get shape(): Player { return { ...this.player, height: this.crouched ? this.player.crouch : this.player.height }; }

  get state() {
    const feet: V3 = [this.support[0], this.support[1], this.support[2] + this.lift];
    const height = this.shape.height;
    return { feet, eye: [feet[0], feet[1], feet[2] + height * 0.93] as V3, height, radius: this.player.radius,
      stance: this.crouched ? "crouching" : "standing", grounded: this.lift === 0, verticalSpeed: this.velocity };
  }

  /** Samples support and capsule clearance at bounded substeps, including while lifted. */
  private move(dx: number, dy: number): void {
    const steps = Math.max(1, Math.ceil(Math.hypot(dx, dy) / Math.min(0.05, this.player.radius * 0.25)));
    for (let i = 0; i < steps; i++) {
      const candidate: V3 = [this.support[0] + dx / steps, this.support[1] + dy / steps, this.support[2]];
      const next = this.physical.resolve(candidate, this.support, this.shape);
      if (!next || this.physical.blocked([next[0], next[1], next[2] + this.lift], this.shape)) break;
      this.support = next;
    }
  }

  /** Jump helpers retain a supported horizontal anchor and do not establish gap-jump reachability. */
  advance(input: MotionInput, seconds: number, helper: WalkHelper): void {
    if (![input.forward, input.right, input.yaw, seconds].every(Number.isFinite)
      || Math.abs(input.forward) > 1 || Math.abs(input.right) > 1 || seconds < 0 || seconds > 0.25
      || !["off", "jump", "crouch"].includes(helper)) throw new Error("Invalid bounded inspection input.");
    const jumpPressed = input.jump && !this.jumpHeld;
    if (seconds > 0) this.jumpHeld = input.jump;
    if (input.crouch || helper === "crouch") this.crouched = true;
    else if (this.crouched && !this.physical.blocked(this.state.feet, this.player)) this.crouched = false;
    const count = Math.max(1, Math.ceil(seconds * 120));
    const dt = seconds / count;
    for (let i = 0; i < count; i++) {
      if (dt > 0 && this.lift === 0 && (helper === "jump" || jumpPressed && i === 0)) this.velocity = 5.2;
      const scale = (this.crouched ? 1.7 : 3.4) * dt / Math.max(1, Math.hypot(input.forward, input.right));
      this.move((Math.cos(input.yaw) * input.forward + Math.sin(input.yaw) * input.right) * scale,
        (Math.sin(input.yaw) * input.forward - Math.cos(input.yaw) * input.right) * scale);
      if (this.lift === 0 && this.velocity === 0) continue;
      this.velocity -= 20 * dt;
      const next = Math.max(0, this.lift + this.velocity * dt);
      if (this.physical.blocked([this.support[0], this.support[1], this.support[2] + next], this.shape)) this.velocity = 0;
      else this.lift = next;
      if (this.lift === 0) this.velocity = 0;
    }
  }
}
