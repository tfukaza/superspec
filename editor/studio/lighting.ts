import { DEFAULT_LIGHTING, LightingError, defaultLighting, hexFromLinear, linearFromHex, parseLighting, sunAngles, sunDirection, sunOf,
  type Light, type Lighting } from "../../browser/renderer/lighting.ts";

/** One control of the panel: where it sits, what it shows of a record and how its value changes one. */
interface Control {
  id: string;
  label: string;
  /** Said on hover, where the label alone does not say what the value means. */
  hint?: string;
  group: "Sun" | "Sun shadow" | "Ambient and environment" | "Ambient occlusion" | "Image";
  kind: "number" | "range" | "color" | "check" | "choice";
  /** Hints for the input; the record's own validation decides what is admitted. */
  min?: number; max?: number; step?: number;
  choices?: string[];
  /** Needs the sun: disabled while the record has no light. */
  sun?: boolean;
  read(lighting: Lighting): number | string | boolean;
  /** Writes the control's value into a draft of the record. `panel` reads sibling controls (the two sun angles). */
  write(lighting: Lighting, value: number | string | boolean, panel: LightingPanel): void;
}

const degrees = (radians: number): number => radians * 180 / Math.PI;
const rounded = (value: number, digits = 4): number => Number(value.toFixed(digits));
/** The draft's sun; controls that use it are disabled without one. */
const sun = (lighting: Lighting): Light => sunOf(lighting)!;

