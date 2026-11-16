import type { AuthoringDiagnostic, ParameterValue } from "../../browser/runtime/authoring.ts";
import type { Catalog, EditOp, Entry, Patch, RecipeInspection, RecipeTools, Span } from "../../browser/runtime/recipe.ts";
import { download } from "./project.ts";

/** What the panel needs from its studio: current text, a way to apply patches, and current overrides. */
export interface RecipeHost {
  /** Advances whenever the document is replaced, even by identical text; recompiles leave it alone. */
  revision(): number;
  entry(): string;
  module(): string;
  modules(): Record<string, string>;
  /** Text of the module being edited; the panel's offsets are UTF-16 offsets into it. */
  source(): string;
  /** Applies minimal patches to the editor and recompiles through the normal debounced path. */
  apply(patches: Patch[], source: string): void;
  insert(text: string): void;
  override(name: string): ParameterValue | undefined;
}

const NUMBER = /^-?(\d+\.?\d*|\.\d+)(e[+-]?\d+)?$/i;

/**
 * Builds an edit from the inspection it will run against. Span-based edits find their target again
 * by stable identity, so an edit made while the panel refreshes is applied to the fresh source.
 */
type Resolve = (inspection: RecipeInspection) => EditOp | undefined;

interface InspectedSource {
  revision: number;
  entry: string;
  module: string;
  modules: Record<string, string>;
}

