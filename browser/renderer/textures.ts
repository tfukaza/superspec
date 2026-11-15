import * as THREE from "three";
import { loadKtx2 } from "./ktx2.ts";

interface Entry {
  texture: Promise<THREE.Texture>;
  users: number;
}

/** Shares immutable decoded images across active scenes and unpublished replacements. */
export class TexturePool {
  private readonly entries = new Map<string, Entry>();
  private closed = false;

  constructor(private readonly renderer: THREE.WebGLRenderer) {}

  /** Retains one reference per scene lease, including while an image is still loading. */
  acquire(key: string, bytes: Uint8Array<ArrayBuffer>, mime: string, color: boolean, leases: Set<string>): Promise<THREE.Texture> {
    if (this.closed) return Promise.reject(new Error("Texture pool is closed."));
    let entry = this.entries.get(key);
    if (!entry) {
      const configure = (value: THREE.Texture): THREE.Texture => {
        value.colorSpace = color ? THREE.SRGBColorSpace : THREE.NoColorSpace;
        value.wrapS = value.wrapT = THREE.RepeatWrapping;
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
        const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
        texture = new THREE.TextureLoader().loadAsync(url).then(configure).finally(() => URL.revokeObjectURL(url));
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
      void entry.texture.then(texture => texture.dispose(), () => {});
    }
    leases.clear();
  }

  /** Cancels ownership of all pending and decoded images when the viewer closes. */
  dispose(): void {
    this.closed = true;
    for (const entry of this.entries.values()) {
      void entry.texture.then(texture => texture.dispose(), () => {});
    }
    this.entries.clear();
  }
}

/** Cryptographic content identity lets renamed material assets retain their decoded image. */
export async function imageIdentity(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(digest, value => value.toString(16).padStart(2, "0")).join("");
}