const CONTROLS: Control[] = [
  { id: "lighting-sun", label: "Sun", group: "Sun", kind: "check",
    read: lighting => sunOf(lighting) !== undefined,
    write: (lighting, value, panel) => { lighting.lights = value ? [panel.lastSun()] : []; } },
  { id: "lighting-azimuth", label: "Azimuth (°)", hint: "Where the sun stands, turning from +X toward +Y about +Z.", group: "Sun", kind: "number", min: 0, max: 360, step: 1, sun: true,
    read: lighting => rounded(sunAngles(sun(lighting).direction).azimuth, 2),
    write: (lighting, value, panel) => { sun(lighting).direction = sunDirection(Number(value), panel.number("lighting-elevation")); } },
  { id: "lighting-elevation", label: "Elevation (°)", hint: "The sun's height above the ground plane; 90 is overhead.", group: "Sun", kind: "number", min: -90, max: 90, step: 1, sun: true,
    read: lighting => rounded(sunAngles(sun(lighting).direction).elevation, 2),
    write: (lighting, value, panel) => { sun(lighting).direction = sunDirection(panel.number("lighting-azimuth"), Number(value)); } },
  { id: "lighting-sun-color", label: "Colour", group: "Sun", kind: "color", sun: true,
    read: lighting => `#${hexFromLinear(sun(lighting).color)}`,
    write: (lighting, value) => { sun(lighting).color = linearFromHex(String(value).slice(1)); } },
  { id: "lighting-sun-intensity", label: "Intensity", group: "Sun", kind: "number", min: 0, max: 100, step: 0.1, sun: true,
    read: lighting => sun(lighting).intensity,
    write: (lighting, value) => { sun(lighting).intensity = Number(value); } },

  { id: "lighting-shadows", label: "Casts shadows", group: "Sun shadow", kind: "check", sun: true,
    read: lighting => sun(lighting).castsShadow,
    write: (lighting, value) => { sun(lighting).castsShadow = Boolean(value); } },
  { id: "lighting-shadow-style", label: "Style", group: "Sun shadow", kind: "choice", choices: ["illustration", "plain"], sun: true,
    read: lighting => sun(lighting).shadow.style,
    write: (lighting, value) => { sun(lighting).shadow.style = value as Light["shadow"]["style"]; } },
  { id: "lighting-shadow-softness", label: "Softness (m)", group: "Sun shadow", kind: "number", min: 0, max: 2, step: 0.002, sun: true,
    read: lighting => sun(lighting).shadow.softness,
    write: (lighting, value) => { sun(lighting).shadow.softness = Number(value); } },
  { id: "lighting-shadow-darkness", label: "Darkness", group: "Sun shadow", kind: "number", min: 0, max: 1, step: 0.05, sun: true,
    read: lighting => sun(lighting).shadow.darkness,
    write: (lighting, value) => { sun(lighting).shadow.darkness = Number(value); } },
  { id: "lighting-shadow-tint", label: "Tint", group: "Sun shadow", kind: "color", sun: true,
    read: lighting => `#${hexFromLinear(sun(lighting).shadow.tint)}`,
    write: (lighting, value) => { sun(lighting).shadow.tint = linearFromHex(String(value).slice(1)); } },
  { id: "lighting-shadow-tint-strength", label: "Tint strength", group: "Sun shadow", kind: "number", min: 0, max: 1, step: 0.02, sun: true,
    read: lighting => sun(lighting).shadow.tintStrength,
    write: (lighting, value) => { sun(lighting).shadow.tintStrength = Number(value); } },
  { id: "lighting-noise-amount", label: "Noise amount", group: "Sun shadow", kind: "number", min: 0, max: 1, step: 0.05, sun: true,
    read: lighting => sun(lighting).shadow.noise.amount,
    write: (lighting, value) => { sun(lighting).shadow.noise.amount = Number(value); } },
  { id: "lighting-noise-scale", label: "Noise scale (m)", group: "Sun shadow", kind: "number", min: 0.01, max: 100, step: 0.1, sun: true,
    read: lighting => sun(lighting).shadow.noise.scale,
    write: (lighting, value) => { sun(lighting).shadow.noise.scale = Number(value); } },

  { id: "lighting-ambient-intensity", label: "Ambient intensity", group: "Ambient and environment", kind: "number", min: 0, max: 100, step: 0.05,
    read: lighting => lighting.ambient.intensity,
    write: (lighting, value) => { lighting.ambient.intensity = Number(value); } },
  { id: "lighting-ambient-sky", label: "Sky colour", group: "Ambient and environment", kind: "color",
    read: lighting => `#${hexFromLinear(lighting.ambient.sky)}`,
    write: (lighting, value) => { lighting.ambient.sky = linearFromHex(String(value).slice(1)); } },
  { id: "lighting-ambient-ground", label: "Ground colour", group: "Ambient and environment", kind: "color",
    read: lighting => `#${hexFromLinear(lighting.ambient.ground)}`,
    write: (lighting, value) => { lighting.ambient.ground = linearFromHex(String(value).slice(1)); } },
  { id: "lighting-environment", label: "Environment", group: "Ambient and environment", kind: "choice", choices: ["studio", "none"],
    read: lighting => lighting.environment.source,
    write: (lighting, value) => { lighting.environment.source = String(value); } },
  { id: "lighting-environment-intensity", label: "Environment intensity", group: "Ambient and environment", kind: "number", min: 0, max: 100, step: 0.05,
    read: lighting => lighting.environment.intensity,
    write: (lighting, value) => { lighting.environment.intensity = Number(value); } },
  { id: "lighting-environment-rotation", label: "Environment turn (°)", group: "Ambient and environment", kind: "number", min: -360, max: 360, step: 5,
    read: lighting => rounded(degrees(lighting.environment.rotation), 2),
    write: (lighting, value) => { lighting.environment.rotation = Number(value) * Math.PI / 180; } },

  { id: "lighting-ao", label: "Ambient occlusion", group: "Ambient occlusion", kind: "check",
    read: lighting => lighting.ambientOcclusion.enabled,
    write: (lighting, value) => { lighting.ambientOcclusion.enabled = Boolean(value); } },
  { id: "lighting-ao-radius", label: "Radius (m)", group: "Ambient occlusion", kind: "number", min: 0.01, max: 10, step: 0.05,
    read: lighting => lighting.ambientOcclusion.radius,
    write: (lighting, value) => { lighting.ambientOcclusion.radius = Number(value); } },
  { id: "lighting-ao-intensity", label: "Intensity", group: "Ambient occlusion", kind: "number", min: 0, max: 4, step: 0.1,
    read: lighting => lighting.ambientOcclusion.intensity,
    write: (lighting, value) => { lighting.ambientOcclusion.intensity = Number(value); } },

  { id: "exposure", label: "Exposure", group: "Image", kind: "range", min: 0.25, max: 3, step: 0.05,
    read: lighting => lighting.exposure,
    write: (lighting, value) => { lighting.exposure = Number(value); } },
  { id: "lighting-tone-mapping", label: "Tone mapping", group: "Image", kind: "choice", choices: ["aces", "neutral", "none"],
    read: lighting => lighting.toneMapping,
    write: (lighting, value) => { lighting.toneMapping = value as Lighting["toneMapping"]; } },
];

