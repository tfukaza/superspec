import * as THREE from "three";
import { applyLiveOperations } from "./live.ts";
import type { CpuField, MaterialShading } from "./shade.ts";
import { GpuBaker, SCENE_MAP_BYTES, gpuBakeUnsupported, type AtlasMaps, type FieldTexels, type StackInput } from "./bake.ts";
import { bakeGeometry, displayGeometry, type AtlasLayout, type AtlasProgramLayout } from "./atlas.ts";
import { shareInstanceAttributes } from "./batch.ts";

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
 * - A mesh's maps are the nodes of a pass graph (bake-graph.ts), each a texture named by what it holds. The evaluate
 *   nodes are named by `GpuBaker.atlasKey`: the program, its uniform values and images, the receiver layout's content,
 *   the receiver's features and the CPU fields it read; the normal by the height it was made from. The graph owns the
 *   textures: a node is shared by every mesh that wants the same texels and kept for a while after its last user, so
 *   going back to a scene, or rebuilding the scene for an edit to another material, shows them again without drawing.
 *   This module only holds each mesh's use of its nodes and the allowance for what is on screen.
 * - A mesh keeps both looks alive (two materials, two geometries), so switching between them compiles nothing.
 * - Only nodes whose key changed are drawn again: the meshes of a changed material, and of those only what the change reaches.
 * - An instanced batch (batch.ts) is one mesh here, with one atlas: its template's. Its maps are baked once, in the
 *   batch's own space, and every instance shows them; nothing in the key names a placement. The baked look's geometry
 *   takes the per-instance attributes of the geometry the batch came with.
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
  /** The receiver the CPU bakes this mesh's fields for, when it is not named as the mesh is (an instanced batch's template). */
  fieldsId?: string;
  /**
   * A layered slot's weathering layers over `shading` (the base): the mesh's maps are composed from the planes of the
   * base and each layer on the GPU (`GpuBaker.bakeStack`), not baked from one program.
   */
  stack?: StackInput;
  /** Why the mesh cannot be baked, known before any bake (a layered slot whose layers have no GPU plan). */
  unsupported?: string;
}

/** A CPU field a target's bake reads: the base's, kept by its sampler uniform, or a layer's (not a plane beneath it), by `<layer>/<uniform>`. */
interface WantedField { request: string; slot: string; field: CpuField; shading: MaterialShading; uniforms: Uniforms }

/** The CPU fields of a target, in the order they are asked for: the base's, then each layer's (`stack.<k>/<field>` to the source). */
function wantedFields(target: SceneBakeTarget): WantedField[] {
  const own = (target.shading.shader.cpuFields ?? []).map(field => ({ request: field.name, slot: field.uniform, field, shading: target.shading, uniforms: target.uniforms }));
  const layered = (target.stack?.layers ?? []).flatMap(layer => (layer.shading.shader.cpuFields ?? []).filter(field => !field.name.startsWith("below."))
    .map(field => ({ request: `${layer.id}/${field.name}`, slot: `${layer.id}/${field.uniform}`, field, shading: layer.shading, uniforms: layer.uniforms })));
  return [...own, ...layered];
}

/**
 * CPU-baked fields for hybrid programs, by mesh: one entry per requested name, on the mesh's atlas grid. Null when
 * superseded. `planes` lists, for each name, the external planes the field reads (the occluder service's `shadow`,
 * `occlusion`, `sunExposure`): the source binds them. `keys`, when the source gives them, name for each field the
 * content of the planes it was baked from, so a field whose planes did not change keeps its identity (and the textures
 * drawn from it) through a request made for another plane. A mesh whose bake failed carries the engine's reason.
 */
export type FieldSource = (requests: { mesh: string; names: string[]; planes: string[][] }[]) =>
  Promise<Map<string, { width: number; height: number; fields: Float32Array[]; keys?: string[] } | { error: string }> | null>;

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
  /** The same by node of the bakes' pass graphs: textures drawn, and textures that already held what was wanted. */
  nodesDrawn: number;
  nodesReused: number;
  /** GPU memory of the maps on screen. */
  bytes: number;
  /** Every complete texture the pass graph holds and their memory, and of those the ones without a user (kept for later). */
  graph: { nodes: number; bytes: number; idle: number; idleBytes: number };
  /** Whether the per-pixel looks are compiled, so a changed value shows per pixel at once (it is baked on settle either way). */
  shaderReady: boolean;
  /** One line per bake drawn: mesh, material and its milliseconds (c compile, e evaluate, n normals, m mips). */
  details: string[];
  /** Atlas sizes by mesh. */
  atlases: Record<string, string>;
  /**
   * Meshes of layered slots whose maps are composed from weathering layers on the GPU, and what their per-pixel look
   * (first look, edits and drags) shows: the base alone, since a layer reads the planes of the bake beneath it.
   */
  layers?: { meshes: number; perPixel: "base only" };
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

