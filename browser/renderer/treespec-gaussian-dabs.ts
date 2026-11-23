import { Matrix4, Quaternion, Vector3 } from "three";
import { batchAttribute, PLACEMENT_FLOATS, type SceneBatch, type ScenePacket } from "../model/packet.ts";
import { densityField, outward, sampleDensity, type DensityField } from "./leaf-density.ts";
import { treeSpecGaussianCoreSites } from "./treespec-gaussian-core.ts";
import {
  reshapeGaussianClumpNormal, reshapeGaussianClumpPoint, treeSpecGaussianClumpForms, type TreeSpecGaussianClumpForm,
} from "./treespec-gaussian-clump-form.ts";
import { treeSpecGaussianSurfaceTufts } from "./treespec-gaussian-surface-tufts.ts";

const leafColors = ["#6c8149", "#708649", "#74884a", "#788a4b", "#7d8d4b", "#818f4e"];
const bladeColors = ["#384e32", "#4c6037", "#5d743e", "#778946", "#96a153", "#afaf61", "#c0ad6b"];
const leafDabRollLimits = [0.50, 0.21];

function cellRandom(x: number, y: number, z: number, seed: number): number {
  let bits = Math.imul(x, 0x27d4eb2d) ^ Math.imul(y, 0x165667b1)
    ^ Math.imul(z, 0xc2b2ae35) ^ Math.imul(seed, 0x9e3779b1);
  bits = Math.imul(bits ^ (bits >>> 15), 0x85ebca6b);
  return ((bits ^ (bits >>> 13)) >>> 0) / 4294967296;
}

function smoothNoise(x: number, y: number, z: number, seed: number): number {
  const i = Math.floor(x), j = Math.floor(y), k = Math.floor(z);
  const fade = (t: number) => t * t * (3 - 2 * t);
  const fx = fade(x - i), fy = fade(y - j), fz = fade(z - k);
  let result = 0;
  for (let dz = 0; dz < 2; dz++) {
    for (let dy = 0; dy < 2; dy++) {
      for (let dx = 0; dx < 2; dx++) {
        const weight = (dx ? fx : 1 - fx) * (dy ? fy : 1 - fy) * (dz ? fz : 1 - fz);
        result += weight * cellRandom(i + dx, j + dy, k + dz, seed);
      }
    }
  }
  return result;
}

/** Broad pigment variation in plant space, shared by dabs and coarse canopy shells. */
export function treeSpecCanopyHue(x: number, y: number, z: number): number {
  const broad = smoothNoise(0.9 * x + 0.25 * z, 0.9 * y - 0.16 * z, 1.2 * z, 43);
  const fine = smoothNoise(1.8 * x, 1.8 * y, 2.4 * z, 47);
  return 0.78 * (2 * (0.8 * broad + 0.2 * fine) - 1);
}

function leafHueAt({ x, y, z }: Vector3): number {
  const canopyHue = treeSpecCanopyHue(x, y, z);
  const leafHue = 2 * cellRandom(Math.round(1024 * x), Math.round(1024 * y), Math.round(1024 * z), 59) - 1;
  return canopyHue + Math.min(0.52, 1 - Math.abs(canopyHue)) * leafHue;
}

/** Draw stretched gaps and shorter breaks across native sprays with a pair of coherent scales. */
function coverAt({ x, y, z }: Vector3): number {
  const broad = smoothNoise(1.5 * x + 0.8 * z, 1.6 * y - 0.7 * z, 2.7 * z, 21);
  const fine = smoothNoise(3.1 * x - 1.1 * z, 3.3 * y + 0.9 * z, 5.5 * z, 26);
  const t = Math.max(0, Math.min(1, (0.72 * broad + 0.28 * fine - 0.23) / 0.54));
  return t * t * (3 - 2 * t);
}

/** Nearby leaves pick up the same pigment. Grass clumps share a sample in the ground plane. */
function patchPigment(point: Vector3, grass: boolean, scale = 1): string {
  const x = point.x / ((grass ? 0.39 : 0.19) * scale);
  const y = point.y / ((grass ? 0.34 : 0.18) * scale);
  const z = grass ? 0 : point.z / (0.16 * scale);
  const column = Math.floor(x), row = Math.floor(y), layer = Math.floor(z);
  const layerRadius = grass ? 0 : 1;
  let closest = Infinity;
  let pigment = 0;
  for (let i = column - 1; i <= column + 1; i++) {
    for (let j = row - 1; j <= row + 1; j++) {
      for (let k = layer - layerRadius; k <= layer + layerRadius; k++) {
        const dx = x - i - 0.28 - 0.44 * cellRandom(i, j, k, 1);
        const dy = y - j - 0.28 - 0.44 * cellRandom(i, j, k, 2);
        const dz = grass ? 0 : z - k - 0.28 - 0.44 * cellRandom(i, j, k, 3);
        const distance = dx * dx + dy * dy + dz * dz;
        if (distance < closest) {
          closest = distance;
          pigment = cellRandom(i, j, k, 4);
        }
      }
    }
  }
  const colors = grass ? bladeColors : leafColors;
  return colors[1 + Math.floor(pigment * (colors.length - 2))];
}