function same(a: InspectedSource, b: InspectedSource): boolean {
  return a.revision === b.revision && a.entry === b.entry && a.module === b.module
    && Object.keys(a.modules).length === Object.keys(b.modules).length
    && Object.entries(a.modules).every(([name, text]) => b.modules[name] === text);
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

/** Makes `children` the children of `parent`, in order, touching only what differs: a child already in place is not moved. */
function place(parent: HTMLElement, children: HTMLElement[]): void {
  const wanted = new Set<Node>(children);
  for (const child of [...parent.childNodes]) if (!wanted.has(child)) child.remove();
  children.forEach((child, index) => { if (parent.childNodes[index] !== child) parent.insertBefore(child, parent.childNodes[index] ?? null); });
}

/**
 * Structured recipe editing for `.material.ts` modules. Every control issues the same edit
 * operation the command line offers; Rust returns minimal patches or the diagnostics that blocked it,
 * so comments and unrelated expressions never change and an invalid edit never reaches the source.
 */
export class RecipePanel {
  private generation = 0;
  private editGeneration = 0;
  private inspected?: InspectedSource;
  /** The source the displayed controls describe; unlike `inspected`, kept while they refresh. */
  private shown?: InspectedSource;
  /** The source exactly as this panel's last applied patch left it. */
  private patched?: InspectedSource;
  /**
   * The newest edit made while the controls were refreshing, with the source current when it was
   * made. It runs only if the refreshed source is still that snapshot, so any later typing,
   * reopening or module switch drops it.
   */
  private deferred?: { resolve: Resolve; source: InspectedSource };
  private controls?: HTMLFieldSetElement;
  private inspection?: RecipeInspection;
  private catalog?: Catalog;
  private query = "";
  private signature = "";
  private readonly message = element("p", "recipe-message");

  constructor(private readonly host: HTMLElement, private readonly tools: RecipeTools, private readonly io: RecipeHost) {
    this.message.setAttribute("role", "alert");
  }

  /** Captures every source dependency used to check a structured edit. */
  private source(): InspectedSource {
    const module = this.io.module();
    return { revision: this.io.revision(), entry: this.io.entry(), module, modules: { ...this.io.modules(), [module]: this.io.source() } };
  }

  /** Old spans are valid only for the complete source snapshot that produced them. */
  private matches(source: InspectedSource): boolean { return same(source, this.source()); }

  /** Re-reads the current module; superseded or failed reads leave the previous view in place. */
  async refresh(): Promise<void> {
    const generation = ++this.generation;
    const source = this.source();
    const { module } = source;
    this.inspected = undefined;
    if (this.controls) this.controls.disabled = true;
    if (!module.endsWith(".material.ts")) {
      this.inspection = undefined; this.signature = ""; this.deferred = undefined;
      this.host.replaceChildren(element("p", "recipe-hint", "Select a .material.ts module to edit parameters, layers and groups structurally."));
      return;
    }
    const { revision: _revision, ...request } = source;
    const result = await this.tools.inspect(request);
    if (generation !== this.generation || !this.matches(source)) return;
    if (!result.success) {
      this.signature = "";
      this.show(this.inspection, result.diagnostics[0]);
      if (this.deferred) { this.deferred = undefined; this.fail("Edit not applied: the source cannot be inspected until its errors are fixed."); }
      return;
    }
    // An unchanged structure keeps its controls, so a half-typed value is never discarded.
    const signature = JSON.stringify([result.inspection, result.inspection.parameters.map(parameter => this.io.override(parameter.name))]);
    this.inspection = result.inspection;
    this.inspected = source;
    this.shown = source;
    if (signature === this.signature) {
      if (this.controls) this.controls.disabled = false;
    } else {
      this.signature = signature;
      this.show(result.inspection);
    }
    const deferred = this.deferred;
    this.deferred = undefined;
    if (!deferred) return;
    if (same(deferred.source, source)) void this.run(deferred.resolve);
    else this.fail("Edit not applied: the source changed before the recipe controls refreshed.");
  }

  private show(inspection: RecipeInspection | undefined, failure?: AuthoringDiagnostic): void {
    const children: HTMLElement[] = [];
    if (failure) children.push(element("p", "recipe-hint", `The recipe cannot be inspected yet (${failure.line}:${failure.column} ${failure.message}). Showing the last readable structure.`));
    if (inspection) {
      this.controls ??= element("fieldset", "recipe-edit-controls");
      this.controls.disabled = !this.inspected;
      // A section is built again only when what it shows changed, and only what changed is put into the page: a
      // parameter value changed in the parameter panel replaces the parameter rows, while the layers, groups and field
      // uses (thousands of elements for a large recipe) are neither built nor laid out again.
      const overrides = inspection.parameters.map(parameter => this.io.override(parameter.name));
      place(this.controls, [
        this.kept("parameters", [inspection.parameters, overrides], () => this.parameters(inspection)),
        this.kept("layers", inspection.layers, () => this.layers(inspection)),
        this.kept("groups", inspection.groups, () => this.groups(inspection)),
        // Field uses are placed by line and column, which the text before them decides.
        this.kept("fields", [inspection.fields, inspection.groups.map(group => [group.name, group.outputs]), this.io.source()], () => this.fields(inspection)),
      ]);
      children.push(this.controls);
    } else this.sections.clear();
    children.push(this.operations ??= this.reference(), this.message);
    place(this.host, children);
  }

  /** The operation catalog's section: it keeps itself current, so it is made once. */
  private operations?: HTMLElement;

  /** The sections on display by name, with what each was built from. */
  private readonly sections = new Map<string, { signature: string; node: HTMLElement }>();

  /** The section built from `content`: the one on display when its content is unchanged (its controls keep what was typed into them). */
  private kept(name: string, content: unknown, build: () => HTMLElement): HTMLElement {
    const signature = JSON.stringify(content);
    const held = this.sections.get(name);
    if (held?.signature === signature) return held.node;
    const node = build();
    this.sections.set(name, { signature, node });
    return node;
  }

  private fail(text: string): void { this.message.textContent = text; }

  /**
   * Runs one edit; a rejected edit shows its first diagnostic and changes nothing.
   *
   * Controls can be stale for two harmless reasons: a refresh of unchanged text (after a
   * recompile), or the refresh after this panel's own previous patch. An edit made then waits
   * for the refresh and is resolved against it by identity, provided the source is still exactly
   * what it was when the edit was made, so quick successive control edits are not lost. When the
   * source changed any other way (typing, reopening, switching modules), a stale control does
   * nothing, and a reply still in flight is discarded.
   */
  private async run(edit: EditOp | Resolve): Promise<void> {
    const resolve: Resolve = typeof edit === "function" ? edit : () => edit;
    const source = this.inspected, inspection = this.inspection;
    if (!source || !inspection || !this.matches(source)) {
      const harmless = [this.shown, this.patched].some(snapshot => snapshot && this.matches(snapshot));
      if (harmless) {
        this.deferred = { resolve, source: this.source() };
        this.fail("Applying after the recipe controls refresh…");
      } else this.fail("Source changed. Wait for the recipe controls to refresh.");
      void this.refresh();
      return;
    }
    const op = resolve(inspection);
    if (!op) { this.fail("Edit not applied: its target is no longer in the recipe."); return; }
    const generation = ++this.editGeneration;
    const { revision: _revision, ...request } = source;
    const result = await this.tools.edit(request, op);
    if (generation !== this.editGeneration || !this.matches(source)) return;
    if (!result.success) {
      const first = result.diagnostics[0];
      this.fail(first ? `Edit rejected · ${first.sourceName ?? source.module}:${first.line}:${first.column} ${first.message}` : "Edit rejected.");
      return;
    }
    this.message.textContent = "";
    if (result.patches.length) {
      // Recorded before applying: the host's refresh disables the controls synchronously, and a
      // focused control holding a newer value commits it (a change event) during that call.
      this.patched = { ...source, modules: { ...source.modules, [source.module]: result.source } };
      this.io.apply(result.patches, result.source);
    }
  }

  private section(title: string, count?: number): HTMLElement {
    const section = element("section", "recipe-section");
    section.append(element("h3", "", count === undefined ? title : `${title} (${count})`));
    return section;
  }

  /**
   * A text input that issues a literal or reconnect edit for the expression it shows. `locate` finds
   * the same entry in a later inspection, so the edit follows the entry when the source shifts.
   */
  private entry(label: string, entry: Entry, locate: (inspection: RecipeInspection) => Entry | undefined): HTMLElement {
    const row = element("label", "recipe-entry");
    row.append(element("span", "", label));
    const input = element("input");
    const numeric = NUMBER.test(entry.text);
    input.type = numeric ? "number" : "text";
    input.step = "any";
    input.value = entry.text;
    input.dataset.span = `${entry.span.start}:${entry.span.end}`;
    input.setAttribute("aria-label", label);
    if (entry.text.startsWith("{")) { input.readOnly = true; input.title = "Edit this layer in the source."; }
    input.addEventListener("change", () => {
      if (input.readOnly) return;
      const value = input.value;
      const literal = numeric && input.type === "number" && value !== "" && NUMBER.test(value);
      void this.run(inspection => {
        const target = locate(inspection);
        if (!target) return undefined;
        // A literal edit needs the entry to still be a number; otherwise the text replaces it.
        return literal && NUMBER.test(target.text)
          ? { op: "setLiteral", offset: target.span.start, value: Number(value) }
          : { op: "reconnect", start: target.span.start, end: target.span.end, expression: value };
      });
    });
    row.append(input);
    return row;
  }

  private parameters(inspection: RecipeInspection): HTMLElement {
    const section = this.section("Parameters", inspection.parameters.length);
    for (const parameter of inspection.parameters) {
      const row = element("div", "recipe-parameter");
      row.dataset.recipeParameter = parameter.name;
      row.append(element("strong", "", parameter.name), element("small", "", [parameter.type, parameter.unit].filter(Boolean).join(" · ")));
      const input = element("input");
      input.type = "text"; input.value = parameter.defaultText; input.setAttribute("aria-label", `${parameter.name} default`);
      input.addEventListener("change", () => {
        try {
          const text = input.value.trim();
          const vector = /^vec[23]\(([^)]*)\)$/.exec(text);
          const value = vector ? vector[1].split(",").map(part => Number(part.trim())) : text === "true" || text === "false" ? text === "true" : Number(text);
          if (typeof value === "number" && !Number.isFinite(value)) throw new Error("A scalar default is a number.");
          void this.run({ op: "setParameterDefault", name: parameter.name, value });
        } catch (error) { this.fail(error instanceof Error ? error.message : String(error)); }
      });
      const range = element("input");
      range.type = "text"; range.placeholder = "min, max"; range.disabled = parameter.type === "bool";
      range.value = parameter.range ? `${parameter.range[0]}, ${parameter.range[1]}` : "";
      range.setAttribute("aria-label", `${parameter.name} range`);
      range.addEventListener("change", () => {
        const parts = range.value.split(",").map(part => part.trim()).filter(Boolean);
        if (!parts.length) void this.run({ op: "setParameterRange", name: parameter.name, range: null });
        else if (parts.length === 2 && parts.every(part => NUMBER.test(part))) void this.run({ op: "setParameterRange", name: parameter.name, range: [Number(parts[0]), Number(parts[1])] });
        else this.fail("A range is two numbers: min, max.");
      });
      row.append(input, range);
      const current = this.io.override(parameter.name);
      if (current !== undefined && JSON.stringify(current) !== undefined) {
        const commit = element("button", "quiet compact", "Use current as default");
        commit.addEventListener("click", () => {
          const current = this.io.override(parameter.name);
          if (typeof current === "number" || typeof current === "boolean" || (Array.isArray(current) && current.every(part => typeof part === "number")))
            void this.run({ op: "setParameterDefault", name: parameter.name, value: current as number | boolean | number[] });
        });
        row.append(commit);
      }
      section.append(row);
    }
    return section;
  }

  private layers(inspection: RecipeInspection): HTMLElement {
    const section = this.section("Layers", inspection.layers.length);
    inspection.layers.forEach((layer, index) => {
      const card = element("div", "recipe-layer");
      card.dataset.layer = layer.id;
      const header = element("div", "recipe-layer-header");
      header.append(element("strong", "", layer.id));
      const up = element("button", "quiet compact", "↑"); up.disabled = index === 0; up.setAttribute("aria-label", `Move ${layer.id} earlier`);
      up.addEventListener("click", () => void this.run({ op: "moveLayer", id: layer.id, to: index - 1 }));
      const down = element("button", "quiet compact", "↓"); down.disabled = index === inspection.layers.length - 1; down.setAttribute("aria-label", `Move ${layer.id} later`);
      down.addEventListener("click", () => void this.run({ op: "moveLayer", id: layer.id, to: index + 1 }));
      const remove = element("button", "quiet compact", "✕"); remove.setAttribute("aria-label", `Remove ${layer.id}`);
      remove.addEventListener("click", () => void this.run({ op: "removeLayer", id: layer.id }));
      header.append(up, down, remove);
      card.append(header);
      const same = (inspection: RecipeInspection) => inspection.layers.find(item => item.id === layer.id);
      if (layer.mask) card.append(this.entry("mask", layer.mask, inspection => same(inspection)?.mask));
      for (const channel of layer.channels)
        card.append(this.entry(channel.name, channel, inspection => same(inspection)?.channels.find(item => item.name === channel.name)));
      section.append(card);
    });
    const form = element("form", "recipe-add");
    const id = element("input"); id.type = "text"; id.placeholder = "layer id"; id.required = true; id.pattern = "[A-Za-z_][A-Za-z0-9_]*"; id.setAttribute("aria-label", "New layer id");
    const mask = element("input"); mask.type = "text"; mask.placeholder = "mask, e.g. 0.5 or a field"; mask.required = true; mask.setAttribute("aria-label", "New layer mask");
    form.append(id, mask, element("button", "compact", "Add layer"));
    form.addEventListener("submit", event => {
      event.preventDefault();
      void this.run({ op: "addLayer", source: `layer({ id: ${JSON.stringify(id.value)}, mask: ${mask.value} })` });
    });
    section.append(form);
    return section;
  }

  private groups(inspection: RecipeInspection): HTMLElement {
    const section = this.section("Groups", inspection.groups.length);
    for (const group of inspection.groups) {
      const card = element("div", "recipe-layer");
      card.dataset.group = group.name;
      card.append(element("strong", "", `${group.name} · ${group.group}`));
      card.append(element("small", "", `fields: ${group.outputs.map(output => `${group.name}.${output}`).join(", ")}`));
      for (const argument of group.arguments)
        card.append(this.entry(argument.name, argument, inspection =>
          inspection.groups.find(item => item.name === group.name)?.arguments.find(item => item.name === argument.name)));
      section.append(card);
    }
    return section;
  }

  /** Every use of a field can be reconnected on its own, leaving the other uses untouched. */
  private fields(inspection: RecipeInspection): HTMLElement {
    const section = this.section("Fields and uses");
    const choices = [
      ...inspection.fields.map(field => field.name),
      ...inspection.groups.flatMap(group => group.outputs.map(output => `${group.name}.${output}`)),
    ];
    const source = this.io.source();
    const place = (span: Span) => {
      const before = source.slice(0, span.start).split("\n");
      return `${before.length}:${before[before.length - 1].length + 1}`;
    };
    // Every use lists every choice: the list is made once and copied (a copy is one native call, where making
    // each option is several), then the use's own field is moved to the front.
    const list = element("select");
    for (const choice of choices) list.add(new Option(choice, choice));
    const position = new Map(choices.map((choice, index) => [choice, index] as const));
    for (const field of inspection.fields.filter(item => item.references.length)) {
      const card = element("div", "recipe-field");
      card.dataset.field = field.name;
      card.append(element("strong", "", field.name), element("small", "", `${field.valueText.length > 48 ? `${field.valueText.slice(0, 48)}…` : field.valueText} · ${field.references.length} use${field.references.length === 1 ? "" : "s"}`));
      field.references.forEach((use, index) => {
        const row = element("label", "recipe-use");
        row.append(element("span", "", `use ${index + 1} at ${place(use)}`));
        let select: HTMLSelectElement;
        if (position.size === choices.length) {
          select = list.cloneNode(true) as HTMLSelectElement;
          select.prepend(select.options[position.get(field.name)!]);
          select.selectedIndex = 0;
        } else {
          // (Two choices of one name: listed as before, the field first and then every other name.)
          select = element("select");
          select.add(new Option(field.name, field.name));
          for (const choice of choices.filter(name => name !== field.name)) select.add(new Option(choice, choice));
        }
        select.setAttribute("aria-label", `Reconnect ${field.name} use ${index + 1}`);
        select.addEventListener("change", () => {
          const expression = select.value;
          if (expression === field.name) return;
          void this.run(inspection => {
            const target = inspection.fields.find(item => item.name === field.name)?.references[index];
            return target && { op: "reconnect", start: target.start, end: target.end, expression };
          });
        });
        row.append(select);
        card.append(row);
      });
      section.append(card);
    }
    return section;
  }

  /** The operation catalog: searchable, with an example that inserts at the caret. */
  private reference(): HTMLElement {
    const section = this.section("SDK operations");
    const search = element("input");
    search.type = "search"; search.placeholder = "Search operations"; search.setAttribute("aria-label", "Search SDK operations");
    const list = element("div", "recipe-operations");
    const render = () => {
      if (!this.catalog) { list.textContent = "Loading catalog…"; return; }
      const query = search.value.trim().toLowerCase();
      const items: { id: string; summary: string; example: string }[] = [
        ...this.catalog.operations.map(operation => ({ ...operation, id: operation.id.replace(/_(\w)/g, (_, letter: string) => letter.toUpperCase()) })),
        ...this.catalog.operators, ...this.catalog.groups, ...this.catalog.rendererOperations,
      ];
      list.replaceChildren(...items.filter(item => !query || `${item.id} ${item.summary}`.toLowerCase().includes(query)).slice(0, 40).map(item => {
        const row = element("button", "quiet recipe-operation");
        row.type = "button"; row.title = item.summary;
        row.append(element("strong", "", item.id), element("small", "", item.example));
        row.addEventListener("click", () => this.io.insert(item.example));
        return row;
      }));
    };
    search.value = this.query;
    search.addEventListener("input", () => { this.query = search.value; render(); });
    render();
    if (!this.catalog) void this.tools.catalog().then(catalog => { this.catalog = catalog; render(); });
    const downloads = element("div", "button-pair");
    for (const [label, file, type] of [["Download SDK types", "index.d.ts", "text/plain"], ["Download AI reference", "ai-reference.md", "text/markdown"]] as const) {
      const button = element("button", "quiet compact", label);
      button.type = "button";
      button.addEventListener("click", () => void this.tools.sdk().then(sdk => download(file, sdk.files[file], type)));
      downloads.append(button);
    }
    section.append(search, list, downloads);
    return section;
  }
}
