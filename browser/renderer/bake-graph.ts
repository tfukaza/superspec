import * as THREE from "three";

/**
 * The pass graph of the GPU preview bakes: a bake is a list of NODES, each one texture that an operation draws, reading
 * the textures of the nodes it names as inputs. Every node stays materialized and addressable by name, so a later pass
 * (a normal recomputed from a height, a blur, a weathering layer composed over a base, a per-receiver mapping of a
 * shared intrinsic bake, a heavy shader split into smaller programs) is one more node and nothing else changes.
 *
 * Keys. A node's own `key` names everything that changes its texels EXCEPT its inputs: the program, the output it
 * selects, uniform values, geometry. Its resolved key adds the target's format and the resolved keys of its inputs, so
 * it names the texels completely. A node is drawn only when no texture holds its resolved key: not when the instance
 * being replaced holds it (nothing changed for that node), and not when any other instance or an idle node does (the
 * same content, drawn for something else: the nodes are shared, counted by users). What a node excludes from its key
 * is therefore what it is shared across: an intrinsic node keyed without receiver geometry is one texture for every
 * receiver, and the mapping nodes that read it are per receiver.
 *
 * This module knows nothing of materials or of WebGL beyond the render-target objects: operations do their own drawing
 * (bake.ts), and the device supplies what needs the context. Its scheduling is tested without a GPU.
 */

/** The texture a node is drawn into. */
export interface TargetSpec {
  width: number;
  height: number;
  format: THREE.PixelFormat;
  type: THREE.TextureDataType;
  colorSpace: string;
  /** `mips`: sampled trilinearly, its mip chain built whenever the node is drawn; `linear`; `nearest`: read by texel. */
  filter: "mips" | "linear" | "nearest";
  wrap: THREE.Wrapping;
  anisotropy?: number;
  /** For inspection only; not part of any key. */
  name: string;
}

const targetSignature = (spec: TargetSpec): string =>
  `${spec.width}x${spec.height}:${spec.format}:${spec.type}:${spec.colorSpace}:${spec.filter}:${spec.wrap}:${spec.anisotropy ?? 1}`;

/** GPU memory of a target, mips included. */
export function targetBytes(spec: TargetSpec): number {
  const channels = spec.format === THREE.RedFormat ? 1 : spec.format === THREE.RGFormat ? 2 : 4;
  const size = spec.type === THREE.FloatType ? 4 : spec.type === THREE.HalfFloatType ? 2 : 1;
  return Math.round(channels * size * (spec.filter === "mips" ? 4 / 3 : 1) * spec.width * spec.height);
}

/** One materialized texture. It may be held by several graph instances at once. */
export interface GraphNode {
  /** The resolved key of what the texture holds, or empty while it holds nothing complete. */
  key: string;
  readonly signature: string;
  readonly target: THREE.WebGLRenderTarget;
  readonly texture: THREE.Texture;
  readonly bytes: number;
  /** Graph instances holding the node. */
  users: number;
  /** The context generation the texture belongs to. */
  readonly generation: number;
  /** True until the node has been drawn completely once (an operation that does not cover its target clears it first). */
  fresh: boolean;
}

/** One draw of a node: all of it, or a band of rows (framebuffer rows, the lowest first). */
export interface DrawStep {
  node: GraphNode;
  /** Which of the operation's outputs the node is (`NodeSpec.output`). */
  output: number;
  inputs: Record<string, GraphNode>;
  rows: { start: number; count: number } | null;
  /** Whether this is the node's first band. */
  first: boolean;
}

/** How nodes are drawn: a compiled program or a built-in operation. One operation may draw several nodes (its outputs). */
export interface NodeOp {
  /** Names the operation in timings. */
  label: string;
  /**
   * Names what the operation's speed per texel depends on (its program), for a device that sizes bands by the speed it
   * measured (`GraphDevice.yield`, `RealizeOptions.bandTexels`); the label when absent.
   */
  pace?: string;
  /** Compiles what the operation draws with. Every dirty node's operation is prepared before anything is drawn. */
  prepare?(): Promise<void>;
  draw(step: DrawStep): void;
  /** After a node's last band and before its mips: may throw to fail the bake. */
  finish?(node: GraphNode, inputs: Record<string, GraphNode>): void | Promise<void>;
}