export interface TreeSpecGaussianDab {
  position: [number, number, number];
  major: [number, number, number];
  minor: [number, number, number];
  normal: [number, number, number];
  crownOffset: [number, number, number];
  openness: number;
  thickness: number;
  color: string;
  patchColor: string;
  finePatchColor?: string;
  opacity: number;
  hue?: number;
  kind: "leaves" | "grass";
  interior?: true;
}

function leafDabRoll({ x, y, z }: Vector3, limit: number): number {
  const spin = cellRandom(Math.round(1024 * x), Math.round(1024 * y), Math.round(1024 * z), 73);
  return limit * (2 * spin - 1);
}

interface TreeSpecGaussianForm {
  center: Vector3;
  radii: Vector3;
  field: DensityField;
  clumps: ReadonlyMap<number, TreeSpecGaussianClumpForm>;
}

/** The crown and its local density are sampled from the same close foliage at every LOD. */
export function treeSpecGaussianForm(packet: ScenePacket): TreeSpecGaussianForm | undefined {
  const data = packet.batches;
  const batch = packet.manifest.batches?.find(batch => batch.foliage?.family === "leaves" && batch.foliage.level === 0);
  if (!data || !batch?.instanceCount) return;
  const low = new Vector3().fromArray(batch.bounds[0]), high = new Vector3().fromArray(batch.bounds[1]);
  const center = low.clone().lerp(high, 0.5);
  const radii = high.sub(low).multiplyScalar(0.5).max(new Vector3(0.05, 0.05, 0.05));
  const samples = new Float32Array(batch.instanceCount * 4);
  const weights = new Float32Array(batch.instanceCount);
  const clumps = treeSpecGaussianClumpForms(packet);
  const attribute = batch.attributes?.find(entry => entry.name === "cluster" && entry.domain === "instance");
  const clusters = attribute && batchAttribute(data, batch, attribute);
  const point = new Vector3(), rotation = new Quaternion();
  for (let instance = 0; instance < batch.instanceCount; instance++) {
    const offset = (batch.instanceStart + instance) * PLACEMENT_FLOATS;
    rotation.fromArray(data.placements, offset + 3);
    point.set(0, 0.5 * data.placements[offset + 8], 0).applyQuaternion(rotation);
    point.x += data.placements[offset];
    point.y += data.placements[offset + 1];
    point.z += data.placements[offset + 2];
    const clump = clumps.get(clusters?.[instance] ?? 0);
    if (clump) reshapeGaussianClumpPoint(point, clump);
    point.toArray(samples, instance * 4);
    samples[instance * 4 + 3] = data.placements[offset + 8];
    weights[instance] = 0.44 + 0.56 * coverAt(point);
  }
  const radius = Math.min(0.44, Math.max(0.18, Math.max(radii.x, radii.y) * 0.16));
  return { center, radii, clumps, field: densityField(samples, radius, radius * 0.5, weights) };
}

/** Small leaf fans that break up larger forms at the closest level. */
export function treeSpecGaussianTuftDabs(packet: ScenePacket, form: TreeSpecGaussianForm): TreeSpecGaussianDab[] {
  const point = new Vector3(), normal = new Vector3(), crownOffset = new Vector3();
  const major = new Vector3(), minor = new Vector3(), face = new Vector3(), roll = new Quaternion();
  return treeSpecGaussianSurfaceTufts(packet).map(dab => {
    point.fromArray(dab.position);
    normal.fromArray(dab.normal);
    const clump = form.clumps.get(dab.cluster);
    if (clump) {
      reshapeGaussianClumpPoint(point, clump);
      reshapeGaussianClumpNormal(normal, clump);
    }
    major.fromArray(dab.major);
    minor.fromArray(dab.minor);
    face.crossVectors(major, minor).normalize();
    roll.setFromAxisAngle(face, leafDabRoll(point, 0.61));
    major.applyQuaternion(roll);
    minor.applyQuaternion(roll);
    crownOffset.copy(point).sub(form.center).divide(form.radii);
    return {
      position: point.toArray(),
      major: major.toArray(),
      minor: minor.toArray(),
      normal: normal.toArray(),
      crownOffset: crownOffset.toArray(),
      openness: 0.68,
      thickness: 0.52 * Math.hypot(...dab.minor),
      color: leafColors[3],
      patchColor: leafColors[Math.floor(cellRandom(dab.cluster, 0, 0, 9) * leafColors.length)],
      hue: leafHueAt(point),
      opacity: 0.9,
      kind: "leaves",
    };
  });
}

