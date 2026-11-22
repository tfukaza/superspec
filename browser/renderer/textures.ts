import * as THREE from "three";
import { loadKtx2 } from "./ktx2.ts";

/**
 * Anisotropic filtering of material maps in the viewer, for a build's textures and a GPU preview's alike, so a preview
 * does not change sharpness when the build replaces it.
 */
export const MAP_ANISOTROPY = 1;

interface Entry {
  texture: Promise<THREE.Texture>;
  users: number;
}

/**
 * Disposes a texture for good. three.js uploads a disposed texture again, into a new GPU texture that nothing frees,
 * whenever a program is still handed it, and its renderer does hand stale textures on: the depth material its shadow
 * pass shares between meshes keeps the last `map` it was given in the uniform when the next mesh has none
 * (`WebGLShadowMap`'s `getDepthMaterial`; `refreshUniformsCommon` sets the uniform only for a map), so the next shadow
 * map drawn after a textured scene was replaced uploaded that scene's last image once more, for good. A texture at
 * version 0 is never uploaded (`WebGLTextures.setTexture2D`): a stale reference then binds nothing.
 */
function retire(texture: THREE.Texture): void {
  texture.dispose();
  texture.version = 0;
}

/** Shares immutable decoded images across active scenes and unpublished replacements. */
export class TexturePool {
  private readonly entries = new Map<string, Entry>();
  private closed = false;
  private anisotropy = MAP_ANISOTROPY;

  constructor(private readonly renderer: THREE.WebGLRenderer) {}

  /** Runtime filtering of decoded maps, independent of the authored texture or bake resolution. */
  setAnisotropy(requested = MAP_ANISOTROPY): void {
    const samples = Math.max(MAP_ANISOTROPY, Math.min(requested, this.renderer.capabilities.getMaxAnisotropy()));
    if (samples === this.anisotropy) return;
    this.anisotropy = samples;
    for (const [key, entry] of this.entries) {
      void entry.texture.then(texture => {
        if (this.entries.get(key) === entry && this.filter(texture)) texture.needsUpdate = true;
      }, () => {});
    }
  }

  private filter(texture: THREE.Texture): boolean {
    const samples = texture.magFilter === THREE.LinearFilter && texture.minFilter === THREE.LinearMipmapLinearFilter
      ? this.anisotropy : MAP_ANISOTROPY;
    if (texture.anisotropy === samples) return false;
    texture.anisotropy = samples;
    return true;
  }

  /** Retains one reference per scene lease, including while an image is still loading. */
  acquire(key: string, bytes: Uint8Array<ArrayBuffer>, mime: string, color: boolean, leases: Set<string>): Promise<THREE.Texture> {
    if (this.closed) return Promise.reject(new Error("Texture pool is closed."));
    let entry = this.entries.get(key);
    if (!entry) {
      const configure = (value: THREE.Texture): THREE.Texture => {
        value.colorSpace = color ? THREE.SRGBColorSpace : THREE.NoColorSpace;
        value.wrapS = value.wrapT = THREE.RepeatWrapping;
        this.filter(value);
        return value;
      };
      let texture: Promise<THREE.Texture>;
      if (mime === "image/ktx2") {
        // Compressed textures upload rows top to bottom and cannot be flipped on upload, unlike
        // PNG and JPEG (flipY); flipping V in the texture transform samples the same texels.
        texture = loadKtx2(this.renderer, bytes).then(value => {
          configure(value);
          value.repeat.set(1, -1);
          value.offset.set(0, 1);
          return value;
        });
      } else {
        // Decoded off the page's thread: an image element is decoded again inside its upload, on the page's thread
        // (WebGL wants the pixels unpremultiplied and unconverted, which is not the decode the element keeps), and
        // that grows with the image. The bitmap is the same pixels, already in the row order of the upload (what
        // `flipY` asks of an image element), so the texture itself must not flip.
        texture = createImageBitmap(new Blob([bytes], { type: mime }), { imageOrientation: "flipY", premultiplyAlpha: "none", colorSpaceConversion: "none" }).then(bitmap => {
          const value = new THREE.Texture(bitmap);
          value.flipY = false;
          value.needsUpdate = true;
          value.addEventListener("dispose", () => bitmap.close());
          return configure(value);
        });
      }
      entry = { texture, users: 0 };
      this.entries.set(key, entry);
      const created = entry;
      void texture.catch(() => {
        if (this.entries.get(key) === created) this.entries.delete(key);
      });
    }
    if (!leases.has(key)) {
      leases.add(key);
      entry.users++;
    }
    return entry.texture;
  }

  /** Drops this scene's references, releasing textures after their final consumer leaves. */
  release(leases: Set<string>): void {
    for (const key of leases) {
      const entry = this.entries.get(key);
      if (!entry || --entry.users > 0) continue;
      this.entries.delete(key);
      void entry.texture.then(retire, () => {});
    }
    leases.clear();
  }

  /** Cancels ownership of all pending and decoded images when the viewer closes. */
  dispose(): void {
    this.closed = true;
    for (const entry of this.entries.values()) {
      void entry.texture.then(retire, () => {});
    }
    this.entries.clear();
  }
}

/** Cryptographic content identity lets renamed material assets retain their decoded image. */
export async function imageIdentity(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(digest, value => value.toString(16).padStart(2, "0")).join("");
}
