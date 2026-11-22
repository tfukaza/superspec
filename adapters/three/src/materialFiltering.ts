import * as THREE from "three";

/** Increase filtering on mipmapped surface maps while respecting the GPU and authored settings.
 * Apply before drawing or promoting streamed maps, which copy their sampler from the coarse map. */
export function applyMaterialAnisotropy(root: THREE.Object3D, requested: number, supported: number): void {
  const samples = Math.min(requested, supported);
  if (samples <= 1) return;
  const materials = new Set<THREE.Material>();
  root.traverse(object => {
    const material = (object as THREE.Mesh).material;
    if (Array.isArray(material)) for (const entry of material) materials.add(entry);
    else if (material) materials.add(material);
  });
  for (const material of materials) for (const value of Object.values(material)) {
    if (!(value instanceof THREE.Texture) || value.mapping !== THREE.UVMapping || value.anisotropy >= samples
      || value.magFilter !== THREE.LinearFilter || value.minFilter !== THREE.LinearMipmapLinearFilter) continue;
    value.anisotropy = samples;
    value.needsUpdate = true;
  }
}
