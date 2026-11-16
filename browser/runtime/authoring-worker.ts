import initialize, { Authoring, prepareTextures } from "../../generated/wasm/superspec_wasm.js";
import wasmUrl from "../../generated/wasm/superspec_wasm_bg.wasm?url";
import { OCCLUSION_PLANES, type AuthoringMessage, type AuthoringProgress, type AuthoringResult, type OcclusionReport, type PlanesAnswer,
  type PlanesBaked, type PlanesWanted } from "./authoring.ts";
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
      // Every bake of the build at once: each helper bakes its share of one after another.
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

/** The planes the page holds (`PlaneStore`), as the current request was sent. */
let pageHeld: string[] = [];
/** The page's answer to a request for planes, while one is awaited. */
let answer: ((planes: Record<string, Uint8Array>) => void) | undefined;

/** The planes of `names` the page holds, copied into this worker; names the page no longer holds are left out. */
function fromPage(names: string[]): Promise<Record<string, Uint8Array>> {
  return new Promise(resolve => {
    answer = resolve;
    self.postMessage({ planesWanted: names } satisfies PlanesWanted);
  });
}

type OccludedPlane = { name: string; key: string; asset: string; width: number; height: number; rays: number; occluders: number; baked: boolean };

/**
 * Bakes the occluder service's planes that a scene project's materials read (`Authoring::occlude_project`) and returns
 * the project with them bound, as JSON, with a report. Planes are kept by content: by this session, and by the page
 * for every worker (`PlaneStore`), so only planes whose key neither holds cast rays (after a moved sun, `shadow`
 * alone); a plane the page holds is copied in, and every plane baked here goes to the page. One call per plane name,
 * so each is timed on its own. A project without scene materials, or none of whose materials reads a plane, is
 * returned as it is.
 */
async function occluded(active: Authoring, project: Record<string, unknown>, requestId: number, meshes?: string[]): Promise<{ encoded: string; report?: OcclusionReport; keys: Map<string, Record<string, string>> }> {
  const keys = new Map<string, Record<string, string>>();
  const encoded = JSON.stringify(project);
  if (project.schemaVersion !== 2 || project.kind !== "material" || !Array.isArray(project.materials) || !project.materials.length) return { encoded, keys };
  const report: OcclusionReport = { ms: {}, planes: [] };
  const bound: Record<string, Record<string, unknown>> = {};
  for (const name of OCCLUSION_PLANES) {
    const started = performance.now();
    const occlude = (held: string[]) => {
      const reply = JSON.parse(active.occlude_project(encoded, requestId, JSON.stringify({ names: [name], held, ...(meshes ? { meshes } : {}) })));
      if (!reply.success) throw new Error(JSON.stringify(reply.diagnostics));
      return reply as { receivers: { id: string; planes: OccludedPlane[] }[]; planes: Record<string, Record<string, unknown>> };
    };
    const offered = pageHeld.filter(asset => asset.startsWith(`occlusion/${name}/`));
    let reply = occlude(offered);
    // Planes keyed as held that this session lacks are the page's: copy them in. One the page let go meanwhile is
    // baked by a second call that is not offered it.
    const lacking = [...new Set(reply.receivers.flatMap(receiver => receiver.planes).filter(plane => !plane.baked && !active.holds_occlusion_plane(plane.asset)).map(plane => plane.asset))];
    if (lacking.length) {
      const copied = await fromPage(lacking);
      for (const [asset, bytes] of Object.entries(copied)) active.hold_occlusion_plane(asset, bytes);
      if (lacking.some(asset => !copied[asset])) reply = occlude(offered.filter(asset => copied[asset] || !lacking.includes(asset)));
    }
    const receivers = reply.receivers;
    if (!receivers.length) continue;
    report.ms[name] = +(performance.now() - started).toFixed(1);
    const baked: Record<string, Uint8Array> = {};
    for (const receiver of receivers) for (const plane of receiver.planes) {
      report.planes.push({ mesh: receiver.id, name: plane.name, key: plane.key, width: plane.width, height: plane.height, rays: plane.rays, occluders: plane.occluders, baked: plane.baked });
      (keys.get(receiver.id) ?? keys.set(receiver.id, {}).get(receiver.id)!)[plane.name] = plane.key;
      if (plane.baked) baked[plane.asset] = active.occlusion_plane(plane.asset);
    }
    if (Object.keys(baked).length) self.postMessage({ planesBaked: baked } satisfies PlanesBaked, { transfer: Object.values(baked).map(bytes => bytes.buffer as ArrayBuffer) });
    for (const [mesh, planes] of Object.entries(reply.planes)) bound[mesh] = { ...bound[mesh], ...planes };
  }
  if (!report.planes.length) return { encoded, keys };
  // Over what the project binds itself: the service's plane of a name is the current one.
  const stated = (project.planes ?? {}) as Record<string, Record<string, unknown>>;
  const planes = { ...stated };
  for (const [mesh, added] of Object.entries(bound)) planes[mesh] = { ...stated[mesh], ...added };
  return { encoded: JSON.stringify({ ...project, planes }), report, keys };
}

