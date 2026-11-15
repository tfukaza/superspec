import initialize, { recipe_catalog, recipe_check, recipe_edit, recipe_inspect, recipe_sdk } from "../../generated/wasm/superspec_wasm.js";
import wasmUrl from "../../generated/wasm/superspec_wasm_bg.wasm?url";
import type { RecipeAction, RecipeRequestMessage } from "./recipe.ts";

let ready: Promise<unknown> | undefined;

/** Recipe tools are stateless, so one lightweight worker serves every call without blocking bakes. */
function invoke(action: RecipeAction, request: string): string {
  switch (action) {
    case "check": return recipe_check(request);
    case "inspect": return recipe_inspect(request);
    case "edit": return recipe_edit(request);
    case "catalog": return recipe_catalog();
    case "sdk": return recipe_sdk();
  }
}

self.onmessage = async (event: MessageEvent<RecipeRequestMessage>) => {
  const { requestId, action, request } = event.data;
  try {
    await (ready ??= initialize({ module_or_path: wasmUrl }));
    self.postMessage({ requestId, result: JSON.parse(invoke(action, request === undefined ? "" : JSON.stringify(request))) });
  } catch (error) {
    self.postMessage({ requestId, error: error instanceof Error ? error.message : String(error) });
  }
};