interface Entry {
  target: SceneBakeTarget;
  program: AtlasProgramLayout;
  shaderGeometry: THREE.BufferGeometry;
  display?: THREE.BufferGeometry;
  bake?: THREE.BufferGeometry;
  /** The standard material of the baked look; its maps are placeholders until the first bake. */
  material?: THREE.MeshStandardMaterial;
  /** The entry's use of its bake's nodes; incomplete while a bake draws over them. */
  maps?: AtlasMaps;
  /** Edits asked for and edits baked: equal when the maps are up to date. */
  wanted: number;
  done: number;
  showing: "shader" | "baked";
  unsupported?: string;
  fields?: Record<string, FieldTexels>;
  fieldsKey?: string;
  /**
   * The next bake follows a changed plane, not a changed value: the per-pixel look has no planes at all, so the maps
   * on screen stay until the new ones are complete instead of being drawn over.
   */
  keepShown?: boolean;
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
  totals: { compileMs: number; drawMs: number; fieldMs: number; drawn: number; reused: number; nodesDrawn: number; nodesReused: number };
  details: string[];
  /** Compiles of the baked looks' materials still under way. */
  materials: Promise<void>;
  /** Called whenever nothing is left to do for now. */
  idle?: () => void;
}

/** Pause after the last parameter change before its materials are baked again. */
const SETTLE_MS = 160;
/**
 * GPU memory the maps on screen may take; meshes beyond it stay on the per-pixel look. What the graph keeps after a
 * node's last user is bounded by the same number (`SCENE_MAP_BYTES` in bake.ts).
 */
const SCENE_BAKE_BYTES = SCENE_MAP_BYTES;
const NO_TOTALS = { compileMs: 0, drawMs: 0, fieldMs: 0, drawn: 0, reused: 0, nodesDrawn: 0, nodesReused: 0 };

