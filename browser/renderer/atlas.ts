import * as THREE from "three";
import type { MaterialShading, ProgramShader, Reads } from "./shade.ts";
import { NORMAL_ENCODE, RELIEF_ADJUST } from "./relief-glsl.ts";

/**
 * Receiver atlases for the GPU bake of scene projects. The layout comes from the engine
 * (`Session::project_layouts`, Wasm `layout_project`): it is the atlas the CPU bake of the same project gives each
 * receiver, so nothing here sizes or packs charts. This module only expands the engine's triangle records into the
 * geometry a baked mesh is displayed with and the geometry its atlas is drawn with, and holds the shaders that turn a
 * record into the program's inputs at a texel (`Atlas::texel` and `Atlas::sample` in `authoring/receiver.rs`).
 *
 * A bake is a pass graph over one receiver's atlas (bake-graph.ts; `GpuBaker.atlasNodes` lists its nodes): each
 * texture is a node drawn by an operation and may read other nodes. `evaluate` (the program) draws base color,
 * properties, height and, by program, emissive, normal offset and live renderer arguments; `normals` reads the height
 * (and offset) node. Every node stays materialized and named, so a later pass (a weathering layer, a blur, a mapping
 * of a shared intrinsic bake: `ATLAS_MAPPING_FRAGMENT`) reads them without evaluating the program again.
 */

/** One receiver's atlas as the engine describes it (`ReceiverLayout` in `authoring/layout.rs`). */
export interface ReceiverLayout {
  id: string; material: string; width: number; height: number; density: number; triangles: number; offset: number;
  /** Ten numbers per chart: how its texels map to the material's surface domain (`ReceiverLayout::surface`). */
  surface?: number[];
}
/** Every assigned receiver's atlas of a scene project, with the triangle records they index. */
export interface ProjectLayouts { gutter: number; record: number; receivers: ReceiverLayout[]; records: Float32Array }

/** Numbers per triangle record and where its parts start (`LAYOUT_RECORD` in `authoring/layout.rs`). */
const RECORD = 38, RECT = 0, META = 4, POSITIONS = 8, NORMALS = 17, UVS = 26, ATLAS_UVS = 32;

/** One receiver's atlas: its size and its triangles' records. `key` names the content (two equal layouts share it). */
export interface AtlasLayout {
  id: string;
  material: string;
  width: number;
  height: number;
  gutter: number;
  /** Texels per meter. */
  density: number;
  triangles: number;
  records: Float32Array;
  /**
   * Ten numbers per chart, as the engine states them (`ReceiverLayout::surface`): the chart's x and y in atlas texels,
   * the surface position of its first inner texel, that position's change one texel along atlas +x and +y, the sample
   * footprint in meters, and whether the first inner texel is on the lattice of its footprint.
   */
  surface: Float64Array;
  key: string;
}

/** Numbers per chart in `AtlasLayout.surface`. */
const SURFACE = 10;

/** FNV-1a over the bits of floats, for content keys; not a security boundary. */
function hashFloats(values: Float32Array, seed = 0x811c9dc5): string {
  const bits = new Uint32Array(values.buffer, values.byteOffset, values.length);
  let value = seed;
  for (let index = 0; index < bits.length; index++) value = Math.imul(value ^ bits[index], 0x01000193);
  return (value >>> 0).toString(16).padStart(8, "0");
}

/** The receivers' atlases by mesh identity. Throws when the records are not the format this module reads. */
export function atlasLayouts(layouts: ProjectLayouts): Map<string, AtlasLayout> {
  if (layouts.record !== RECORD) throw new Error(`Receiver layout records have ${layouts.record} numbers; this viewer reads ${RECORD}.`);
  const found = new Map<string, AtlasLayout>();
  for (const receiver of layouts.receivers) {
    const records = layouts.records.subarray(receiver.offset, receiver.offset + receiver.triangles * RECORD);
    if (records.length !== receiver.triangles * RECORD) throw new Error(`Receiver layout of ${receiver.id} is incomplete.`);
    const surface = Float64Array.from(receiver.surface ?? []);
    // The surface map names where a chart's texels sample the pattern: part of the content, like the records.
    const mapped = surface.length ? `:${hashFloats(new Float32Array(new Float64Array(surface).buffer))}` : "";
    found.set(receiver.id, { id: receiver.id, material: receiver.material, width: receiver.width, height: receiver.height, gutter: layouts.gutter,
      density: receiver.density, triangles: receiver.triangles, records, surface,
      key: `${receiver.width}x${receiver.height}:${layouts.gutter}:${hashFloats(records)}:${hashFloats(records, 0x9747b28c)}${mapped}` });
  }
  return found;
}

/** The mesh a baked receiver is displayed with: the receiver's triangles, unshared, with their atlas UVs. */
export function displayGeometry(layout: AtlasLayout): THREE.BufferGeometry {
  const count = layout.triangles * 3;
  const positions = new Float32Array(count * 3), normals = new Float32Array(count * 3), uvs = new Float32Array(count * 2);
  for (let triangle = 0; triangle < layout.triangles; triangle++) {
    const record = triangle * RECORD;
    positions.set(layout.records.subarray(record + POSITIONS, record + POSITIONS + 9), triangle * 9);
    normals.set(layout.records.subarray(record + NORMALS, record + NORMALS + 9), triangle * 9);
    uvs.set(layout.records.subarray(record + ATLAS_UVS, record + ATLAS_UVS + 6), triangle * 6);
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new THREE.BufferAttribute(normals, 3));
  geometry.setAttribute("uv", new THREE.BufferAttribute(uvs, 2));
  geometry.computeBoundingBox();
  geometry.computeBoundingSphere();
  return geometry;
}

/**
 * Floats per vertex of the bake geometry: atlas position (3), a triangle record, then its chart's surface map without
 * the chart's position (eight numbers; zero for a chart that is not on a pattern anchor's lattice).
 */
const STRIDE = 3 + RECORD + SURFACE - 2;
/** The role of a triangle of a whole flat face on a pattern anchor's lattice (`LAYOUT_RECORD` in `authoring/layout.rs`). */
const FACE = 3;

/** The part of a convex polygon where `side * f(point) >= 0` (Sutherland-Hodgman against one line). */
function clip(points: number[][], f: (point: number[]) => number, side: number): number[][] {
  const kept: number[][] = [];
  for (let index = 0; index < points.length; index++) {
    const a = points[index], b = points[(index + 1) % points.length];
    const fa = side * f(a), fb = side * f(b);
    if (fa >= 0) kept.push(a);
    if ((fa > 0 && fb < 0) || (fa < 0 && fb > 0)) {
      const t = fa / (fa - fb);
      kept.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
    }
  }
  return kept;
}

