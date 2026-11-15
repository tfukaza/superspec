import type { Parameter, ParameterValue } from "../../browser/runtime/authoring.ts";

/** Creates controls from the compiler's public declarations, retaining focus while values are regenerated. */
export class ParameterControls {
  private signature = "";
  constructor(private readonly host: HTMLElement, private readonly change: (name: string, value: ParameterValue) => void) {}

  /** A declaration change replaces controls; value-only changes update their existing inputs. */
  show(parameters: Parameter[], values: Record<string, ParameterValue>): void {
    const signature = JSON.stringify(parameters);
    if (signature !== this.signature) {
      this.signature = signature;
      this.host.replaceChildren();
      for (const parameter of parameters) this.host.append(this.control(parameter));
      if (!parameters.length) this.host.textContent = "Declare public parameters in the source to expose controls.";
    }
    for (const parameter of parameters) {
      const value = values[parameter.name] ?? parameter.default;
      const inputs = this.host.querySelectorAll<HTMLInputElement | HTMLSelectElement>(`[data-parameter="${CSS.escape(parameter.name)}"]`);
      inputs.forEach((input, index) => {
        if (input === document.activeElement) return;
        if (input instanceof HTMLInputElement && input.type === "checkbox") input.checked = Boolean(value);
        else input.value = String(Array.isArray(value) ? value[index] : value);
      });
    }
  }

  /** Each field commits typed values; Rust remains responsible for range and semantic validation. */
  private control(parameter: Parameter): HTMLElement {
    const row = document.createElement("div"); row.className = "parameter";
    const displayName = parameter.label ?? parameter.name;
    const label = document.createElement("label"); label.textContent = displayName;
    const inputs = document.createElement("div"); inputs.className = "parameter-inputs";
    let count = 1;
    if (parameter.type === "vec2") count = 2;
    if (parameter.type === "vec3" || (parameter.type === "color" && Array.isArray(parameter.default))) count = 3;
    if (parameter.type === "choice") {
      const select = document.createElement("select"); select.id = `parameter-${parameter.name}-0`;
      select.dataset.parameter = parameter.name; label.htmlFor = select.id;
      for (const choice of parameter.choices ?? []) select.add(new Option(choice, choice));
      select.addEventListener("change", () => this.change(parameter.name, select.value));
      inputs.append(select);
    }
    for (let axis = 0; parameter.type !== "choice" && axis < count; axis++) {
      const input = document.createElement("input");
      input.id = `parameter-${parameter.name}-${axis}`;
      if (axis === 0) label.htmlFor = input.id;
      input.setAttribute("aria-label", `${displayName}${count > 1 ? ` ${["x", "y", "z"][axis]}` : ""}`);
      input.dataset.parameter = parameter.name;
      input.type = "number";
      if (["bool", "boolean"].includes(parameter.type)) input.type = "checkbox";
      if (parameter.type === "color" && count === 1) input.type = "color";
      if (parameter.min !== undefined) input.min = String(parameter.min);
      if (parameter.max !== undefined) input.max = String(parameter.max);
      input.step = String(parameter.step ?? (parameter.type === "integer" ? 1 : 0.01));
      input.addEventListener("change", () => {
        const siblings = [...inputs.querySelectorAll<HTMLInputElement>("input")];
        let value: ParameterValue = input.valueAsNumber;
        if (count > 1) value = siblings.map(item => item.valueAsNumber);
        if (input.type === "checkbox") value = input.checked;
        if (input.type === "color") value = input.value;
        this.change(parameter.name, value);
      });
      inputs.append(input);
    }
    const detail = document.createElement("small"); detail.textContent = [parameter.unit, parameter.role].filter(Boolean).join(" · ");
    row.append(label, inputs, detail);
    return row;
  }
}
