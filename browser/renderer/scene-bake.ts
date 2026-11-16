import * as THREE from "three";
import { applyLiveOperations } from "./live.ts";
import type { MaterialShading } from "./shade.ts";
import { GpuBaker, gpuBakeUnsupported, type AtlasBakeTimings, type AtlasMaps, type FieldTexels } from "./bake.ts";
import { bakeGeometry, displayGeometry, type AtlasLayout, type AtlasProgramLayout } from "./atlas.ts";

/**
 * GPU-baked maps for the meshes of a shaded scene (the scene view's default look). Each mesh has two looks: the
 * material that evaluates its program per pixel (shade.ts), and an ordinary standard material with maps baked from the
 * same program over the mesh's receiver atlas on the GPU (bake.ts, atlas.ts), drawn with the atlas's own geometry.
 * The baked look is what a settled scene shows; the per-pixel look is shown while a value is being changed, and first
 * when it is ready first. Everything here is a PREVIEW of the CPU bake: nothing is saved or exported.
 *
 * A scene is prepared before it is displayed (`prepare`): its programs compile, its CPU fields are baked and its maps
 * drawn while the scene before it stays on screen; `commit` then puts the looks on the meshes.
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
  /** The mesh's per-pixel look; the mesh shows it, with the geometry it has when the scene is committed, until its maps do. */
  shader: THREE.Material;
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
  /** Compiles materials as they will be drawn (each on its mesh's shadow settings, with a geometry), without blocking the page. */
  compile(items: { mesh: THREE.Mesh; geometry: THREE.BufferGeometry; material: THREE.Material }[]): Promise<void>;
  /** Whether the mesh is still part of the displayed scene. */
  displayed(mesh: THREE.Mesh): boolean;
  /** Draws a frame. */
  draw(): void;
  report(status: SceneBakeStatus): void;
}

export interface SceneBakeStatus {
  /** `baking` while any mesh waits for maps; `baked` when every mesh that can be baked shows its maps. */
  state: "baking" | "baked";
  meshes: number;
  baked: number;
  /** Meshes that stay on the per-pixel look, with the reason. */
  unsupported: Record<string, string>;
  /** Milliseconds from the start of the run (the scene's preparation, or the changed value) to the last map shown. */
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
  /** Whether the per-pixel looks are compiled, so a changed value shows per pixel at once (it is baked on settle either way). */
  shaderReady: boolean;
  /** One line per bake drawn: mesh, material and its milliseconds (c compile, e evaluate, n normals, m mips). */
  details: string[];
  /** Atlas sizes by mesh. */
  atlases: Record<string, string>;
}

/** A scene being prepared (`SceneBaker.prepare`). */
export interface PreparedScene {
  /** Resolves when every mesh has its baked look ready or is known not to get one (also when the scene is given up). */
  ready: Promise<void>;
  /** Meshes whose baked look is not ready (yet, or ever). */
  unprepared(): THREE.Mesh[];
  /**
   * Displays the scene in place of the one before it: every mesh must already carry its per-pixel look
   * (`SceneBakeTarget.shader`); meshes whose maps are ready show them at once, the others when they are.
   * `shaderReady` says whether the per-pixel looks are compiled (see `SceneBaker.shadersReady`). Nothing happens if a
   * newer scene has been prepared since.
   */
  commit(shaderReady: boolean): void;
  /** Gives the scene up, if it was never committed. */
  discard(): void;
}

interface Shared { maps: AtlasMaps; users: number }

