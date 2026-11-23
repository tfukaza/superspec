import { Quaternion, Vector3 } from "three";
import { batchAttribute, PLACEMENT_FLOATS, type ScenePacket } from "../model/packet.ts";

export interface TreeSpecGaussianClumpForm {
  center: Vector3;
  axis: Vector3;
  along: number;
  across: number;
}

function clumpRandom(cluster: number, salt: number): number {
  let bits = cluster ^ Math.imul(salt, 0x9e3779b1);
  bits = Math.imul(bits ^ (bits >>> 16), 0x7feb352d);
  bits = Math.imul(bits ^ (bits >>> 15), 0x846ca68b);
  return ((bits ^ (bits >>> 16)) >>> 0) / 4294967296;
}

function clumpForm(cluster: number, points: Vector3[], crown: Vector3): TreeSpecGaussianClumpForm | undefined {
  if (points.length < 8) return;
  const center = new Vector3();
  for (const point of points) center.add(point);
  center.multiplyScalar(1 / points.length);

  let xx = 0, yy = 0, zz = 0, xy = 0, xz = 0, yz = 0;
  for (const point of points) {
    const x = point.x - center.x, y = point.y - center.y, z = point.z - center.z;
    xx += x * x;
    yy += y * y;
    zz += z * z;
    xy += x * y;
    xz += x * z;
    yz += y * z;
  }
  const spread = xx + yy + zz;
  if (spread < points.length * 0.0004) return;

  const axis = new Vector3();
  // Follow the strongest direction in the sampled spray.
  if (xx >= yy && xx >= zz) axis.set(1, 0, 0);
  else if (yy >= zz) axis.set(0, 1, 0);
  else axis.set(0, 0, 1);
  for (let iteration = 0; iteration < 10; iteration++) {
    const { x, y, z } = axis;
    axis.set(xx * x + xy * y + xz * z, xy * x + yy * y + yz * z, xz * x + yz * y + zz * z).normalize();
  }
  const { x, y, z } = axis;
  const axialSpread = xx * x * x + yy * y * y + zz * z * z + 2 * (xy * x * y + xz * x * z + yz * y * z);
  if (axialSpread < 0.4 * spread) {
    // Round sprays have little preferred direction. Give them a shallow, outward inclination.
    const angle = Math.atan2(center.y - crown.y, center.x - crown.x) + (clumpRandom(cluster, 3) - 0.5) * 1.4;
    axis.set(Math.cos(angle), Math.sin(angle), 0.18 + 0.45 * clumpRandom(cluster, 4)).normalize();
  }
  return {
    center,
    axis,
    along: 1.16 + 0.16 * clumpRandom(cluster, 1),
    across: 0.8 + 0.13 * clumpRandom(cluster, 2),
  };
}

/** Fit from the nearest native foliage so the same clump shape can be used at every distance. */
export function treeSpecGaussianClumpForms(packet: ScenePacket): ReadonlyMap<number, TreeSpecGaussianClumpForm> {
  const forms = new Map<number, TreeSpecGaussianClumpForm>();
  const data = packet.batches;
  const batch = packet.manifest.batches?.find(batch => batch.foliage?.family === "leaves" && batch.foliage.level === 0);
  if (!data || !batch?.instanceCount) return forms;
  const attribute = batch.attributes?.find(attr => attr.name === "cluster" && attr.domain === "instance" && attr.components === 1);
  if (!attribute) return forms;
  const clusters = batchAttribute(data, batch, attribute);
  const pointsByCluster = new Map<number, Vector3[]>();
  const rotation = new Quaternion();
  for (let instance = 0; instance < batch.instanceCount; instance++) {
    const cluster = clusters[instance];
    // Negative cluster IDs are native mass clumps. Loose leaves retain their placements.
    if (!(cluster < 0) || clumpRandom(cluster, 0) >= 0.54) continue;
    let points = pointsByCluster.get(cluster);
    if (!points) pointsByCluster.set(cluster, points = []);
    const start = (batch.instanceStart + instance) * PLACEMENT_FLOATS;
    rotation.fromArray(data.placements, start + 3);
    const point = new Vector3(0, 0.5 * data.placements[start + 8], 0).applyQuaternion(rotation);
    point.x += data.placements[start];
    point.y += data.placements[start + 1];
    point.z += data.placements[start + 2];
    points.push(point);
  }
  const crown = new Vector3().fromArray(batch.bounds[0]).lerp(new Vector3().fromArray(batch.bounds[1]), 0.5);
  for (const [cluster, points] of pointsByCluster) {
    const form = clumpForm(cluster, points, crown);
    if (form) forms.set(cluster, form);
  }
  return forms;
}

export function reshapeGaussianClumpPoint(point: Vector3, { center, axis, along, across }: TreeSpecGaussianClumpForm): Vector3 {
  point.sub(center);
  const axial = point.dot(axis);
  return point.multiplyScalar(across).addScaledVector(axis, (along - across) * axial).add(center);
}

export function reshapeGaussianClumpNormal(normal: Vector3, { axis, along, across }: TreeSpecGaussianClumpForm): Vector3 {
  const axial = normal.dot(axis);
  return normal.multiplyScalar(1 / across).addScaledVector(axis, (1 / along - 1 / across) * axial).normalize();
}
