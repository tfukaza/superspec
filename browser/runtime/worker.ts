import initialize, { Compiler, prepareTextures } from "../../generated/wasm/superspec_wasm.js";
import wasmUrl from "../../generated/wasm/superspec_wasm_bg.wasm?url";
import { libraryAsset } from "./library.ts";
import { encodeTextures, type TextureJob } from "./texture-encoder.ts";

let ready: Promise<void> | undefined;
let compiler: Compiler | undefined;
let CompilerType = Compiler;
let prepare = prepareTextures;
let workers = 1;
interface ThreadedModule {
  default: typeof initialize;
  Compiler: typeof Compiler;
  prepareTextures: typeof prepareTextures;
  initThreadPool: (workers: number) => Promise<void>;
}
const threadedModules = import.meta.glob<ThreadedModule>("../../generated/wasm-threaded/superspec_wasm.js");
const threadedBinaries = import.meta.glob<string>("../../generated/wasm-threaded/superspec_wasm_bg.wasm", { query: "?url", import: "default", eager: true });

/** Selects a shared-memory build only on an isolated page with an available threaded artifact. */
async function initializeRuntime(): Promise<void> {
  const load = Object.values(threadedModules)[0];
  const binary = Object.values(threadedBinaries)[0];
  if (crossOriginIsolated && typeof SharedArrayBuffer !== "undefined" && load && binary) {
    const module = await load();
    await module.default({ module_or_path: binary });
    workers = Math.max(1, Math.min(4, navigator.hardwareConcurrency || 1));
    await module.initThreadPool(workers);
    CompilerType = module.Compiler;
    prepare = module.prepareTextures;
  } else {
    await initialize({ module_or_path: wasmUrl });
  }
}
self.onmessage = async (event: MessageEvent<{ source: string; revision: number; baseRevision?: number; assets?: Record<string, Uint8Array>; format?: "scene" | "update" | "glb" | "glb-png" | "validate" }>) => {
  const { source, revision } = event.data;
  try {
    await (ready ??= initializeRuntime());
    const start = performance.now();
    // Each request owns a complete asset snapshot; replacements never count old and new entries together.
    compiler ??= new CompilerType();
    compiler.clear_assets();
    // GLBs may carry KTX2 textures, which admission validates with the texture codec module.
    if (Object.keys(event.data.assets ?? {}).some(name => /\.(glb|ktx2)$/i.test(name))) await prepare();
    for (const [name, bytes] of Object.entries(event.data.assets ?? {})) compiler.set_asset(name, bytes);
    const names = event.data.format === "validate" ? [] : JSON.parse(compiler.library_assets(source)) as string[];
    const loaded = await Promise.all(names.map(libraryAsset));
    for (let i = 0; i < names.length; i++) compiler.set_asset(names[i], loaded[i]);
    if (event.data.format === "validate") {
      self.postMessage({ revision, validation: JSON.parse(compiler.validate(source, revision)), compileMs: performance.now() - start, workers });
      return;
    }
    const update = event.data.format === "update";
    const glb = event.data.format === "glb" || event.data.format === "glb-png";
    let bytes: Uint8Array;
    if (event.data.format === "glb") {
      const staged = compiler;
      const jobs = JSON.parse(staged.stage_glb(source)) as TextureJob[];
      const textures = await encodeTextures(jobs, index => staged.staged_texture(index),
        (done, total) => self.postMessage({ revision, progress: { encoded: done, textures: total } }));
      bytes = staged.finish_glb(textures.encoded, textures.lengths);
    } else if (update) {
      bytes = compiler.preview_packet(source, revision, event.data.baseRevision, true, true);
    } else {
      bytes = glb ? compiler.export_glb(source, false, false) : compiler.compile(source, revision);
    }
    const compileMs = performance.now() - start;
    // wasm-bindgen returns an owned copy, not a view of mutable Wasm linear memory.
    self.postMessage({ revision, buffer: bytes.buffer, compileMs, workers, cacheStats: JSON.parse(compiler.cache_stats()), bakeStats: JSON.parse(compiler.bake_stats()) }, { transfer: [bytes.buffer] });
  } catch (error) {
    self.postMessage({ revision, error: error instanceof Error ? error.message : String(error) });
  } finally {
    compiler?.clear_assets();
  }
};
