import wasmUrl from "../../generated/wasm/superspec_wasm_bg.wasm?url";

let compiled: Promise<WebAssembly.Module> | undefined;

/**
 * The SuperSpec module compiled once per worker, for helper workers to instantiate. A compiled
 * module is shared when posted to another worker, so helpers neither download nor compile it again.
 */
export function compiledModule(): Promise<WebAssembly.Module> {
  return compiled ??= fetch(wasmUrl).then(response => {
    if (!response.ok) throw new Error(`Cannot load ${wasmUrl}: HTTP ${response.status}`);
    return response.arrayBuffer();
  }).then(bytes => WebAssembly.compile(bytes)).catch(error => {
    compiled = undefined;
    throw error;
  });
}