/**
 * The geometry an atlas is drawn with: every chart's rectangle (gutters included) in atlas texels, each vertex carrying
 * the record of the triangle whose points its texels sample. A lone triangle's chart is one rectangle; a pair's is cut
 * along the pair's shared edge, so each texel is drawn once, by the triangle the atlas gives it.
 *
 * A whole flat face on a pattern anchor's lattice (role 3) is drawn where the engine put it, never along its
 * triangles' own legs: each triangle covers the texels whose centers lie in it (its atlas UVs), and the shaders map a
 * texel to the triangle's plane affinely from its three atlas UV and position pairs. The face's rectangle is drawn
 * first, with the record of the face's first triangle, so texels in no triangle (the gutter, a hole, a notch of the
 * outline) continue the face's plane; the engine gives those the nearest triangle of their row, which on a flat face
 * is the same point of the plane. Every vertex of such a chart carries the chart's surface map
 * (`AtlasLayout.surface`), from which the bake takes the lattice point each texel samples.
 */
export function bakeGeometry(layout: AtlasLayout): THREE.BufferGeometry {
  const vertices: number[] = [], indices: number[] = [];
  const gutter = layout.gutter;
  const charts = new Map<string, Float64Array>();
  for (let chart = 0; chart + SURFACE <= layout.surface.length; chart += SURFACE) {
    charts.set(`${layout.surface[chart]},${layout.surface[chart + 1]}`, layout.surface.subarray(chart + 2, chart + SURFACE));
  }
  const none = new Float64Array(SURFACE - 2);
  const faces = new Set<string>();
  const emit = (polygon: number[][], record: Float32Array, surface: Float64Array): void => {
    if (polygon.length < 3) return;
    const first = vertices.length / STRIDE;
    for (const point of polygon) {
      vertices.push(point[0], point[1], 0);
      for (let index = 0; index < RECORD; index++) vertices.push(record[index]);
      for (let index = 0; index < SURFACE - 2; index++) vertices.push(surface[index]);
    }
    for (let corner = 1; corner + 1 < polygon.length; corner++) indices.push(first, first + corner, first + corner + 1);
  };
  for (let triangle = 0; triangle < layout.triangles; triangle++) {
    const record = layout.records.subarray(triangle * RECORD, (triangle + 1) * RECORD);
    const [x, y, width, height] = record;
    const turn = record[META], role = record[META + 3];
    let polygon = [[x, y], [x + width, y], [x + width, y + height], [x, y + height]];
    if (role === FACE) {
      const name = `${x},${y}`, surface = charts.get(name);
      if (!surface) throw new Error(`Receiver layout of ${layout.id} states no surface map for the chart at ${name}.`);
      // The last number says the chart is one the shaders read through its surface map.
      const mapped = Float64Array.from(surface); mapped[SURFACE - 3] = 1;
      if (!faces.has(name)) { faces.add(name); emit(polygon, record, mapped); }
      emit([0, 1, 2].map(corner => [record[ATLAS_UVS + corner * 2] * layout.width, (1 - record[ATLAS_UVS + corner * 2 + 1]) * layout.height]), record, mapped);
      continue;
    }
    if (role > 0) {
      // Past the shared edge the atlas samples the pair's second triangle: where the legs' parameters sum to more than one.
      const rotated = (turn & 1) === 1;
      const edge = (point: number[]): number => {
        const s = (point[0] - x - gutter) / (width - 2 * gutter), t = (point[1] - y - gutter) / (height - 2 * gutter);
        return rotated ? t - s : s + t - 1;
      };
      polygon = clip(polygon, edge, role === 1 ? -1 : 1);
    }
    emit(polygon, record, none);
  }
  const buffer = new THREE.InterleavedBuffer(new Float32Array(vertices), STRIDE);
  const geometry = new THREE.BufferGeometry();
  const attribute = (name: string, size: number, offset: number) => geometry.setAttribute(name, new THREE.InterleavedBufferAttribute(buffer, size, offset));
  attribute("position", 3, 0);
  attribute("aRect", 4, 3 + RECT); attribute("aMeta", 4, 3 + META);
  for (let corner = 0; corner < 3; corner++) {
    attribute(`aP${corner}`, 3, 3 + POSITIONS + corner * 3);
    attribute(`aN${corner}`, 3, 3 + NORMALS + corner * 3);
  }
  // The third corner's surface UV with the first corner's atlas UV, then the other two corners' atlas UVs.
  attribute("aUv01", 4, 3 + UVS); attribute("aUv2A0", 4, 3 + UVS + 4); attribute("aA12", 4, 3 + ATLAS_UVS + 2);
  attribute("aSurface0", 4, 3 + RECORD); attribute("aSurface1", 4, 3 + RECORD + 4);
  geometry.setIndex(new THREE.BufferAttribute(new Uint32Array(indices), 1));
  return geometry;
}

/** A texture one pass of an atlas bake writes. */
export type AtlasAttachment = "base" | "properties" | "height" | "emissive" | "offset" | `live${number}`;

/**
 * A live operation as a baked material executes it. An argument that does not vary over the surface costs no texture:
 * a constant is a value, and one computed from parameters alone is its GLSL expression over the parameters' uniforms,
 * evaluated in the material's own shader (so it follows a parameter with no bake). The rest are channels of the bake's
 * live textures.
 */
export interface AtlasLiveOperation {
  id: string;
  arguments: Record<string, { value: number } | { glsl: string } | { texture: string; channel: number; scale: number; offset: number }>;
}

/**
 * What the evaluate pass of a program writes: only the textures the program has outputs for, then its live renderer
 * arguments, all from one compiled program.
 */
export interface AtlasProgramLayout {
  /** Names the layout; part of program and target cache keys. */
  key: string;
  attachments: AtlasAttachment[];
  /** Whether the height texture also holds the program's `receiver_normal` output. */
  orient: boolean;
  /** GLSL expressions of the live arguments that vary over the surface, four to a live attachment. */
  liveChannels: string[];
  liveOperations: AtlasLiveOperation[];
  /** Parameter uniforms the operations' GLSL arguments read, as GLSL declares them. */
  liveParameters: { uniform: string; type: string }[];
  /** Why the program cannot be baked on this device, if it cannot. */
  unsupported?: string;
  /**
   * The cutoff of a cutout program (`ProgramShader.alphaCutoff`): the base color texture then holds the program's
   * opacity in its alpha, and the baked look is drawn only where that reaches the cutoff.
   */
  cutout?: number;
}

/** What a live argument is to a bake, as the engine classifies it (`LiveArgument` in `program/glsl.rs`). */
type LiveClass = { constant: number } | { parameters: string[]; glsl: string } | { varying: string };

