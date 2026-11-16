import type { BakeShareRequest } from "./bake-worker.ts";
import { compiledModule } from "./wasm-module.ts";

/**
 * Browser Wasm runs single-threaded and a material bake is thousands of evaluator nodes per texel,
 * so large bakes are split by rows across helper workers (`Authoring.distribute_bakes`). Rows are
 * the same bytes wherever they are evaluated. Helpers stay alive with the authoring worker that
 * owns them, keeping their caches between edits, and end when it is terminated.
 */
const MAX_HELPERS = 6;
let helpers: Worker[] | undefined;

function share(worker: Worker, request: BakeShareRequest): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    worker.onmessage = (event: MessageEvent<{ rows?: Uint8Array; error?: string }>) =>
      event.data.rows ? resolve(event.data.rows) : reject(new Error(event.data.error ?? "Bake helper failed."));
    worker.onerror = event => reject(new Error(event.message || "Bake helper failed."));
    worker.postMessage(request);
  });
}

/** How many helpers a bake is split across on this device. */
export function bakeHelpers(): number {
  return Math.max(1, Math.min(MAX_HELPERS, (navigator.hardwareConcurrency || 2) - 1));
}

/**
 * Evaluates every share of a pending bake (JSON from `Authoring.take_pending_bake`) and returns
 * the shares for `Authoring.provide_bake_share`. `assets` must hold the images the bake names.
 */
export async function bakeShares(pending: string, assets: Record<string, Uint8Array>): Promise<Uint8Array[]> {
  const module = await compiledModule();
  const count = bakeHelpers();
  helpers ??= Array.from({ length: count }, () => new Worker(new URL("./bake-worker.ts", import.meta.url), { type: "module" }));
  const names = (JSON.parse(pending) as { images: string[] }).images;
  const images: Record<string, Uint8Array> = {};
  for (const name of names) {
    if (!assets[name]) throw new Error(`Bake image ${name} is not loaded.`);
    images[name] = assets[name];
  }
  try {
    return await Promise.all(helpers.map((worker, index) => share(worker, { module, pending, index, count, assets: images })));
  } catch (error) {
    // A failed helper may be mid-request; start fresh ones next time.
    for (const worker of helpers) worker.terminate();
    helpers = undefined;
    throw error;
  }
}
