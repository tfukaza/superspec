import * as THREE from "three";
import { CityAppearance } from "./city-appearance.ts";
import { PlacementPreview } from "./placement-preview.ts";
import { renamedSelection } from "./rename-preview.ts";
import { MAX_COMPANION_BYTES, MAX_COMPANIONS, MAX_FILE_BYTES, megabytes } from "../browser/model/limits.ts";
import "./style.css";
import sample from "./starter.aml?raw";
import kowloonBench from "./demos/kowloon-bench.aml?raw";
import { readAmlSelection, type AmlSelection } from "./open-aml.ts";
import { worldCompiler } from "./compiler-host.ts";
import type { SceneMesh } from "../browser/model/packet.ts";
import { SceneUpdates } from "../browser/model/updates.ts";
import { RustScene } from "../browser/renderer/scene.ts";
import type { ValidationReport } from "../browser/model/validation.ts";
import { connectAuthoringGraphics } from "./graphics-controls.ts";

const source = document.querySelector<HTMLTextAreaElement>("#aml")!;
const status = document.querySelector<HTMLOutputElement>("#status")!;
const diagnostics = document.querySelector<HTMLElement>("#diagnostics")!;
const title = document.querySelector<HTMLElement>("#title")!;
const selection = document.querySelector<HTMLElement>("#selection")!;
const viewport = document.querySelector<HTMLElement>("#viewport")!;
const exportButton = document.querySelector<HTMLButtonElement>("#download-glb")!;
const validateButton = document.querySelector<HTMLButtonElement>("#validate")!;
const exportStatus = document.querySelector<HTMLOutputElement>("#export-status")!;
let revision = 0;
const updates = new SceneUpdates();
const assets: Record<string, Uint8Array> = Object.create(null);
let assetImport = 0;
let pendingOpen: (AmlSelection & { revision: number }) | undefined;
let timer: ReturnType<typeof setTimeout> | undefined;
const cityAppearance = new CityAppearance();
const placement = new PlacementPreview();
let displayedSource = "";
let inspected: SceneMesh | undefined;
let renamedMesh: string | undefined;
function selectionLabel(name?: string): void {
  selection.textContent = inspected ? `${name ?? inspected.id} · source ${inspected.source.line}:${inspected.source.column}` : "Click a model to inspect its source.";
}
window.addEventListener("pagehide", () => cityAppearance.close(), { once: true });
const scene = new RustScene(viewport, mesh => {
  inspected = mesh;
  renamedMesh = undefined;
  selectionLabel();
});
connectAuthoringGraphics(document.querySelector<HTMLElement>("#graphics-profile")!, scene, profile => {
  if (profile.diagnostics.instrumentation.gpuProbe) Object.assign(window, { __three: THREE });
});
document.querySelector("#walk")!.addEventListener("click", () => scene.toggleWalk());
viewport.addEventListener("footprintgeometryframe", event => {
  if ((event as CustomEvent<number>).detail === revision && status.dataset.state === "compiling")
    status.textContent = "Showing regenerated building · Updating surface details…";
});
viewport.addEventListener("walkstatus", event => { selection.textContent = (event as CustomEvent<string>).detail; });
const walkHelper = document.querySelector<HTMLSelectElement>("#walk-helper")!;
walkHelper.addEventListener("change", () => scene.setWalkHelper(walkHelper.value));
viewport.addEventListener("walkstop", () => { walkHelper.value = "off"; });
const compiler = worldCompiler(async result => {
  if (result.revision !== revision) return;
  status.dataset.workers = String(result.workers ?? 1);
  try {
    if (result.validation) {
      const report = result.validation;
      showFindings(report);
      scene.showNavigation(report);
      exportButton.disabled = !report.success;
      validateButton.disabled = false;
      status.dataset.state = "ready";
      status.textContent = `Validation ${report.success ? "completed" : "found errors"} · ${report.diagnostics.length} findings · ${(result.compileMs ?? 0).toFixed(1)} ms`;
      const routes = report.routes?.map(route => {
        const maximum = route.maximumRoutes == null ? "" : `, declared maximum ${route.maximumRoutes}`;
        return `${route.id} ${route.verifiedRoutes}/${route.requestedRoutes} entrance routes${maximum}`;
      }).join(" · ") ?? "";
      document.querySelector("#details")!.textContent = `${routes}. Cyan shows land navigation. Blue shows boat navigation.`;
      return;
    }
    if (result.error || !result.buffer) throw new Error(result.error ?? "Compiler returned no scene.");
    const packet = updates.decodePreview(result.buffer);
    if (packet.manifest.revision !== revision) throw new Error("Scene revision mismatch.");
    if (!packet.manifest.success) {
      showFindings({ success: false, diagnostics: packet.manifest.diagnostics }, !pendingOpen || pendingOpen.source === source.value);
      showError();
      return;
    }
    const selectedAfterRename = renamedMesh;
    const selectionRevision = scene.selectionRevision;
    status.textContent = `Preparing preview · Rust compilation ${(result.compileMs ?? 0).toFixed(1)} ms…`;
    if (!cityAppearance.retain(scene, packet)) {
      const current = () => revision === result.revision;
      // A lightweight draw of the exact regenerated shape may finish between the lot preview and the material pass.
      void scene.previewFootprintGeometry(packet, current).catch(() => { /* The full material pipeline remains active. */ });
      if (!await cityAppearance.apply(scene, packet, current)) return;
    }
    if (result.revision !== revision) return;
    if (pendingOpen?.revision === result.revision) {
      source.value = pendingOpen.source;
      for (const name of Object.keys(assets)) delete assets[name];
      Object.assign(assets, pendingOpen.assets);
      for (const id of ["#assets", "#asset-folder"]) document.querySelector<HTMLInputElement>(id)!.value = "";
      pendingOpen = undefined;
    }
    placement.accept(source.value, packet.manifest);
    displayedSource = source.value;
    if (selectedAfterRename && selectionRevision === scene.selectionRevision && packet.manifest.meshes.some(mesh => mesh.id === selectedAfterRename)) scene.select(selectedAfterRename);
    title.textContent = packet.manifest.name;
    showFindings({ success: true, diagnostics: packet.manifest.diagnostics, reviews: packet.manifest.reviews });
    status.dataset.state = "ready";
    exportButton.disabled = false;
    validateButton.disabled = false;
    status.textContent = `Revision ${revision} ready · ${packet.manifest.meshes.length} objects · ${result.workers ?? 1} workers · Rust compile and packet copy ${(result.compileMs ?? 0).toFixed(1)} ms`;
  } catch (error) {
    if (result.revision !== revision) return;
    diagnostics.textContent = error instanceof Error ? error.message : String(error);
    showError();
  }
});

