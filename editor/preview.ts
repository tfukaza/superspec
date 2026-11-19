import { CityAppearance } from "./city-appearance.ts";
import "./style.css";
import { decodePacket } from "../browser/model/packet.ts";
import { RustScene } from "../browser/renderer/scene.ts";

interface PreviewStatus {
  session: string;
  revision: number;
  sceneRevision: number | null;
  state: "loading" | "compiling" | "ready" | "error";
  diagnostics: { code: string; message: string; source: { line: number; column: number } }[];
  error: string | null;
  compileMs: number;
}

const viewport = document.querySelector<HTMLElement>("#viewport")!;
const output = document.querySelector<HTMLOutputElement>("#status")!;
const diagnostics = document.querySelector<HTMLElement>("#diagnostics")!;
const selection = document.querySelector<HTMLElement>("#selection")!;
const title = document.querySelector<HTMLElement>("#title")!;
const cityAppearance = new CityAppearance();
window.addEventListener("pagehide", () => cityAppearance.close(), { once: true });
const scene = new RustScene(viewport, mesh => {
  selection.textContent = mesh ? `${mesh.id} · source ${mesh.source.line}:${mesh.source.column}` : "Click a model to inspect its source.";
});
document.querySelector("#walk")!.addEventListener("click", () => scene.toggleWalk());
viewport.addEventListener("walkstatus", event => { selection.textContent = (event as CustomEvent<string>).detail; });
const walkHelper = document.querySelector<HTMLSelectElement>("#walk-helper")!;
walkHelper.addEventListener("change", () => scene.setWalkHelper(walkHelper.value));
viewport.addEventListener("walkstop", () => { walkHelper.value = "off"; });
let latest: PreviewStatus | undefined;
let connected = false;
let closed = false;
let target: number | undefined;
let appliedSession: string | undefined;
let loadError: { session: string; revision: number; message: string } | undefined;
let generation = 0;
let packetRequest: AbortController | undefined;
let timer: ReturnType<typeof setTimeout> | undefined;

/** Shows attempted revision state separately from the retained successfully rendered revision. */
function showStatus(): void {
  const retained = viewport.dataset.revision;
  const previous = retained ? ` Showing last valid revision ${retained}.` : " No valid scene yet.";
  const sceneError = loadError && latest && loadError.session === latest.session && loadError.revision === latest.sceneRevision ? loadError.message : undefined;
  diagnostics.textContent = [latest?.error, ...(latest?.diagnostics ?? []).map(item => `${item.source.line}:${item.source.column} ${item.code} — ${item.message}`), sceneError].filter(Boolean).join("\n");
  if (!connected) {
    output.dataset.state = "disconnected";
    output.textContent = `Preview service disconnected. Retrying…${previous}`;
  } else if (latest?.state === "error") {
    output.dataset.state = "error";
    output.textContent = `Revision ${latest.revision} failed.${previous}`;
  } else if (sceneError) {
    output.dataset.state = "error";
    output.textContent = `Preview delivery failed. Retrying…${previous}`;
  } else if (latest?.state === "ready" && appliedSession === latest.session && retained === String(latest.sceneRevision)) {
    output.dataset.state = "ready";
    output.textContent = `Revision ${latest.revision} ready · native compile and packet ${latest.compileMs.toFixed(1)} ms`;
  } else {
    output.dataset.state = "compiling";
    output.textContent = `Preparing revision ${latest?.revision ?? 0}…${previous}`;
  }
}

/** Fetches one immutable revision; a newer status invalidates pending network and GPU preparation. */
async function loadScene(session: string, revision: number): Promise<void> {
  const requestGeneration = ++generation;
  packetRequest?.abort();
  packetRequest = new AbortController();
  target = revision;
  const current = () => !closed && requestGeneration === generation;
  try {
    const response = await fetch(`/api/scene?session=${encodeURIComponent(session)}&revision=${revision}`, { signal: AbortSignal.any([packetRequest.signal, AbortSignal.timeout(10000)]) });
    if (!response.ok) throw new Error(`Scene unavailable (${response.status}).`);
    const packet = decodePacket(await response.arrayBuffer());
    if (!current()) return;
    if (packet.manifest.revision !== revision || !packet.manifest.success) throw new Error("Preview scene revision mismatch.");
    if (await cityAppearance.apply(scene, packet, current)) { loadError = undefined; appliedSession = session; viewport.dataset.session = session; title.textContent = packet.manifest.name; showStatus(); }
  } catch (error) {
    if (!current()) return;
    target = undefined;
    loadError = { session, revision, message: error instanceof Error ? error.message : String(error) };
    showStatus();
  }
}

/** Polling stays independent of image decoding so newer snapshots can cancel obsolete application. */
async function poll(): Promise<void> {
  try {
    const response = await fetch("/api/status", { signal: AbortSignal.timeout(2000) });
    if (!response.ok) throw new Error(`Status unavailable (${response.status}).`);
    const status = await response.json() as PreviewStatus;
    if (closed) return;
    if (latest?.session !== status.session) { generation++; packetRequest?.abort(); target = undefined; }
    latest = status;
    connected = true;
    if (status.sceneRevision !== null && status.sceneRevision !== target) void loadScene(status.session, status.sceneRevision);
  } catch {
    if (closed) return;
    connected = false;
  }
  showStatus();
  if (!closed) timer = setTimeout(() => void poll(), 250);
}

window.addEventListener("pagehide", () => { closed = true; generation++; clearTimeout(timer); packetRequest?.abort(); scene.dispose(); }, { once: true });
window.addEventListener("pageshow", event => { if (event.persisted) location.reload(); });
void poll();
