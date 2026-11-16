import initialize, { Authoring, prepareTextures } from "../../generated/wasm/superspec_wasm.js";
import wasmUrl from "../../generated/wasm/superspec_wasm_bg.wasm?url";
import type { AuthoringMessage, AuthoringProgress } from "./authoring.ts";
import { bakeHelpers, bakeShares } from "./bake-pool.ts";
import { libraryAsset } from "./library.ts";
import { encodeTextures, type TextureJob } from "./texture-encoder.ts";

let ready: Promise<unknown> | undefined;
let session: Authoring | undefined;

/**
 * Runs one engine call, sharing large material bakes with helper workers. The engine stops a
 * build at an uncached large bake and describes it; the helpers' rows are handed back and the
 * same call runs again, once per material that needs baking. If helpers fail, the bake runs here,
 * which also reports any error in the material itself.
 */
async function distributed<T>(active: Authoring, assets: Record<string, Uint8Array>, run: () => T): Promise<T> {
  active.distribute_bakes(bakeHelpers() > 1);
  let previous = "";
  try {
    for (;;) {
      let result: T | undefined;
      let failure: { error: unknown } | undefined;
      try { result = run(); } catch (error) { failure = { error }; }
      const pending = active.take_pending_bake();
      if (!pending) {
        if (failure) throw failure.error;
        return result as T;
      }
      // The same bake reported twice means its rows were not accepted: bake it here instead.
      if (pending === previous) {
        active.distribute_bakes(false);
        continue;
      }
      previous = pending;
      try {
        for (const rows of await bakeShares(pending, assets)) active.provide_bake_share(rows);
      } catch {
        active.distribute_bakes(false);
      }
    }
  } finally {
    active.distribute_bakes(false);
  }
}

self.onmessage = async (event: MessageEvent<AuthoringMessage>) => {
  const { requestId, assets, ...request } = event.data;
  try {
    await (ready ??= initialize({ module_or_path: wasmUrl }));
    session ??= new Authoring();
    const active = session;
    active.clear_assets();
    // GLBs may carry KTX2 textures, which admission validates with the texture codec module.
    if (Object.keys(assets).some(name => /\.(glb|ktx2)$/i.test(name))) await prepareTextures();
    // Every image this request can read, for helpers that bake a share of it.
    const images: Record<string, Uint8Array> = { ...assets };
    for (const [name, bytes] of Object.entries(assets)) active.set_asset(name, bytes);
    const action = "action" in request ? request.action : undefined;
    const exporting = action === "export-project";
    const whole = exporting || action === "preview-project";
    const encoded = whole && "project" in request ? JSON.stringify(request.project) : JSON.stringify(request);
    const catalog = JSON.parse(whole ? active.project_library_assets(encoded) : active.library_assets(encoded)) as string[];
    for (const name of catalog) {
      images[name] = await libraryAsset(name);
      active.set_asset(name, images[name]);
    }
    if (exporting) {
      const ktx2 = !("textures" in request) || request.textures !== "png";
      // KTX2 exports bake here, then encode their textures in parallel workers (texture-encoder.ts).
      let bytes: Uint8Array;
      if (ktx2) {
        const jobs = JSON.parse(await distributed(active, images, () => active.stage_project(encoded, requestId))) as TextureJob[];
        const textures = await encodeTextures(jobs, index => active.staged_texture(index),
          (done, total) => self.postMessage({ requestId, progress: { encoded: done, textures: total } } satisfies AuthoringProgress));
        bytes = active.finish_project(textures.encoded, textures.lengths);
      } else {
        bytes = await distributed(active, images, () => active.export_project(encoded, requestId, false));
      }
      self.postMessage({ requestId, download: bytes.buffer }, { transfer: [bytes.buffer] });
      return;
    }
    const result = JSON.parse(await distributed(active, images, () => whole ? active.preview_project(encoded, requestId) : active.build(encoded, requestId)));
    if (!result.success) {
      self.postMessage({ requestId, diagnostics: result.diagnostics });
      return;
    }
    const bytes = active.packet();
    self.postMessage({ requestId, metadata: result.metadata, packet: bytes.buffer }, { transfer: [bytes.buffer] });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    let diagnostics: unknown;
    try { diagnostics = JSON.parse(message); } catch { /* Host errors need no source location. */ }
    if (Array.isArray(diagnostics) && diagnostics.every(value => value && typeof value.message === "string" && Number.isInteger(value.offset))) {
      self.postMessage({ requestId, diagnostics });
    } else {
      self.postMessage({ requestId, error: message });
    }
  } finally {
    session?.clear_assets();
  }
};
