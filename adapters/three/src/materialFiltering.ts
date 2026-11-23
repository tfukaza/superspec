import * as THREE from "three";

function surfaceMipMaps(root: THREE.Object3D): Set<THREE.Texture> {
  const materials = new Set<THREE.Material>();
  root.traverse(object => {
    const material = (object as THREE.Mesh).material;
    if (Array.isArray(material)) for (const entry of material) materials.add(entry);
    else if (material) materials.add(material);
  });
  const maps = new Set<THREE.Texture>();
  for (const material of materials) for (const value of Object.values(material)) {
    if (value instanceof THREE.Texture && value.mapping === THREE.UVMapping
      && value.magFilter === THREE.LinearFilter && value.minFilter === THREE.LinearMipmapLinearFilter) maps.add(value);
  }
  return maps;
}

function setAnisotropy(texture: THREE.Texture, samples: number): boolean {
  if (texture.anisotropy === samples) return false;
  texture.anisotropy = samples;
  texture.needsUpdate = true;
  return true;
}

/** Increase filtering on mipmapped surface maps while respecting GPU limits and authored settings. */
export function applyMaterialAnisotropy(root: THREE.Object3D, requested: number, supported: number): void {
  const samples = Math.min(requested, supported);
  if (samples > 1) for (const texture of surfaceMipMaps(root)) setAnisotropy(texture, Math.max(samples, texture.anisotropy));
}

/** Capture authored samples before a quality override so a later preset can restore them. */
export function materialAnisotropyController(root: THREE.Object3D) {
  const defaults = Array.from(surfaceMipMaps(root), texture => ({ texture, samples: texture.anisotropy }));
  return (requested: number | undefined, supported: number) => {
    const samples = Math.min(requested ?? 1, supported);
    const updated = new Set<THREE.Texture>();
    for (const initial of defaults) if (setAnisotropy(initial.texture, Math.max(samples, initial.samples))) updated.add(initial.texture);
    return updated;
  };
}
