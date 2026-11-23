import * as THREE from "three";
import { brushPigmentAt } from "./paintedBrush.ts";

// Tangent to a concentric machining groove, recovered from the centered disc UVs.
// Its sign cancels out, including on mirrored charts and the vertical shell.
// Interpolate each ring's vertex radius to keep isolines straight across its triangles.
export function radialSkyVertex(source: string): string {
  return "attribute vec3 radialTangent;\nvarying vec3 msGrooveDir;\nvarying vec3 msToolChart;\n" + source.replace("#include <uv_vertex>",
    "#include <uv_vertex>\nmsGrooveDir = mat3(modelViewMatrix) * radialTangent;\nmsToolChart = vec3(uv - 0.5, length(uv - 0.5));");
}

export const radialSkyShader = /* glsl */ `
varying vec3 msGrooveDir;
varying vec3 msToolChart;
vec3 msRadialReflection(sampler2D skyMap, vec3 surface, vec3 eye,
                        vec3 normalVS, float roughness, bool zUp) {
#ifdef USE_TANGENT
  vec3 tangent = msToolChart.x * normalize(vBitangent) - msToolChart.y * normalize(vTangent);
#else
  vec3 tangent = dFdx(surface) * dFdy(msToolChart.z) - dFdy(surface) * dFdx(msToolChart.z);
#endif
  if (dot(msGrooveDir, msGrooveDir) > 1e-10) tangent = msGrooveDir;
  float norm2 = dot(tangent, tangent);
  tangent *= inversesqrt(max(norm2, 1e-24));
  vec3 key = normalize(mat3(viewMatrix) * (zUp ? vec3(0.70, -0.68, -0.15) : vec3(0.70, -0.15, 0.68)));
  vec3 halfDir = eye + key;
  float across = dot(tangent, halfDir) * inversesqrt(max(dot(halfDir, halfDir), 1e-8));
  float sweep = pow(max(0.0, 1.0 - across * across), mix(13.0, 5.0, roughness));
  vec3 bend = eye - tangent * dot(eye, tangent);
  vec3 brushed = normalize(normalVS + 0.65 * bend);
  vec3 direction = transformDirectionByInverseViewMatrix(reflect(-eye, brushed), viewMatrix);
  if (zUp) direction = vec3(direction.x, direction.z, -direction.y);
  vec3 sky = textureLod(skyMap, equirectUv(direction), 0.7 + 2.5 * roughness).rgb;
  // Blue skylight and warm reflected fill occupy opposite sides of the panorama.
  float warm = smoothstep(-0.55, 0.55, dot(direction, vec3(-0.81, -0.18, 0.55)));
  vec3 skyTint = mix(vec3(0.52, 0.92, 1.46), vec3(1.41, 0.87, 0.60), warm);
  return skyTint * step(1e-24, norm2) * sweep * (1.0 - 0.55 * roughness)
    * (vec3(0.18, 0.20, 0.22) + 0.86 * sky);
}
`;

// One world-aligned panorama shared by all published metals. Bake the Kuwahara
// filter before uploading so each fragment needs only one reflection lookup.
const skies = new Map<string, THREE.DataTexture>();

export function paintedMetalSky(broadBands = false, brushEdges = false): THREE.DataTexture {
  const key = `${broadBands}/${brushEdges}`;
  const cached = skies.get(key);
  if (cached) return cached;
  const width = 64, height = 32;
  let state = 0x4b6c616c;
  const random = () => {
    state = (Math.imul(1664525, state) + 1013904223) >>> 0;
    return state / 4294967296;
  };
  const skyline = Array.from({ length: 16 }, () => random() * 0.28 - 0.13);
  const phases = Array.from({ length: 5 }, () => random() * 2 * Math.PI);
  const source = new Float32Array(width * height * 3);
  for (let y = 0; y < height; y++) {
    const elevation = Math.sin(((y + 0.5) / height - 0.5) * Math.PI);
    for (let x = 0; x < width; x++) {
      const angle = (x + 0.5) / width * 2 * Math.PI;
      const clouds = Math.sin(3 * angle + 5 * elevation + phases[0])
        + 0.7 * Math.cos(5 * angle - 8 * elevation + phases[1])
        + 0.45 * Math.sin(2 * angle + 12 * elevation + phases[2]);
      const opening = elevation > skyline[Math.floor(x / 4)];
      const light = opening ? 0.55 + 0.19 * clouds : 0.14 + 0.055 * Math.cos(4 * angle + phases[3]);
      const tone = THREE.MathUtils.lerp(light + (random() - 0.5) * 0.16,
        elevation > 0 ? 0.64 : 0.12, Math.abs(elevation) ** 6);
      const warm = Math.max(0, 1 - 3 * Math.abs(elevation)) * (0.5 + 0.5 * Math.cos(angle + phases[4]));
      // Group luminance before filtering so the sector average preserves broad patches of color.
      const value = broadBands ? THREE.MathUtils.lerp(tone, Math.round(tone * 5) / 5, 0.8) : tone;
      source.set([value * (0.85 + 0.20 * warm), value * 0.94, value * (1.03 - 0.22 * warm)], (y * width + x) * 3);
    }
  }

  const radius = brushEdges ? (broadBands ? 6 : 4) : (broadBands ? 4 : 2);
  const sky = reflectionPanorama(source, width, height, "Kuwahara-filtered metal sky", true, radius);
  if (brushEdges) paintReflectionEdges(sky, width, height);
  skies.set(key, sky);
  return sky;
}