/** A node as a bake asks for it. Nodes are listed after the nodes they read. */
export interface NodeSpec {
  /** The node's name in its graph. */
  id: string;
  /** Everything that changes the node's texels besides its inputs and its target's format. */
  key: string;
  target: TargetSpec;
  op: NodeOp;
  output?: number;
  /** The nodes it reads, by the role the operation knows them by. */
  inputs?: Record<string, string>;
  /**
   * An intermediate: other nodes of the bake read it while they are drawn, and nothing reads it afterwards (a height
   * the normal was made from, a layer's plane under the composed map). The instance does not hold it: once the bake is
   * complete it is kept like any node without a user, for as long as the graph's limits allow, and a later bake that
   * draws a node reading it finds it there or draws it again. It is not drawn at all while every node that reads it
   * holds what is wanted.
   */
  transient?: boolean;
}

/** What needs the WebGL context. */
export interface GraphDevice {
  /** Advances when the context is lost or restored: nodes of another generation no longer exist. */
  generation(): number;
  /** Builds the mip chains of drawn textures. */
  mips(textures: THREE.Texture[]): void;
  /** Waits for the GPU to finish drawing into a target (measurements only). */
  settle?(target: THREE.WebGLRenderTarget): void;
  /**
   * Gives the browser a turn between bands. `issued` lists what the band just drawn was: each operation and the texels
   * it drew, for a device that measures how long the GPU took over them.
   */
  yield?(issued: BandWork[]): Promise<void>;
  /** Runs the draws of one band of a level, so what they share (saving and restoring the renderer's state) is done once. */
  batch?(draws: () => void): void;
}

/** One operation's share of a band: the texels it drew. */
export interface BandWork { op: NodeOp; texels: number }

export interface GraphLimits {
  /**
   * The memory the nodes kept after their last user may take together, least recently used dropped first. Memory is
   * the bound, not a count: a scene of many small receivers and one of a few large ones keep the same amount.
   */
  idleBytes: number;
  /**
   * The least a kept node is charged against `idleBytes` (0 by default): a render target costs the driver a texture
   * and a framebuffer object whatever its size, so a great many tiny textures do not hide under the allowance.
   */
  nodeFloorBytes?: number;
  /** A further bound on their number, for a host that wants one (none by default). */
  idleNodes?: number;
  /**
   * A bound on everything the graph holds, nodes in use included (none by default): nodes without a user are dropped,
   * least recently used first, while the nodes in use and the kept ones together exceed it. Nodes in use are never
   * dropped, so the bound is exceeded only by what their users hold.
   */
  totalBytes?: number;
}

/** The nodes of one realized graph, by name, without its intermediates (`NodeSpec.transient`). It holds one use of each until it is released. */
export class GraphInstance {
  released = false;
  readonly nodes: ReadonlyMap<string, GraphNode>;
  /** The resolved key each node was realized for, intermediates included. */
  readonly keys: ReadonlyMap<string, string>;
  constructor(nodes: ReadonlyMap<string, GraphNode>, keys: ReadonlyMap<string, string>) { this.nodes = nodes; this.keys = keys; }
  node(id: string): GraphNode | undefined { return this.nodes.get(id); }
  texture(id: string): THREE.Texture | undefined { return this.nodes.get(id)?.texture; }
  /** Whether every node still holds what the instance was realized for (a bake abandoned while drawing over it leaves it incomplete). */
  get complete(): boolean {
    if (this.released) return false;
    for (const [id, node] of this.nodes) if (node.key !== this.keys.get(id)) return false;
    return true;
  }
}