const CONSTANT = /^\(?\s*-?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?\s*\)?$/;
/** What an expression may call and still be a function of its arguments alone. */
const PURE = new Set(["mix", "clamp", "min", "max", "pow", "sqrt", "abs", "smoothstep", "step", "floor", "fract", "sign", "exp", "log", "sin", "cos", "float"]);

/** What the source assigns a field: the shader assigns every field once (in each of its forms), as `name = expression;`. */
function assignedIn(source: string, symbol: string): string | undefined {
  if (!/^\w+$/.test(symbol)) return undefined;
  const found = new Set([...source.matchAll(new RegExp(`\\b${symbol}\\s*=(?!=)\\s*([^;]+);`, "g"))].map(match => match[1].trim()));
  return found.size === 1 ? [...found][0] : undefined;
}

/**
 * The field a field only repeats (`ms_f20 = ms_f7;`), through any such chain, or the field itself. Arguments that
 * are one value then share one texture channel. (The record's classes name each argument's own field, so this is still
 * read from the source; an engine that names the repeated field in `varying` makes it a no-op.)
 */
function repeatedField(source: string, symbol: string): string {
  let value = assignedIn(source, symbol);
  for (let depth = 0; depth < 8 && value !== undefined && /^ms_f\d+$/.test(value) && assignedIn(source, value) !== undefined; depth++) { symbol = value; value = assignedIn(source, symbol); }
  return symbol;
}

/**
 * The classes of an operation's live arguments for a shader record that does not state them (an engine older than
 * `live[].classes`), read from the source: the shader assigns every field once, as `name = expression;`, and what a
 * field is assigned says whether it varies.
 */
function sourceLiveClasses(shader: ProgramShader, operation: ProgramShader["live"][number]): Record<string, LiveClass> {
  const source = shader.source;
  const assigned = (symbol: string) => assignedIn(source, symbol);
  const scalars = new Set([...source.matchAll(/uniform\s+float\s+(\w+)\s*;/g)].map(match => match[1]));
  const parameters = new Set(shader.parameters.map(parameter => parameter.uniform));
  return Object.fromEntries(Object.entries(operation.arguments).map(([name, expression]): [string, LiveClass] => {
    const symbol = repeatedField(source, expression.trim()), value = assigned(symbol);
    const text = value ?? symbol;
    if (CONSTANT.test(text)) return [name, { constant: Number(text.replace(/[()\s]/g, "")) }];
    // Computed from scalar parameters and constants alone: the material's shader evaluates it from the parameters' uniforms.
    const names = [...text.matchAll(/(?<![\w.])[A-Za-z_]\w*/g)].map(match => match[0]);
    if (value !== undefined && names.every(id => PURE.has(id) || (parameters.has(id) && scalars.has(id)))) {
      return [name, { parameters: [...new Set(names.filter(id => parameters.has(id)))], glsl: text }];
    }
    return [name, { varying: symbol }];
  }));
}

/** Lays out the evaluate pass of `shading`: the textures it writes and how its live renderer arguments reach a material. */
export function programLayout(shading: MaterialShading): AtlasProgramLayout {
  const shader = shading.shader;
  // A shader record is laid out once: every mesh of its material asks.
  let layout = layouts.get(shader);
  if (!layout) layouts.set(shader, layout = layoutOf(shader));
  return layout;
}

const layouts = new WeakMap<ProgramShader, AtlasProgramLayout>();

function layoutOf(shader: ProgramShader): AtlasProgramLayout {
  const { outputs } = shader;
  const orient = outputs.includes("receiver_normal");
  const attachments: AtlasAttachment[] = ["base", "properties", "height"];
  if (outputs.includes("emissive")) attachments.push("emissive");
  if (outputs.includes("normal_offset")) attachments.push("offset");
  // The GLSL type of each uniform, as the record declares them (a parameter a class names is a scalar: a float).
  const types = new Map((shader.uniforms ?? []).map(uniform => [uniform.name, uniform.type]));
  const liveChannels: string[] = [];
  const used = new Map<string, string>();
  const liveOperations: AtlasLiveOperation[] = shader.live.map(operation => {
    // The engine says what each argument is; a record without that (or without it for some argument) is read from its source.
    const stated = operation.classes;
    const classes = stated && Object.keys(operation.arguments).every(name => stated[name]) ? stated : sourceLiveClasses(shader, operation);
    return { id: operation.id, arguments: Object.fromEntries(Object.keys(operation.arguments).map(name => {
      const found = classes[name];
      if ("constant" in found) return [name, { value: found.constant }];
      // Computed from scalar parameters alone: the material's shader evaluates it from the parameters' uniforms.
      if ("glsl" in found) {
        for (const uniform of found.parameters) used.set(uniform, types.get(uniform) ?? "float");
        return [name, { glsl: found.glsl }];
      }
      const symbol = repeatedField(shader.source, found.varying);
      let channel = liveChannels.indexOf(symbol);
      if (channel < 0) channel = liveChannels.push(symbol) - 1;
      return [name, { texture: `gpu-live-${Math.floor(channel / 4)}`, channel: channel % 4, scale: 1, offset: 0 }];
    })) };
  });
  const liveTextures = Math.ceil(liveChannels.length / 4);
  for (let index = 0; index < liveTextures; index++) attachments.push(`live${index}`);
  const liveParameters = [...used].map(([uniform, type]) => ({ uniform, type }));
  const cutout = typeof shader.alphaCutoff === "number" && outputs.includes("opacity") ? shader.alphaCutoff : undefined;
  const layout: AtlasProgramLayout = { key: `${attachments.join("+")}${orient ? ":orient" : ""}${cutout === undefined ? "" : `:cutout${cutout}`}:${fnv(JSON.stringify([liveChannels, liveOperations]))}`,
    attachments, orient, liveChannels, liveOperations, liveParameters, ...(cutout === undefined ? {} : { cutout }) };
  return layout;
}

/** The outputs each texture of the evaluate pass holds (see `atlasFragment`); the live textures hold the live fields. */
const ATTACHMENT_OUTPUTS: Record<string, string[]> = {
  base: ["base_color", "opacity"], properties: ["roughness", "metalness"], height: ["height", "receiver_normal"], emissive: ["emissive"], offset: ["normal_offset"],
};

/**
 * The program's own uniforms that a value with the engine's stated reads depends on, or undefined when the record is
 * not one this understands (the value may then read anything). Parameters are uniforms already; a read seed is every
 * named seed's uniform (the record does not say which); images and CPU fields are named and found in the shader's
 * lists. Source edges and external planes reach a shader only through CPU fields, so a value that reads them reads
 * every CPU field that does, besides the fields it lists. Features and the parts of a sample are the receiver's, which
 * is in every key.
 */
