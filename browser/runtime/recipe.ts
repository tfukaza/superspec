export * from "./recipe-client.ts";
import { RecipeClient, type RecipeAction, type RecipeRequestMessage } from "./recipe-client.ts";

/** Calls the shared Rust recipe tools in a dedicated worker, independent of running bakes. */
export class RecipeTools extends RecipeClient {
  private worker?: Worker;
  private next = 1;
  private readonly pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();

  private start(): Worker {
    if (this.worker) return this.worker;
    const worker = new Worker(new URL("./recipe-worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (event: MessageEvent<{ requestId: number; result?: unknown; error?: string }>) => {
      const waiting = this.pending.get(event.data.requestId);
      if (!waiting) return;
      this.pending.delete(event.data.requestId);
      if (event.data.error !== undefined) waiting.reject(new Error(event.data.error));
      else waiting.resolve(event.data.result);
    };
    worker.onerror = event => {
      const error = new Error(event.message || "Recipe worker failed.");
      for (const waiting of this.pending.values()) waiting.reject(error);
      this.pending.clear(); worker.terminate(); this.worker = undefined;
    };
    return this.worker = worker;
  }

  protected call<T>(action: RecipeAction, request?: unknown): Promise<T> {
    const requestId = this.next++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(requestId, { resolve: resolve as (value: unknown) => void, reject });
      this.start().postMessage({ requestId, action, request } satisfies RecipeRequestMessage);
    });
  }

  dispose(): void {
    for (const waiting of this.pending.values()) waiting.reject(new DOMException("Recipe tools closed.", "AbortError"));
    this.pending.clear(); this.worker?.terminate(); this.worker = undefined;
  }
}
