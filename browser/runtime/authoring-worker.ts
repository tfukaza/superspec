import initialize, { Authoring, prepareTextures } from "../../generated/wasm/superspec_wasm.js";
import wasmUrl from "../../generated/wasm/superspec_wasm_bg.wasm?url";
import type { AuthoringMessage } from "./authoring.ts";
import { libraryAsset } from "./library.ts";

let ready: Promise<unknown> | undefined;
let session: Authoring | undefined;

self.onmessage = async (event: MessageEvent<AuthoringMessage>) => {
  const { requestId, assets, ...request } = event.data;
  try {
    await (ready ??= initialize({ module_or_path: wasmUrl }));
    session ??= new Authoring();
    session.clear_assets();
    // GLBs may carry KTX2 textures, which admission validates with the texture codec module.
    if (Object.keys(assets).some(name => /\.(glb|ktx2)$/i.test(name))) await prepareTextures();
    for (const [name, bytes] of Object.entries(assets)) session.set_asset(name, bytes);
    const action = "action" in request ? request.action : undefined;
    const exporting = action === "export-project";
    const whole = exporting || action === "preview-project";
    const encoded = whole && "project" in request ? JSON.stringify(request.project) : JSON.stringify(request);
    const catalog = JSON.parse(whole ? session.project_library_assets(encoded) : session.library_assets(encoded)) as string[];
    for (const name of catalog) session.set_asset(name, await libraryAsset(name));
    if (exporting) {
      // The KTX2 codec is a separate module, loaded on the first KTX2 export only.
      const ktx2 = !("textures" in request) || request.textures !== "png";
      if (ktx2) await prepareTextures();
      const bytes = session.export_project(encoded, requestId, ktx2);
      self.postMessage({ requestId, download: bytes.buffer }, { transfer: [bytes.buffer] });
      return;
    }
    const result = JSON.parse(whole ? session.preview_project(encoded, requestId) : session.build(encoded, requestId));
    if (!result.success) {
      self.postMessage({ requestId, diagnostics: result.diagnostics });
      return;
    }
    const bytes = session.packet();
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