function statedUniforms(shader: ProgramShader, read: Reads): Set<string> | undefined {
  if (typeof read !== "object" || read === null) return undefined;
  if ([read.parameters, read.images ?? [], read.cpuFields ?? [], read.planes ?? []].some(list => !Array.isArray(list))) return undefined;
  const found = new Set<string>(read.parameters);
  if (read.seed) for (const seed of shader.seeds) found.add(seed.uniform);
  for (const name of read.images ?? []) {
    const image = shader.images.find(image => image.name === name);
    // An image the shader does not bind is not on the GPU path (a CPU field reads it): the field's uniform carries it.
    if (image) found.add(image.uniform);
  }
  const planes = new Set(read.planes ?? []);
  for (const field of shader.cpuFields ?? []) {
    if (read.cpuFields?.includes(field.name) || (read.edges && field.edges) || field.planes?.some(plane => planes.has(plane))) found.add(field.uniform);
  }
  return found;
}

/**
 * The uniforms of the program (parameters, named seeds, images, CPU fields) that each texture of its evaluate pass
 * reads, by attachment; an attachment missing from the answer may read any of them. A bake keys each texture by the
 * values of what it reads (`GpuBaker.atlasNodes`), so a changed parameter draws only the textures it reaches. THE ONE
 * PLACE that says what an output depends on:
 *
 * - `shader.reads`, the dependencies the engine states (`ShaderReads` in `program/glsl.rs`): `outputs[name]` for each
 *   output the program sets and `live[operation][argument]` for each live argument. A texture reads what the outputs
 *   it holds read; a live texture reads what every argument given one of its channels reads (arguments that are one
 *   value share a channel). `statedUniforms` turns one such record into uniforms.
 * - else the record's entry points (`shader.entries`): each is the code of some outputs alone, helpers expanded, so
 *   the uniforms its text names are the ones those outputs can read. (`ms_evaluate_height` is part of the shader's
 *   source and carries no text; the relief entry covers the height texture.)
 * - else nothing: every texture is keyed by every value.
 *
 * Receiver inputs (features, bounds, gravity, mapping) are not program uniforms and are in every texture's key.
 */
export function attachmentReads(shader: ProgramShader, layout: AtlasProgramLayout): Partial<Record<AtlasAttachment, ReadonlySet<string>>> {
  const own = [...shader.parameters, ...shader.seeds, ...shader.images, ...(shader.cpuFields ?? [])].map(entry => entry.uniform);
  const reads: Partial<Record<AtlasAttachment, ReadonlySet<string>>> = {};
  const stated = shader.reads;
  if (stated) {
    // A record of another shape (not the engine's `outputs` by name) says nothing this can use: every texture may read anything.
    if (typeof stated !== "object" || typeof stated.outputs !== "object" || stated.outputs === null) return reads;
    for (const attachment of layout.attachments) {
      const found: (Reads | undefined)[] = [];
      if (attachment.startsWith("live")) {
        const texture = `gpu-live-${attachment.slice(4)}`;
        for (const operation of layout.liveOperations) {
          for (const [name, argument] of Object.entries(operation.arguments)) {
            if ("texture" in argument && argument.texture === texture) found.push(stated.live?.[operation.id]?.[name]);
          }
        }
      } else {
        // An output the program does not set reads nothing; one the engine does not list may read anything.
        for (const output of ATTACHMENT_OUTPUTS[attachment]) if (stated.outputs[output] || shader.outputs.includes(output)) found.push(stated.outputs[output]);
      }
      const uniforms = found.map(read => read && statedUniforms(shader, read));
      if (uniforms.every(set => set)) reads[attachment] = new Set(uniforms.flatMap(set => [...set!]));
    }
    return reads;
  }
  // The uniforms an entry point's text names, found once per entry.
  const named = new Map<string, Set<string>>();
  const namedBy = (entry: NonNullable<ProgramShader["entries"]>[number]): Set<string> => {
    let found = named.get(entry.function);
    if (!found) {
      const words = new Set(entry.source.match(/[A-Za-z_]\w*/g) ?? []);
      named.set(entry.function, found = new Set(own.filter(uniform => words.has(uniform))));
    }
    return found;
  };
  for (const attachment of layout.attachments) {
    const live = attachment.startsWith("live");
    const outputs = live ? ["live"] : ATTACHMENT_OUTPUTS[attachment];
    const entry = shader.entries?.find(entry => entry.source && (live ? entry.live : outputs.every(output => entry.outputs.includes(output))));
    if (entry) reads[attachment] = namedBy(entry);
  }
  return reads;
}

function fnv(text: string): string {
  let value = 0x811c9dc5;
  for (let index = 0; index < text.length; index++) value = Math.imul(value ^ text.charCodeAt(index), 0x01000193);
  return (value >>> 0).toString(16).padStart(8, "0");
}

/**
 * GLSL that assigns `target` the surface position a program reads at a point (`Atlas::sample`), from the names of the
 * point's world and object positions, its normal and its scaled UV, by `msMapping`: 0 UV, 1 object, 2 world, 3 anchored.
 * Anchored, the position is measured in the receiver's pattern anchor frame (`msAnchorFromWorld`: the anchor's axes as
 * rows and its origin taken off), and the plane it is projected on is chosen by the normal in that frame. The per-pixel
 * look (shade.ts) uses the same text, so a bake and the look it replaces agree on where the pattern is.
 */
export const SURFACE_OF = (world: string, object: string, normal: string, uv: string, target: string): string => `{
    vec3 msAt = msMapping == 2 ? ${world} : (msMapping == 3 ? (msAnchorFromWorld * vec4(${world}, 1.0)).xyz : ${object});
    vec3 msFacing = abs(msMapping == 1 ? mat3(msObjectFromWorld) * ${normal} : (msMapping == 3 ? mat3(msAnchorFromWorld) * ${normal} : ${normal}));
    ${target} = msMapping == 0 ? ${uv}
      : (msFacing.z >= max(msFacing.x, msFacing.y) ? msAt.xy : (msFacing.y >= msFacing.x ? msAt.xz : msAt.yz));
  }`;

/** The values of `msMapping`, in order. */
export const MAPPINGS = ["uv", "object", "world", "anchored"] as const;

/** A pattern anchor as its owner states it (`authoring::Anchor`): an origin and two axes spanning the pattern's plane, world meters. */
export interface PatternAnchor { origin: number[]; u: number[]; v: number[] }

/**
 * What takes a world point to a pattern anchor's frame, as sixteen numbers in column order: the anchor's axes made
 * unit and perpendicular (`Atlas::build`: u, v without its part along u, and their cross product) as rows, after its
 * origin is taken off. Without an anchor the frame is the world's.
 */
