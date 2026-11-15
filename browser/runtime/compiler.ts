import type { ValidationReport } from "../model/validation.ts";
export interface Compilation { revision: number; buffer?: ArrayBuffer; update?: boolean; compileMs?: number; workers?: number; error?: string; validation?: ValidationReport }

/** Owns one worker; replacing busy workers cancels synchronous Wasm without false cooperative polling. */
export class BrowserCompiler {
  private worker?: Worker;
  private active?: number;
  private disposed = false;

  constructor(private readonly receive: (result: Compilation) => void) {}

  /** Supersedes an outstanding build immediately; idle workers retain their initialized module. */
  compile(source: string, revision: number, assets: Record<string, Uint8Array> = {}, format: "scene" | "glb" | "glb-png" | "validate" = "scene"): void {
    if (this.disposed) return;
    if (this.active !== undefined) this.reset();
    if (!this.worker) {
      const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
      this.worker = worker;
      worker.onmessage = (event: MessageEvent<Compilation>) => {
        if (this.worker !== worker || this.active !== event.data.revision) return;
        this.active = undefined;
        if (event.data.error) this.reset();
        this.receive(event.data);
      };
      worker.onerror = event => {
        if (this.worker !== worker || this.active === undefined) return;
        const revision = this.active;
        this.reset();
        this.receive({ revision, error: event.message || "Compiler worker failed." });
      };
    }
    this.active = revision;
    this.worker.postMessage({ source, revision, assets, format: format === "scene" ? "update" : format });
  }

  /** Cancels in-flight work and releases its entire Wasm memory; the next compile starts fresh. */
  reset(): void {
    this.worker?.terminate();
    this.worker = undefined;
    this.active = undefined;
  }

  /** Cancels only outstanding work so ordinary edits can reuse an initialized idle worker. */
  cancelPending(): void { if (this.active !== undefined) this.reset(); }

  /** Stops all worker activity when its editor is closed. */
  dispose(): void { this.reset(); this.disposed = true; }
}
