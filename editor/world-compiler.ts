import { NativeInput } from "./studio/native-input.ts";
import type { Compilation } from "../browser/runtime/compiler.ts";

/** Adapts AML editor callbacks to the native packet, export and validation endpoints. */
export class NativeWorldCompiler {
  private session = new NativeInput();
  private generation = 0;
  private closed = false;
  constructor(private readonly publish: (result: Compilation) => void) {}

  compile(source: string, revision: number, assets: Record<string, Uint8Array> = {}, format: "scene" | "glb" | "glb-png" | "validate" = "scene", baseRevision?: number) {
    if (this.closed) return;
    const current = ++this.generation;
    const started = performance.now();
    const draft = { source, revision, assets, baseRevision, batchSpans: format === "scene", foliageMassRefs: format === "scene" };
    void this.session.submit(format === "scene" ? "aml-update" : "aml-" + format, draft).then(async response => {
      if (!response.ok) {
        const error = await response.json();
        throw Error(error.error || error.diagnostics?.map((d: {message: string}) => d.message).join("\n") || "Native compilation failed.");
      }
      const result = format === "validate" ? { validation: await response.json() } : { buffer: await response.arrayBuffer() };
      if (current === this.generation) this.publish({ revision, ...result, compileMs: performance.now() - started });
    }).catch(error => { if (current === this.generation) this.publish({ revision, error: String(error) }); });
  }
  cancelPending() { this.generation++; this.session.cancel(); }
  reset() { this.generation++; this.session.close(); }
  dispose() { this.reset(); this.closed = true; }
}