/** Vary the footprint of a directional filter at reflection boundaries. */
function paintReflectionEdges(sky: THREE.DataTexture, width: number, height: number): void {
  const pixels = sky.image.data as Uint8Array;
  const { clamp, lerp, smoothstep } = THREE.MathUtils;
  // Quantize after the sector averaging so neighbouring plateaus stay distinct.
  // Scale the RGB together to preserve the sky's tint at each value.
  for (let i = 0; i < pixels.length; i += 4) {
    const luma = 0.2126 * pixels[i] + 0.7152 * pixels[i + 1] + 0.0722 * pixels[i + 2];
    const step = 255 / 7;
    const band = Math.max(0.35 * step, Math.round(luma / step) * step);
    const gain = luma > 0 ? lerp(1, band / luma, 0.87) : 1;
    for (let channel = 0; channel < 3; channel++)
      pixels[i + channel] = Math.round(Math.min(255, pixels[i + channel] * gain));
  }
  const source = pixels.slice();
  const sample = (x: number, y: number, channel: number) => {
    const ix = Math.floor(x), iy = Math.floor(y);
    const at = (col: number, row: number) => source[(clamp(row, 0, height - 1) * width
      + (col + width) % width) * 4 + channel];
    return lerp(lerp(at(ix, iy), at(ix + 1, iy), x - ix),
      lerp(at(ix, iy + 1), at(ix + 1, iy + 1), x - ix), y - iy);
  };
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const dx = sample(x + 1, y, 1) - sample(x - 1, y, 1);
    const dy = sample(x, y + 1, 1) - sample(x, y - 1, 1);
    const edge = Math.hypot(dx, dy);
    const weight = smoothstep(edge, 7, 32);
    if (weight === 0) continue;
    const yBrush = y * 0.17;
    const brush = lerp(brushPigmentAt(x * 0.17, yBrush, 0.75),
      brushPigmentAt((x - width) * 0.17, yBrush, 0.75), smoothstep(x, width * 0.8, width));
    const acrossX = dx / edge, acrossY = dy / edge;
    const shift = 8.5 * brush;
    const reach = 1.0 + 2.4 * brush;
    const u = x + acrossX * shift, v = y + acrossY * shift;
    const offsetX = -acrossY * reach, offsetY = acrossX * reach;
    const index = (y * width + x) * 4;
    for (let channel = 0; channel < 3; channel++) {
      const along = (sample(u - offsetX, v - offsetY, channel) + sample(u + offsetX, v + offsetY, channel)) * 0.15;
      const value = 0.70 * sample(u, v, channel) + along;
      pixels[index + channel] = Math.round(lerp(source[index + channel], value, weight));
    }
  }
}

/** Build a linear panorama, with an optional edge-preserving bake in four overlapping sectors. */
export function reflectionPanorama(source: Float32Array, width: number, height: number, name: string,
  filter = true, radius = 2): THREE.DataTexture {
  const samples = (radius + 1) ** 2;
  const pixels = new Uint8Array(width * height * 4);
  const byte = (v: number) => Math.round(255 * THREE.MathUtils.clamp(v, 0, 1));
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const out = (y * width + x) * 4;
    pixels[out + 3] = 255;
    if (!filter) {
      pixels.set(source.subarray((out / 4) * 3, (out / 4) * 3 + 3).map(byte), out);
      continue;
    }
    let bestVariance = Infinity;
    for (const dy of [-1, 1]) for (const dx of [-1, 1]) {
      let r = 0, g = 0, b = 0, sum = 0, squares = 0;
      for (let j = 0; j <= radius; j++) for (let i = 0; i <= radius; i++) {
        const row = Math.max(0, Math.min(height - 1, y + dy * j));
        const k = (row * width + (x + dx * i + width) % width) * 3;
        const luma = 0.2126 * source[k] + 0.7152 * source[k + 1] + 0.0722 * source[k + 2];
        r += source[k]; g += source[k + 1]; b += source[k + 2];
        sum += luma; squares += luma * luma;
      }
      const variance = squares / samples - (sum / samples) ** 2;
      if (variance < bestVariance) {
        bestVariance = variance;
        pixels.set([r, g, b].map(v => byte(v / samples)), out);
      }
    }
  }
  const texture = new THREE.DataTexture(pixels, width, height);
  texture.name = name;
  texture.wrapS = THREE.RepeatWrapping;
  texture.magFilter = THREE.LinearFilter;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.generateMipmaps = true;
  texture.needsUpdate = true;
  return texture;
}