export class SceneBaker {
  private current?: Run;
  private pending?: Run;
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
      settleAt: 0, started: performance.now(), totals: { ...NO_TOTALS }, details: [], materials: Promise.resolve() };
    for (const target of targets) {
      // A layered slot's maps are laid out as its stack's result: the base's live arguments, and the channels the stack has.
      const program = target.stack ? baker.stackProgram(target.shading, target.stack) : baker.atlasProgram(target.shading);
      // A mesh kept from the scene on screen may be showing that scene's baked look: its own geometry is beside it.
      const shaderGeometry = (target.mesh.userData.sourceGeometry as THREE.BufferGeometry | undefined) ?? target.mesh.geometry;
      const entry: Entry = { target, program, shaderGeometry, wanted: 1, done: 0, showing: "shader" };
      entry.unsupported = baker.unavailable() ?? target.unsupported ?? gpuBakeUnsupported(target.shading, true) ?? program.unsupported
        ?? (wantedFields(target).length && !fields ? "the program needs CPU-baked fields" : undefined);
      run.entries.push(entry);
    }
    this.pending = run;
    // The baked looks' materials compile beside the bake programs, on stand-in maps: the first bake only swaps textures in.
    const fresh = run.entries.filter(entry => !entry.unsupported);
    for (const entry of fresh) {
      entry.material = this.material(entry); entry.display = displayGeometry(entry.target.layout);
      shareInstanceAttributes(entry.shaderGeometry, entry.display);
    }
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

  /** Whether the displayed scene has nothing to bake and no change settling: every mesh that can show maps shows current ones. */
  settled(): boolean {
    const run = this.current;
    return !!run && !run.pumping && !run.fieldsInFlight && performance.now() >= run.settleAt
      && run.entries.every(entry => entry.unsupported || (entry.done === entry.wanted && entry.showing === "baked"));
  }

  /**
   * Gives a mesh of the displayed scene another per-pixel look: the same program in another form (one that draws
   * faster, compiled behind the scene), so its picture is the same. `receiver` holds the new look's own receiver
   * uniforms, which the mesh's bakes read from then on (the look they came from is the caller's to dispose). The mesh
   * shows the new look at once if it is showing the old one. Returns the look replaced, or undefined when nothing was
   * done: the mesh is not one this scene bakes, or a bake is under way with the old look's uniforms (ask again later).
   */
  replaceShader(mesh: THREE.Mesh, shader: THREE.Material, receiver: Uniforms): THREE.Material | undefined {
    const run = this.current;
    if (!run || run.closed || run.pumping) return undefined;
    const entry = run.entries.find(candidate => candidate.target.mesh === mesh);
    if (!entry || entry.unsupported) return undefined;
    const replaced = entry.target.shader;
    entry.target.shader = shader;
    Object.assign(entry.target.uniforms, receiver);
    if (entry.showing === "shader" && run.attached) {
      mesh.material = shader;
      mesh.userData.liveOperations = shader.userData.liveOperations;
    }
    return replaced;
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

  /** Counts the changes of each external plane's source (`planesChanged`); part of what a mesh's CPU fields are keyed by. */
  private readonly planeEpochs = new Map<string, number>();

  /**
   * The source of some external planes changed (the sun moved: `shadow`): the CPU fields that read them are asked for
   * again and the meshes that show them are baked again, at once. Only the textures whose fields changed are drawn; a
   * material that reads none of the planes is not touched. Returns the number of meshes asked again.
   */
  planesChanged(names: string[]): number {
    for (const name of names) this.planeEpochs.set(name, (this.planeEpochs.get(name) ?? 0) + 1);
    const run = this.current;
    if (!run) return 0;
    let asked = 0;
    for (const entry of run.entries) {
      if (entry.unsupported || !wantedFields(entry.target).some(wanted => wanted.field.planes?.some(plane => names.includes(plane)))) continue;
      entry.wanted++; entry.keepShown = true;
      asked++;
    }
    if (!asked) return 0;
    if (run.entries.every(entry => entry.unsupported || entry.done === entry.wanted || entry.keepShown)) this.restart(run);
    this.report(run);
    void this.pump(run);
    return asked;
  }

  private restart(run: Run): void {
    run.details = [];
    run.started = performance.now();
    run.totals = { ...NO_TOTALS };
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
    this.placeholders = undefined;
    if (!run) return;
    // The graph forgot its nodes with the context; nothing is left to release.
    for (const entry of run.entries) { this.showShader(run, entry); entry.maps = undefined; entry.done = 0; }
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
    // Nodes without a user stay with the baker's graph, which frees them when the baker is disposed.
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

  /** Gives up an entry's maps: complete nodes wait in the graph for another user, others are freed. */
  private release(entry: Entry): void {
    const maps = entry.maps;
    if (!maps) return;
    entry.maps = undefined;
    this.host.baker().releaseAtlasMaps(maps);
  }

  /** GPU memory of the distinct textures the run's meshes hold. */
  private bytesHeld(run: Run): number {
    return this.host.baker().atlasBytes(run.entries.map(entry => entry.maps));
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
      bytes: this.bytesHeld(run), graph: this.host.baker().graph.stats(), shaderReady: run.shaderReady, details: run.details,
      atlases: Object.fromEntries(run.entries.map(entry => [entry.target.mesh.name, `${entry.target.layout.width}x${entry.target.layout.height}`])),
      ...(run.entries.some(entry => entry.target.stack) ? { layers: { meshes: run.entries.filter(entry => entry.target.stack && !entry.unsupported).length, perPixel: "base only" as const } } : {}),
    });
  }

  /** What an entry's CPU fields depend on: the values of the parameters they read (seed, source and layout are the run's). */
  private fieldsKey(entry: Entry): string {
    const wanted = wantedFields(entry.target);
    const values: unknown[] = [];
    // The base's fields as they always were keyed; a layer's after them, by the layer.
    for (const { shading, uniforms, prefix } of [{ shading: entry.target.shading, uniforms: entry.target.uniforms, prefix: "" },
      ...(entry.target.stack?.layers ?? []).map(layer => ({ shading: layer.shading, uniforms: layer.uniforms, prefix: `${layer.id}/` }))]) {
      const names = new Set(wanted.filter(item => item.shading === shading).flatMap(item => item.field.parameters));
      values.push(...shading.shader.parameters.filter(parameter => names.has(parameter.name)).map(parameter => [`${prefix}${parameter.name}`, uniforms[parameter.uniform]?.value]));
    }
    // And the state of the external planes they read (`planesChanged`).
    const planes = [...new Set(wanted.flatMap(item => item.field.planes ?? []))].sort();
    return JSON.stringify(planes.length ? [values, planes.map(plane => [plane, this.planeEpochs.get(plane) ?? 0])] : values);
  }

  /** Asks the CPU for the fields of every hybrid entry that lacks current ones; the entries are baked when they arrive. */
  private async fetchFields(run: Run): Promise<void> {
    if (run.fieldsInFlight || !run.fields) return;
    const needy = run.entries.filter(entry => !entry.unsupported && wantedFields(entry.target).length && entry.fieldsKey !== this.fieldsKey(entry));
    if (!needy.length) return;
    run.fieldsInFlight = true;
    const keys = new Map(needy.map(entry => [entry, this.fieldsKey(entry)]));
    const started = performance.now();
    try {
      const baked = await run.fields(needy.map(entry => ({ mesh: entry.target.fieldsId ?? entry.target.mesh.name,
        names: wantedFields(entry.target).map(wanted => wanted.request), planes: wantedFields(entry.target).map(wanted => wanted.field.planes ?? []) })));
      if (run.closed) return;
      if (!baked) {
        // Superseded by a newer request of the page; asked again, a few times at most.
        if (++run.fieldRetries > 3) for (const entry of needy) entry.unsupported = "the CPU field bake was cancelled";
        return;
      }
      run.fieldRetries = 0;
      run.totals.fieldMs += performance.now() - started;
      for (const entry of needy) {
        const found = baked.get(entry.target.fieldsId ?? entry.target.mesh.name);
        const wanted = wantedFields(entry.target);
        const { width, height } = entry.target.layout;
        if (found && "error" in found) {
          entry.unsupported = `the CPU field bake failed: ${found.error}`;
          continue;
        }
        if (!found || found.fields.length !== wanted.length || found.width !== width || found.height !== height) {
          entry.unsupported = "the CPU field bake did not match the receiver's atlas";
          continue;
        }
        const key = keys.get(entry)!;
        // A field that reads planes is named by their content where the source states it (the value of what the field
        // reads of the parameters besides), so a request made for another plane leaves it, and what is drawn from it, alone.
        const parameters = ({ field, shading, uniforms }: WantedField) => JSON.stringify(field.parameters.map(name =>
          uniforms[shading.shader.parameters.find(parameter => parameter.name === name)?.uniform ?? ""]?.value));
        const named = (item: WantedField, index: number) => item.field.planes?.length && found.keys?.[index] !== undefined
          ? `planes:${found.keys[index]}:${parameters(item)}` : key;
        entry.fields = Object.fromEntries(wanted.map((item, index) => [item.slot,
          { id: `${entry.target.layout.key}:${entry.target.mesh.name}:${item.request}:${named(item, index)}`, width, height, rgba: found.fields[index] }]));
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
          && (!wantedFields(entry.target).length || entry.fieldsKey === this.fieldsKey(entry));
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
    entry.bake ??= bakeGeometry(target.layout);
    // Every node a texture already holds (on this mesh, on another mesh, or kept from before) is taken as it is. The
    // others are drawn: over this mesh's own textures where only it holds them, unless they are on screen with no
    // per-pixel look to show meanwhile; into new textures otherwise, which replace the old ones when all are complete.
    const onScreen = run.attached && entry.showing === "baked" && (!run.shaderReady || entry.keepShown === true);
    const input = { layout: target.layout, geometry: entry.bake, extent: target.extent, receiver: target.receiver, program };
    const options = {
      fields: entry.fields, current, inPlace: !onScreen,
      allocate: (bytes: number) => {
        if (this.bytesHeld(run) + bytes > SCENE_BAKE_BYTES) throw new Error("the scene's baked maps would exceed the preview's memory allowance");
      },
      beforeOverwrite: () => this.showShader(run, entry),
    };
    // A layered slot is composed from its planes; any other mesh is baked from its one program.
    const baked = target.stack ? await baker.bakeStack(target.shading, target.uniforms, target.stack, input, entry.maps, options)
      : await baker.bakeAtlas(target.shading, target.uniforms, input, entry.maps, options);
    if (!baked) return;
    // The bake released the maps it replaced. A run closed meanwhile has no use for the new ones.
    if (run.closed) { baker.releaseAtlasMaps(baked.maps); return; }
    const { maps, timings } = baked;
    entry.maps = maps;
    run.totals.nodesDrawn += timings.drawn.length; run.totals.nodesReused += timings.reused;
    if (timings.drawn.length) {
      run.totals.compileMs += timings.compile; run.totals.drawMs += timings.total - timings.compile; run.totals.drawn++;
      run.details.push(`${target.mesh.name.replace(/^.*node\/(\d+).*$/, "$1")}:${target.material} c${Math.round(timings.compile)} e${timings.evaluate.toFixed(1)} n${timings.normals.toFixed(1)} m${timings.mips.toFixed(1)} d${timings.drawn.length} r${timings.reused}${target.stack ? ` [${timings.drawn.join(" ")}]` : ""}`);
    } else run.totals.reused++;
    if (!current()) return;
    // The material is compiled before its mesh shows it, so the switch does not stall a frame.
    await run.materials;
    if (!current() || entry.maps !== maps || !baker.atlasMapsComplete(maps)) return;
    this.show(entry);
    entry.done = version; entry.keepShown = false;
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
    const maps = entry.maps!;
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
