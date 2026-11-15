import type { AuthoringDiagnostic } from "./authoring.ts";

export interface TypeCheckRequest { requestId: number; sdk: string; modules: Record<string, string> }
export interface TypeCheckResult { requestId: number; findings: AuthoringDiagnostic[]; error?: string }

/**
 * On-demand strict TypeScript for `.material.ts` modules in a worker of its own, so it never delays
 * the Rust preview. The worker loads the TypeScript compiler on first use and reuses its program.
 */
export class TypeChecker {
  private worker?: Worker;
  private next = 0;
  private pending = new Map<number, (result: TypeCheckResult) => void>();

  check(sdk: string, modules: Record<string, string>): Promise<TypeCheckResult> {
    if (!this.worker) {
      this.worker = new Worker(new URL("./typecheck-worker.ts", import.meta.url), { type: "module" });
      this.worker.onmessage = (event: MessageEvent<TypeCheckResult>) => {
        this.pending.get(event.data.requestId)?.(event.data);
        this.pending.delete(event.data.requestId);
      };
      this.worker.onerror = event => {
        for (const resolve of this.pending.values()) resolve({ requestId: -1, findings: [], error: event.message || "Type checker failed." });
        this.pending.clear(); this.worker?.terminate(); this.worker = undefined;
      };
    }
    const requestId = ++this.next;
    return new Promise(resolve => {
      this.pending.set(requestId, resolve);
      this.worker!.postMessage({ requestId, sdk, modules } satisfies TypeCheckRequest);
    });
  }

  dispose(): void { this.worker?.terminate(); this.worker = undefined; this.pending.clear(); }
}
