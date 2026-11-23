import "./style.css";
import { graphicsPackUrl } from "#render-engine";
import { installGraphicsControls, loadRenderProfile, navigateGraphicsProfile } from "./graphics-controls.ts";

const query = new URLSearchParams(location.search);
const gltf = query.get("gltf");
if (gltf) {
  const status = document.querySelector<HTMLOutputElement>("#status")!;
  document.querySelector("header label")?.remove();
  document.querySelector("#export")?.remove();
  document.querySelector("#walk")?.remove();
  document.querySelector(".caption")?.remove();
  void showPublishedGltf(new URL(gltf, location.href), document.querySelector("#viewport")!, status)
    .catch(error => { status.dataset.state = "error"; status.textContent = String(error); });
} else {
  void import("./native.ts");
}

async function showPublishedGltf(url: URL, host: HTMLElement, status: HTMLOutputElement) {
  const requested = query.get("renderer") ?? "auto";
  if (!["auto", "webgpu", "webgl"].includes(requested)) throw Error("renderer must be auto, webgpu or webgl.");
  let share: (() => URLSearchParams) | undefined;
  let gpu: Awaited<ReturnType<typeof import("./native-webgpu.ts").openWebGpuGltf>> | undefined;
  let loss: string | undefined;
  const select = document.createElement("select");
  select.setAttribute("aria-label", "Renderer");
  for (const [value, label] of [["auto", "Auto"], ["webgpu", "WebGPU"], ["webgl", "WebGL"]])
    select.add(new Option(label, value));
  select.value = requested;
  select.onchange = () => {
    const next = share?.() ?? new URLSearchParams(query);
    next.set("renderer", select.value);
    location.search = next.toString();
  };
  const label = document.createElement("label");
  label.append("Renderer ", select);
  status.before(label);
  const graphics = await loadRenderProfile(query);
  let entry = url;
  // The resolved asset variant names the pack. The preset no longer does.
  const variant = graphicsPackUrl(url, graphics.variant);
  if (variant && variant.origin === location.origin) {
    const response = await fetch(variant, { method: "HEAD" }).catch(() => undefined);
    if (response?.ok && !response.headers.get("content-type")?.includes("text/html")) entry = variant;
  }
  const currentQuery = () => share?.() ?? new URLSearchParams(query);
  installGraphicsControls(status, graphics, profile => navigateGraphicsProfile(currentQuery(), profile));
  const ready = (backend: "webgpu" | "webgl", message: string, portable = false) => {
    status.dataset.renderer = backend;
    status.dataset.appearance = portable ? "portable" : "standard";
    status.textContent = message;
    status.dataset.state = "ready";
  };
  async function openWebGl(reason?: string) {
    const view = share?.() ?? query;
    share = undefined;
    const previous = gpu;
    gpu = undefined;
    previous?.dispose();
    status.dataset.state = "loading";
    status.textContent = reason ? `Opening WebGL · ${reason}` : "Loading published glTF…";
    const { openGltf } = await import("./native-gltf.ts");
    const gl = await openGltf(entry, host, view, graphics, () => { status.textContent = "Preparing shaders and textures…"; });
    share = gl.share;
    if (gl.paintedLighting) {
      const painted = graphics.diagnostics.lighting.paintedShadows && graphics.diagnostics.lighting.paintedSpots;
      const toggle = document.createElement("button");
      toggle.textContent = "Painted lighting";
      toggle.setAttribute("aria-pressed", String(painted));
      toggle.onclick = () => {
        const diagnostics = graphics.profile.diagnostics;
        navigateGraphicsProfile(gl.share(), { ...graphics.profile, diagnostics: {
          ...diagnostics, lighting: { ...diagnostics?.lighting, paintedShadows: !painted, paintedSpots: !painted },
        } });
      };
      status.before(toggle);
    }
    ready("webgl", reason ? `Ready · WebGL · ${reason}` : "Ready · WebGL");
  }
  if (requested === "webgl") return openWebGl();
  if (!("gpu" in navigator) || !navigator.gpu) return openWebGl("WebGPU unavailable");
  status.textContent = "Opening WebGPU…";
  const module = await import("./native-webgpu.ts").catch(() => undefined);
  if (!module) return openWebGl("WebGPU renderer could not load");
  try {
    gpu = await module.openWebGpuGltf(entry, host, query, graphics, { auto: requested === "auto", onLost: reason => {
      if (loss !== undefined) return;
      loss = reason || "WebGPU device lost";
      if (gpu) void openWebGl(loss).catch(error => { status.dataset.state = "error"; status.textContent = String(error); });
    } });
  } catch (error) {
    if (error instanceof module.WebGpuFallbackError || loss !== undefined)
      return openWebGl(loss ?? (error as Error).message);
    throw error;
  }
  share = gpu.share;
  if (loss !== undefined) return openWebGl(loss);
  ready("webgpu", gpu.portable ? "Ready · WebGPU · portable materials and lighting" : "Ready · WebGPU", gpu.portable);
}
