import * as THREE from "three";
import { applyLiveOperations } from "./live.ts";
import type { MaterialShading } from "./shade.ts";
import { GpuBaker, gpuBakeUnsupported, type AtlasBakeTimings, type AtlasMaps, type FieldTexels } from "./bake.ts";
import { bakeGeometry, displayGeometry, type AtlasLayout, type AtlasProgramLayout } from "./atlas.ts";

/**
 * GPU-baked maps for the meshes of a shaded scene (the scene view's default look). Each mesh starts with the material
 * that evaluates its program per pixel (shade.ts): the first look, and the look while a parameter is being changed.
 * Once an edit has settled, the program is baked over the mesh's receiver atlas on the GPU (bake.ts, atlas.ts) and
 * the mesh is switched to an ordinary standard material with the baked maps, drawn with the atlas's own geometry.
 * Everything here is a PREVIEW of the CPU bake: nothing is saved or exported.
 *
 * Ownership and keys (what later stages build on):
 * - A bake is named by `GpuBaker.atlasKey`: the program, its uniform values and images, the receiver layout's content,
 *   the receiver's features and the CPU fields it read. Baked maps are shared by every mesh with the same key and kept
 *   for a while after their last user, so going back to a value, or rebuilding the scene for an edit to another
 *   material, shows them again without drawing.
 * - A mesh keeps both looks alive (two materials, two geometries), so switching between them compiles nothing.
 * - Only meshes whose material's values changed are baked again.
 */

type Uniforms = Record<string, THREE.IUniform>;

/** One mesh to bake: the program to bake (a hybrid plan's shader when the program has CPU-only parts) and its inputs. */
export interface SceneBakeTarget {
  mesh: THREE.Mesh;
  /** The material assignment the mesh shows. */
  material: string;
  shading: MaterialShading;
  /** The program's parameter, seed and image uniforms and the receiver's own; shared objects, so edits reach the bake. */
  uniforms: Uniforms;
  layout: AtlasLayout;
  /** Names the receiver inputs that are textures (its features), for the bake key. */
  receiver: string;
  /** Physical size of the UV square for `uv` mapping, meters. */
  extent: [number, number];
}

/** CPU-baked fields for hybrid programs, by mesh: one entry per requested name, on the mesh's atlas grid. Null when superseded. */
export type FieldSource = (requests: { mesh: string; names: string[] }[]) => Promise<Map<string, { width: number; height: number; fields: Float32Array[] }> | null>;

/** What the scene view gives the baker. */
export interface SceneBakeHost {
  baker(): GpuBaker;
  /** Prepares a standard material for the scene (shadow style, view settings) before maps and live operations go on. */
  style(material: THREE.MeshStandardMaterial): void;
  /** Compiles `material` as it will be drawn on `mesh` with `geometry`, without blocking the page. */
  compile(mesh: THREE.Mesh, geometry: THREE.BufferGeometry, material: THREE.Material): Promise<void>;
  /** Whether the mesh is still part of the displayed scene. */
  displayed(mesh: THREE.Mesh): boolean;
  /** Draws a frame now. */
  draw(): void;
  /** Resolves when the GPU has finished what was issued. */
  finished(): Promise<void>;
  report(status: SceneBakeStatus): void;
}

export interface SceneBakeStatus {
  /** `baking` while any mesh waits for maps; `baked` when every mesh that can be baked shows its maps. */
  state: "baking" | "baked";
  meshes: number;
  baked: number;
  /** Meshes that stay on the per-pixel look, with the reason. */
  unsupported: Record<string, string>;
  /** Milliseconds from the start of the run (the first look, or the settled edit) to the last map shown. */
  ms: number;
  /** Sums over the run's bakes, milliseconds: shader compiles, the evaluate and normals passes with mips, CPU field bakes. */
  compileMs: number;
  drawMs: number;
  fieldMs: number;
  /** Bakes drawn in the run, and bakes answered from maps already held. */
  drawn: number;
  reused: number;
  /** GPU memory of the maps on screen. */
  bytes: number;
  /** One line per bake drawn: mesh, material and its milliseconds (c compile, e evaluate, n normals, m mips). */
  details: string[];
  /** Atlas sizes by mesh. */
  atlases: Record<string, string>;
}

interface Shared { maps: AtlasMaps; users: number }