/**
 * The Studio's controls for a project's lighting record (`browser/renderer/lighting.ts`): the sun and its shadow's
 * look, ambient and environment light, ambient occlusion, exposure and tone mapping. Global and per-light rendering
 * settings of the scene, never part of a material.
 *
 * Every edit makes a draft of the record and admits it with the record's own validation (`parseLighting`); an admitted
 * record goes to `changed`, a refused one is said under the controls and changes nothing. The panel never touches the
 * viewport or the project itself.
 */
export class LightingPanel {
  private lighting = defaultLighting();
  /** Whether the author chose ambient occlusion on or off (see `storedLighting`). */
  private statesOcclusion = false;
  /** The sun as it last was, so turning it off and on again loses nothing. */
  private sunKept: Light = structuredClone(DEFAULT_LIGHTING.lights[0]);
  private readonly inputs = new Map<string, HTMLInputElement | HTMLSelectElement>();
  private readonly status: HTMLOutputElement;
  private readonly note: HTMLParagraphElement;

  constructor(host: HTMLElement, private readonly changed: (lighting: Lighting, statesOcclusion: boolean) => void) {
    host.replaceChildren();
    let fields: HTMLElement | undefined, group = "";
    for (const control of CONTROLS) {
      if (control.group !== group) {
        group = control.group;
        const set = document.createElement("fieldset");
        set.className = "lighting-group";
        const legend = document.createElement("legend");
        legend.textContent = group;
        fields = document.createElement("div");
        fields.className = "lighting-fields";
        set.append(legend, fields);
        host.append(set);
      }
      const label = document.createElement("label");
      let input: HTMLInputElement | HTMLSelectElement;
      if (control.kind === "choice") {
        input = document.createElement("select");
        for (const choice of control.choices ?? []) input.add(new Option(choice, choice));
      } else {
        input = document.createElement("input");
        input.type = control.kind === "check" ? "checkbox" : control.kind;
        for (const hint of ["min", "max", "step"] as const) if (control[hint] !== undefined) input.setAttribute(hint, String(control[hint]));
      }
      input.id = control.id;
      if (control.hint) label.title = control.hint;
      if (control.kind === "check") { label.className = "lighting-check"; label.append(input, control.label); }
      else { if (control.kind === "range") label.className = "lighting-wide"; label.append(control.label, input); }
      // A number field reports each keystroke; a refused value is said and the record keeps its last admitted state.
      input.addEventListener(control.kind === "choice" || control.kind === "check" ? "change" : "input", () => this.edit(control));
      this.inputs.set(control.id, input);
      fields!.append(label);
    }
    this.note = document.createElement("p");
    this.note.className = "studio-note";
    this.note.id = "lighting-note";
    this.status = document.createElement("output");
    this.status.id = "lighting-status";
    this.status.setAttribute("role", "status");
    const reset = document.createElement("button");
    reset.id = "lighting-reset";
    reset.className = "quiet compact";
    reset.textContent = "Reset to defaults";
    reset.addEventListener("click", () => { this.sunKept = structuredClone(DEFAULT_LIGHTING.lights[0]); this.accept(defaultLighting(), false); });
    host.append(this.note, this.status, reset);
    this.reflect();
  }