interface Entry {
  target: SceneBakeTarget;
  program: AtlasProgramLayout;
  shaderGeometry: THREE.BufferGeometry;
  display?: THREE.BufferGeometry;
  bake?: THREE.BufferGeometry;
  /** The standard material of the baked look; its maps are placeholders until the first bake. */
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

/** The meshes of one shaded scene: prepared, then displayed until the next scene replaces them. */
interface Run {
  entries: Entry[];
  fields?: FieldSource;
  fieldsInFlight: boolean;
  fieldRetries: number;
  /** Whether the scene is the displayed one: only then do its looks go on its meshes. */
  attached: boolean;
  /** Whether the per-pixel looks are compiled. */
  shaderReady: boolean;
  closed: boolean;
  pumping: boolean;
  settleAt: number;
  timer?: ReturnType<typeof setTimeout>;
  started: number;
  totals: { compileMs: number; drawMs: number; fieldMs: number; drawn: number; reused: number };
  details: string[];
  /** Compiles of the baked looks' materials still under way. */
  materials: Promise<void>;
  /** Called whenever nothing is left to do for now. */
  idle?: () => void;
}

/** Pause after the last parameter change before its materials are baked again. */
const SETTLE_MS = 160;
/** GPU memory the maps on screen may take; meshes beyond it stay on the per-pixel look. */
const SCENE_BAKE_BYTES = 512 * 1024 * 1024;
/** Map sets kept after their last user (least recently used dropped first), and the memory they may take together. */
const KEPT_IDLE_MAPS = 16;
const IDLE_BAKE_BYTES = 128 * 1024 * 1024;

export class SceneBaker {
  private current?: Run;
  private pending?: Run;
  /** Complete maps by bake key: those on screen and those kept idle. */
  private readonly byKey = new Map<string, Shared>();
  /** Keys of maps without a user, least recently used first. */
  private readonly idle = new Set<string>();
  /** Stand-ins for a baked look's maps while its material compiles, by color role. */
  private placeholders?: { color: THREE.Texture; data: THREE.Texture };
  private closed = false;

  constructor(private readonly host: SceneBakeHost) {}

  /** Meshes of the displayed scene that stay on the per-pixel look (the caller may refine their relief instead). */
  unbaked(): THREE.Mesh[] { return (this.current?.entries ?? []).filter(entry => entry.unsupported).map(entry => entry.target.mesh); }

  /**
   * Starts preparing a newly shaded scene, which is not displayed yet: bake programs and baked-look materials compile,
   * CPU fields are asked for and maps are drawn as each becomes possible. `ready` resolves when every mesh has its
   * baked look ready or is known not to get one; the scene may be committed before that (the rest follows on screen).
   * A scene prepared earlier and never committed is given up.
   */
  prepare(targets: SceneBakeTarget[], fields?: FieldSource): PreparedScene {
    this.discard();
    const baker = this.host.baker();
    const run: Run = { entries: [], fields, fieldsInFlight: false, fieldRetries: 0, attached: false, shaderReady: false, closed: false, pumping: false,
      settleAt: 0, started: performance.now(), totals: { compileMs: 0, drawMs: 0, fieldMs: 0, drawn: 0, reused: 0 }, details: [], materials: Promise.resolve() };
    for (const target of targets) {
      const program = baker.atlasProgram(target.shading);
      // A mesh kept from the scene on screen may be showing that scene's baked look: its own geometry is beside it.
      const shaderGeometry = (target.mesh.userData.sourceGeometry as THREE.BufferGeometry | undefined) ?? target.mesh.geometry;
      const entry: Entry = { target, program, shaderGeometry, wanted: 1, done: 0, showing: "shader" };
      entry.unsupported = baker.unavailable() ?? gpuBakeUnsupported(target.shading, true) ?? program.unsupported
        ?? (target.shading.shader.cpuFields?.length && !fields ? "the program needs CPU-baked fields" : undefined);
      run.entries.push(entry);
    }
    this.pending = run;
    // The baked looks' materials compile beside the bake programs, on stand-in maps: the first bake only swaps textures in.
    const fresh = run.entries.filter(entry => !entry.unsupported);
    for (const entry of fresh) { entry.material = this.material(entry); entry.display = displayGeometry(entry.target.layout); }
    run.materials = this.host.compile(fresh.map(entry => ({ mesh: entry.target.mesh, geometry: entry.display!, material: entry.material! }))).catch(() => {});
    const settled = new Promise<void>(resolve => {
      run.idle = () => {
        if (run.closed || run.entries.every(entry => entry.unsupported || entry.done === entry.wanted)) resolve();
      };
    });
    void this.pump(run);
    return {
      ready: settled,
      unprepared: () => run.entries.filter(entry => entry.done !== entry.wanted).map(entry => entry.target.mesh),
      commit: shaderReady => this.commit(run, shaderReady),
      // Only this scene: a newer one being prepared is not this one's to give up.
      discard: () => { if (this.pending === run) this.discard(); },
    };
  }

