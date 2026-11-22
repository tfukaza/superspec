import { worktopLampShader } from "./worktopLamp";
import * as THREE from "three";
import { reflectionPanorama } from "./paintedMetalSky";
import { legacyRenderDiagnostics, type RenderDiagnostics } from "./renderProfile";

const probe = new THREE.Vector3(18.67, 0.93, -3.74);
const room = new THREE.Box3(new THREE.Vector3(15.9, 0.03, -6.48), new THREE.Vector3(19.12, 2.80, -2.48));

type Pigment = readonly [number, number, number];
type Slab = { bounds: THREE.Box3; color: Pigment };
function slab(min: [number, number, number], max: [number, number, number], color: Pigment): Slab {
  return { bounds: new THREE.Box3(new THREE.Vector3(...min), new THREE.Vector3(...max)), color };
}

// A small proxy of the roast bay. The strips, hood and lacquered hanging stock
// supply the main reflected value groups, without rendering the scene a second time.
const slabs = [
  slab([15.93, 2.29, -4.59], [16.06, 2.64, -3.00], [0.67, 0.47, 0.24]),
  slab([16.19, 1.85, -4.85], [16.92, 2.22, -3.28], [0.065, 0.088, 0.09]),
  slab([16.19, 1.83, -4.85], [16.92, 1.90, -3.28], [0.34, 0.39, 0.35]),
  slab([16.08, 1.35, -4.75], [16.16, 1.40, -2.54], [0.25, 0.19, 0.14]),
  slab([16.80, 2.64, -5.28], [17.15, 2.77, -3.24], [0.88, 0.78, 0.53]),
  slab([17.51, 2.72, -5.92], [17.89, 2.78, -2.66], [0.075, 0.106, 0.12]),
  slab([18.28, 2.10, -4.10], [18.55, 2.16, -3.87], [0.93, 0.59, 0.30]),
  slab([18.28, 2.10, -3.32], [18.55, 2.16, -3.09], [0.96, 0.68, 0.40]),
  slab([18.14, 1.94, -4.38], [18.20, 1.96, -2.61], [0.46, 0.49, 0.47]),
  slab([18.88, 1.18, -5.24], [19.00, 2.74, -5.03], [0.10, 0.20, 0.19]),
];
const stock = [-4.11, -3.77, -3.42, -3.05, -2.75].map((z, i) => ({
  center: new THREE.Vector3(18.20 + 0.025 * (i % 2), 1.55 + 0.03 * (i % 3), z),
  radius: new THREE.Vector3(0.098, 0.236, 0.101),
}));

function proxy(radiance: Float32Array, width: number, height: number): void {
  const ray = new THREE.Ray(probe);
  const localRay = new THREE.Ray();
  const hit = new THREE.Vector3();
  const duckHit = new THREE.Vector3();
  const sphere = new THREE.Sphere(new THREE.Vector3(), 1);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const azimuth = ((x + 0.5) / width - 0.5) * Math.PI * 2;
    const elevation = ((y + 0.5) / height - 0.5) * Math.PI;
    const across = Math.cos(elevation);
    ray.direction.set(Math.cos(azimuth) * across, Math.sin(elevation), Math.sin(azimuth) * across);
    ray.intersectBox(room, hit)!;
    let distance = hit.distanceTo(probe);
    const sky = hit.x > 19.11 || hit.z > -2.49;
    const ceiling = hit.y > 2.79;
    const frontage = hit.y < 1.83 && hit.z > -4.96 && hit.z < -3.42;
    let color: Pigment = sky ? [0.58, 0.64, 0.66] : ceiling ? [0.30, 0.28, 0.23]
      : frontage ? [0.15, 0.22, 0.18] : [0.32, 0.40, 0.35];
    let value = 0.92 + 0.09 * Math.sin(hit.z * 5.1 + 0.6 * Math.sin(hit.y * 8.7))
      + 0.045 * Math.cos(hit.x * 11.2 + hit.y * 9.3);
    for (const {bounds, color: paint} of slabs) {
      if (!ray.intersectBox(bounds, hit)) continue;
      const at = hit.distanceTo(probe);
      if (at >= distance) continue;
      distance = at;
      color = paint;
    }
    for (const {center, radius} of stock) {
      localRay.origin.copy(probe).sub(center).divide(radius);
      localRay.direction.copy(ray.direction).divide(radius).normalize();
      if (!localRay.intersectSphere(sphere, duckHit)) continue;
      hit.copy(duckHit).multiply(radius).add(center);
      const at = hit.distanceTo(probe);
      if (at >= distance) continue;
      distance = at;
      color = duckHit.y + 0.3 * duckHit.z > 0.22 ? [0.73, 0.34, 0.12] : [0.44, 0.19, 0.07];
      value *= 0.96 + 0.11 * Math.sin(duckHit.y * 8 + duckHit.z * 4);
    }
    const pigment = value * (0.96 + 0.055 * Math.sin(x * 1.37 + y * 2.28));
    radiance.set(color.map(channel => channel * pigment), (y * width + x) * 3);
  }
}

/** @deprecated The default finish uses the painted worktop. Use the session's worktopReflection setting for overrides. */
export const usePaintedWorktop = true;
const panoramas = new Map<boolean, THREE.DataTexture>();

export function paintedWorktop(mode: RenderDiagnostics["materials"]["worktopReflection"] =
  legacyRenderDiagnostics().materials.worktopReflection): THREE.DataTexture {
  const paint = mode !== "raw";
  const cached = panoramas.get(paint);
  if (cached) return cached;
  const width = 96, height = 48;
  const source = new Float32Array(width * height * 3);
  proxy(source, width, height);
  const panorama = reflectionPanorama(source, width, height, "Roast bay reflection", paint);
  panoramas.set(paint, panorama);
  return panorama;
}

export const paintedWorktopShader = /* glsl */ `
${worktopLampShader}
uniform sampler2D msWorktopPanorama;
vec3 msWorktopPaint(vec3 position, vec3 reflection, float roughness) {
  // Correct the environment lookup from the shaded point to the surface it
  // sees inside the proxy. Keep the brush variation in world space.
  vec3 lo = vec3(15.90, 0.03, -6.48);
  vec3 hi = vec3(19.12, 2.80, -2.48);
  vec3 probe = vec3(18.67, 0.93, -3.74);
  vec3 origin = clamp(position, lo + .01, hi - .01);
  vec3 ray = reflection + vec3(.00001);
  vec3 toward = mix(lo, hi, step(vec3(0.), ray));
  vec3 exitAt = (toward - origin) / ray;
  float reach = min(exitAt.x, min(exitAt.y, exitAt.z));
  vec3 local = origin + reflection * reach - probe;
  vec3 corrected = normalize(mix(reflection, normalize(local + vec3(.00001)), .82));
  corrected.y += .018 * sin(position.z * 14.0 + 1.6 * sin(position.x * 28.0));
  corrected.z += .011 * sin(position.x * 61.0 + position.z * 2.2);
  vec3 paint = textureLod(msWorktopPanorama, equirectUv(normalize(corrected)), .12 + roughness * 1.95).rgb;
  return msWorktopLamp(paint,position,reflection);
}
`;
