import type * as THREE from "three";

type Mip = { data: ArrayBufferView; width: number; height: number };
const scheduled = new WeakSet<object>();

/** A transcoded KTX2 texture carries every mip level as a typed array, and
 * Three keeps those arrays for the texture's whole life: a second copy of the
 * GPU allocation on the JavaScript heap. Release them once WebGL has uploaded
 * the texture. Level sizes stay, and later sampler changes still apply because
 * the source is marked as holding no data to transfer.
 *
 * The bytes cannot be uploaded a second time. Call this only for textures that
 * stay resident until their owner disposes them, and load the scene again
 * after a lost WebGL context. Returns false for a texture without mip arrays. */
export function releaseMipsAfterUpload(texture: THREE.Texture): boolean {
  const mips = texture.mipmaps as Mip[] | undefined;
  if (texture.isRenderTargetTexture || !mips?.length || !mips.every(mip => ArrayBuffer.isView(mip?.data))) return false;
  // Clones share the level records and the uploaded source.
  if (scheduled.has(mips[0])) return true;
  scheduled.add(mips[0]);
  const previous = texture.onUpdate;
  texture.onUpdate = uploaded => {
    texture.onUpdate = previous;
    previous?.(uploaded);
    texture.source.dataReady = false;
    const image = texture.image as { data?: ArrayBufferView } | null;
    for (const mip of mips) {
      const empty = new (mip.data.constructor as new (length: number) => ArrayBufferView)(0);
      if (image?.data === mip.data) image.data = empty;
      mip.data = empty;
    }
  };
  return true;
}

/** Schedule every mip-array texture bound to a mesh material under the roots. */
export function releaseSceneMipsAfterUpload(roots: Iterable<THREE.Object3D>): number {
  const textures = new Set<THREE.Texture>();
  for (const root of roots) root.traverse(object => {
    const material = (object as THREE.Mesh).material;
    for (const surface of Array.isArray(material) ? material : material ? [material] : []) {
      for (const value of Object.values(surface)) if ((value as THREE.Texture | null)?.isTexture) textures.add(value as THREE.Texture);
    }
  });
  let count = 0;
  for (const texture of textures) if (releaseMipsAfterUpload(texture)) count++;
  return count;
}