/** Each finding opens its AML range and spatial inspection target. */
function showFindings(report: ValidationReport, currentSource = true): void {
  diagnostics.replaceChildren();
  if (report.error) diagnostics.append(report.error, "\n");
  for (const finding of report.diagnostics) {
    const button = document.createElement("button");
    button.textContent = `${finding.source.line}:${finding.source.column} ${finding.severity} ${finding.code} — ${finding.message}`;
    button.disabled = !currentSource;
    button.style.whiteSpace = "pre-wrap";
    button.style.textAlign = "left";
    button.addEventListener("click", () => {
      source.focus();
      source.setSelectionRange(finding.source.offset, finding.source.end ?? finding.source.offset + 1);
      if (finding.spatial) scene.focusBounds(finding.spatial.bounds);
    });
    diagnostics.append(button, "\n");
  }
  for (const review of report.reviews ?? []) {
    const details = document.createElement("details");
    const summary = document.createElement("summary");
    summary.textContent = `${review.id} · ${review.state}`;
    const identity = document.createElement("pre");
    identity.textContent = `Fingerprint ${review.fingerprint}${review.suppression ? `\n${review.suppression.reason}${review.suppression.stale ? " · geometry changed" : ""}` : ""}`;
    details.append(summary, identity);
    diagnostics.append(details);
  }
}

