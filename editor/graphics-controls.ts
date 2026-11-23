import "./graphics-controls.css";
import { GRAPHICS_PRESETS, GRAPHICS_RENDER_SCALE, graphicsParameterOverrides, isGraphicsPreset, resolveRenderProfile, writeRenderProfile,
  type AnyRenderProfile, type GraphicsOverrides, type GraphicsPreset, type ResolvedRenderProfile } from "#render-engine";
import type { RustScene } from "../browser/renderer/scene.ts";

export async function loadRenderProfile(query: URLSearchParams): Promise<ResolvedRenderProfile> {
  const source = query.get("profile");
  if (source === null) return resolveRenderProfile(undefined, query);
  // A linked profile may be version 2. It resolves in full, and the controls below keep its version.
  if (source.trimStart().startsWith("{")) return resolveRenderProfile(JSON.parse(source), query) as ResolvedRenderProfile;
  const response = await fetch(new URL(source, location.href));
  if (!response.ok) throw Error(`Could not load render profile (${response.status}).`);
  return resolveRenderProfile(await response.json(), query) as ResolvedRenderProfile;
}

export function installGraphicsControls(anchor: HTMLElement, initial: ResolvedRenderProfile,
  apply: (profile: AnyRenderProfile) => ResolvedRenderProfile | void): void {
  let active = initial;
  const stated = (): AnyRenderProfile => active.profile;
  const row = document.createElement("div");
  row.className = "graphics-control-row";
  const label = document.createElement("label");
  const preset = presetSelect(active.preset, true);
  preset.setAttribute("aria-label", "Graphics");
  preset.addEventListener("change", () => {
    const profile = stated(), { diagnostics } = profile;
    if (!isGraphicsPreset(preset.value)) return;
    // A version 2 profile keeps its platform and its parameter overrides under the new preset.
    if (profile.version === 2) commit({ ...profile, quality: preset.value });
    else commit({ version: 1, preset: preset.value, ...(diagnostics ? { diagnostics } : {}) });
  });
  label.append("Graphics ", preset);

  const details = document.createElement("details");
  details.className = "graphics-controls";
  const summary = document.createElement("summary");
  summary.textContent = "Graphics options";
  const form = document.createElement("form");
  const scale = document.createElement("input");
  scale.type = "number";
  scale.min = String(GRAPHICS_RENDER_SCALE.min * 100);
  scale.max = String(GRAPHICS_RENDER_SCALE.max * 100);
  scale.step = "any";
  scale.value = String(Number((active.settings.renderScale * 100).toFixed(6)));
  form.append(labeled("Render scale (%)", scale));

  const groups = { geometry: "Geometry LOD", materials: "Material detail", shadows: "Shadows" } as const;
  const fields = Object.entries(groups).map(([name, text]) => {
    const group = name as keyof typeof groups;
    const select = presetSelect(active.profile.overrides?.[group]);
    form.append(labeled(text, select));
    return [group, select] as const;
  });
  const button = document.createElement("button");
  button.textContent = "Apply";
  form.append(button);
  form.addEventListener("submit", event => {
    event.preventDefault();
    const renderScale = scale.valueAsNumber / 100;
    const overrides: GraphicsOverrides = {
      ...(Number.isFinite(renderScale) ? { renderScale } : {}),
      ...Object.fromEntries(fields.flatMap(([group, select]) => isGraphicsPreset(select.value) ? [[group, select.value]] : [])),
    };
    // In a version 2 profile each group becomes the parameter values it stands for, laid over
    // the profile's own overrides. A group left on "Use preset" changes none of them.
    const profile = stated();
    if (profile.version === 2) commit({ ...profile, overrides: layered(profile.overrides ?? {}, graphicsParameterOverrides(overrides)) });
    else commit({ ...profile, overrides });
  });
  details.append(summary, form);
  row.append(label, details);
  anchor.before(row);

  function commit(profile: AnyRenderProfile) {
    const next = apply(profile);
    if (!next) return;
    active = next;
    preset.value = next.preset;
    scale.value = String(Number((next.settings.renderScale * 100).toFixed(6)));
    for (const [group, select] of fields) select.value = next.profile.overrides?.[group] ?? "";
    details.open = false;
  }
}

/** Authoring uses the same profile without restarting the editor, compiler, or camera. */
export function connectAuthoringGraphics(anchor: HTMLElement, scene: Pick<RustScene, "setGraphicsProfile">,
  onProfile?: (profile: ResolvedRenderProfile) => void): void {
  const error = document.createElement("output");
  error.className = "graphics-profile-error";
  error.hidden = true;
  anchor.before(error);
  const apply = (profile: ResolvedRenderProfile) => {
    scene.setGraphicsProfile(profile);
    if (profile.diagnostics.instrumentation.gpuProbe) Object.assign(window, { __scene: scene });
    onProfile?.(profile);
  };
  const install = (profile: ResolvedRenderProfile) => {
    apply(profile);
    installGraphicsControls(error, profile, draft => {
      try {
        const next = resolveRenderProfile(draft) as ResolvedRenderProfile;
        apply(next);
        const url = new URL(location.href);
        writeRenderProfile(url.searchParams, next.profile);
        history.replaceState(history.state, "", url);
        error.hidden = true;
        return next;
      } catch (cause) {
        showError(cause);
      }
    });
  };
  const showError = (cause: unknown) => {
    error.textContent = cause instanceof Error ? cause.message : String(cause);
    error.hidden = false;
  };
  const query = new URLSearchParams(location.search);
  void loadRenderProfile(query).catch(cause => {
    query.delete("graphics");
    showError(cause);
    return resolveRenderProfile(undefined, query);
  }).then(install).catch(showError);
}

export function navigateGraphicsProfile(query: URLSearchParams, profile: AnyRenderProfile): void {
  writeRenderProfile(query, profile);
  location.search = query.toString();
}

function layered<T>(base: T, added: T): T {
  const plain = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object";
  if (!plain(base) || !plain(added)) return added;
  const out: Record<string, unknown> = { ...base };
  for (const [key, value] of Object.entries(added)) out[key] = key in base ? layered(base[key], value) : value;
  return out as T;
}

function presetSelect(selected?: GraphicsPreset, required = false) {
  const select = document.createElement("select");
  if (!required) select.add(new Option("Use preset", ""));
  for (const preset of GRAPHICS_PRESETS) select.add(new Option(preset.label, preset.id));
  if (selected) select.value = selected;
  return select;
}

function labeled(text: string, input: HTMLElement) {
  const label = document.createElement("label");
  input.setAttribute("aria-label", text);
  label.append(text, input);
  return label;
}
