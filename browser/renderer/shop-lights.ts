import * as THREE from "three";

// A shaded shop lamp at stock-shelf distance. Candela and cutoff are in game metres.
const CANDELA = 52;
const RANGE = 5.2;
const MAX_LIGHTS = 3;

/** Downward flood from the soffit-mounted shop tubes. The wide soft cone keeps the
 * fluorescent light on the shelves and pavement beneath each fixture. */
export class ShopLights {
  private readonly fixtures: { position: THREE.Vector3; distance: number }[] = [];
  private readonly lights: THREE.SpotLight[] = [];
  private readonly lastEye = new THREE.Vector3(Infinity, Infinity, Infinity);

  constructor(scene: THREE.Scene, positions: unknown) {
    if (!Array.isArray(positions)) return;
    for (const p of positions) {
      if (Array.isArray(p) && p.length === 3 && p.every(Number.isFinite)) {
        this.fixtures.push({ position: new THREE.Vector3(p[0], p[1], p[2]), distance: Infinity });
      }
    }
    for (let i = 0; i < Math.min(MAX_LIGHTS, this.fixtures.length); i++) {
      const light = new THREE.SpotLight(0xebf1e6, 0, RANGE, Math.PI * 0.445, 0.45, 2);
      light.name = "shop-fixture";
      this.lights.push(light);
      scene.add(light, light.target);
    }
  }

  update(eye: THREE.Vector3): void {
    if (!this.lights.length || eye.distanceToSquared(this.lastEye) < 0.04) return;
    this.lastEye.copy(eye);
    for (const f of this.fixtures) f.distance = eye.distanceTo(f.position);
    this.fixtures.sort((a, b) => a.distance - b.distance);
    // At equal rank, fade the outgoing lamp over its last metre before reassignment.
    const omitted = this.fixtures[this.lights.length]?.distance ?? Infinity;
    for (const [i, light] of this.lights.entries()) {
      const f = this.fixtures[i];
      light.position.copy(f.position);
      light.target.position.set(f.position.x, f.position.y - 1, f.position.z);
      light.intensity = CANDELA * THREE.MathUtils.smoothstep(omitted - f.distance, 0, 1)
        * (1 - THREE.MathUtils.smoothstep(f.distance, 14, 22));
    }
  }

  dispose(): void {
    for (const light of this.lights) { light.removeFromParent(); light.target.removeFromParent(); light.dispose(); }
  }
}
