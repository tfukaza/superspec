import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";

function imageSource(image: unknown): boolean {
  return (typeof ImageBitmap !== "undefined" && image instanceof ImageBitmap)
    || (typeof HTMLImageElement !== "undefined" && image instanceof HTMLImageElement)
    || (typeof HTMLCanvasElement !== "undefined" && image instanceof HTMLCanvasElement)
    || (typeof OffscreenCanvas !== "undefined" && image instanceof OffscreenCanvas);
}

/** Keep every texel and mip while storing only the channels consumed by the
 * admitted surface shaders. TexImageSource conversion happens during GL upload. */
export function narrowDataTextureChannels(gltf: GLTF, admitted: ReadonlySet<THREE.Material>): number {
  const channels = new Map<THREE.Texture, number>();
  const blockedSources = new Set<THREE.Texture["source"]>();
  const materials = new Set<THREE.Material>(), visited = new WeakSet<object>();
  const blockTextures = (value: unknown): void => {
    if (!value || typeof value !== "object" || visited.has(value)) return;
    visited.add(value);
    if (value instanceof THREE.Texture) { blockedSources.add(value.source); return; }
    if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer) return;
    for (const child of Object.values(value)) blockTextures(child);
  };
  for (const scene of new Set([gltf.scene, ...gltf.scenes])) {
    if (scene instanceof THREE.Scene) { blockTextures(scene.background); blockTextures(scene.environment); }
    scene.traverse(object => {
      blockTextures(object.userData);
      if ("map" in object) blockTextures(object.map);
      if (!(object instanceof THREE.Mesh)) return;
      blockTextures(object.geometry.userData);
      blockTextures(object.customDepthMaterial); blockTextures(object.customDistanceMaterial);
      for (const material of Array.isArray(object.material) ? object.material : [object.material]) {
        if (materials.has(material)) continue;
        materials.add(material);
        for (const [key, value] of Object.entries(material)) {
          if (key === "aoMap" && value instanceof THREE.Texture && admitted.has(material)) {
            const required = material.userData.materialSpecEdge ? 2 : 1;
            channels.set(value, Math.max(channels.get(value) ?? 0, required));
          } else blockTextures(value);
        }
      }
    });
  }

  // Include serialized references that a host or later adapter may consume.
  // Unknown texture-bearing extras are conservative conflicts, as are ordinary
  // color, normal, packed metallic/roughness and emission texture slots.
  const blockedSlots = new Set<number>();
  const scanSlots = (value: unknown): void => {
    if (!value || typeof value !== "object") return;
    for (const [key, child] of Object.entries(value)) {
      if (key === "occlusionTexture") {
        scanSlots((child as { extensions?: unknown } | null)?.extensions);
        scanSlots((child as { extras?: unknown } | null)?.extras);
        continue;
      }
      if (/texture/i.test(key)) {
        const index = typeof child === "number" ? child : (child as { index?: unknown } | null)?.index;
        if (Number.isInteger(index) && (index as number) >= 0) blockedSlots.add(index as number);
      }
      scanSlots(child);
    }
  };
  scanSlots(gltf.parser.json);
  const definitions = gltf.parser.json.textures ?? [];
  const blockedImages = new Set<number>();
  for (const slot of blockedSlots) {
    const image = definitions[slot]?.source;
    if (Number.isInteger(image)) blockedImages.add(image);
  }
  for (const [object, reference] of gltf.parser.associations) {
    if (object instanceof THREE.Texture && reference.textures !== undefined
      && (blockedSlots.has(reference.textures) || blockedImages.has(definitions[reference.textures]?.source)))
      blockedSources.add(object.source);
  }

  // Clones can share a GPU source while selecting different UV channels. All
  // permitted consumers of that source receive its widest required storage.
  const sourceChannels = new Map<THREE.Texture["source"], number>();
  for (const [texture, count] of channels) sourceChannels.set(texture.source, Math.max(sourceChannels.get(texture.source) ?? 0, count));
  let reduced = 0;
  for (const texture of channels.keys()) {
    if (blockedSources.has(texture.source) || texture.format !== THREE.RGBAFormat || texture.type !== THREE.UnsignedByteType
      || texture.colorSpace !== THREE.NoColorSpace || texture.mipmaps.length || texture.premultiplyAlpha
      || (texture.internalFormat !== null && texture.internalFormat !== "RGBA8") || !imageSource(texture.image)) continue;
    const count = sourceChannels.get(texture.source)!;
    texture.format = count === 2 ? THREE.RGFormat : THREE.RedFormat;
    texture.internalFormat = count === 2 ? "RG8" : "R8";
    texture.needsUpdate = true;
    reduced++;
  }
  return reduced;
}
