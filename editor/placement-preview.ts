import type { Manifest, SourceLocation } from "../browser/model/packet.ts";
import type { Footprint, PlacementMove } from "../browser/renderer/placement-preview.ts";

interface CityNode {
  kind: string;
  id: string;
  source: SourceLocation;
  children?: unknown[];
  attributes: {
    position?: number[]; count?: number; step?: number[]; heading?: number;
    width?: number; depth?: number; footprint?: number[][]; model?: string;
  };
}
interface PlacementTag {
  start: number;
  end: number;
  node: CityNode;
  fields: Map<string, string>;
  owners: string[];
  origins: number[][];
}

// Routed work and vegetation can affect receivers or visibility outside their own placement.
const independent = new Set(["building", "pillar", "wall", "ceiling-bulkhead", "ceiling-twin-strip",
  "wall-extraction-grille", "ceiling-girder", "utility-cabinet", "passage-sign-lamp"]);
const metric = /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/;
function metres(text: string): number | undefined {
  if (!metric.test(text) || text.trim() !== text) return;
  const value = Number(text);
  if (!Number.isFinite(value) || Math.abs(value) > 1_000_000) return;
  // Match the AML reader's millimeter rounding, including negative ties.
  const scaled = Math.abs(value) * 1000;
  return Math.sign(value) * Math.floor(scaled + 0.5 + Number.EPSILON * Math.max(1, scaled)) / 1000;
}

function coordinates(text: string, dimensions: number): number[] | undefined {
  const parts = text.replace(/^[ \t\r\n]+|[ \t\r\n]+$/g, "").split(/[ \t\r\n]+/);
  if (parts.length !== dimensions) return;
  const values = parts.map(metres);
  return values.every(v => v !== undefined) ? values : undefined;
}

function footprint(fields: Map<string, string>): Footprint | undefined {
  if (fields.has("footprint")) {
    if (fields.has("width") || fields.has("depth")) return;
    const points = fields.get("footprint")!.split(";").map(p => coordinates(p, 2));
    if (points.length !== 4 || !points.every(p => p !== undefined)) return;
    for (let i = 0; i < 4; i++) {
      const a = points[i], b = points[(i + 1) % 4], c = points[(i + 2) % 4];
      const x = b[0] - a[0], y = b[1] - a[1], squared = x*x + y*y;
      if (squared < 36 || squared > 196 || x * (c[1] - b[1]) - y * (c[0] - b[0]) <= 0) return;
    }
    return points;
  }
  const width = metres(fields.get("width") ?? "9.8"), depth = metres(fields.get("depth") ?? "7");
  if (width === undefined || width < 6 || width > 14 || depth === undefined || depth < 6 || depth > 14) return;
  return [[0, 0], [width, 0], [width, depth], [0, depth]];
}

/** Read only the opening tag of a compiler-located placement. Unknown or malformed syntax defers to Rust. */
function opening(source: string, start: number, kind: string) {
  const tag = "<" + kind;
  if (!source.startsWith(tag, start)) return;
  const attr = /[ \t\r\n]+([\w:.-]+)[ \t\r\n]*=[ \t\r\n]*(["'])(.*?)\2/gsy;
  let cursor = start + tag.length;
  const fields = new Map<string, string>();
  while (true) {
    attr.lastIndex = cursor;
    const field = attr.exec(source);
    if (!field) break;
    if (fields.has(field[1]) || field[3].includes("<")) return;
    fields.set(field[1], field[3]);
    cursor = attr.lastIndex;
  }
  const close = /[ \t\r\n]*\/?>/y;
  close.lastIndex = cursor;
  if (!close.exec(source)) return;
  return { fields, end: close.lastIndex, selfClosing: source[close.lastIndex - 2] === "/" };
}

export class PlacementPreview {
  private source = "";
  private tags: PlacementTag[] = [];

  accept(source: string, manifest: Manifest): void {
    this.source = source;
    this.tags = [];
    for (const section of manifest.architecture?.metadata ?? []) {
      if (section.kind !== "city") continue;
      for (const node of (section as unknown as { children: CityNode[] }).children) {
        const { position, count = 1, step = [0, 0, 0] } = node.attributes;
        if (!independent.has(node.kind) || !position || !Number.isInteger(count) || count < 1 || count > 1024) continue;
        const start = node.source.offset, tag = opening(source, start, node.kind);
        if (!tag || tag.fields.get("id") !== node.id) continue;
        const xyz = coordinates(tag.fields.get("position") ?? "", 3);
        if (!xyz || xyz.some((v, i) => v !== position[i])) continue;
        this.tags.push({ start, ...tag, node,
          owners: Array.from({ length: count }, (_, i) => "city/" + node.id + (count === 1 ? "" : "/" + (i + 1))),
          origins: Array.from({ length: count }, (_, i) => position.map((v, k) => v + i * step[k])),
        });
      }
    }
  }

  /** One admitted placement can be translated or reshaped, including adding its previously default dimensions. */
  moves(draft: string): PlacementMove[] {
    if (!this.tags.length || draft === this.source) return [];
    let first = 0;
    const common = Math.min(draft.length, this.source.length);
    // Most of a city document precedes a late lot. Compare blocks in the string engine before scanning its edited tag.
    while (first + 4096 <= common && draft.startsWith(this.source.slice(first, first + 4096), first)) first += 4096;
    while (first < common && draft.charCodeAt(first) === this.source.charCodeAt(first)) first++;
    const tag = this.tags.find(t => first >= t.start && first < t.end);
    if (!tag) return [];
    const edited = opening(draft, tag.start, tag.node.kind);
    if (!edited || edited.end !== tag.end + draft.length - this.source.length
      || edited.selfClosing !== (this.source[tag.end - 2] === "/") || !draft.endsWith(this.source.slice(tag.end))) return [];
    const building = tag.node.kind === "building" && !tag.node.attributes.model && !tag.node.children?.length;
    for (const key of new Set([...edited.fields.keys(), ...tag.fields.keys()])) {
      const editable = key === "position" || building && ["width", "depth", "footprint"].includes(key);
      if (!editable && edited.fields.get(key) !== tag.fields.get(key)) return [];
    }
    const xyz = coordinates(edited.fields.get("position") ?? "", 3);
    if (!xyz) return [];
    const translation = xyz.map((v, i) => v - tag.origins[0][i]);
    const move: PlacementMove = { owners: tag.owners, translation };
    if (building && ["width", "depth", "footprint"].some(key => edited.fields.get(key) !== tag.fields.get(key))) {
      const from = footprint(tag.fields), to = footprint(edited.fields);
      if (!from || !to) return [];
      if (from.some((p, i) => p.some((v, k) => v !== to[i][k]))) {
        move.footprint = { from, to, origins: tag.origins, heading: tag.node.attributes.heading ?? 0 };
        return [move];
      }
    }
    return translation.some(v => v !== 0) ? [move] : [];
  }
}
