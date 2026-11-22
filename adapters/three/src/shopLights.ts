import * as THREE from "three";
import { PaintedSpotMaps } from "./paintedSpots";
import { legacyRenderDiagnostics } from "./renderProfile";

// A shaded shop lamp at stock-shelf distance. Candela and cutoff are in game metres.
const CANDELA = 52;
const RANGE = 5.2;
const MAX_LIGHTS = 3;

type Point = readonly [number, number, number];
type LightProfile = {
  color: number;
  candela: number;
  range: number;
  angle: number;
  penumbra: number;
};
type Fixture = LightProfile & {
  position: THREE.Vector3;
  target: THREE.Vector3;
  distance: number;
  ceiling?: number;
  floor?: number;
  poolMap?: THREE.DataTexture;
};

const TUBE: LightProfile = { color: 0xebf1e6, candela: CANDELA, range: RANGE, angle: Math.PI * 0.445, penumbra: 0.45 };
const CASE: LightProfile = { color: 0xffdbb2, candela: 4.8, range: 2.05, angle: 0.77, penumbra: 0.62 };
const THRESHOLD: LightProfile = { color: 0xffdbb4, candela: 9, range: 3.25, angle: 0.82, penumbra: 0.62 };
const PASSAGE: LightProfile = { color: 0xffe1c2, candela: 0.48, range: 1.15, angle: 1.02, penumbra: 0.96 };

function isLightProfile(p: LightProfile): boolean {
  return Number.isInteger(p.color) && p.color >= 0 && p.color <= 0xffffff
    && Number.isFinite(p.candela) && p.candela > 0 && Number.isFinite(p.range) && p.range > 0
    && p.angle > 0 && p.angle < Math.PI / 2 && p.penumbra >= 0 && p.penumbra <= 1
    && Number.isFinite(p.angle) && Number.isFinite(p.penumbra);
}

function isPoint(value: unknown): value is Point {
  return Array.isArray(value) && value.length === 3 && value.every(Number.isFinite);
}
// Stoa world coordinates. The case lights sit below their shades, aimed at
// the customer-facing side of the birds and the trays beneath them.
const STOA_ACCENTS: readonly { position: Point; target: Point; profile: LightProfile }[] = [
  { position: [18.90, 2.08, -3.08], target: [18.43, 1.20, -3.28], profile: CASE },
  { position: [18.91, 2.06, -4.18], target: [18.49, 1.16, -4.31], profile: CASE },
  { position: [20.2, 1.95, 11.57], target: [18.59, 0.68, 11.49], profile: THRESHOLD },
];

/** Reuse the closest fixtures and their shadow targets across the map. */
export class ShopLights {
  private readonly fixtures: Fixture[] = [];
  private readonly lights: THREE.SpotLight[] = [];
  get shadowLights(): readonly THREE.SpotLight[] { return this.lights; }
  private readonly lastEye = new THREE.Vector3(Infinity, Infinity, Infinity);
  private readonly staticShadows: boolean;
  private readonly stableSlots: boolean;
  private readonly assigned: (Fixture | undefined)[] = [];
  private readonly shadowFixtures: (Fixture | undefined)[] = [];

  private readonly paint?: PaintedSpotMaps;

  constructor(scene: THREE.Scene, positions: unknown, stoaAccents = false,
    painted = legacyRenderDiagnostics().lighting.paintedSpots,
    options: { staticShadows?: boolean; stableSlots?: boolean; shadowMapSize?: number } = {}) {
    this.staticShadows = options.staticShadows === true;
    this.stableSlots = this.staticShadows && options.stableSlots === true;
    if (painted && Array.isArray(positions) && positions.some(p => p?.kind === "arcade-batten" || p?.kind === "planter-ceiling")) {
      this.paint = new PaintedSpotMaps();
    }
    if (Array.isArray(positions)) {
      for (const p of positions) {
        if (p?.kind === "shop-fixture" && isLightProfile(p) && isPoint(p.position) && isPoint(p.target)) {
          if (!p.accent || stoaAccents) {
            const accentColor = p.accentColor;
            const color = stoaAccents && Number.isInteger(accentColor) && accentColor >= 0 && accentColor <= 0xffffff
              ? accentColor : p.color;
            this.add(p.position, p.target, { ...p, color });
          }
          continue;
        }
        if (p?.kind === "arcade-batten" && isLightProfile(p) && isPoint(p.position) && isPoint(p.target) && p.target[1] < p.position[1]) {
          this.add(p.position, p.target, p, p.position[1] + 0.11, undefined, true);
          continue;
        }
        if (p?.kind === "planter-ceiling" && isLightProfile(p) && isPoint(p.position) && isPoint(p.target)
          && Number.isFinite(p.floorY) && Number.isFinite(p.ceilingY)
          && p.floorY === p.target[1] && p.floorY < p.position[1] && p.position[1] <= p.ceilingY) {
          const cullByFloor = p.cameraFloorCull !== false;
          this.add(p.position, p.target, p, cullByFloor ? p.ceilingY : undefined, cullByFloor ? p.floorY : undefined,
            p.fixture === "ceiling-twin-strip" || p.fixture === "ceiling-bulkhead");
          continue;
        }
        if (p?.kind === "passage-lamp" && isPoint(p.position) && isPoint(p.target)) {
          this.add(p.position, p.target, isLightProfile(p) ? p : PASSAGE);
          continue;
        }
        if (!isPoint(p)) continue;
        const [x, y, z] = p;
        const isLiquorTube = stoaAccents && Math.abs(x - 18.07) < 0.15 && z >= 10.4 && z <= 12.65 && y < 2.9;
        const profile = isLiquorTube ? { ...TUBE, color: z < 11.5 ? 0xe1ecdc : 0xf3e5cd } : TUBE;
        this.add([x, y, z], [x, y - 1, z], profile);
      }
    }
    if (stoaAccents && !(Array.isArray(positions) && positions.some(p => p?.kind === "shop-fixture"))) {
      for (const accent of STOA_ACCENTS) this.add(accent.position, accent.target, accent.profile);
    }
    const shadowMapSize = options.shadowMapSize ?? 512;
    for (let i = 0; i < Math.min(MAX_LIGHTS, this.fixtures.length); i++) {
      const light = new THREE.SpotLight(TUBE.color, 0, TUBE.range, TUBE.angle, TUBE.penumbra, 2);
      light.name = "shop-fixture";
      light.castShadow = true;
      light.shadow.mapSize.set(shadowMapSize, shadowMapSize);
      light.shadow.camera.near = 0.05;
      light.shadow.normalBias = 0.008;
      // Keep projector count stable when a slot switches to an ordinary shop accent.
      light.map = this.paint?.neutral ?? null;
      this.lights.push(light);
      scene.add(light, light.target);
    }
  }

