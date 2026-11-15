import type { ScenePacket } from "../../browser/model/packet.ts";

/** Displays the actual Rust-generated image channels and owns their browser object URLs. */
export class Channels {
  private urls: string[] = [];
  constructor(private readonly host: HTMLElement) {}

  /** Replaces thumbnails only after a successful material publication. */
  show(packet: ScenePacket): void {
    this.clear();
    const material = packet.manifest.materials.find(material => material.texture !== null);
    if (!material) return;
    for (const [name, path] of [["Base color", material.texture], ["Normal", material.normalTexture], ["Roughness / metalness", material.metallicRoughnessTexture]] as const) {
      const asset = packet.manifest.assets.find(asset => asset.id === path);
      if (!asset) continue;
      const url = URL.createObjectURL(new Blob([packet.assets.slice(asset.byteStart, asset.byteStart + asset.byteLength)], { type: asset.mime }));
      this.urls.push(url);
      const figure = document.createElement("figure");
      const image = document.createElement("img"); image.src = url; image.alt = name;
      const caption = document.createElement("figcaption"); caption.textContent = name;
      figure.append(image, caption); this.host.append(figure);
    }
  }

  /** Frees image URLs when changing a material or leaving the studio. */
  clear(): void {
    for (const url of this.urls) URL.revokeObjectURL(url);
    this.urls = []; this.host.replaceChildren();
  }
}