/**
 * Requests run one at a time, in order. A handler awaits (the library's images, helper bakes, texture encoding) while
 * it uses the one session, so a second request starting in the middle would replace the first one's assets and the
 * first one's cleanup would clear the second's. A kept worker (the GPU preview lane's) does receive a request while an
 * abandoned one is still running.
 */
let turn: Promise<void> = Promise.resolve();
self.onmessage = (event: MessageEvent<AuthoringMessage | PlanesAnswer>) => {
  // The page's answer to the request being handled, not a request of its own.
  if ("planesAnswer" in event.data) {
    const resolve = answer;
    answer = undefined;
    resolve?.(event.data.planesAnswer);
    return;
  }
  const message = event.data;
  turn = turn.then(() => handle(message));
};

/** Answers one request with exactly one final message; it never rejects. */
async function handle(message: AuthoringMessage): Promise<void> {
  const { requestId, assets, cancelToken: token, heldPlanes, ...request } = message as AuthoringMessage & { cancelToken?: string; heldPlanes?: string[] };
  pageHeld = heldPlanes ?? [];
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
      for (const name of JSON.parse(active.project_library_assets(JSON.stringify(request.project))) as string[]) {
        images[name] = await libraryAsset(name);
        active.set_asset(name, images[name]);
      }
      // The occluder service's planes these meshes' materials read, bound for the field bakes below.
      const occlusion = request.occlusion ? await occluded(active, request.project as unknown as Record<string, unknown>, requestId, request.meshes.map(entry => entry.mesh)) : undefined;
      const encodedProject = occlusion?.encoded ?? JSON.stringify(request.project);
      const projectFields: NonNullable<AuthoringResult["projectFields"]> = [];
      const transfer: ArrayBufferLike[] = [];
      for (const { mesh, names } of request.meshes) {
        const reply = JSON.parse(active.bake_project_fields(encodedProject, requestId, mesh, JSON.stringify(names)));
        if (!reply.success) { projectFields.push({ mesh, error: (reply.diagnostics as { message: string }[]).map(finding => finding.message).join("\n") }); continue; }
        const fields = Array.from({ length: reply.fields as number }, (_, index) => active.field_texels(index));
        transfer.push(...fields.map(field => field.buffer));
        projectFields.push({ mesh, width: reply.width, height: reply.height, fields, ...(occlusion?.keys.has(mesh) ? { planes: occlusion.keys.get(mesh) } : {}) });
      }
      self.postMessage({ requestId, projectFields, ...(occlusion?.report ? { occlusion: occlusion.report } : {}) }, { transfer: transfer as Transferable[] });
      return;
    }
    const exporting = action === "export-project";
    const shading = action === "shade-project";
    const whole = exporting || shading || action === "preview-project";
    let encoded = whole && "project" in request ? JSON.stringify(request.project) : JSON.stringify(request);
    // A shader preview compiles recipes but reads no image.
    const catalog = shading ? [] : JSON.parse(whole ? active.project_library_assets(encoded) : active.library_assets(encoded)) as string[];
    for (const name of catalog) {
      images[name] = await libraryAsset(name);
      active.set_asset(name, images[name]);
    }
    // A CPU bake of a scene (a build or an export) reads the occluder service's planes its materials declare: they are
    // baked here, lit by the project's lighting record, and bound for this build alone.
    let occlusion: OcclusionReport | undefined;
    if (whole && !shading && "project" in request) {
      const made = await occluded(active, request.project as unknown as Record<string, unknown>, requestId);
      encoded = made.encoded; occlusion = made.report;
      // Helpers that bake a share of a material read the planes as assets, like its images.
      if (occlusion && bakeHelpers() > 1) {
        for (const plane of occlusion.planes) { const asset = `occlusion/${plane.name}/${plane.key}`; images[asset] ??= active.occlusion_plane(asset); }
      }
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
      self.postMessage({ requestId, download: bytes.buffer, ...(occlusion ? { occlusion } : {}) }, { transfer: [bytes.buffer] });
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
    self.postMessage({ requestId, metadata: result.metadata, packet: bytes.buffer, ...(occlusion ? { occlusion } : {}) }, { transfer: [bytes.buffer] });
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