  private add(position: Point, target: Point, profile: LightProfile, ceiling?: number, floor?: number, painted = false): void {
    this.fixtures.push({
      ...profile,
      position: new THREE.Vector3(...position),
      target: new THREE.Vector3(...target),
      distance: Infinity,
      poolMap: painted ? this.paint?.get(profile, position[1] - target[1]) : undefined,
      ceiling,
      floor,
    });
  }

  update(eye: THREE.Vector3): void {
    if (!this.lights.length || eye.distanceToSquared(this.lastEye) < 0.04) return;
    this.lastEye.copy(eye);
    for (const f of this.fixtures) {
      const outsideFloor = (f.ceiling !== undefined && eye.y > f.ceiling) || (f.floor !== undefined && eye.y < f.floor);
      f.distance = outsideFloor ? Number.MAX_SAFE_INTEGER : eye.distanceTo(f.position);
    }
    this.fixtures.sort((a, b) => a.distance - b.distance);
    // At equal rank, fade the outgoing lamp over its last metre before reassignment.
    const omitted = this.fixtures[this.lights.length]?.distance ?? Infinity;
    // Exchanging ranks does not change the lighting sum. Keep each surviving
    // fixture's shadow target and allocate only the newly selected fixtures.
    const selected = this.stableSlots ? this.fixtures.slice(0, this.lights.length) : this.fixtures;
    const retained = this.stableSlots
      ? this.assigned.map(fixture => fixture && selected.includes(fixture) ? fixture : undefined) : [];
    const incoming = this.stableSlots ? selected.filter(fixture => !retained.includes(fixture)) : selected;
    let next = 0;
    for (const [i, light] of this.lights.entries()) {
      const f = retained[i] ?? incoming[next++];
      light.position.copy(f.position);
      light.target.position.copy(f.target);
      light.color.setHex(f.color);
      light.distance = f.range;
      light.angle = f.angle;
      if (this.paint) light.map = f.poolMap ?? this.paint.neutral;
      light.penumbra = f.penumbra;
      light.intensity = f.candela * THREE.MathUtils.smoothstep(omitted - f.distance, 0, 1)
        * (1 - THREE.MathUtils.smoothstep(f.distance, 14, 22));
      light.shadow.autoUpdate = !this.staticShadows && light.intensity > 0;
      // A static view reuses the map while its fixture stays assigned. Brightness
      // changes only affect lighting. A new fixture changes the projection too.
      // A dark slot can defer capturing a new projection until it contributes.
      // An uninitialized target or explicit invalidation still gets a full refresh.
      light.shadow.needsUpdate ||= light.shadow.map === null ||
        (this.shadowFixtures[i] !== f && (!this.stableSlots || light.intensity > 0));
      if (light.shadow.needsUpdate) this.shadowFixtures[i] = f;
      this.assigned[i] = f;
    }
  }

  /** Call after a static caster edit or context restoration. Moving actors use
   * the default dynamic mode, which redraws active shadows each frame. */
  invalidateShadows(): void {
    for (const light of this.lights) light.shadow.needsUpdate = true;
  }

  dispose(): void {
    this.paint?.dispose();
    for (const light of this.lights) { light.removeFromParent(); light.target.removeFromParent(); light.dispose(); }
  }
}