  /** Shows a record (a project's, on open). */
  show(lighting: Lighting, statesOcclusion: boolean): void {
    this.lighting = structuredClone(lighting);
    this.statesOcclusion = statesOcclusion;
    const kept = sunOf(lighting);
    if (kept) this.sunKept = structuredClone(kept);
    this.refuse();
    this.reflect();
  }

  /**
   * Says what stands between the record and the picture, if anything (`note`), and whether this renderer leaves
   * ambient occlusion off where the record does not say (`occlusionOff`): its box then shows off until the author
   * ticks it, which states it in the record.
   */
  viewer(note: string, occlusionOff: boolean): void {
    this.note.textContent = note;
    this.note.hidden = !note;
    this.occlusionOff = occlusionOff;
    // Only that box: a field being typed in keeps its text.
    (this.inputs.get("lighting-ao") as HTMLInputElement).checked = this.lighting.ambientOcclusion.enabled && !(occlusionOff && !this.statesOcclusion);
  }
  private occlusionOff = false;

  /** The number a sibling control shows now. */
  number(id: string): number { return (this.inputs.get(id) as HTMLInputElement).valueAsNumber; }
  /** The sun to bring back when it is turned on again. */
  lastSun(): Light { return structuredClone(this.sunKept); }

  private edit(control: Control): void {
    const input = this.inputs.get(control.id)!;
    const value = control.kind === "check" ? (input as HTMLInputElement).checked
      : control.kind === "number" || control.kind === "range" ? (input as HTMLInputElement).valueAsNumber : input.value;
    if (typeof value === "number" && !Number.isFinite(value)) { this.refuse(input, `${control.label} needs a number.`); return; }
    const draft = structuredClone(this.lighting);
    control.write(draft, value, this);
    let admitted: Lighting;
    try { admitted = parseLighting(draft); }
    catch (error) {
      if (!(error instanceof LightingError)) throw error;
      this.refuse(input, error.findings.map(finding => `${finding.field} ${finding.message}`).join("; "));
      return;
    }
    this.accept(admitted, this.statesOcclusion || control.id === "lighting-ao", input);
  }

  private accept(lighting: Lighting, statesOcclusion: boolean, edited?: HTMLElement): void {
    this.lighting = lighting;
    this.statesOcclusion = statesOcclusion;
    const kept = sunOf(lighting);
    if (kept) this.sunKept = structuredClone(kept);
    this.refuse();
    this.reflect(edited);
    this.changed(structuredClone(lighting), statesOcclusion);
  }

  /** Says why a value was refused and marks its control, or clears both. */
  private refuse(input?: HTMLElement, message = ""): void {
    for (const other of this.inputs.values()) other.removeAttribute("aria-invalid");
    input?.setAttribute("aria-invalid", "true");
    this.status.textContent = message;
    this.status.dataset.state = message ? "error" : "ready";
  }

  /** Puts the record's values in the controls; the one being typed in keeps its text. */
  private reflect(edited?: HTMLElement): void {
    const lit = sunOf(this.lighting) !== undefined;
    for (const control of CONTROLS) {
      const input = this.inputs.get(control.id)!;
      input.disabled = control.sun === true && !lit;
      if (input === edited || input.disabled) continue;
      const value = control.read(this.lighting);
      if (control.id === "lighting-ao") (input as HTMLInputElement).checked = Boolean(value) && !(this.occlusionOff && !this.statesOcclusion);
      else if (control.kind === "check") (input as HTMLInputElement).checked = Boolean(value);
      else input.value = String(typeof value === "number" ? rounded(value, 6) : value);
    }
  }
}
