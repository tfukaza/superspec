import { RecipeClient, type RecipeAction } from "../../browser/runtime/recipe-client.ts";

export class NativeRecipe extends RecipeClient {
  private readonly controller = new AbortController();
  protected call<T>(action: RecipeAction, request?: unknown): Promise<T> {
    return fetch("/api/recipe/" + action, { method: "POST", signal: this.controller.signal, headers: { "X-SuperSpec": "1" },
      body: JSON.stringify(request ?? {}) }).then(async response => {
        const result = await response.json();
        if (!response.ok) throw Error(result.error || "Native recipe request failed.");
        return result as T;
      });
  }
  dispose() { this.controller.abort(); }
}