export interface RealizeOptions {
  /** Checked whenever the browser had a turn; a false answer abandons the bake. */
  current?: () => boolean;
  /** Whether the keys the specs were built with still name what the operations draw (values may change during a turn). */
  stable?: () => boolean;
  /** Whether nodes only the previous instance holds may be drawn over (the default); otherwise changed nodes get new textures. */
  inPlace?: boolean;
  /** Called with the memory of the textures about to be created; throws to refuse them. */
  allocate?: (bytes: number) => void;
  /** Called once, before the first draw over a texture of the previous instance. */
  beforeOverwrite?: () => void;
  /**
   * Texels per band: larger nodes are drawn in bands of rows with the browser given a turn between them. A function
   * gives each node its own (an operation the GPU is known to draw fast may take more at a time).
   */
  bandTexels?: number | ((spec: NodeSpec) => number);
  /** Waits for the GPU after each level so timings are real (a stall; tests and diagnostics). */
  measure?: boolean;
}

export interface GraphResult {
  instance: GraphInstance;
  /** Nodes drawn, in order, and the number answered from textures that already held them. */
  drawn: string[];
  reused: number;
  /** The most bands any node was drawn in. */
  bands: number;
  timings: { prepare: number; draw: Record<string, number>; mips: number; total: number };
}

type Choice = { spec: NodeSpec; want: string } & ({ kind: "held"; node: GraphNode } | { kind: "over"; node: GraphNode } | { kind: "fresh" });

export class BakeGraph {
  /** Complete nodes by resolved key: those in use and those kept idle. */
  private readonly complete = new Map<string, GraphNode>();
  /** Complete nodes without a user, least recently used first. */
  private readonly idle = new Set<GraphNode>();
  /** Nodes drawn and nodes answered from held textures since the graph was made (diagnostics). */
  readonly totals = { drawn: 0, reused: 0 };
  private readonly device: GraphDevice;
  private readonly limits: GraphLimits;

  // (Plain fields, not parameter properties: the scheduling tests run this file under Node's type stripping.)
  constructor(device: GraphDevice, limits: GraphLimits = { idleBytes: 128 * 1024 * 1024 }) { this.device = device; this.limits = limits; }

