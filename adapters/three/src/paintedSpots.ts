import * as THREE from "three";
import { brushPigmentAt } from "./paintedBrush.ts";

export type SpotOptics = { angle: number; penumbra: number; range: number };

const SIZE = 128;
const { smoothstep, clamp } = THREE.MathUtils;

function projector(data: Uint16Array, size: number): THREE.DataTexture {
  const map = new THREE.DataTexture(data, size, size, THREE.RGBAFormat, THREE.HalfFloatType);
  map.name = "painted-fixture-pool";
  map.magFilter = THREE.LinearFilter;
  map.minFilter = THREE.LinearMipmapLinearFilter;
  map.generateMipmaps = true;
  map.needsUpdate = true;
  return map;
}

function poolProjector(optics: SpotOptics, height: number): THREE.DataTexture {
  const { angle, penumbra, range } = optics;
  const halfWidth = height * Math.tan(angle);
  const outer = Math.cos(angle), inner = Math.cos(angle * (1 - penumbra));
  // Match Three's cone, inverse-square attenuation, smooth distance cutoff and
  // cosine at the receiving floor. The texture stores a ratio to that profile.
  const floorLight = (radius: number) => {
    const d = Math.hypot(height, radius), cosine = height / d;
    const cone = inner === outer ? Number(cosine >= outer) : smoothstep(cosine, outer, inner);
    return cone * Math.max(0, 1 - (d / range) ** 4) ** 2 * cosine / (d * d);
  };
  const peak = floorLight(0);
  const base = new Float32Array(SIZE * SIZE);
  const field = new Float32Array(base.length);
  const gain = new Float32Array(base.length).fill(1);
  const weights = new Float32Array(base.length);
  let correction = 0, weightSum = 0;
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      const u = (2 * (x + 0.5) / SIZE - 1) * halfWidth;
      const v = (2 * (y + 0.5) / SIZE - 1) * halfWidth;
      const i = y * SIZE + x, radius = Math.hypot(u, v);
      base[i] = floorLight(radius) / peak;
      // Broad wash carries the shoulder; the GI chisel leaves shorter marks across it.
      const broad = Math.sin(3.8 * u + 2.2 * v + 0.7 * Math.sin(5.4 * v));
      const dab = brushPigmentAt(u, v, 0.48);
      field[i] = floorLight(Math.max(0, radius + 0.14 * broad + 0.46 * dab)) / peak;
    }
  }
  const stride = clamp(Math.round(0.10 * SIZE / (2 * halfWidth)), 1, 4);
  for (let y = stride; y < SIZE - stride; y++) {
    for (let x = stride; x < SIZE - stride; x++) {
      const i = y * SIZE + x;
      const weight = smoothstep(base[i], 0.012, 0.07) * (1 - smoothstep(base[i], 0.72, 0.96));
      if (!weight) continue;
      // Four overlapping quadrants: select the neighbourhood with the least
      // variance, keeping a stroke's boundary while flattening its interior.
      let least = Infinity, selected = field[i];
      for (const dx of [-stride, stride]) {
        for (const dy of [-stride * SIZE, stride * SIZE]) {
          const a = field[i], b = field[i + dx], c = field[i + dy], d = field[i + dx + dy];
          const mean = (a + b + c + d) * 0.25;
          const variance = (a*a + b*b + c*c + d*d) * 0.25 - mean*mean;
          if (variance < least) { least = variance; selected = mean; }
        }
      }
      weights[i] = weight;
      gain[i] += weight * clamp(selected / base[i] - 1, -0.53, 0.58);
      correction += base[i] * (gain[i] - 1);
      weightSum += base[i] * weight;
    }
  }
  const bias = weightSum > 0 ? correction / weightSum : 0;
  const rgb = new Uint16Array(base.length * 4);
  const one = THREE.DataUtils.toHalfFloat(1);
  for (let i = 0; i < base.length; i++) {
    const value = THREE.DataUtils.toHalfFloat(gain[i] - weights[i] * bias);
    rgb[i * 4] = rgb[i * 4 + 1] = rgb[i * 4 + 2] = value;
    rgb[i * 4 + 3] = one;
  }
  return projector(rgb, SIZE);
}

/** One room normally has three distinct optical profiles. Cache by optics, not placement. */
export class PaintedSpotMaps {
  readonly neutral = projector(new Uint16Array(4).fill(THREE.DataUtils.toHalfFloat(1)), 1);
  private readonly profiles = new Map<string, THREE.DataTexture>();

  get(optics: SpotOptics, height: number): THREE.DataTexture {
    if (height <= 0 || height >= optics.range) return this.neutral;
    const key = [optics.angle, optics.penumbra, optics.range, height].map(n => n.toFixed(4)).join("/");
    let map = this.profiles.get(key);
    if (!map) {
      if (this.profiles.size >= 8) return this.neutral;
      map = poolProjector(optics, height);
      this.profiles.set(key, map);
    }
    return map;
  }

  dispose(): void {
    this.neutral.dispose();
    for (const map of this.profiles.values()) map.dispose();
    this.profiles.clear();
  }
}