const exporter = worldCompiler(result => {
  if (result.revision !== revision || status.dataset.state !== "ready") return;
  exportButton.disabled = status.dataset.state !== "ready";
  if (result.error || !result.buffer) {
    exportStatus.textContent = `Export failed: ${result.error ?? "No model returned."}`;
    return;
  }
  const url = URL.createObjectURL(new Blob([result.buffer], { type: "model/gltf-binary" }));
  const link = document.createElement("a"); link.href = url; link.download = "level.glb"; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 0);
  exportStatus.textContent = "GLB downloaded.";
}, (exported, progress) => {
  if (exported === revision) exportStatus.textContent = `Encoding KTX2 textures ${progress.encoded} of ${progress.textures}…`;
});

/** Makes retained older geometry visibly stale without destroying it. */
function showError(): void {
  if (renamedMesh) selectionLabel();
  renamedMesh = undefined;
  scene.previewPlacements([], revision);
  pendingOpen = undefined;
  exporter.cancelPending();
  exportButton.disabled = true;
  validateButton.disabled = true;
  status.dataset.state = "error";
  status.textContent = viewport.dataset.revision ? `Edit failed. Showing last valid revision ${viewport.dataset.revision}.` : "Edit failed. No valid scene yet.";
}

/** Coalesces typing while terminating obsolete Wasm work immediately. */
function edit(previewPlacement = false): void {
  pendingOpen = undefined;
  assetImport++;
  revision++;
  const name = previewPlacement ? renamedSelection(displayedSource, source.value, inspected) : undefined;
  if (name || renamedMesh) selectionLabel(name);
  renamedMesh = name;
  const moves = previewPlacement ? placement.moves(source.value) : [];
  const moved = scene.previewPlacements(moves, revision);
  clearTimeout(timer);
  cityAppearance.cancelPendingLayouts();
  compiler.cancelPending();
  exporter.cancelPending();
  exportButton.disabled = true;
  validateButton.disabled = true;
  exportStatus.textContent = "";
  status.dataset.state = "compiling";
  const preview = moved && moves.some(move => move.footprint) ? "footprint" : moved ? "placement" : name ? "name" : undefined;
  status.textContent = preview ? `Previewing ${preview} · Compiling revision ${revision}…` : `Compiling revision ${revision}…${viewport.dataset.revision ? " Previous scene remains visible." : ""}`;
  // An admitted drag already identifies a valid placement. Allow three idle
  // 60 Hz frames to coalesce pointer input before regenerating its geometry.
  timer = setTimeout(() => {
    compiler.compile(source.value, revision, assets, "scene", updates.revision);
    if (moved) cityAppearance.prepareFootprint(moves);
  }, moved ? 50 : 150);
}

source.value = sample;

