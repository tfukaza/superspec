import initialize, { Authoring, prepareTextures } from "../../generated/wasm/superspec_wasm.js";
import wasmUrl from "../../generated/wasm/superspec_wasm_bg.wasm?url";
import type { AuthoringMessage, AuthoringProgress, AuthoringResult } from "./authoring.ts";
import { bakeHelpers, bakeShares } from "./bake-pool.ts";
import { libraryAsset } from "./library.ts";
import { encodeTextures, type TextureJob } from "./texture-encoder.ts";

let ready: Promise<unknown> | undefined;
let session: Authoring | undefined;

/**
 * Cooperative cancellation. While the engine computes, this worker cannot receive a message, so "stop" cannot be sent
 * to it. Each request instead comes with a blob URL that the page revokes to cancel it, and the engine's cancellation
 * poll (`globalThis.superspecCancelled`, see `Authoring::set_cancellable`) asks, at most every `PROBE_MS`, whether the
 * URL still loads: a synchronous request, which a worker may make. A cancelled build fails with a diagnostic the page
 * ignores, and the worker, with everything its session retains, is free for the next request.
 */
// The engine polls a few hundred to a few thousand times per build (about once per atlas row); a probe goes through
// the browser's blob registry, so probes are spaced: a cancelled build stops within this long, plus one row.
const PROBE_MS = 100;
let cancelToken: string | undefined;
let cancelled = false;
let lastProbe = 0;

/** Whether a blob URL still loads; revoked ones fail. */
function loads(url: string): boolean {
  try {
    const request = new XMLHttpRequest();
    request.open("GET", url, false);
    request.send();
    return request.status === 200;
  } catch { return false; }
}

/** Checked once: a live URL loads and a revoked one does not. Elsewhere requests are simply never cancelled early. */
let probing: boolean | undefined;
function canProbe(): boolean {
  if (probing !== undefined) return probing;
  try {
    const url = URL.createObjectURL(new Blob(["1"]));
    const live = loads(url);
    URL.revokeObjectURL(url);
    probing = live && !loads(url);
  } catch { probing = false; }
  return probing;
}

function requestCancelled(): boolean {
  if (cancelled || !cancelToken) return cancelled;
  const now = performance.now();
  if (now - lastProbe < PROBE_MS) return false;
  lastProbe = now;
  cancelled = !loads(cancelToken);
  return cancelled;
}
(globalThis as unknown as { superspecCancelled: () => boolean }).superspecCancelled = requestCancelled;

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
      // The helpers' turn is over: a request cancelled meanwhile does not build again.
      if (requestCancelled()) throw new Error("Authoring request cancelled.");
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
  const { requestId, assets, cancelToken: token, ...request } = message as AuthoringMessage & { cancelToken?: string };
  try {
    await (ready ??= initialize({ module_or_path: wasmUrl }));
    session ??= new Authoring();
    const active = session;
    cancelToken = token && canProbe() ? token : undefined;
    cancelled = false; lastProbe = 0;
    active.set_cancellable(cancelToken !== undefined);
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
      // Each field is moved out of the module on its own: nothing joins them into a second copy.
      const fields = Array.from({ length: reply.fields as number }, (_, index) => active.field_texels(index));
      self.postMessage({ requestId, fields: { width: reply.width, height: reply.height, fields } }, { transfer: fields.map(field => field.buffer) });
      return;
    }
    if (action === "bake-project-fields" && "meshes" in request) {
      // The CPU-only fields of a scene's hybrid materials, one receiver at a time; a mesh that fails does not stop the others.
      const encodedProject = JSON.stringify(request.project);
      for (const name of JSON.parse(active.project_library_assets(encodedProject)) as string[]) {
        images[name] = await libraryAsset(name);
        active.set_asset(name, images[name]);
      }
      const projectFields: NonNullable<AuthoringResult["projectFields"]> = [];
      const transfer: ArrayBufferLike[] = [];
      for (const { mesh, names } of request.meshes) {
        const reply = JSON.parse(active.bake_project_fields(encodedProject, requestId, mesh, JSON.stringify(names)));
        if (!reply.success) { projectFields.push({ mesh, error: (reply.diagnostics as { message: string }[]).map(finding => finding.message).join("\n") }); continue; }
        const fields = Array.from({ length: reply.fields as number }, (_, index) => active.field_texels(index));
        transfer.push(...fields.map(field => field.buffer));
        projectFields.push({ mesh, width: reply.width, height: reply.height, fields });
      }
      self.postMessage({ requestId, projectFields }, { transfer: transfer as Transferable[] });
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
    if (shading && "layouts" in request && request.layouts) {
      // The receivers' atlases as the CPU bake lays them out, for a GPU bake in UV space. A scene the layout refuses
      // (a degenerate receiver) is still shown per pixel: the reason travels with the result.
      const reply = JSON.parse(active.layout_project(encoded, requestId));
      if (reply.success) {
        const records = active.layout_records();
        self.postMessage({ requestId, metadata: result.metadata, packet: bytes.buffer, layouts: { ...reply.layouts, records } }, { transfer: [bytes.buffer, records.buffer] });
        return;
      }
      result.metadata.layoutError = (reply.diagnostics as { message: string }[]).map(finding => finding.message).join("\n");
    }
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