  /** Gives up the scene being prepared. */
  private discard(): void {
    const run = this.pending;
    if (!run) return;
    this.pending = undefined;
    this.close(run);
  }

  private commit(run: Run, shaderReady: boolean): void {
    if (this.pending !== run || run.closed) return;
    this.reset();
    this.pending = undefined;
    this.current = run;
    run.attached = true;
    run.shaderReady = shaderReady;
    for (const entry of run.entries) {
      // The geometry the scene's packet gave the mesh, whichever look it shows.
      entry.target.mesh.userData.sourceGeometry = entry.shaderGeometry;
      if (!entry.unsupported && entry.done === entry.wanted) this.display(entry);
    }
    this.report(run);
    void this.pump(run);
  }

  /** The per-pixel looks of the displayed scene are compiled: a changed value shows per pixel from now on. */
  shadersReady(): void {
    if (!this.current) return;
    this.current.shaderReady = true;
    this.report(this.current);
  }

  /**
   * A material's values changed: its meshes go back to the per-pixel look at once (which follows the change through its
   * uniforms; while those looks still compile, the maps stay) and are baked again when no further change arrives for a
   * moment.
   */
  invalidate(material: string): void {
    const run = this.current;
    if (!run) return;
    let any = false;
    for (const entry of run.entries) {
      if (entry.target.material !== material) continue;
      entry.wanted++;
      if (run.shaderReady) this.showShader(run, entry);
      any = true;
    }
    if (!any) return;
    if (run.entries.every(entry => entry.unsupported || entry.done === entry.wanted || entry.target.material === material)) this.restart(run);
    run.settleAt = performance.now() + SETTLE_MS;
    clearTimeout(run.timer);
    run.timer = setTimeout(() => void this.pump(run), SETTLE_MS + 1);
    this.report(run);
    this.host.draw();
  }

  private restart(run: Run): void {
    run.details = [];
    run.started = performance.now();
    run.totals = { compileMs: 0, drawMs: 0, fieldMs: 0, drawn: 0, reused: 0 };
  }

  /** Bakes every mesh again (after a restored context). */
  rebake(): void {
    const run = this.current;
    if (!run) return;
    for (const entry of run.entries) { entry.wanted++; if (entry.unsupported === "the WebGL context is lost") entry.unsupported = undefined; }
    this.restart(run);
    void this.pump(run);
  }

  /** The context went away with every map: meshes show their per-pixel look and nothing baked is kept. */
  lost(): void {
    this.discard();
    const run = this.current;
    this.byKey.clear(); this.idle.clear();
    this.placeholders = undefined;
    if (!run) return;
    for (const entry of run.entries) { this.showShader(run, entry); entry.shared = undefined; entry.done = 0; }
    this.report(run);
  }

  /** Puts every displayed mesh back on its per-pixel look and releases the baked looks; complete maps stay for a later scene. */
  reset(): void {
    const run = this.current;
    if (!run) return;
    this.current = undefined;
    for (const entry of run.entries) {
      this.showShader(run, entry);
      delete entry.target.mesh.userData.sourceGeometry;
    }
    this.close(run);
  }

  private close(run: Run): void {
    run.closed = true;
    clearTimeout(run.timer);
    for (const entry of run.entries) {
      this.release(entry);
      entry.material?.dispose(); entry.display?.dispose(); entry.bake?.dispose();
    }
    run.idle?.();
  }

  dispose(): void {
    this.closed = true;
    this.discard();
    this.reset();
    const baker = this.host.baker();
    for (const shared of this.byKey.values()) baker.disposeAtlasMaps(shared.maps);
    this.byKey.clear(); this.idle.clear();
    this.placeholders?.color.dispose(); this.placeholders?.data.dispose();
  }