/** Publishes a source and companions only after their scene compiles and displays. */
async function openAml(read: (isCurrent: () => boolean) => AmlSelection | Promise<AmlSelection>): Promise<void> {
  const generation = ++assetImport;
  const startingRevision = ++revision;
  if (renamedMesh) selectionLabel();
  renamedMesh = undefined;
  scene.previewPlacements([], revision);
  pendingOpen = undefined;
  clearTimeout(timer);
  cityAppearance.cancelPendingLayouts();
  compiler.cancelPending();
  exporter.cancelPending();
  exportButton.disabled = true;
  validateButton.disabled = true;
  status.dataset.state = "compiling";
  status.textContent = "Opening AML… Previous scene remains visible.";
  const isCurrent = () => generation === assetImport && startingRevision === revision;
  try {
    const selection = await read(isCurrent);
    if (!isCurrent()) return;
    // Textareas normalize Windows line endings. Compile the identical text so diagnostics and placement ranges line up.
    pendingOpen = { ...selection, source: selection.source.replace(/\r\n?/g, "\n"), revision };
    compiler.compile(pendingOpen.source, revision, selection.assets);
  } catch (error) {
    if (isCurrent()) { diagnostics.textContent = String(error); showError(); }
  }
}
for (const id of ["#open-aml", "#open-folder"]) {
  const input = document.querySelector<HTMLInputElement>(id)!;
  input.addEventListener("change", async () => {
    const files = Array.from(input.files ?? []);
    if (files.length) await openAml(isCurrent => readAmlSelection(files, isCurrent));
    input.value = "";
  });
}
document.querySelector("#open-bench-demo")!.addEventListener("click", () => {
  void openAml(() => ({ source: kowloonBench, assets: {} }));
});
document.querySelector("#open-modern-industrial-demo")!.addEventListener("click", () => {
  void openAml(async isCurrent => {
    const { modernIndustrialDemo } = await import("./demos/modern-industrial.ts");
    return modernIndustrialDemo(isCurrent);
  });
});
/** Reads a complete asset selection before publishing it; newer selections supersede older reads. */
async function importAssets(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const generation = ++assetImport;
  const startingRevision = revision;
  try {
    const files = Array.from(input.files ?? []);
    const sizes = new Map(Object.entries(assets).map(([name, bytes]) => [name, bytes.length]));
    const names = new Set<string>();
    for (const file of files) {
      if (file.size > MAX_FILE_BYTES) throw new Error(`${file.name} exceeds the ${megabytes(MAX_FILE_BYTES)} file limit.`);
      const name = file.webkitRelativePath || file.name;
      if (names.has(name)) throw new Error(`Duplicate asset path: ${name}. Select their parent folder to preserve distinct paths.`);
      names.add(name);
      sizes.set(name, file.size);
    }
    if (sizes.size > MAX_COMPANIONS || [...sizes.values()].reduce((sum, size) => sum + size, 0) > MAX_COMPANION_BYTES) throw new Error(`Assets exceed ${MAX_COMPANIONS} files or ${megabytes(MAX_COMPANION_BYTES)} in total.`);
    const staged: Record<string, Uint8Array> = Object.create(null);
    for (const file of files) {
      if (generation !== assetImport) return;
      const bytes = new Uint8Array(await file.arrayBuffer());
      if (generation !== assetImport) return;
      staged[file.webkitRelativePath || file.name] = bytes;
    }
    if (generation !== assetImport || startingRevision !== revision) return;
    Object.assign(assets, staged);
    edit();
  } catch (error) {
    if (generation === assetImport && startingRevision === revision) { diagnostics.textContent = String(error); showError(); }
  }
}
for (const id of ["#assets", "#asset-folder"]) document.querySelector<HTMLInputElement>(id)!.addEventListener("change", importAssets);
document.querySelector("#clear-assets")!.addEventListener("click", () => {
  assetImport++;
  for (const key of Object.keys(assets)) delete assets[key];
  for (const id of ["#assets", "#asset-folder"]) document.querySelector<HTMLInputElement>(id)!.value = "";
  compiler.reset();
  exporter.reset();
  edit();
});
source.addEventListener("input", () => edit(true));
document.querySelector("#restart")!.addEventListener("click", () => { compiler.reset(); edit(); });
document.querySelector("#download")!.addEventListener("click", () => {
  const url = URL.createObjectURL(new Blob([source.value], { type: "application/xml" }));
  const link = document.createElement("a"); link.href = url; link.download = "level.aml"; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 0);
});
exportButton.addEventListener("click", () => {
  if (status.dataset.state !== "ready") return;
  exportButton.disabled = true;
  exportStatus.textContent = "Preparing GLB…";
  exporter.compile(source.value, revision, assets, document.querySelector<HTMLInputElement>("#export-ktx2")!.checked ? "glb" : "glb-png");
});
validateButton.addEventListener("click", () => {
  validateButton.disabled = true;
  status.textContent = "Checking collision and sampled navigation…";
  compiler.compile(source.value, revision, assets, "validate");
});
window.addEventListener("pagehide", () => { clearTimeout(timer); compiler.dispose(); exporter.dispose(); scene.dispose(); }, { once: true });
// A bfcache restore must recreate disposed worker and renderer owners.
window.addEventListener("pageshow", event => { if (event.persisted) location.reload(); });
edit();
