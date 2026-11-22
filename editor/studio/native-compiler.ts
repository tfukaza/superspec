import { decodeNativeReport, NativeShadingBlocks } from "../../browser/runtime/native-report.ts";
import { NativeInput } from "./native-input.ts";
import type { AuthoringProject, AuthoringRequest, AuthoringResult, ExportTextures } from "../../browser/runtime/authoring.ts";

/** Native authoring, receiver fields, shader descriptions and export. */
export class NativeCompiler {
  private readonly native = new NativeInput();
  private blocks?: NativeShadingBlocks;
  private generation = 0;
  compile(input: AuthoringRequest) { return this.run("editor-build", input, input.requestId); }
  previewProject(input: AuthoringProject, id: number) { return this.run("editor-project", input, id); }
  exportProject(input: AuthoringProject, id: number, textures: ExportTextures = "ktx2") {
    return this.run(textures === "png" ? "glb-png" : "glb", input, id);
  }
  shadeProject(input: AuthoringProject, id: number, layouts = false, includeScenePacket = true, latticeSurfaceMapsOnly = false) {
    const blocks = layouts && !includeScenePacket && latticeSurfaceMapsOnly ? this.blocks ?? new NativeShadingBlocks() : undefined;
    const draft = { ...input, includeScenePacket, latticeSurfaceMapsOnly, heldShadingBlocks: blocks?.keys(),
      packedFeatureCoords: !!blocks, chartRecordDeltas: !!blocks };
    return this.run(layouts ? "shade-layouts" : "shade", draft, id, blocks);
  }
  bakeFields(input: Omit<AuthoringRequest, "requestId">, names: string[], id: number) {
    const draft = { ...input, names };
    return this.run("fields", draft, id);
  }
  bakeProjectFields(input: AuthoringProject, meshes: { mesh: string; names: string[] }[], id: number, occlusion = false) {
    const draft = { ...input, fieldMeshes: meshes, withOcclusion: occlusion };
    return this.run("project-fields", draft, id);
  }
  private async run(action: string, input: { assets: Record<string, Uint8Array> }, requestId: number, blocks?: NativeShadingBlocks): Promise<AuthoringResult> {
    const generation = ++this.generation;
    const response = await this.native.submit(action, input);
    if (!response.ok) {
      const error = await response.json();
      return { requestId, error: error.error, diagnostics: error.diagnostics };
    }
    const bytes = await response.arrayBuffer();
    if (action.startsWith("glb")) return { requestId, download: bytes };
    return decodeNativeReport(bytes, requestId, blocks, next => {
      if (generation === this.generation) this.blocks = next;
    });
  }
  cancelPending() { this.generation++; this.native.cancel(); }
  close() { this.generation++; this.blocks = undefined; this.native.close(); }
}
