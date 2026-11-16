import type { TextureRequest } from "./texture-worker.ts";
import { compiledModule } from "./wasm-module.ts";

/** One texture a staged KTX2 export lists (`stage_project` / `stage_glb`). */
export interface TextureJob { id: string; kind: "color" | "data" }
/** Called after each encoded texture, for progress reporting. */
export type TextureProgress = (done: number, total: number) => void;

/**
 * Browser Wasm runs single-threaded and KTX2 encoding dominates large exports, so a staged export's
 * textures are encoded in a pool of short-lived workers. Each worker instantiates the same compiled
 * module, so the bytes match a one-call export. The pool is small because every Basis encoder holds
 * its own working memory for 2048-pixel textures.
 */
const MAX_WORKERS = 4;
function encode(worker: Worker, request: TextureRequest): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    worker.onmessage = (event: MessageEvent<{ bytes?: Uint8Array; error?: string }>) =>
      event.data.bytes ? resolve(event.data.bytes) : reject(new Error(event.data.error ?? "Texture encoding failed."));
    worker.onerror = event => reject(new Error(event.message || "Texture worker failed."));
    worker.postMessage(request, [request.source.buffer]);
  });
}

/**
 * Encodes every listed texture, reading each source on demand with `source(index)`, and returns the
 * payloads concatenated in job order with their lengths, as `finish_project` / `finish_glb` take them.
 */
export async function encodeTextures(jobs: TextureJob[], source: (index: number) => Uint8Array,
  progress?: TextureProgress): Promise<{ encoded: Uint8Array; lengths: Uint32Array }> {
  const results: Uint8Array[] = new Array(jobs.length);
  if (jobs.length) {
    const module = await compiledModule();
    const count = Math.min(jobs.length, MAX_WORKERS, Math.max(1, (navigator.hardwareConcurrency || 2) - 1));
    const workers = Array.from({ length: count }, () => new Worker(new URL("./texture-worker.ts", import.meta.url), { type: "module" }));
    let next = 0;
    let done = 0;
    progress?.(0, jobs.length);
    try {
      await Promise.all(workers.map(async worker => {
        while (next < jobs.length) {
          const index = next++;
          results[index] = await encode(worker, { module, source: source(index), data: jobs[index].kind === "data" });
          progress?.(++done, jobs.length);
        }
      }));
    } finally {
      for (const worker of workers) worker.terminate();
    }
  }
  const lengths = Uint32Array.from(results, bytes => bytes.length);
  const encoded = new Uint8Array(lengths.reduce((sum, length) => sum + length, 0));
  let offset = 0;
  for (const bytes of results) {
    encoded.set(bytes, offset);
    offset += bytes.length;
  }
  return { encoded, lengths };
}
