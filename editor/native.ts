import { NativeSession } from "../browser/runtime/native-session.ts";
import { decodePacket } from "../browser/model/packet.ts";
import { projectLighting } from "../browser/renderer/lighting.ts";
import { RustScene } from "../browser/renderer/scene.ts";
import { connectAuthoringGraphics } from "./graphics-controls.ts";

const element = <T extends HTMLElement>(id: string) => document.querySelector<T>("#" + id)!;
const status = element<HTMLOutputElement>("status");
const diagnostics = element("diagnostics");
const viewport = element("viewport");
const scene = new RustScene(viewport, mesh => {
  element("selection").textContent = mesh ? `${mesh.id} · ${mesh.source.line}:${mesh.source.column}` : "";
});
connectAuthoringGraphics(status, scene);
const exporter = element<HTMLButtonElement>("export");
const walk = element<HTMLButtonElement>("walk");
const native = new NativeSession();
let revision = 0;
let project: File | undefined;
let pending: AbortController | undefined;

async function checked(response: Response) {
  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw Error(body.diagnostics?.map((d: { sourceName: string; line: number; column: number; message: string }) => [d.sourceName && (d.sourceName + ":" + d.line + ":" + d.column), d.message].filter(Boolean).join(" · ")).join("\n")
      || body.error || `Request failed (${response.status}).`);
  }
  return response;
}
async function build(action: string, file: File) {
  const response = await native.request(action, file, file.name.endsWith(".tree.json") ? "tree" : "project");
  return (await checked(response)).arrayBuffer();
}
async function open(file?: File, url?: URL) {
  const id = ++revision;
  pending?.abort(); native.cancel();
  const controller = pending = new AbortController();
  const isProject = !!file && /\.(json|zip)$/i.test(file.name);
  exporter.disabled = true;
  status.textContent = isProject ? "Building native scene…" : "Loading scene…";
  diagnostics.textContent = "";
  try {
    const data = file ? (isProject ? await build("scene", file) : await file.arrayBuffer())
      : await (await checked(await fetch(url!, { signal: controller.signal }))).arrayBuffer();
    if (id !== revision) return;
    const packet = decodePacket(data);
    if (!packet.manifest.success) throw Error("The packet contains an unsuccessful build.");
    const lighting = projectLighting(packet.manifest);
    if (!await scene.apply(packet, () => id === revision, true) || id !== revision) return;
    scene.setLighting(lighting, packet.manifest.lighting);
    project = isProject ? file : undefined;
    element("title").textContent = packet.manifest.name;
    walk.disabled = false;
    status.textContent = `Ready · ${packet.manifest.meshes.length} meshes`;
    status.dataset.state = "ready";
  } catch (error) {
    if (id !== revision) return;
    status.textContent = "Scene load failed.";
    status.dataset.state = "error";
    diagnostics.textContent = String(error);
  } finally {
    if (id === revision) exporter.disabled = !project;
  }
}
element<HTMLInputElement>("file").onchange = event => {
  const input = event.currentTarget as HTMLInputElement;
  const file = input.files?.[0];
  input.value = "";
  if (file) void open(file);
};
walk.onclick = () => scene.toggleWalk();
exporter.onclick = async () => {
  if (!project) return;
  const id = ++revision;
  exporter.disabled = true;
  status.textContent = "Exporting…";
  try {
    const bytes = await build("glb", project);
    if (id !== revision) return;
    const link = document.createElement("a");
    link.href = URL.createObjectURL(new Blob([bytes], { type: "model/gltf-binary" }));
    link.download = project.name.replace(/\.(json|zip)$/i, ".glb");
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 60000);
    status.textContent = "Export ready.";
  } catch (error) {
    if (id === revision) { diagnostics.textContent = String(error); status.textContent = "Export failed."; }
  } finally { if (id === revision) exporter.disabled = !project; }
};
window.addEventListener("pagehide", () => {
  revision++; pending?.abort(); scene.dispose();
  native.close();
}, { once: true });
const source = new URL(location.href).searchParams.get("scene");
if (source) void open(undefined, new URL(source, location.href));
