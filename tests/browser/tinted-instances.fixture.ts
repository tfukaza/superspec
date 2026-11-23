/** A synthetic pack of shared part prototypes, as a publisher would write them: one part
 * mesh placed by `EXT_mesh_gpu_instancing` nodes, some with a per-instance `_COLOR_0`
 * tint and some without, and an untinted instanced window pane. The part's primitive
 * carries `_MS_ROLE`: its left quad (the "case") takes the instance tint and its right
 * quad (the "bracket") keeps its own colour. glTF Y-up metres. Runs in Node and a browser. */

export type Vec3 = [number, number, number];
export interface Batch { node: string; translations: Vec3[]; tints?: Vec3[] }

/** Linear vertex colours of the part, authored here and nowhere else. */
export const CASE_SHADE: Vec3 = [0.8, 0.8, 0.8];
export const BRACKET_COLOUR: Vec3 = [0.1, 0.6, 0.9];
/** The case spans x in [-0.5, 0] and the bracket x in [0, 0.5], both y in [0, 0.5], facing +Z. */
export const PART_SIZE = { width: 1, height: 0.5 };

export const defaultParts: Batch[] = [
  { node: "parts-a", translations: [[-2, 0, -5], [-1, 0, -30], [1, 0, -60]], tints: [[1, 0.2, 0.2], [0.2, 1, 0.2], [0.2, 0.2, 1]] },
  { node: "parts-b", translations: [[2, 0, -6], [5, 0, -31], [7, 0, -61]], tints: [[0.9, 0.8, 0.1], [0.1, 0.9, 0.8], [0.8, 0.1, 0.9]] },
  { node: "plain-a", translations: [[-2, 2, -5], [-1, 2, -30]] },
  { node: "plain-b", translations: [[2, 2, -6], [5, 2, -31]] },
];
export const defaultPanes: Batch[] = [
  { node: "glass-a", translations: [[-2, 4, -80], [0, 4, -80]] },
  { node: "glass-b", translations: [[2, 4, -80], [4, 4, -80]] },
];

export function tintedPartsGlb(parts: Batch[] = defaultParts, panes: Batch[] = defaultPanes,
  options: { roles?: boolean; partExtras?: Record<string, unknown>; lod?: string } = {}): ArrayBuffer {
  const chunks: Uint8Array[] = [], bufferViews: object[] = [], accessors: object[] = [];
  let length = 0;
  const accessor = (data: Float32Array | Uint16Array | Uint8Array, type: "SCALAR" | "VEC3", extra: object = {}) => {
    const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    bufferViews.push({ buffer: 0, byteOffset: length, byteLength: bytes.length });
    chunks.push(bytes);
    length += bytes.length;
    const pad = (4 - length % 4) % 4;
    if (pad) { chunks.push(new Uint8Array(pad)); length += pad; }
    const componentType = data instanceof Float32Array ? 5126 : data instanceof Uint16Array ? 5123 : 5121;
    accessors.push({ bufferView: bufferViews.length - 1, componentType, count: data.length / (type === "VEC3" ? 3 : 1), type, ...extra });
    return accessors.length - 1;
  };
  const { width: w, height: h } = PART_SIZE;
  const quad = (x0: number, x1: number) => [x0, 0, 0, x1, 0, 0, x1, h, 0, x0, h, 0];
  const partPosition = accessor(new Float32Array([...quad(-w / 2, 0), ...quad(0, w / 2)]), "VEC3", { min: [-w / 2, 0, 0], max: [w / 2, h, 0] });
  const normals = (count: number) => new Float32Array(Array.from({ length: count }, () => [0, 0, 1]).flat());
  const partNormal = accessor(normals(8), "VEC3");
  const partColour = accessor(new Float32Array([...Array(4).fill(CASE_SHADE), ...Array(4).fill(BRACKET_COLOUR)].flat()), "VEC3");
  const partRole = accessor(new Uint8Array([1, 1, 1, 1, 0, 0, 0, 0]), "SCALAR");
  const partIndices = accessor(new Uint16Array([0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]), "SCALAR");
  const panePosition = accessor(new Float32Array(quad(-0.5, 0.5)), "VEC3", { min: [-0.5, 0, 0], max: [0.5, h, 0] });
  const paneNormal = accessor(normals(4), "VEC3");
  const paneIndices = accessor(new Uint16Array([0, 1, 2, 0, 2, 3]), "SCALAR");
  const node = (mesh: number) => ({ node: name, translations, tints }: Batch) => ({
    name, mesh, extensions: { EXT_mesh_gpu_instancing: { attributes: {
      TRANSLATION: accessor(new Float32Array(translations.flat()), "VEC3"),
      ...(tints ? { _COLOR_0: accessor(new Float32Array(tints.flat()), "VEC3") } : {}),
    } } } });
  const nodes = [...parts.map(node(0)), ...panes.map(node(1))];
  const json = {
    asset: { version: "2.0" }, scene: 0, scenes: [{ nodes: nodes.map((_, index) => index), ...(options.lod ? { extras: { cityLod: options.lod } } : {}) }],
    extensionsUsed: ["EXT_mesh_gpu_instancing"], nodes,
    meshes: [
      { name: "part", primitives: [{ material: 0, indices: partIndices, attributes: { POSITION: partPosition, NORMAL: partNormal,
        COLOR_0: partColour, ...(options.roles === false ? {} : { _MS_ROLE: partRole }) } }] },
      { name: "pane", primitives: [{ material: 1, indices: paneIndices, attributes: { POSITION: panePosition, NORMAL: paneNormal } }] },
    ],
    materials: [
      // The part is a sheet: drawn from both sides, so it also casts a shadow away from a light it faces.
      { name: "parts", doubleSided: true, pbrMetallicRoughness: { metallicFactor: 0, roughnessFactor: 1 }, extras: { worldSpecPartAtlas: true, ...options.partExtras } },
      { name: "glass", alphaMode: "BLEND", doubleSided: true, pbrMetallicRoughness: { baseColorFactor: [0.6, 0.8, 0.9, 0.4] },
        extras: { worldSpecSurface: { recipe: "architectural-glazing" } } },
    ],
    accessors, bufferViews, buffers: [{ byteLength: length }],
  };
  const text = new TextEncoder().encode(JSON.stringify(json));
  const jsonLength = Math.ceil(text.length / 4) * 4;
  const glb = new Uint8Array(12 + 8 + jsonLength + 8 + length), view = new DataView(glb.buffer);
  view.setUint32(0, 0x46546c67, true); view.setUint32(4, 2, true); view.setUint32(8, glb.length, true);
  view.setUint32(12, jsonLength, true); view.setUint32(16, 0x4e4f534a, true);
  glb.fill(0x20, 20, 20 + jsonLength);
  glb.set(text, 20);
  view.setUint32(20 + jsonLength, length, true); view.setUint32(24 + jsonLength, 0x004e4942, true);
  let at = 28 + jsonLength;
  for (const chunk of chunks) { glb.set(chunk, at); at += chunk.length; }
  return glb.buffer;
}
