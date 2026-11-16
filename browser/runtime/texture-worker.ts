import initialize, { encodeTexture, prepareTextures } from "../../generated/wasm/superspec_wasm.js";

export interface TextureRequest {
  /** The compiled SuperSpec module, shared with the staging worker instead of compiled again. */
  module: WebAssembly.Module;
  source: Uint8Array;
  data: boolean;
}

let ready: Promise<unknown> | undefined;

/** Encodes one staged PNG or JPEG texture as KTX2 through the same Rust path as native exports. */
self.onmessage = async (event: MessageEvent<TextureRequest>) => {
  try {
    await (ready ??= initialize({ module_or_path: event.data.module }).then(() => prepareTextures()));
    const bytes = encodeTexture(event.data.source, event.data.data);
    self.postMessage({ bytes }, { transfer: [bytes.buffer] });
  } catch (error) {
    self.postMessage({ error: error instanceof Error ? error.message : String(error) });
  }
};