interface Entry {
  target: SceneBakeTarget;
  program: AtlasProgramLayout;
  shaderGeometry: THREE.BufferGeometry;
  shaderMaterial: THREE.Material;
  display?: THREE.BufferGeometry;
  bake?: THREE.BufferGeometry;
  material?: THREE.MeshStandardMaterial;
  shared?: Shared;
  /** Edits asked for and edits baked: equal when the maps are up to date. */
  wanted: number;
  done: number;
  showing: "shader" | "baked";
  unsupported?: string;
  fields?: Record<string, FieldTexels>;
  fieldsKey?: string;
}

/** Pause after the last parameter change before its materials are baked again. */
const SETTLE_MS = 160;
/** GPU memory the maps on screen may take; meshes beyond it stay on the per-pixel look. */
const SCENE_BAKE_BYTES = 512 * 1024 * 1024;
/** Map sets kept after their last user (least recently used dropped first), and the memory they may take together. */
const KEPT_IDLE_MAPS = 16;
const IDLE_BAKE_BYTES = 128 * 1024 * 1024;

export class SceneBaker {
  private entries: Entry[] = [];
  /** Complete maps by bake key: those on screen and those kept idle. */
  private readonly byKey = new Map<string, Shared>();
  /** Keys of maps without a user, least recently used first. */
  private readonly idle = new Set<string>();
  private fieldSource?: FieldSource;
  private fieldsInFlight = false;
  private fieldRetries = 0;
  private run = 0;
  private pumping = false;
  private settleAt = 0;
  private timer?: ReturnType<typeof setTimeout>;
  private started = 0;
  private totals = { compileMs: 0, drawMs: 0, fieldMs: 0, drawn: 0, reused: 0 };
  private closed = false;

  constructor(private readonly host: SceneBakeHost) {}

  /** Meshes registered for baking that stay on the per-pixel look (the caller may refine their relief instead). */
  unbaked(): THREE.Mesh[] { return this.entries.filter(entry => entry.unsupported).map(entry => entry.target.mesh); }

  /**
   * Starts baking a newly shaded scene: `targets` are its meshes, each showing its per-pixel material. Looks and maps of
   * the scene before it are released first; maps whose bake key comes up again are shown without drawing.
   */
  begin(targets: SceneBakeTarget[], fields?: FieldSource): void {
    this.reset();
    this.fieldSource = fields;
    const baker = this.host.baker();
    for (const target of targets) {
      const program = baker.atlasProgram(target.shading);
      const entry: Entry = { target, program, shaderGeometry: target.mesh.geometry, shaderMaterial: target.mesh.material as THREE.Material,
        wanted: 1, done: 0, showing: "shader" };
      entry.unsupported = baker.unavailable() ?? gpuBakeUnsupported(target.shading, true) ?? program.unsupported
        ?? (target.shading.shader.cpuFields?.length && !fields ? "the program needs CPU-baked fields" : undefined);
      // The geometry the scene's packet gave the mesh, whichever look it shows.
      target.mesh.userData.sourceGeometry = entry.shaderGeometry;
      this.entries.push(entry);
    }
    this.restart();
    this.report();
    // The first look reaches the screen before anything competes with it.
    const run = this.run;
    void this.host.finished().then(() => new Promise(done => requestAnimationFrame(done))).then(() => { if (run === this.run) void this.pump(); });
  }

  /** Per-bake timings of the run (compile, evaluate, normals, mips), for diagnostics. */
  private details: string[] = [];

  private restart(): void {
    this.details = [];
    this.started = performance.now();
    this.totals = { compileMs: 0, drawMs: 0, fieldMs: 0, drawn: 0, reused: 0 };
  }

