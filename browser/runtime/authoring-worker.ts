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
 * build at its uncached large bakes and describes them; the helpers' rows are handed back and the
 * same call runs again, normally once. If helpers fail, the bakes run here, which also reports
 * any error in a material itself.
 */
async function distributed<T>(active: Authoring, assets: Record<string, Uint8Array>, run: () => T): Promise<T> {
  active.distribute_bakes(bakeHelpers() > 1);
  const reported = new Set<string>();
  try {
    for (;;) {
      let result: T | undefined;
      let failure: { error: unknown } | undefined;
      try { result = run(); } catch (error) { failure = { error }; }
      // A project reports every large bake of one build; take them all before building again.
      const pending: string[] = [];
      for (let next = active.take_pending_bake(); next; next = active.take_pending_bake()) pending.push(next);
      if (!pending.length) {
        if (failure) throw failure.error;
        return result as T;
      }
      // A bake reported twice means its result was not kept (its rows were refused, or the
      // session's cache is too small to hold the scene between runs): bake the rest here.
      if (pending.some(bake => reported.has(bake))) {
        active.distribute_bakes(false);
        continue;
      }
      for (const bake of pending) reported.add(bake);
      try {
        for (const bake of pending) for (const rows of await bakeShares(bake, assets)) active.provide_bake_share(rows);
      } catch {
        active.distribute_bakes(false);
      }
    }
  } finally {
    active.distribute_bakes(false);
  }
}

/**
 * Requests run one at a time, in order. A handler awaits (the library's images, helper bakes, texture encoding) while
 * it uses the one session, so a second request starting in the middle would replace the first one's assets and the
 * first one's cleanup would clear the second's. A kept worker (the GPU preview lane's) does receive a request while an
 * abandoned one is still running.
 */
let turn: Promise<void> = Promise.resolve();
self.onmessage = (event: MessageEvent<AuthoringMessage>) => { turn = turn.then(() => handle(event.data)); };

/** Answers one request with exactly one final message; it never rejects. */
async function handle(message: AuthoringMessage): Promise<void> {
  const { requestId, assets, ...request } = message;
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
    if (action === "bake-fields" && "request" in request) {
      // A field bake of the CPU-only parts of a program, for a GPU preview: no build, nothing accepted.
      const encodedRequest = JSON.stringify(request.request);
      for (const name of JSON.parse(active.library_assets(encodedRequest)) as string[]) {
        images[name] = await libraryAsset(name);
        active.set_asset(name, images[name]);
      }
      const reply = JSON.parse(active.bake_fields(encodedRequest, requestId, JSON.stringify(request.names)));
      if (!reply.success) { self.postMessage({ requestId, diagnostics: reply.diagnostics }); return; }
      const texels = active.field_texels();
      self.postMessage({ requestId, fields: { width: reply.width, height: reply.height, texels } }, { transfer: [texels.buffer] });
      return;
    }
    const exporting = action === "export-project";
    const shading = action === "shade-project";
    const whole = exporting || shading || action === "preview-project";
    const encoded = whole && "project" in request ? JSON.stringify(request.project) : JSON.stringify(request);
    // A shader preview compiles recipes but reads no image.
    const catalog = shading ? [] : JSON.parse(whole ? active.project_library_assets(encoded) : active.library_assets(encoded)) as string[];
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
    const result = JSON.parse(shading ? active.shade_project(encoded, requestId)
      : await distributed(active, images, () => whole ? active.preview_project(encoded, requestId) : active.build(encoded, requestId)));
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
}
