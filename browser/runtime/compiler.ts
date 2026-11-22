import type { ValidationReport } from "../model/validation.ts";
import type { ExportProgress } from "./authoring.ts";
export interface Compilation { revision: number; buffer?: ArrayBuffer; compileMs?: number; workers?: number; error?: string; validation?: ValidationReport }

/** Owns one worker; replacing busy workers cancels synchronous Wasm without false cooperative polling. */
export class BrowserCompiler {
  private worker?: Worker;
  private active?: number;
  private disposed = false;

  constructor(private readonly receive: (result: Compilation) => void,
    private readonly progress?: (revision: number, progress: ExportProgress) => void) {}

  /** Supersedes an outstanding build immediately; idle workers retain their initialized module. */
  compile(source: string, revision: number, assets: Record<string, Uint8Array> = {}, format: "scene" | "glb" | "glb-png" | "validate" = "scene", baseRevision?: number): void {
    if (this.disposed) return;
    if (this.active !== undefined) this.reset();
    if (!this.worker) {
      let worker: Worker;
      try {
        worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
      } catch (error) {
        this.receive({ revision, error: String(error) });
        return;
      }
      this.worker = worker;
      worker.onmessage = (event: MessageEvent<Compilation | { revision: number; progress: ExportProgress }>) => {
        if (this.worker !== worker || this.active !== event.data.revision) return;
        if ("progress" in event.data) {
          this.progress?.(event.data.revision, event.data.progress);
          return;
        }
        this.active = undefined;
        if (event.data.error) this.reset();
        this.receive(event.data);
      };
      const failed = (message: string) => {
        if (this.worker !== worker || this.active === undefined) return;
        const revision = this.active;
        this.reset();
        this.receive({ revision, error: message });
      };
      worker.onerror = event => failed(event.message || "Compiler worker failed.");
      worker.onmessageerror = () => failed("Could not read the compiler worker response.");
    }
    this.active = revision;
    try {
      this.worker.postMessage({ source, revision, assets, baseRevision, format: format === "scene" ? "update" : format });
    } catch (error) {
      this.reset();
      this.receive({ revision, error: String(error) });
    }
  }

  /** Cancels in-flight work and releases its entire Wasm memory; the next compile starts fresh. */
  reset(): void {
    this.worker?.terminate();
    this.worker = undefined;
    this.active = undefined;
  }

  /** Cancels only outstanding work so ordinary edits can reuse an initialized idle worker. */
  cancelPending(): void { if (this.active !== undefined) this.reset(); }

  dispose(): void { this.reset(); this.disposed = true; }
}