  private showShader(run: Run, entry: Entry): void {
    if (entry.showing === "shader") return;
    entry.showing = "shader";
    if (!run.attached) return;
    const mesh = entry.target.mesh;
    mesh.geometry = entry.shaderGeometry;
    mesh.material = entry.target.shader;
    mesh.userData.liveOperations = entry.target.shader.userData.liveOperations;
  }

  /** Puts an entry's baked look on its mesh. */
  private display(entry: Entry): void {
    const mesh = entry.target.mesh;
    mesh.geometry = entry.display!;
    mesh.material = entry.material!;
    mesh.userData.liveOperations = entry.material!.userData.liveOperations;
    entry.showing = "baked";
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

  private bytesHeld(run: Run): number {
    let bytes = 0;
    for (const shared of new Set(run.entries.map(entry => entry.shared))) if (shared) bytes += shared.maps.bytes;
    return bytes;
  }

  private report(run: Run): void {
    if (run !== this.current) return;
    const pending = run.entries.filter(entry => !entry.unsupported && (entry.done !== entry.wanted || entry.showing !== "baked"));
    this.host.report({
      state: pending.length ? "baking" : "baked", meshes: run.entries.length,
      baked: run.entries.filter(entry => entry.showing === "baked").length,
      unsupported: Object.fromEntries(run.entries.filter(entry => entry.unsupported).map(entry => [entry.target.mesh.name, entry.unsupported!])),
      ms: Math.round(performance.now() - run.started), ...run.totals,
      compileMs: Math.round(run.totals.compileMs), drawMs: +run.totals.drawMs.toFixed(1), fieldMs: Math.round(run.totals.fieldMs),
      bytes: this.bytesHeld(run), shaderReady: run.shaderReady, details: run.details,
      atlases: Object.fromEntries(run.entries.map(entry => [entry.target.mesh.name, `${entry.target.layout.width}x${entry.target.layout.height}`])),
    });
  }

  /** What an entry's CPU fields depend on: the values of the parameters they read (seed, source and layout are the run's). */
  private fieldsKey(entry: Entry): string {
    const { shading, uniforms } = entry.target;
    const names = new Set((shading.shader.cpuFields ?? []).flatMap(field => field.parameters));
    return JSON.stringify(shading.shader.parameters.filter(parameter => names.has(parameter.name)).map(parameter => [parameter.name, uniforms[parameter.uniform]?.value]));
  }

  /** Asks the CPU for the fields of every hybrid entry that lacks current ones; the entries are baked when they arrive. */
  private async fetchFields(run: Run): Promise<void> {
    if (run.fieldsInFlight || !run.fields) return;
    const needy = run.entries.filter(entry => !entry.unsupported && entry.target.shading.shader.cpuFields?.length && entry.fieldsKey !== this.fieldsKey(entry));
    if (!needy.length) return;
    run.fieldsInFlight = true;
    const keys = new Map(needy.map(entry => [entry, this.fieldsKey(entry)]));
    const started = performance.now();
    try {
      const baked = await run.fields(needy.map(entry => ({ mesh: entry.target.mesh.name, names: entry.target.shading.shader.cpuFields!.map(field => field.name) })));
      if (run.closed) return;
      if (!baked) {
        // Superseded by a newer request of the page; asked again, a few times at most.
        if (++run.fieldRetries > 3) for (const entry of needy) entry.unsupported = "the CPU field bake was cancelled";
        return;
      }
      run.fieldRetries = 0;
      run.totals.fieldMs += performance.now() - started;
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
      if (run.closed) return;
      for (const entry of needy) entry.unsupported = `the CPU field bake failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      if (!run.closed) { run.fieldsInFlight = false; void this.pump(run); }
    }
  }

  /** Bakes what is out of date, one mesh at a time, until nothing is. */
  private async pump(run: Run): Promise<void> {
    if (run.pumping || run.closed || this.closed) return;
    run.pumping = true;
    const baker = this.host.baker();
    try {
      for (;;) {
        if (run.closed) return;
        // A change still settling is not baked: its next value would only bake again.
        if (performance.now() < run.settleAt) return;
        void this.fetchFields(run);
        const ready = (entry: Entry) => !entry.unsupported && entry.done !== entry.wanted
          && (!entry.target.shading.shader.cpuFields?.length || entry.fieldsKey === this.fieldsKey(entry));
        const entry = run.entries.find(ready);
        if (!entry) return;
        const version = entry.wanted;
        try {
          await this.bake(run, entry, () => !run.closed && version === entry.wanted && (!run.attached || this.host.displayed(entry.target.mesh)));
        } catch (error) {
          if (run.closed) return;
          entry.unsupported = error instanceof Error ? error.message : String(error);
          this.showShader(run, entry);
        }
        if (run.closed) return;
        // Nothing changed and still not shown (the context went away under it): it is not tried in a loop.
        if (version === entry.wanted && entry.done !== version && !entry.unsupported) {
          entry.unsupported = baker.unavailable() ?? "the bake was interrupted";
          this.showShader(run, entry);
        }
        this.report(run);
      }
    } finally {
      run.pumping = false;
      if (!run.closed) { this.report(run); if (!run.fieldsInFlight) run.idle?.(); }
    }
  }

  private async bake(run: Run, entry: Entry, current: () => boolean): Promise<void> {
    const baker = this.host.baker();
    const { target, program } = entry;
    const version = entry.wanted;
    const fieldIds = (target.shading.shader.cpuFields ?? []).map(field => `${field.name}=${entry.fields?.[field.uniform]?.id}`);
    const key = GpuBaker.atlasKey(target.shading, target.uniforms, target.layout, program, fieldIds, target.receiver);
    const held = this.byKey.get(key);
    if (held && baker.atlasMapsAlive(held.maps)) {
      // These exact maps exist: on this mesh already, on another mesh, or kept from before.
      if (entry.shared !== held) { this.release(entry); held.users++; this.idle.delete(key); entry.shared = held; }
      run.totals.reused++;
    } else {
      // Maps only this mesh holds are drawn over, unless they are on screen with no per-pixel look to show meanwhile;
      // shared or missing ones are replaced.
      let shared = entry.shared;
      const onScreen = run.attached && entry.showing === "baked" && !run.shaderReady;
      if (shared && (shared.users > 1 || onScreen || shared.maps.layout.key !== program.key || !baker.atlasMapsAlive(shared.maps))) shared = undefined;
      if (shared) { if (this.byKey.get(shared.maps.key) === shared) this.byKey.delete(shared.maps.key); }
      else {
        const maps = baker.createAtlasMaps(target.layout.width, target.layout.height, program);
        if (this.bytesHeld(run) + maps.bytes > SCENE_BAKE_BYTES) {
          baker.disposeAtlasMaps(maps);
          throw new Error("the scene's baked maps would exceed the preview's memory allowance");
        }
        shared = { maps, users: 0 };
      }
      entry.bake ??= bakeGeometry(target.layout);
      if (shared === entry.shared) this.showShader(run, entry);
      let timings: AtlasBakeTimings | null = null;
      try {
        timings = await baker.bakeAtlas(target.shading, target.uniforms,
          { layout: target.layout, geometry: entry.bake, extent: target.extent, receiver: target.receiver }, shared.maps, { fields: entry.fields, current });
      } finally {
        // New maps take the place of the entry's old ones only when they are complete.
        if (shared !== entry.shared) {
          if (timings && shared.maps.key === key && !run.closed) { this.release(entry); shared.users = 1; entry.shared = shared; }
          else baker.disposeAtlasMaps(shared.maps);
        }
      }
      if (!timings || run.closed) return;
      run.totals.compileMs += timings.compile; run.totals.drawMs += timings.total - timings.compile; run.totals.drawn++;
      run.details.push(`${target.mesh.name.replace(/^.*node\/(\d+).*$/, "$1")}:${target.material} c${Math.round(timings.compile)} e${timings.evaluate.toFixed(1)} n${timings.normals.toFixed(1)} m${timings.mips.toFixed(1)}`);
      // A value that changed while the bands were drawn is in some texels and not others.
      if (shared.maps.key !== key) { shared.maps.key = ""; return; }
      this.byKey.set(key, shared);
    }
    if (!current()) return;
    // The material is compiled before its mesh shows it, so the switch does not stall a frame.
    await run.materials;
    if (!current() || entry.shared?.maps.key !== key) return;
    this.show(entry);
    entry.done = version;
    if (run.attached) { this.display(entry); this.host.draw(); }
  }

  private standIns(): { color: THREE.Texture; data: THREE.Texture } {
    if (this.placeholders) return this.placeholders;
    const make = (colorSpace: THREE.ColorSpace) => {
      const texture = new THREE.DataTexture(new Uint8Array([128, 128, 255, 255]), 1, 1, THREE.RGBAFormat, THREE.UnsignedByteType);
      texture.colorSpace = colorSpace;
      texture.needsUpdate = true;
      return texture;
    };
    return this.placeholders = { color: make(THREE.SRGBColorSpace), data: make(THREE.NoColorSpace) };
  }

  /**
   * The standard material of an entry's baked look, on stand-in maps: what its program is compiled from depends on
   * which maps it has and on its live operations, never on the maps' texels.
   */
  private material(entry: Entry): THREE.MeshStandardMaterial {
    const { program, target } = entry;
    const standIn = this.standIns();
    const material = new THREE.MeshStandardMaterial({ color: 0xffffff });
    this.host.style(material);
    material.side = THREE.DoubleSide;
    material.roughness = 1; material.metalness = 1;
    material.map = standIn.color;
    material.normalMap = standIn.data;
    material.roughnessMap = material.metalnessMap = standIn.data;
    if (program.attachments.includes("emissive")) { material.emissiveMap = standIn.data; material.emissive.set(0xffffff); }
    // Arguments computed from parameters alone are evaluated in the material's shader, from the same uniform objects
    // the program's materials share: they follow a parameter with no bake.
    const parameters = program.liveParameters;
    if (parameters.length) {
      const previous = material.onBeforeCompile, uniforms = target.uniforms;
      const declarations = parameters.map(parameter => `uniform ${parameter.type} ${parameter.uniform};`).join("\n");
      material.onBeforeCompile = (shader, renderer) => {
        previous.call(material, shader, renderer);
        for (const parameter of parameters) shader.uniforms[parameter.uniform] = uniforms[parameter.uniform];
        shader.fragmentShader = shader.fragmentShader.replace("#include <common>", `#include <common>\n${declarations}`);
      };
    }
    // Live renderer operations run in the material's shader on the bake's own argument textures.
    material.userData.liveOperations = applyLiveOperations(material, program.liveOperations as never, () => standIn.data);
    // The operations' own key does not name GLSL arguments; the program layout's does. (Materials without operations
    // share one program whatever they were baked from.)
    if (program.liveOperations.length) {
      const key = material.customProgramCacheKey;
      material.customProgramCacheKey = () => `${key.call(material)}|atlas:${program.key}`;
    }
    return material;
  }

  /** Points an entry's baked-look material at its maps. Textures only: the program stays the one compiled on stand-ins. */
  private show(entry: Entry): void {
    const maps = entry.shared!.maps;
    const material = entry.material!;
    material.map = maps.baseColor;
    material.normalMap = maps.normal;
    material.roughnessMap = material.metalnessMap = maps.properties;
    if (maps.emissive) material.emissiveMap = maps.emissive;
    if (material.userData.liveUniforms) {
      const uniforms = material.userData.liveUniforms as Uniforms;
      for (const [id, slot] of material.userData.liveTextureSlots as Map<string, number>) {
        const uniform = uniforms[`msLiveTex${slot}Map`];
        if (uniform) uniform.value = maps.live[id];
      }
    }
  }
}
