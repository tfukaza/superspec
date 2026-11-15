import { PhysicalWalk, type Player, type V3 } from "./physical-walk.ts";
import { WalkMotion, type WalkHelper } from "./walk-motion.ts";

let walk: WalkMotion | undefined;
interface Request {
  triangles?: Float32Array; owners?: Uint32Array; point?: V3; agent?: number[];
  forward?: number; right?: number; yaw?: number; seconds?: number;
  jump?: boolean; crouch?: boolean; helper?: WalkHelper;
}

/** Runs browser movement against immutable collision data produced by the compiler. */
self.onmessage = (event: MessageEvent<Request>) => {
  try {
    const request = event.data;
    if (request.triangles) {
      const [radius, height, step, slope, crouch] = request.agent!;
      const player: Player = { radius, height, step, slope, crouch: crouch || Math.min(height, Math.max(radius * 2, 1.1)) };
      walk = new WalkMotion(new PhysicalWalk(request.triangles, request.owners!), player, request.point!);
    } else {
      if (!walk) throw new Error("Walking geometry is unavailable.");
      walk.advance({ forward: request.forward!, right: request.right!, yaw: request.yaw!, jump: request.jump!, crouch: request.crouch! }, request.seconds!, request.helper!);
    }
    self.postMessage(walk!.state);
  } catch (error) {
    self.postMessage({ error: String(error) });
  }
};
