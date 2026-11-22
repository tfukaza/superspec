import { NativeSession } from "../../browser/runtime/native-session.ts";
import { authoringArchive } from "./project.ts";

/** Successive edits send only the document when the admitted companion bytes still match. */
export class NativeInput extends NativeSession {
  private retained?: { signature: string; revision: string };
  private generation = 0;
  async submit(action: string, input: { assets: Record<string, Uint8Array> }) {
    const generation = ++this.generation;
    const { assets, ...document } = input;
    const hashes = [];
    for (const name of Object.keys(assets).sort()) {
      const bytes = assets[name];
      const hash = await crypto.subtle.digest("SHA-256", bytes as Uint8Array<ArrayBuffer>);
      hashes.push([name, Array.from(new Uint8Array(hash)).join(".")]);
    }
    if (generation !== this.generation) throw new DOMException("Superseded", "AbortError");
    const signature = JSON.stringify(hashes);
    if (this.retained?.signature === signature) {
      const response = await this.request(action, new Blob([JSON.stringify(document)]), "reuse-" + this.retained.revision);
      if (response.status !== 412) return response;
      if (generation !== this.generation) throw new DOMException("Superseded", "AbortError");
    }
    const response = await this.request(action, authoringArchive(input), "archive");
    const revision = response.headers.get("X-Revision");
    if (generation === this.generation && response.ok && revision) this.retained = { signature, revision };
    return response;
  }
  override cancel() { this.generation++; super.cancel(); }
  override close() { this.generation++; this.retained = undefined; super.close(); }
}