/** Soft occlusion marks tucked into the occupied interior of a crown. */
export function treeSpecGaussianCoreDabs(form: TreeSpecGaussianForm): TreeSpecGaussianDab[] {
  const point = new Vector3(), radial = new Vector3(), crownOffset = new Vector3();
  const color = "#53633d";
  return treeSpecGaussianCoreSites(form.field).map(({ center, radius }) => {
    const sigma = radius * 0.42;
    point.fromArray(center);
    radial.copy(point).sub(form.center).normalize();
    if (radial.lengthSq() === 0) radial.set(0, 0, 1);
    crownOffset.copy(point).sub(form.center).divide(form.radii);
    return {
      position: center,
      major: [sigma, 0, 0],
      minor: [0, sigma, 0],
      thickness: sigma,
      normal: outward(form.field, point, radial).toArray(),
      crownOffset: crownOffset.toArray(),
      openness: 0.22,
      color,
      patchColor: color,
      opacity: 0.54,
      kind: "leaves",
      interior: true,
    };
  });
}

/** Fit an oriented dab per leaf card or blade in an authored TreeSpec batch. */
export function treeSpecGaussianDabs(packet: ScenePacket, batch: SceneBatch, finePigment = false, form?: TreeSpecGaussianForm): TreeSpecGaussianDab[] {
  const data = packet.batches;
  const foliage = batch.foliage;
  if (!data || !foliage || (foliage.family !== "grass" && foliage.family !== "leaves")) return [];
  function attribute(name: string, domain: "vertex" | "instance") {
    const record = batch.attributes?.find(entry => entry.name === name && entry.domain === domain);
    return record && data && batchAttribute(data, batch, record);
  }

  const elements = attribute("element", "vertex");
  if (!elements) throw new Error(`TreeSpec batch ${batch.id} has no element attribute.`);
  const random = attribute("random", "vertex");
  const instanceRandom = attribute("random", "instance");
  const variation = attribute("variation", "instance");
  const shadeNormals = attribute("shadeNormal", "instance");
  const massDabs = attribute("dab", "instance");
  const clusters = attribute("cluster", "instance");
  const open = attribute("openness", "instance");
  const grass = foliage.family === "grass";
  const vectorAt = (vertex: number) => new Vector3().fromArray(data.positions, vertex * 3);
  const templates = [];
  // UV.y identifies a row along each element. Fit the longest axis through the
  // end rows, and the width axis from the widest row across the stroke.
  for (let first = 0; first < batch.vertexCount;) {
    let end = first + 1;
    while (end < batch.vertexCount && elements[end] === elements[first]) end++;
    const rows = new Map<number, [number, number]>();
    for (let index = first; index < end; index++) {
      const vertex = batch.vertexStart + index;
      const y = data.uvs[vertex * 2 + 1];
      const row = rows.get(y);
      if (row) {
        const x = data.uvs[vertex * 2];
        if (x < data.uvs[row[0] * 2]) row[0] = vertex;
        if (x > data.uvs[row[1] * 2]) row[1] = vertex;
      } else rows.set(y, [vertex, vertex]);
    }
    const keys = [...rows.keys()].sort((a, b) => a - b);
    const midpoint = ([left, right]: [number, number]) => vectorAt(left).lerp(vectorAt(right), 0.5);
    const root = midpoint(rows.get(keys[0])!);
    const tip = midpoint(rows.get(keys[keys.length - 1])!);
    const middle = keys.reduce((a, b) => Math.abs(a - 0.5) < Math.abs(b - 0.5) ? a : b);
    const center = grass ? midpoint(rows.get(middle)!) : root.clone().lerp(tip, 0.53);
    const length = tip.sub(root);
    const width = new Vector3();
    for (const [left, right] of rows.values()) {
      const across = vectorAt(right).sub(vectorAt(left));
      if (across.lengthSq() > width.lengthSq()) width.copy(across);
    }
    width.addScaledVector(length, -length.dot(width) / length.lengthSq());
    if (length.lengthSq() > 1e-10 && width.lengthSq() > 1e-10) {
      templates.push({
        center,
        major: length.multiplyScalar(grass ? 0.30 : 0.32),
        minor: width.multiplyScalar(grass ? 0.37 : 0.34),
        jitter: random?.[first] ?? 0.5,
      });
    }
    first = end;
  }

  const position = new Vector3(), scale = new Vector3(), rotation = new Quaternion(), transform = new Matrix4();
  const center = new Vector3(), major = new Vector3(), minor = new Vector3(), normal = new Vector3(), canopyNormal = new Vector3();
  const anchor = new Vector3(), formNormal = new Vector3(), crownOffset = new Vector3();
  const rollLimit = grass ? 0 : leafDabRollLimits[foliage.level] ?? 0;
  const roll = new Quaternion();
  const colors = grass ? bladeColors : leafColors;
  const result: TreeSpecGaussianDab[] = [];
  for (let instance = 0; instance < batch.instanceCount; instance++) {
    const offset = (batch.instanceStart + instance) * PLACEMENT_FLOATS;
    position.fromArray(data.placements, offset);
    rotation.fromArray(data.placements, offset + 3);
    scale.fromArray(data.placements, offset + 7);
    transform.compose(position, rotation, scale);
    canopyNormal.set(0, 0, 1);
    if (shadeNormals) canopyNormal.fromArray(shadeNormals, instance * 3).normalize();
    formNormal.copy(canopyNormal);
    let openness = 1;
    let strokeWidth = 1;
    const clump = form?.clumps.get(clusters?.[instance] ?? 0);
    if (!grass) {
      anchor.set(0, 0.5, 0).applyMatrix4(transform);
      if (massDabs && massDabs[instance] > 0.5) {
        // TreeSpec's mass transform keeps the original clump surface normal in its length axis.
        formNormal.set(0, 1, 0).applyQuaternion(rotation);
        if (form && foliage.level < 2) {
          const cover = coverAt(anchor);
          // Leave openings in painted mass. Authored leaves on narrow sprigs still draw through them.
          if (instanceRandom && instanceRandom[instance] > 0.74 + 0.23 * cover) continue;
        }
        if (clump) {
          reshapeGaussianClumpPoint(anchor, clump);
          reshapeGaussianClumpNormal(formNormal, clump);
        }
        formNormal.lerp(canopyNormal, 0.28).normalize();
        if (form && foliage.level < 2) {
          const localFill = Math.min(1, sampleDensity(form.field, anchor) / Math.max(1e-5, form.field.full));
          strokeWidth = 0.82 + 0.15 * localFill;
        }
      } else if (form) {
        formNormal.copy(outward(form.field, anchor, canopyNormal)).lerp(canopyNormal, 0.45).normalize();
      }
      if (open) {
        openness = 0;
        // Same six-axis openness encoding as TreeSpec's painted foliage.
        for (let axis = 0; axis < 3; axis++) {
          const packed = Math.abs(open[instance * 3 + axis]);
          const positive = Math.floor(packed);
          openness += positive / 255 + packed - positive;
        }
        openness /= 6;
      }
    }
    const drift = variation?.[instance] ?? 0.5;
    const grassPatchColor = grass ? patchPigment(position, true) : undefined;
    const clusterColor = clusters && leafColors[Math.floor(cellRandom(clusters[instance], 0, 0, 9) * leafColors.length)];
    for (const template of templates) {
      center.copy(template.center).applyMatrix4(transform);
      if (clump) reshapeGaussianClumpPoint(center, clump);
      major.copy(template.major).multiply(scale).applyQuaternion(rotation);
      minor.copy(template.minor).multiply(scale).applyQuaternion(rotation);
      minor.multiplyScalar(strokeWidth);
      const thickness = minor.length() * (grass ? 0.44 : 0.52);
      normal.crossVectors(major, minor).normalize();
      // Roll within the leaf plane to vary stroke direction while preserving the shading normal.
      if (rollLimit && normal.lengthSq() > 0) {
        roll.setFromAxisAngle(normal, leafDabRoll(center, rollLimit));
        major.applyQuaternion(roll);
        minor.applyQuaternion(roll);
      }
      if (normal.dot(formNormal) < 0) normal.negate();
      normal.lerp(formNormal, grass ? 0.30 : 0.95).normalize();
      if (form) crownOffset.copy(center).sub(form.center).divide(form.radii);
      const shade = grass
        ? 1.1 + 2.9 * drift + 1.8 * template.jitter
        : 1.15 + 2.6 * drift + 0.75 * template.jitter;
      result.push({
        position: center.toArray(),
        major: major.toArray(),
        minor: minor.toArray(),
        normal: normal.toArray(),
        crownOffset: crownOffset.toArray(),
        openness,
        thickness,
        color: colors[Math.min(colors.length - 1, Math.floor(shade))],
        patchColor: grassPatchColor ?? clusterColor ?? patchPigment(center, false),
        ...(finePigment && !grass ? { finePatchColor: patchPigment(center, false, 0.58) } : {}),
        opacity: 0.88 + 0.11 * template.jitter,
        hue: grass ? 0 : leafHueAt(center),
        kind: grass ? "grass" : "leaves",
      });
    }
  }
  return result;
}