export function anchorFromWorld(anchor?: PatternAnchor | null): number[] {
  if (!anchor) return [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  const dot = (a: number[], b: number[]) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const unit = (a: number[]) => { const length = Math.hypot(a[0], a[1], a[2]) || 1; return a.map(value => value / length); };
  const u = unit(anchor.u), along = dot(anchor.v, u);
  const v = unit(anchor.v.map((value, axis) => value - u[axis] * along));
  const n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
  return [u[0], v[0], n[0], 0, u[1], v[1], n[1], 0, u[2], v[2], n[2], 0, -dot(u, anchor.origin), -dot(v, anchor.origin), -dot(n, anchor.origin), 1];
}

const RECORD_INPUTS = `
flat in vec4 vRect;
flat in vec4 vMeta;
flat in vec3 vP0;
flat in vec3 vP1;
flat in vec3 vP2;
flat in vec4 vLegMap;
flat in vec2 vLegOrigin;
flat in vec4 vSurface0;
flat in vec4 vSurface1;
`;

/** The chart texel a fragment is and the leg parameters it samples (\`Atlas::texel\`); \`msGutter\` is the chart margin. */
const TEXEL_LEGS = `
uniform vec2 msAtlas;
uniform float msGutter;
// Rows count from the atlas's upper edge; framebuffer row 0 is the lowest.
vec2 msAtlasTexel() { return vec2(floor(gl_FragCoord.x), msAtlas.y - 1.0 - floor(gl_FragCoord.y)); }
// Whether the chart is a flat face on a pattern anchor's lattice: every texel is its own lattice cell, inside the
// receiver or past its edge, and lies in the face's plane where the triangle's atlas UVs put it.
bool msOnLattice() { return vSurface1.w > 0.5; }
// The lattice point a texel of such a chart samples (\`Atlas::sample\` snaps the same way), from the chart's surface map.
vec2 msLatticeSurface(vec2 texel) {
  vec2 from = texel - vRect.xy - msGutter;
  vec2 at = vSurface0.xy + from.x * vSurface0.zw + from.y * vSurface1.xy;
  return (round(at / vSurface1.z - 0.5) + 0.5) * vSurface1.z;
}
// Parameters along the triangle's two legs. Gutter texels take the nearest inner texel's point; a chart padded to the
// minimum size snaps to its samples; a lone triangle's texels past its long edge project onto it. On the lattice
// nothing clamps: the parameters are the texel center's in the triangle's atlas triangle, which may lie outside it.
vec2 msLegs(vec2 texel) {
  if (msOnLattice()) {
    vec2 from = texel + 0.5 - vLegOrigin;
    return vec2(dot(vLegMap.xy, from), dot(vLegMap.zw, from));
  }
  vec2 inner = vRect.zw - 2.0 * msGutter;
  vec2 st = clamp((texel - vRect.xy + 0.5 - msGutter) / inner, 0.0, 1.0);
  vec2 count = vMeta.yz;
  st = mix(st, (min(floor(st * count), count - 1.0) + 0.5) / count, vec2(lessThan(count, inner)));
  int turn = int(vMeta.x + 0.5);
  vec2 legs = (turn & 1) == 1 ? vec2(st.y, 1.0 - st.x) : st;
  if (turn >= 2) legs = 1.0 - legs;
  else if (vMeta.w < 0.5 && legs.x + legs.y > 1.0) legs /= legs.x + legs.y;
  return legs;
}
`;

/** Draws chart rectangles given in atlas texels and hands each fragment its triangle's record. */
export const ATLAS_VERTEX = `
in vec4 aRect;
in vec4 aMeta;
in vec3 aP0;
in vec3 aP1;
in vec3 aP2;
in vec3 aN0;
in vec3 aN1;
in vec3 aN2;
in vec4 aUv01;
in vec4 aUv2A0;
in vec4 aA12;
in vec4 aSurface0;
in vec4 aSurface1;
uniform vec2 msAtlas;
flat out vec4 vLegMap;
flat out vec2 vLegOrigin;
flat out vec4 vSurface0;
flat out vec4 vSurface1;
flat out vec4 vRect;
flat out vec4 vMeta;
flat out vec3 vP0;
flat out vec3 vP1;
flat out vec3 vP2;
flat out vec3 vN0;
flat out vec3 vN1;
flat out vec3 vN2;
flat out vec4 vUv01;
flat out vec4 vUv2Footprints;
void main() {
  vRect = aRect; vMeta = aMeta;
  vP0 = aP0; vP1 = aP1; vP2 = aP2;
  vN0 = aN0; vN1 = aN1; vN2 = aN2;
  vUv01 = aUv01;
  // Samples along each leg: a texel's footprint is the leg over them (\`Atlas::sample\`).
  vec2 spans = (int(aMeta.x + 0.5) & 1) == 1 ? aMeta.zy : aMeta.yz;
  float footprint = max(length(aP1 - aP0) / spans.x, length(aP2 - aP0) / spans.y);
  float uvFootprint = max(length(aUv01.zw - aUv01.xy) / spans.x, length(aUv2A0.xy - aUv01.xy) / spans.y);
  vUv2Footprints = vec4(aUv2A0.xy, footprint, uvFootprint);
  // The triangle's corners in atlas texels (rows from the upper edge), and the map from a texel there to the
  // parameters along its legs: a point is the corner plus the legs times those.
  vec2 corner = vec2(aUv2A0.z, 1.0 - aUv2A0.w) * msAtlas;
  vec2 first = vec2(aA12.x, 1.0 - aA12.y) * msAtlas - corner, second = vec2(aA12.z, 1.0 - aA12.w) * msAtlas - corner;
  float area = first.x * second.y - first.y * second.x;
  vLegOrigin = corner;
  vLegMap = area != 0.0 ? vec4(second.y, -second.x, -first.y, first.x) / area : vec4(0.0);
  vSurface0 = aSurface0; vSurface1 = aSurface1;
  gl_Position = vec4(position.x / msAtlas.x * 2.0 - 1.0, 1.0 - position.y / msAtlas.y * 2.0, 0.0, 1.0);
}
`;

/**
 * The evaluate pass's fragment shader for a program's shader source and attachment layout. It has ONE color output and
 * a uniform (`msOutput`) choosing which of the layout's textures a draw writes, so a bake draws the charts once per
 * texture. That costs the program's pixels several times over (a millisecond or less each), and saves the compile:
 * ANGLE's Direct3D backend compiles a program's pixel shader for a single color output when it links, and compiles it
 * all over again, on the page's thread, at the first draw into several draw buffers. A single-output program is
 * complete when its parallel compile says so.
 */
export function atlasFragment(source: string, layout: AtlasProgramLayout): string {
  const writes: Record<string, string> = {
    // The base color target is an sRGB attachment: the hardware encodes these linear values on write.
    // A cutout's opacity rides in the alpha (linear; the baked look tests it against the cutoff).
    base: `vec4(clamp(ms_base_color, 0.0, 1.0), ${layout.cutout === undefined ? "1.0" : "clamp(ms_opacity, 0.0, 1.0)"})`,
    properties: "vec4(1.0, clamp(ms_roughness, 0.0, 1.0), clamp(ms_metalness, 0.0, 1.0), 1.0)",
    // Height in meters, with the world-space receiver_normal output beside it when the program has one.
    height: "vec4(ms_height, ms_receiver_normal)",
    emissive: "vec4(max(ms_emissive, vec3(0.0)), 1.0)",
    offset: "vec4(ms_normal_offset, 0.0)",
  };
  return evaluateFragment(source, layout.attachments.map(name => {
    const first = Number(name.slice(4)) * 4;
    return name.startsWith("live") ? `vec4(${[0, 1, 2, 3].map(channel => layout.liveChannels[first + channel] ?? "0.0").join(", ")})` : writes[name];
  }));
}

/**
 * What a PLANE of a weathering stack holds for a channel (`Plane::exact_external` in the engine): the program's value
 * unencoded and unclamped, for the layers above to read (`below.<channel>`). Scalars are in red.
 */
export const PLANE_VALUES: Record<string, string> = {
  base_color: "vec4(ms_base_color, 1.0)", emissive: "vec4(ms_emissive, 1.0)", normal_offset: "vec4(ms_normal_offset, 0.0)",
  roughness: "vec4(ms_roughness, 0.0, 0.0, 1.0)", metalness: "vec4(ms_metalness, 0.0, 0.0, 1.0)", height: "vec4(ms_height, 0.0, 0.0, 1.0)",
  opacity: "vec4(ms_opacity, 0.0, 0.0, 1.0)",
};

/**
 * The evaluate pass of one material of a weathering stack over a receiver's atlas: `values[msOutput]` (plane values,
 * `PLANE_VALUES`, and live arguments) of `source`. `below` names the CPU-field samplers of the layer's plan that read
 * the surface beneath (`below.<channel>`); on the GPU they are planes this pass graph drew, whose rows run from the
 * framebuffer's lowest (the engine's plan reads a CPU field by atlas row, from the upper edge), so their reads are
 * turned over. Throws when the plan reads one in a form this does not know.
 */
export function planeFragment(source: string, values: string[], below: string[]): string {
  let text = source;
  for (const uniform of below) {
    const read = new RegExp(`texelFetch\\(\\s*${uniform}\\s*,\\s*ms_texel\\s*,\\s*0\\s*\\)`, "g");
    if (!read.test(text)) throw new Error(`the layer's plan reads ${uniform} in a form the GPU stack does not know`);
    text = text.replace(read, `texelFetch(${uniform}, msPlaneTexel(ms_texel), 0)`);
  }
  const head = below.length ? "ivec2 msPlaneTexel(ivec2 texel) { return ivec2(texel.x, int(msAtlas.y) - 1 - texel.y); }\n" : "";
  return evaluateFragment(text, values, head);
}

/** Lattice cells per side of an intrinsic tile (`intrinsicTileFragment`), and the cell its first tile starts at. */
export const TILE_CELLS = 512;
export const TILE_START = -64;
/** Tiles a lattice mapping node reads at most (`LATTICE_MAPPING_FRAGMENT`). */
export const MAX_TILES = 8;

/**
 * An intrinsic tile: `source`'s intrinsic outputs (`Reads.intrinsic`: they read a sample's surface position and
 * footprint alone) evaluated at the lattice points of `TILE_CELLS` by `TILE_CELLS` cells of a lattice of step `msStep`
 * meters, from cell `msTileOrigin`, exactly as the evaluate pass gives them a texel on that lattice (`msLatticeSurface`).
 * No receiver is in it: every receiver whose texels lie on those cells reads the same texels (`LATTICE_MAPPING_FRAGMENT`).
 */
export function intrinsicTileFragment(source: string, values: string[]): string {
  const assigns = values.map((value, index) => `  ${index > 0 ? "else " : ""}if (msOutput == ${index}) msOut = ${value};`).join("\n");
  return `
uniform vec2 msTileOrigin;
uniform float msStep;
uniform int msOutput;
layout(location = 0) out vec4 msOut;
${source}
void main() {
  vec2 cell = msTileOrigin + floor(gl_FragCoord.xy);
  ms_surface = (cell + 0.5) * msStep;
  ms_footprint = msStep;
  ms_world = vec3(0.0); ms_object = vec3(0.0); ms_normal = vec3(0.0, 0.0, 1.0); ms_uv = vec2(0.0); ms_uv_footprint = 0.0;
  ms_texel = ivec2(0);
  ms_evaluate();
${assigns}
}
`;
}

/**
 * A lattice mapping: every texel of a receiver whose charts are all whole flat faces on a pattern anchor's lattice
 * reads the intrinsic tile that holds its lattice cell (the cell `msLatticeSurface` samples), texel for texel.
 */
export const LATTICE_MAPPING_FRAGMENT = `
${RECORD_INPUTS}
${TEXEL_LEGS}
${Array.from({ length: MAX_TILES }, (_, index) => `uniform highp sampler2D msTile${index};`).join("\n")}
uniform vec2 msTileOrigins[${MAX_TILES}];
uniform int msTiles;
layout(location = 0) out vec4 msOut;
void main() {
  vec2 texel = msAtlasTexel();
  vec2 from = texel - vRect.xy - msGutter;
  vec2 at = vSurface0.xy + from.x * vSurface0.zw + from.y * vSurface1.xy;
  vec2 cell = round(at / vSurface1.z - 0.5);
  vec4 value = vec4(0.0);
${Array.from({ length: MAX_TILES }, (_, index) => `  if (${index} < msTiles) {
    vec2 local = cell - msTileOrigins[${index}];
    if (all(greaterThanEqual(local, vec2(0.0))) && all(lessThan(local, vec2(${TILE_CELLS}.0)))) value = texelFetch(msTile${index}, ivec2(local), 0);
  }`).join("\n")}
  msOut = value;
}
`;

/**
 * The maps of a stack composed from its planes over a receiver's atlas: base color (encoded by the sRGB target),
 * packed properties and emissive, from the plane of the material that last changed each channel, or the channel's
 * default where none did (`STACK_CHANNELS` in the engine). Drawn over the charts only, like the evaluate pass.
 */
export const STACK_COMPOSE_FRAGMENT = `
uniform vec2 msAtlas;
uniform highp sampler2D msColor;
uniform highp sampler2D msRoughness;
uniform highp sampler2D msMetalness;
uniform highp sampler2D msEmissive;
uniform bool msHasColor;
uniform bool msHasRoughness;
uniform bool msHasMetalness;
uniform int msOutput;
layout(location = 0) out vec4 msOut;
void main() {
  ivec2 at = ivec2(gl_FragCoord.xy);
  if (msOutput == 0) msOut = vec4(clamp(msHasColor ? texelFetch(msColor, at, 0).rgb : vec3(0.5), 0.0, 1.0), 1.0);
  else if (msOutput == 1) msOut = vec4(1.0, clamp(msHasRoughness ? texelFetch(msRoughness, at, 0).r : 1.0, 0.0, 1.0),
    clamp(msHasMetalness ? texelFetch(msMetalness, at, 0).r : 0.0, 0.0, 1.0), 1.0);
  else msOut = vec4(max(texelFetch(msEmissive, at, 0).rgb, vec3(0.0)), 1.0);
}
`;

/**
 * The lattice charts of a receiver's atlas, when every triangle lies on a pattern anchor's lattice (a whole flat face,
 * role 3) at one step: the step and the intrinsic tiles (`TILE_CELLS` cells, from `TILE_START`) its texels read,
 * as tile indices. Null when some chart is not on the lattice, the steps differ, or more than `MAX_TILES` are read.
 */
export function latticeTiles(layout: AtlasLayout): { step: number; tiles: [number, number][] } | null {
  if (!layout.triangles) return null;
  const charts = new Map<string, Float64Array>();
  for (let chart = 0; chart + SURFACE <= layout.surface.length; chart += SURFACE) {
    charts.set(`${layout.surface[chart]},${layout.surface[chart + 1]}`, layout.surface.subarray(chart + 2, chart + SURFACE));
  }
  let step: number | undefined;
  const tiles = new Map<string, [number, number]>();
  const seen = new Set<string>();
  for (let triangle = 0; triangle < layout.triangles; triangle++) {
    const record = layout.records.subarray(triangle * RECORD, (triangle + 1) * RECORD);
    if (record[META + 3] !== FACE) return null;
    const name = `${record[0]},${record[1]}`;
    if (seen.has(name)) continue;
    seen.add(name);
    const map = charts.get(name);
    if (!map) return null;
    // The lattice step as the bake geometry carries it (a float attribute).
    const footprint = Math.fround(map[6]);
    if (!(footprint > 0) || (step !== undefined && footprint !== step)) return null;
    step = footprint;
    const [, , width, height] = record;
    const low = [Infinity, Infinity], high = [-Infinity, -Infinity];
    for (const fx of [-layout.gutter, width - 1 - layout.gutter]) for (const fy of [-layout.gutter, height - 1 - layout.gutter]) {
      for (let axis = 0; axis < 2; axis++) {
        const at = map[axis] + fx * map[2 + axis] + fy * map[4 + axis];
        // A cell either side of the one the corner rounds to: the shader rounds in single precision.
        const cell = Math.round(at / footprint - 0.5);
        low[axis] = Math.min(low[axis], cell - 1); high[axis] = Math.max(high[axis], cell + 1);
      }
    }
    for (let i = Math.floor((low[0] - TILE_START) / TILE_CELLS); i <= Math.floor((high[0] - TILE_START) / TILE_CELLS); i++) {
      for (let j = Math.floor((low[1] - TILE_START) / TILE_CELLS); j <= Math.floor((high[1] - TILE_START) / TILE_CELLS); j++) {
        tiles.set(`${i},${j}`, [i, j]);
        if (tiles.size > MAX_TILES) return null;
      }
    }
  }
  return step === undefined ? null : { step, tiles: [...tiles.values()] };
}

/** The program's own uniforms the values of `outputs` read (see `attachmentReads`), or undefined when not stated. */
export function outputReads(shader: ProgramShader, outputs: string[]): ReadonlySet<string> | undefined {
  const stated = shader.reads;
  if (!stated || typeof stated !== "object" || typeof stated.outputs !== "object" || stated.outputs === null) return undefined;
  const sets = outputs.map(output => stated.outputs[output] && statedUniforms(shader, stated.outputs[output]));
  return sets.every(set => set) ? new Set(sets.flatMap(set => [...set!])) : undefined;
}

/** Whether the engine states that an output is intrinsic (`Reads.intrinsic`). */
export function outputIntrinsic(shader: ProgramShader, output: string): boolean {
  const read = shader.reads?.outputs?.[output];
  return typeof read === "object" && read !== null && read.intrinsic === true;
}

/** The CPU fields an output reads, by name, or undefined when the record does not say. */
export function outputCpuFields(shader: ProgramShader, output: string): string[] | undefined {
  const read = shader.reads?.outputs?.[output];
  return typeof read === "object" && read !== null && Array.isArray(read.cpuFields) ? read.cpuFields : undefined;
}

/** The evaluate pass's fragment shader over a receiver's atlas: the program evaluated at each texel, writing `values[msOutput]`. */
function evaluateFragment(source: string, values: string[], head = ""): string {
  const outputs = "uniform int msOutput;\nlayout(location = 0) out vec4 msOut;";
  const assigns = values.map((value, index) => `  ${index > 0 ? "else " : ""}if (msOutput == ${index}) msOut = ${value};`).join("\n");
  return `
${RECORD_INPUTS}
flat in vec3 vN0;
flat in vec3 vN1;
flat in vec3 vN2;
flat in vec4 vUv01;
flat in vec4 vUv2Footprints;
uniform mat4 msObjectFromWorld;
uniform mat4 msAnchorFromWorld;
uniform vec2 msExtent;
uniform int msMapping;
${TEXEL_LEGS}
${outputs}
${head}${source}
void main() {
  vec2 texel = msAtlasTexel();
  vec2 legs = msLegs(texel);
  float corner = 1.0 - legs.x - legs.y;
  ms_world = vP0 * corner + vP1 * legs.x + vP2 * legs.y;
  ms_object = (msObjectFromWorld * vec4(ms_world, 1.0)).xyz;
  ms_normal = normalize(vN0 * corner + vN1 * legs.x + vN2 * legs.y);
  ms_uv = vUv01.xy * corner + vUv01.zw * legs.x + vUv2Footprints.xy * legs.y;
  ${SURFACE_OF("ms_world", "ms_object", "ms_normal", "ms_uv * msExtent", "ms_surface")}
  ms_footprint = vUv2Footprints.z;
  // On a pattern anchor's lattice the pattern is read at the lattice point itself and at the lattice step.
  if (msOnLattice()) { ms_surface = msLatticeSurface(texel); ms_footprint = vSurface1.z; }
  ms_uv_footprint = vUv2Footprints.w;
  ms_texel = ivec2(texel);
  ms_evaluate();
${assigns}
}
`;
}

/**
 * A mapping pass: every texel of a receiver's atlas reads a shared INTRINSIC texture (an appearance baked once, over a
 * rectangle of surface coordinates, with no receiver in its key) at the surface coordinate the evaluate pass would
 * give the program there (`ms_surface`). The intrinsic texture is addressed in meters: `msIntrinsicOrigin` is the
 * surface coordinate of its UV origin and `msIntrinsicSize` what it covers; its wrap mode says what lies beyond. The
 * mip is chosen from the texel's footprint, not from screen derivatives, which break at chart borders.
 */
export const ATLAS_MAPPING_FRAGMENT = `
${RECORD_INPUTS}
flat in vec3 vN0;
flat in vec3 vN1;
flat in vec3 vN2;
flat in vec4 vUv01;
flat in vec4 vUv2Footprints;
uniform mat4 msObjectFromWorld;
uniform mat4 msAnchorFromWorld;
uniform vec2 msExtent;
uniform int msMapping;
${TEXEL_LEGS}
uniform sampler2D msIntrinsic;
uniform vec2 msIntrinsicOrigin;
uniform vec2 msIntrinsicSize;
uniform float msIntrinsicDensity;
layout(location = 0) out vec4 msOut;
void main() {
  vec2 texel = msAtlasTexel();
  vec2 legs = msLegs(texel);
  float corner = 1.0 - legs.x - legs.y;
  vec3 world = vP0 * corner + vP1 * legs.x + vP2 * legs.y;
  vec3 normal = normalize(vN0 * corner + vN1 * legs.x + vN2 * legs.y);
  vec2 uv = vUv01.xy * corner + vUv01.zw * legs.x + vUv2Footprints.xy * legs.y;
  vec3 object = (msObjectFromWorld * vec4(world, 1.0)).xyz;
  vec2 surface;
  ${SURFACE_OF("world", "object", "normal", "uv * msExtent", "surface")}
  float footprint = vUv2Footprints.z;
  if (msOnLattice()) { surface = msLatticeSurface(texel); footprint = vSurface1.z; }
  float lod = max(0.0, log2(max(footprint * msIntrinsicDensity, 1e-6)));
  msOut = textureLod(msIntrinsic, (surface - msIntrinsicOrigin) / msIntrinsicSize, lod);
}
`;

/**
 * The normals pass: the height's differences one sample along each leg (\`bake_rows\` in \`authoring/material.rs\`),
 * resolved in the triangle's tangent frame (\`Atlas::normal\`, \`Atlas::frame\`), then turned and bent by the program's
 * receiver-normal outputs. A step that would leave the chart is taken backwards instead (the CPU evaluates the point
 * beyond the chart; a preview has no texel for it).
 */
export const ATLAS_NORMAL_FRAGMENT = `
${RECORD_INPUTS}
${TEXEL_LEGS}
uniform sampler2D msHeightMap;
uniform sampler2D msOffsetMap;
uniform bool msOrient;
uniform bool msBend;
layout(location = 0) out vec4 msNormalOut;
${RELIEF_ADJUST}
${NORMAL_ENCODE}
vec4 msHeightAt(ivec2 texel) { return texelFetch(msHeightMap, ivec2(texel.x, int(msAtlas.y) - 1 - texel.y), 0); }
vec3 msRelief(ivec2 texel) {
  // On a pattern anchor's lattice every texel of the chart is a sample, the gutter's too, and the chart's frame is its
  // rectangle's (\`Atlas::legs\`): atlas +x and +y, whatever way the triangle's own legs run.
  bool lattice = msOnLattice();
  int margin = lattice ? 0 : int(msGutter);
  ivec2 low = ivec2(vRect.xy) + margin, high = ivec2(vRect.xy + vRect.zw) - margin - 1;
  texel = clamp(texel, low, high);
  int turn = lattice ? 0 : int(vMeta.x + 0.5);
  // Atlas directions (x right, y down the rows) of a step along the first and the second leg.
  ivec2 first = turn == 0 ? ivec2(1, 0) : turn == 1 ? ivec2(0, 1) : turn == 2 ? ivec2(-1, 0) : ivec2(0, -1);
  ivec2 second = turn == 0 ? ivec2(0, 1) : turn == 1 ? ivec2(-1, 0) : turn == 2 ? ivec2(0, -1) : ivec2(1, 0);
  vec4 center = msHeightAt(texel);
  ivec2 a = texel + first, b = texel + second;
  float du = all(greaterThanEqual(a, low)) && all(lessThanEqual(a, high)) ? msHeightAt(a).r - center.r
    : center.r - msHeightAt(clamp(texel - first, low, high)).r;
  float dv = all(greaterThanEqual(b, low)) && all(lessThanEqual(b, high)) ? msHeightAt(b).r - center.r
    : center.r - msHeightAt(clamp(texel - second, low, high)).r;
  vec2 spans = (turn & 1) == 1 ? vMeta.zy : vMeta.yz;
  vec3 u = vP1 - vP0, v = vP2 - vP0;
  if (lattice) {
    // The plane's change one texel along atlas +x and +y, from the triangle's legs: a step of one sample each.
    vec3 across = u * vLegMap.x + v * vLegMap.z, down = u * vLegMap.y + v * vLegMap.w;
    u = across; v = down; spans = vec2(1.0);
  }
  vec3 t = normalize(u), n = normalize(cross(u, v)), bitangent = cross(n, t);
  float gt = du * spans.x / dot(u, t);
  float gb = (dv * spans.y - gt * dot(v, t)) / dot(v, bitangent);
  vec3 world = normalize(n - gt * t - gb * bitangent);
  // The tangent space the normal map is read in: x along atlas +x and y up the image, by the triangle's turn.
  vec3 frameX = turn == 0 ? t : turn == 1 ? -bitangent : turn == 2 ? -t : bitangent;
  vec3 frameY = turn == 0 ? -bitangent : turn == 1 ? -t : turn == 2 ? bitangent : t;
  vec3 relief = vec3(dot(world, frameX), dot(world, frameY), dot(world, n));
  ivec2 at = ivec2(texel.x, int(msAtlas.y) - 1 - texel.y);
  vec3 offset = msBend ? texelFetch(msOffsetMap, at, 0).rgb : vec3(0.0);
  return msAdjustRelief(relief, center.gba, offset, frameX, frameY, n, msOrient, msBend);
}
void main() {
  ivec2 texel = ivec2(msAtlasTexel());
  vec3 n = msRelief(texel);
  vec3 before = msRelief(texel - ivec2(1, 0));
  // A receiver normal facing away from the receiver cannot be encoded; the relief is kept unturned there.
  msNormalOut = vec4(msEncodeNormal(n, before, texel), 1.0);
}
`;
