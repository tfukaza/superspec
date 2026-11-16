import initialize, { Authoring } from "../../generated/wasm/superspec_wasm.js";

export interface BakeShareRequest {
  /** The compiled SuperSpec module, shared with the authoring worker. */
  module: WebAssembly.Module;
  /** JSON from `Authoring.take_pending_bake`. */
  pending: string;
  index: number;
  count: number;
  /** The images the pending request reads. */
  assets: Record<string, Uint8Array>;
}

let ready: Promise<unknown> | undefined;
let session: Authoring | undefined;

/**
 * Evaluates one share of a material bake's rows. The session persists between requests, so the
 * compiled program, decoded images and receiver layouts of an edited material are reused.
 */
self.onmessage = async (event: MessageEvent<BakeShareRequest>) => {
  try {
    await (ready ??= initialize({ module_or_path: event.data.module }));
    session ??= new Authoring();
    session.clear_assets();
    for (const [name, bytes] of Object.entries(event.data.assets)) session.set_asset(name, bytes);
    const rows = session.bake_share(event.data.pending, event.data.index, event.data.count);
    self.postMessage({ rows }, { transfer: [rows.buffer] });
  } catch (error) {
    self.postMessage({ error: error instanceof Error ? error.message : String(error) });
  } finally {
    session?.clear_assets();
  }
};