  /**
   * A material's values changed: its meshes go back to the per-pixel look at once (which follows the change through its
   * uniforms) and are baked again when no further change arrives for a moment.
   */
  invalidate(material: string): void {
    let any = false;
    for (const entry of this.entries) {
      if (entry.target.material !== material) continue;
      entry.wanted++;
      this.showShader(entry);
      any = true;
    }
    if (!any) return;
    if (this.entries.every(entry => entry.unsupported || entry.done === entry.wanted || entry.target.material === material)) this.restart();
    this.settleAt = performance.now() + SETTLE_MS;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.pump(), SETTLE_MS + 1);
    this.report();
    this.host.draw();
  }

  /** Bakes every mesh again (after a restored context). */
  rebake(): void {
    for (const entry of this.entries) { entry.wanted++; if (entry.unsupported === "the WebGL context is lost") entry.unsupported = undefined; }
    this.restart();
    void this.pump();
  }

  /** The context went away with every map: meshes show their per-pixel look and nothing baked is kept. */
  lost(): void {
    for (const entry of this.entries) { this.showShader(entry); entry.shared = undefined; entry.done = 0; }
    this.byKey.clear(); this.idle.clear();
    this.run++;
    this.report();
  }

  /** Puts every mesh back on its per-pixel look and releases the baked looks; complete maps stay for a later scene. */
  reset(): void {
    this.run++;
    clearTimeout(this.timer);
    for (const entry of this.entries) {
      this.showShader(entry);
      this.release(entry);
      entry.material?.dispose(); entry.display?.dispose(); entry.bake?.dispose();
      delete entry.target.mesh.userData.sourceGeometry;
    }
    this.entries = [];
    this.fieldsInFlight = false;
  }

  dispose(): void {
    this.closed = true;
    this.reset();
    const baker = this.host.baker();
    for (const shared of this.byKey.values()) baker.disposeAtlasMaps(shared.maps);
    this.byKey.clear(); this.idle.clear();
  }

  private showShader(entry: Entry): void {
    if (entry.showing === "shader") return;
    const mesh = entry.target.mesh;
    mesh.geometry = entry.shaderGeometry;
    mesh.material = entry.shaderMaterial;
    mesh.userData.liveOperations = entry.shaderMaterial.userData.liveOperations;
    entry.showing = "shader";
  }

  /** Gives up an entry's maps: complete ones wait for another user, others are freed. */
  private release(entry: Entry): void {
    const shared = entry.shared;
    if (!shared) return;
    entry.shared = undefined;
    if (--shared.users > 0) return;
    const baker = this.host.baker();
    if (!shared.maps.key || this.byKey.get(shared.maps.key) !== shared || !baker.atlasMapsAlive(shared.maps)) { baker.disposeAtlasMaps(shared.maps); return; }
    this.idle.add(shared.maps.key);
    let bytes = 0;
    for (const key of this.idle) bytes += this.byKey.get(key)!.maps.bytes;
    for (const key of [...this.idle]) {
      if (this.idle.size <= KEPT_IDLE_MAPS && bytes <= IDLE_BAKE_BYTES) break;
      const dropped = this.byKey.get(key)!;
      bytes -= dropped.maps.bytes;
      baker.disposeAtlasMaps(dropped.maps);
      this.byKey.delete(key); this.idle.delete(key);
    }
  }

  private bytesShown(): number {
    let bytes = 0;
    for (const shared of new Set(this.entries.map(entry => entry.shared))) if (shared) bytes += shared.maps.bytes;
    return bytes;
  }

  private report(): void {
    const pending = this.entries.filter(entry => !entry.unsupported && (entry.done !== entry.wanted || entry.showing !== "baked"));
    this.host.report({
      state: pending.length ? "baking" : "baked", meshes: this.entries.length,
      baked: this.entries.filter(entry => entry.showing === "baked").length,
      unsupported: Object.fromEntries(this.entries.filter(entry => entry.unsupported).map(entry => [entry.target.mesh.name, entry.unsupported!])),
      ms: Math.round(performance.now() - this.started), ...this.totals,
      compileMs: Math.round(this.totals.compileMs), drawMs: +this.totals.drawMs.toFixed(1), fieldMs: Math.round(this.totals.fieldMs),
      bytes: this.bytesShown(), details: this.details,
      atlases: Object.fromEntries(this.entries.map(entry => [entry.target.mesh.name, `${entry.target.layout.width}x${entry.target.layout.height}`])),
    });
  }

  /** What an entry's CPU fields depend on: the values of the parameters they read (seed, source and layout are the run's). */
  private fieldsKey(entry: Entry): string {
    const { shading, uniforms } = entry.target;
    const names = new Set((shading.shader.cpuFields ?? []).flatMap(field => field.parameters));
    return JSON.stringify(shading.shader.parameters.filter(parameter => names.has(parameter.name)).map(parameter => [parameter.name, uniforms[parameter.uniform]?.value]));
  }

  /** Asks the CPU for the fields of every hybrid entry that lacks current ones; the entries are baked when they arrive. */
  private async fetchFields(run: number): Promise<void> {
    if (this.fieldsInFlight || !this.fieldSource) return;
    const needy = this.entries.filter(entry => !entry.unsupported && entry.target.shading.shader.cpuFields?.length && entry.fieldsKey !== this.fieldsKey(entry));
    if (!needy.length) return;
    this.fieldsInFlight = true;
    const keys = new Map(needy.map(entry => [entry, this.fieldsKey(entry)]));
    const started = performance.now();
    try {
      const baked = await this.fieldSource(needy.map(entry => ({ mesh: entry.target.mesh.name, names: entry.target.shading.shader.cpuFields!.map(field => field.name) })));
      if (run !== this.run) return;
      if (!baked) {
        // Superseded by a newer request of the page; asked again, a few times at most.
        if (++this.fieldRetries > 3) for (const entry of needy) entry.unsupported = "the CPU field bake was cancelled";
        return;
      }
      this.fieldRetries = 0;
      this.totals.fieldMs += performance.now() - started;
      for (const entry of needy) {
        const found = baked.get(entry.target.mesh.name);
        const wanted = entry.target.shading.shader.cpuFields!;
        const { width, height } = entry.target.layout;
        if (!found || found.fields.length !== wanted.length || found.width !== width || found.height !== height) {
          entry.unsupported = "the CPU field bake did not match the receiver's atlas";
          continue;
        }
        const key = keys.get(entry)!;
        entry.fields = Object.fromEntries(wanted.map((field, index) => [field.uniform,
          { id: `${entry.target.layout.key}:${entry.target.mesh.name}:${field.name}:${key}`, width, height, rgba: found.fields[index] }]));
        entry.fieldsKey = key;
      }
    } catch (error) {
      if (run !== this.run) return;
      for (const entry of needy) entry.unsupported = `the CPU field bake failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      if (run === this.run) { this.fieldsInFlight = false; void this.pump(); }
    }
  }

  /** Bakes what is out of date, one mesh at a time, until nothing is. */
  private async pump(): Promise<void> {
    if (this.pumping || this.closed) return;
    this.pumping = true;
    const run = this.run;
    const baker = this.host.baker();
    try {
      for (;;) {
        if (run !== this.run) return;
        // A change still settling is not baked: its next value would only bake again.
        if (performance.now() < this.settleAt) return;
        void this.fetchFields(run);
        const ready = (entry: Entry) => !entry.unsupported && entry.done !== entry.wanted
          && (!entry.target.shading.shader.cpuFields?.length || entry.fieldsKey === this.fieldsKey(entry));
        const entry = this.entries.find(ready);
        if (!entry) return;
        const version = entry.wanted;
        try {
          await this.bake(entry, () => run === this.run && version === entry.wanted && this.host.displayed(entry.target.mesh));
        } catch (error) {
          if (run !== this.run) return;
          entry.unsupported = error instanceof Error ? error.message : String(error);
          this.showShader(entry);
        }
        if (run !== this.run) return;
        // Nothing changed and still not shown (the context went away under it): it is not tried in a loop.
        if (version === entry.wanted && entry.done !== version && !entry.unsupported) {
          entry.unsupported = baker.unavailable() ?? "the bake was interrupted";
          this.showShader(entry);
        }
        this.report();
      }
    } finally {
      this.pumping = false;
      if (run === this.run) this.report();
    }
  }

  private async bake(entry: Entry, current: () => boolean): Promise<void> {
    const baker = this.host.baker();
    const { target, program } = entry;
    const version = entry.wanted;
    const fieldIds = (target.shading.shader.cpuFields ?? []).map(field => `${field.name}=${entry.fields?.[field.uniform]?.id}`);
    const key = GpuBaker.atlasKey(target.shading, target.uniforms, target.layout, program, fieldIds, target.receiver);
    const held = this.byKey.get(key);
    if (held && baker.atlasMapsAlive(held.maps)) {
      // These exact maps exist: on this mesh already, on another mesh, or kept from before.
      if (entry.shared !== held) { this.release(entry); held.users++; this.idle.delete(key); entry.shared = held; }
      this.totals.reused++;
    } else {
      // Maps only this mesh shows are drawn over; shared or missing ones are replaced.
      let shared = entry.shared;
      if (shared && (shared.users > 1 || shared.maps.layout.key !== program.key || !baker.atlasMapsAlive(shared.maps))) { this.release(entry); shared = undefined; }
      if (shared) { if (this.byKey.get(shared.maps.key) === shared) this.byKey.delete(shared.maps.key); }
      else {
        const maps = baker.createAtlasMaps(target.layout.width, target.layout.height, program);
        if (this.bytesShown() + maps.bytes > SCENE_BAKE_BYTES) {
          baker.disposeAtlasMaps(maps);
          throw new Error("the scene's baked maps would exceed the preview's memory allowance");
        }
        shared = entry.shared = { maps, users: 1 };
      }
      entry.bake ??= bakeGeometry(target.layout);
      this.showShader(entry);
      const timings: AtlasBakeTimings | null = await baker.bakeAtlas(target.shading, target.uniforms,
        { layout: target.layout, geometry: entry.bake, extent: target.extent, receiver: target.receiver }, shared.maps, { fields: entry.fields, current });
      if (!timings) return;
      this.totals.compileMs += timings.compile; this.totals.drawMs += timings.total - timings.compile; this.totals.drawn++;
      this.details.push(`${target.mesh.name.replace(/^.*node\/(\d+).*$/, "$1")}:${target.material} c${Math.round(timings.compile)} e${timings.evaluate.toFixed(1)} n${timings.normals.toFixed(1)} m${timings.mips.toFixed(1)}`);
      // A value that changed while the bands were drawn is in some texels and not others.
      if (shared.maps.key !== key) { shared.maps.key = ""; return; }
      this.byKey.set(key, shared);
    }
    if (!current()) return;
    await this.show(entry, current);
    if (version === entry.wanted && entry.showing === "baked") entry.done = version;
  }

  /** Switches a mesh to the standard material with its baked maps (compiled first, so the switch does not stall a frame). */
  private async show(entry: Entry, current: () => boolean): Promise<void> {
    const maps = entry.shared!.maps;
    const mesh = entry.target.mesh;
    let material = entry.material;
    const fresh = !material;
    if (!material) {
      material = new THREE.MeshStandardMaterial({ color: 0xffffff });
      this.host.style(material);
      material.side = THREE.DoubleSide;
      material.roughness = 1; material.metalness = 1;
    }
    material.map = maps.baseColor;
    material.normalMap = maps.normal;
    material.roughnessMap = material.metalnessMap = maps.properties;
    material.emissiveMap = maps.emissive ?? null;
    material.emissive.set(maps.emissive ? 0xffffff : 0x000000);
    if (fresh) {
      // Arguments computed from parameters alone are evaluated in the material's shader, from the same uniform objects
      // the program's materials share: they follow a parameter with no bake.
      const parameters = entry.program.liveParameters;
      if (parameters.length) {
        const previous = material.onBeforeCompile, uniforms = entry.target.uniforms;
        const declarations = parameters.map(parameter => `uniform ${parameter.type} ${parameter.uniform};`).join("\n");
        material.onBeforeCompile = (shader, renderer) => {
          previous.call(material, shader, renderer);
          for (const parameter of parameters) shader.uniforms[parameter.uniform] = uniforms[parameter.uniform];
          shader.fragmentShader = shader.fragmentShader.replace("#include <common>", `#include <common>\n${declarations}`);
        };
      }
      // Live renderer operations run in the material's shader on the bake's own argument textures.
      material.userData.liveOperations = applyLiveOperations(material, entry.program.liveOperations as never, id => maps.live[id]);
      // The operations' own key does not name GLSL arguments; the program layout's does. (Materials without operations
      // share one program whatever they were baked from.)
      if (entry.program.liveOperations.length) {
        const key = material.customProgramCacheKey;
        material.customProgramCacheKey = () => `${key.call(material)}|atlas:${entry.program.key}`;
      }
    } else if (material.userData.liveUniforms) {
      const uniforms = material.userData.liveUniforms as Uniforms;
      for (const [id, slot] of material.userData.liveTextureSlots as Map<string, number>) {
        const uniform = uniforms[`msLiveTex${slot}Map`];
        if (uniform) uniform.value = maps.live[id];
      }
    }
    entry.material = material;
    entry.display ??= displayGeometry(entry.target.layout);
    if (fresh) {
      try { await this.host.compile(mesh, entry.display, material); } catch { /* compiled at the first draw instead */ }
      if (!current()) return;
    }
    mesh.geometry = entry.display;
    mesh.material = material;
    mesh.userData.liveOperations = material.userData.liveOperations;
    entry.showing = "baked";
    this.host.draw();
  }
}
