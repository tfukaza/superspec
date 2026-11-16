import type { BakeShareRequest } from "./bake-worker.ts";
import { compiledModule } from "./wasm-module.ts";

/**
 * Browser Wasm runs single-threaded and a material bake is thousands of evaluator nodes per texel,
 * so large bakes are split by rows across helper workers (`Authoring.distribute_bakes`). Rows are
 * the same bytes wherever they are evaluated. Helpers stay alive with the authoring worker that
 * owns them, keeping their caches between edits, and end when it is terminated. A helper always
 * bakes the same share of a bake, so what it keeps for its rows (a layered slot's planes) serves
 * its next share of an edit.
 */
const MAX_HELPERS = 6;
let helpers: Worker[] | undefined;

function share(worker: Worker, request: BakeShareRequest): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    worker.onmessage = (event: MessageEvent<{ rows?: Uint8Array; error?: string }>) =>
      event.data.rows ? resolve(event.data.rows) : reject(new Error(event.data.error ?? "Bake helper failed."));
    worker.onerror = event => reject(new Error(event.message || "Bake helper failed."));
    // A reply the worker cannot read would otherwise leave the bake waiting for it.
    worker.onmessageerror = () => reject(new Error("A bake helper's reply could not be read."));
    worker.postMessage(request);
  });
}

/** How many helpers a bake is split across on this device. */
export function bakeHelpers(): number {
  return Math.max(1, Math.min(MAX_HELPERS, (navigator.hardwareConcurrency || 2) - 1));
}

/**
 * Evaluates every share of the pending bakes of one build (JSON from `Authoring.take_pending_bake`)
 * and returns the shares for `Authoring.provide_bake_share`. Each helper bakes its share of every
 * bake in turn, so a helper that finishes one bake early goes on to the next instead of waiting for
 * the slowest share of each. `assets` must hold the images and planes the bakes name.
 */
export async function bakeShares(pending: string[], assets: Record<string, Uint8Array>): Promise<Uint8Array[]> {
  const module = await compiledModule();
  const count = bakeHelpers();
  helpers ??= Array.from({ length: count }, () => new Worker(new URL("./bake-worker.ts", import.meta.url), { type: "module" }));
  const bakes = pending.map(bake => {
    // The images the bake reads, and the planes bound to its external inputs (the occluder service's, read as assets).
    const described = JSON.parse(bake) as { images: string[]; planes?: string[] };
    const images: Record<string, Uint8Array> = {};
    for (const name of [...described.images, ...(described.planes ?? [])]) {
      if (!assets[name]) throw new Error(`Bake input ${name} is not loaded.`);
      images[name] = assets[name];
    }
    return { pending: bake, images };
  });
  const workers = helpers;
  // Once one helper fails the others stop at their next bake: their workers are about to end.
  let failed = false;
  try {
    const done = await Promise.all(workers.map(async (worker, index) => {
      const rows: Uint8Array[] = [];
      try {
        for (const bake of bakes) {
          if (failed) break;
          rows.push(await share(worker, { module, pending: bake.pending, index, count, assets: bake.images }));
        }
      } catch (error) { failed = true; throw error; }
      return rows;
    }));
    return done.flat();
  } catch (error) {
    // A failed helper may be mid-request; start fresh ones next time.
    for (const worker of workers) worker.terminate();
    if (helpers === workers) helpers = undefined;
    throw error;
  }
}
