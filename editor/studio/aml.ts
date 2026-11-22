import { worldCompiler } from "../compiler-host.ts";
import type { BrowserCompiler } from "../../browser/runtime/compiler.ts";
import type { ScenePacket } from "../../browser/model/packet.ts";
import { SceneUpdates } from "../../browser/model/updates.ts";

/** Uses the ordinary AML worker and transport admission before a studio changes its accepted model. */
export class AmlLoader {
  private compiler?: Pick<BrowserCompiler, keyof BrowserCompiler>;
  private reject?: (error: Error) => void;

  /** Each open starts an independent source session, preserving the existing scene until acceptance. */
  load(source: string, assets: Record<string, Uint8Array>, revision: number): Promise<ScenePacket> {
    this.cancel();
    return new Promise((resolve, reject) => {
      this.reject = reject;
      const updates = new SceneUpdates();
      const compiler = worldCompiler(result => {
        try {
          if (result.error || !result.buffer) throw new Error(result.error || "No AML scene returned.");
          const packet = updates.decodePreview(result.buffer);
          if (!packet.manifest.success) throw new Error(packet.manifest.diagnostics.map(finding => `${finding.source.line}:${finding.source.column} ${finding.message}`).join("\n"));
          resolve(packet);
        } catch (error) { reject(error); }
        finally { compiler.dispose(); this.compiler = undefined; this.reject = undefined; }
      });
      this.compiler = compiler;
      compiler.compile(source, revision, assets);
    });
  }

  /** Invalidates pending file compilation without touching the viewport or caller-owned input bytes. */
  cancel(): void {
    this.compiler?.dispose(); this.compiler = undefined;
    this.reject?.(new DOMException("AML open superseded.", "AbortError")); this.reject = undefined;
  }
}