  /** The resolved key of every node: its own key, its target's format and its inputs' resolved keys. */
  static resolve(specs: NodeSpec[]): Map<string, string> {
    const keys = new Map<string, string>();
    for (const spec of specs) {
      if (keys.has(spec.id)) throw new Error(`Bake graph names the node ${spec.id} twice.`);
      const inputs = Object.entries(spec.inputs ?? {}).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([role, id]) => {
        const key = keys.get(id);
        if (key === undefined) throw new Error(`Bake graph node ${spec.id} reads ${id}, which is not listed before it.`);
        return `${role}=<${key}>`;
      });
      keys.set(spec.id, `${targetSignature(spec.target)}|${spec.key}${inputs.length ? `|${inputs.join(",")}` : ""}`);
    }
    return keys;
  }

  private alive(node: GraphNode): boolean { return node.generation === this.device.generation(); }

  /**
   * Where each node's texels come from: a texture that holds them, the previous instance's texture drawn over, or a new
   * one. An intermediate is listed only when a node that will be drawn reads it (through other intermediates or not):
   * one that nothing holds is not drawn for nodes that already hold what they made from it.
   */
  private choose(specs: NodeSpec[], keys: Map<string, string>, previous: GraphInstance | undefined, inPlace: boolean): Choice[] {
    const prior = previous && !previous.released ? previous : undefined;
    const choices: Choice[] = specs.map(spec => {
      const want = keys.get(spec.id)!;
      const mine = prior?.nodes.get(spec.id);
      const usable = mine && this.alive(mine) ? mine : undefined;
      if (usable && usable.key === want) return { spec, want, kind: "held", node: usable };
      const held = this.complete.get(want);
      if (held && this.alive(held) && held.key === want) return { spec, want, kind: "held", node: held };
      if (usable && inPlace && usable.users === 1 && usable.signature === targetSignature(spec.target)) return { spec, want, kind: "over", node: usable };
      return { spec, want, kind: "fresh" };
    });
    if (!specs.some(spec => spec.transient)) return choices;
    const byId = new Map(choices.map(choice => [choice.spec.id, choice]));
    const needed = new Set<string>();
    const need = (choice: Choice): void => {
      if (needed.has(choice.spec.id)) return;
      needed.add(choice.spec.id);
      if (choice.kind === "held") return;
      for (const id of Object.values(choice.spec.inputs ?? {})) { const input = byId.get(id); if (input) need(input); }
    };
    for (const choice of choices) if (!choice.spec.transient) need(choice);
    return choices.filter(choice => needed.has(choice.spec.id));
  }

  /** How many nodes of `specs` would be drawn now, given the instance they replace. */
  dirty(specs: NodeSpec[], previous?: GraphInstance, inPlace = true): number {
    return this.choose(specs, BakeGraph.resolve(specs), previous, inPlace).filter(choice => choice.kind !== "held").length;
  }

  private create(spec: TargetSpec, generation: number): GraphNode {
    const target = new THREE.WebGLRenderTarget(spec.width, spec.height, { depthBuffer: false, stencilBuffer: false });
    const texture = target.texture;
    texture.name = spec.name;
    texture.format = spec.format; texture.type = spec.type; texture.colorSpace = spec.colorSpace;
    texture.wrapS = texture.wrapT = spec.wrap;
    // Mips are built once after a node's last band (`GraphDevice.mips`), not by every draw.
    texture.generateMipmaps = false;
    texture.minFilter = spec.filter === "mips" ? THREE.LinearMipmapLinearFilter : spec.filter === "linear" ? THREE.LinearFilter : THREE.NearestFilter;
    texture.magFilter = spec.filter === "nearest" ? THREE.NearestFilter : THREE.LinearFilter;
    if (spec.anisotropy !== undefined) texture.anisotropy = spec.anisotropy;
    return { key: "", signature: targetSignature(spec), target, texture, bytes: targetBytes(spec), users: 0, generation, fresh: true };
  }

  /** Memory of the nodes in use, of the context generation it was counted in. */
  private used = { bytes: 0, generation: -1 };

  /** The memory of the nodes in use now (nodes of a context that went away count for nothing). */
  private usedBytes(): number {
    const generation = this.device.generation();
    if (this.used.generation !== generation) this.used = { bytes: 0, generation };
    return this.used.bytes;
  }

  private retain(node: GraphNode): void {
    const used = this.usedBytes();
    if (node.users++ === 0 && node.generation === this.used.generation) this.used.bytes = used + node.bytes;
    this.idle.delete(node);
  }

  /** Gives up one use of a node: a complete one without users waits for another, an incomplete one is freed. */
  private drop(node: GraphNode): void {
    if (--node.users > 0) return;
    const used = this.usedBytes();
    if (node.generation === this.used.generation) this.used.bytes = used - node.bytes;
    if (!node.key || !this.alive(node) || this.complete.get(node.key) !== node) {
      if (node.key && this.complete.get(node.key) === node) this.complete.delete(node.key);
      node.target.dispose();
      return;
    }
    this.idle.delete(node); this.idle.add(node);
    this.trim();
  }

  /** Drops nodes without a user, least recently used first, until what is kept is within the limits with `coming` more bytes in use. */
  private trim(coming = 0): void {
    const floor = this.limits.nodeFloorBytes ?? 0;
    const charged = (kept: GraphNode) => Math.max(kept.bytes, floor);
    const room = (this.limits.totalBytes ?? Infinity) - this.usedBytes() - coming;
    let bytes = 0;
    for (const kept of this.idle) bytes += charged(kept);
    for (const kept of [...this.idle]) {
      if (this.idle.size <= (this.limits.idleNodes ?? Infinity) && bytes <= this.limits.idleBytes && bytes <= room) break;
      bytes -= charged(kept);
      this.idle.delete(kept); this.complete.delete(kept.key);
      kept.target.dispose();
    }
  }

  /** Gives up an instance's nodes. Releasing twice does nothing. */
  release(instance: GraphInstance | undefined): void {
    if (!instance || instance.released) return;
    instance.released = true;
    for (const node of instance.nodes.values()) this.drop(node);
  }

  /** GPU memory of the distinct nodes of some instances. */
  bytes(instances: Iterable<GraphInstance | undefined>): number {
    const seen = new Set<GraphNode>();
    let bytes = 0;
    for (const instance of instances) {
      if (!instance || instance.released) continue;
      for (const node of instance.nodes.values()) if (!seen.has(node)) { seen.add(node); bytes += node.bytes; }
    }
    return bytes;
  }

  /** Nodes held complete and their memory, how many of them are idle, and the idle ones' memory (diagnostics). */
  stats(): { nodes: number; bytes: number; idle: number; idleBytes: number } {
    let idleBytes = 0, bytes = 0;
    for (const node of this.idle) idleBytes += node.bytes;
    for (const node of this.complete.values()) bytes += node.bytes;
    return { nodes: this.complete.size, bytes, idle: this.idle.size, idleBytes };
  }

  /** The context went away with every texture: nothing is kept (and nothing can be freed). */
  forget(): void { this.complete.clear(); this.idle.clear(); }

  /** Frees the idle nodes; nodes still in use are their instances' to release. */
  dispose(): void {
    for (const node of this.idle) { this.complete.delete(node.key); node.target.dispose(); }
    this.idle.clear();
  }

  /**
   * Makes the textures of `specs` exist, drawing only the nodes nothing holds, and returns them as an instance that
   * takes the place of `previous` (which is released). Dirty nodes are drawn level by level, each after the nodes it
   * reads; the nodes of a level are drawn band by band together, with the browser given a turn between bands.
   * Returns null when the bake was abandoned (`current`, `stable`, a lost context): `previous` is then still the
   * caller's, incomplete if it was being drawn over. Throws what an operation throws, in the same state.
   */
  async realize(specs: NodeSpec[], previous?: GraphInstance, options: RealizeOptions = {}): Promise<GraphResult | null> {
    const started = performance.now();
    const generation = this.device.generation();
    const current = () => generation === this.device.generation() && (options.current?.() ?? true) && options.stable?.() !== false;
    const keys = BakeGraph.resolve(specs);
    const inPlace = options.inPlace !== false;
    // Every program is compiled before the first draw: a compile takes a while, a texture drawn over may be the one a
    // viewer shows next, and a frame in between must not see some maps of the new bake with others of the old.
    const prepared = new Set<NodeOp>();
    for (;;) {
      const needed = new Set(this.choose(specs, keys, previous, inPlace).filter(choice => choice.kind !== "held").map(choice => choice.spec.op));
      for (const op of prepared) needed.delete(op);
      if (!needed.size) break;
      let waited = false;
      for (const op of needed) {
        prepared.add(op);
        if (op.prepare) { waited = true; await op.prepare(); }
      }
      if (waited && !current()) return null;
    }
    const preparedAt = performance.now();
    // From here to the first band's end nothing waits: the choices hold, and the nodes are this bake's.
    const choices = this.choose(specs, keys, previous, inPlace);
    const staged = new Map<string, GraphNode>();
    const result: GraphResult = { instance: undefined as never, drawn: [], reused: 0, bands: 0, timings: { prepare: preparedAt - started, draw: {}, mips: 0, total: 0 } };
    const undo = () => { for (const node of staged.values()) this.drop(node); };
    try {
      let freshBytes = 0;
      for (const choice of choices) {
        if (choice.kind === "fresh") { freshBytes += targetBytes(choice.spec.target); continue; }
        this.retain(choice.node); staged.set(choice.spec.id, choice.node);
        if (choice.kind === "held") result.reused++;
      }
      if (freshBytes) {
        options.allocate?.(freshBytes);
        // Kept nodes make room for the new ones (those this bake reads are in use by now).
        this.trim(freshBytes);
      }
      const work: { spec: NodeSpec; want: string; node: GraphNode }[] = [];
      let overwrite = false;
      for (const choice of choices) {
        if (choice.kind === "held") continue;
        let node: GraphNode;
        if (choice.kind === "over") {
          node = choice.node;
          if (!overwrite) { overwrite = true; options.beforeOverwrite?.(); }
          // The texture holds a mix of two bakes until its last band is drawn.
          if (this.complete.get(node.key) === node) this.complete.delete(node.key);
          node.key = "";
        } else {
          node = this.create(choice.spec.target, generation);
          this.retain(node); staged.set(choice.spec.id, node);
        }
        work.push({ spec: choice.spec, want: choice.want, node });
      }
      const bandTexels = typeof options.bandTexels === "function" ? options.bandTexels : () => (options.bandTexels as number | undefined) ?? Infinity;
      const pending = new Set(work.map(item => item.spec.id));
      let remaining = work;
      while (remaining.length) {
        const level = remaining.filter(item => Object.values(item.spec.inputs ?? {}).every(id => !pending.has(id)));
        if (!level.length) throw new Error("Bake graph nodes read each other in a circle.");
        const plans = level.map(item => {
          const { width, height } = item.spec.target;
          const rows = Math.min(height, Math.max(1, Math.floor(bandTexels(item.spec) / width)));
          const inputs = Object.fromEntries(Object.entries(item.spec.inputs ?? {}).map(([role, id]) => [role, staged.get(id)!]));
          return { ...item, rows, bands: Math.ceil(height / rows), inputs };
        });
        const bands = Math.max(...plans.map(plan => plan.bands));
        let waited = false;
        let issued: BandWork[] = [];
        for (let band = 0; band < bands; band++) {
          if (band > 0) {
            await (this.device.yield?.(issued) ?? new Promise<void>(done => setTimeout(done, 0)));
            if (!current()) { undo(); return null; }
          }
          const draws = (): void => {
            issued = [];
            for (const plan of plans) {
              if (band >= plan.bands) continue;
              issued.push({ op: plan.spec.op, texels: plan.spec.target.width * Math.min(plan.rows, plan.spec.target.height - band * plan.rows) });
              const before = performance.now();
              plan.spec.op.draw({ node: plan.node, output: plan.spec.output ?? 0, inputs: plan.inputs, first: band === 0,
                rows: plan.bands > 1 ? { start: band * plan.rows, count: Math.min(plan.rows, plan.spec.target.height - band * plan.rows) } : null });
              result.timings.draw[plan.spec.op.label] = (result.timings.draw[plan.spec.op.label] ?? 0) + performance.now() - before;
            }
          };
          if (this.device.batch) this.device.batch(draws); else draws();
        }
        for (const plan of plans) {
          const before = performance.now();
          const finishing = plan.spec.op.finish?.(plan.node, plan.inputs);
          if (finishing) { waited = true; await finishing; }
          if (options.measure) this.device.settle?.(plan.node.target);
          result.timings.draw[plan.spec.op.label] = (result.timings.draw[plan.spec.op.label] ?? 0) + performance.now() - before;
        }
        if (waited && !current()) { undo(); return null; }
        const mipped = plans.filter(plan => plan.spec.target.filter === "mips");
        if (mipped.length) {
          const before = performance.now();
          this.device.mips(mipped.map(plan => plan.node.texture));
          if (options.measure) this.device.settle?.(mipped[0].node.target);
          result.timings.mips += performance.now() - before;
        }
        for (const plan of plans) {
          plan.node.key = plan.want; plan.node.fresh = false;
          this.complete.set(plan.want, plan.node);
          pending.delete(plan.spec.id);
          result.drawn.push(plan.spec.id);
        }
        result.bands = Math.max(result.bands, bands);
        remaining = remaining.filter(item => pending.has(item.spec.id));
      }
    } catch (error) { undo(); throw error; }
    this.release(previous);
    this.totals.drawn += result.drawn.length; this.totals.reused += result.reused;
    // The instance holds what the bake is for; its intermediates are kept as nodes without a user are.
    const transient = new Set(specs.filter(spec => spec.transient).map(spec => spec.id));
    const held = new Map([...staged].filter(([id]) => !transient.has(id)));
    result.instance = new GraphInstance(held, new Map(specs.map(spec => [spec.id, keys.get(spec.id)!])));
    for (const [id, node] of staged) if (transient.has(id)) this.drop(node);
    result.timings.total = performance.now() - started;
    return result;
  }
}
