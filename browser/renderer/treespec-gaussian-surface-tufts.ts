import { Box3, Quaternion, Vector3 } from "three";
import { batchAttribute, PLACEMENT_FLOATS, type ScenePacket } from "../model/packet.ts";

export interface TreeSpecGaussianTuftDab {
  cluster: number;
  position: [number, number, number];
  major: [number, number, number];
  minor: [number, number, number];
  normal: [number, number, number];
}

interface SurfaceSite {
  position: Vector3;
  outward: Vector3;
  seed: number;
}

interface MassClump {
  cluster: number;
  sites: SurfaceSite[];
  bounds: Box3;
}

function random(seed: number, channel: number): number {
  let bits = seed ^ Math.imul(channel + 1, 0x9e3779b1);
  bits = Math.imul(bits ^ (bits >>> 16), 0x7feb352d);
  bits = Math.imul(bits ^ (bits >>> 15), 0x846ca68b);
  return ((bits ^ (bits >>> 16)) >>> 0) / 4294967296;
}

const up = new Vector3(0, 0, 1);
const xAxis = new Vector3(1, 0, 0);

/** A small irregular fan rooted on the sampled canopy surface, with light gaps between its leaves. */
function appendTuft(dabs: TreeSpecGaussianTuftDab[], cluster: number, site: SurfaceSite): void {
  const { position, outward, seed } = site;
  const side = new Vector3().crossVectors(outward, Math.abs(outward.z) < 0.9 ? up : xAxis).normalize();
  const upright = new Vector3().crossVectors(outward, side);
  const count = 3 + Math.floor(4 * random(seed, 0));
  const radius = 0.076 + 0.056 * random(seed, 1);
  const heading = 2 * Math.PI * random(seed, 2);
  const spread = 1.6 + 1.05 * random(seed, 3);
  for (let leaf = 0; leaf < count; leaf++) {
    const channel = 4 + 7 * leaf;
    const angle = heading + spread * (leaf / (count - 1) - 0.5) + 0.22 * (random(seed, channel) - 0.5);
    const along = side.clone().multiplyScalar(Math.cos(angle)).addScaledVector(upright, Math.sin(angle));
    const across = new Vector3().crossVectors(outward, along).normalize();
    const reach = radius * (0.34 + 0.61 * random(seed, channel + 1));
    const lift = 0.027 + 0.024 * random(seed, channel + 2);
    const point = position.clone().addScaledVector(along, reach).addScaledVector(outward, lift);
    const major = along.clone().addScaledVector(outward, 0.16 + 0.22 * random(seed, channel + 3))
      .normalize().multiplyScalar(0.023 + 0.016 * random(seed, channel + 4));
    const minor = across.clone().multiplyScalar(0.010 + 0.008 * random(seed, channel + 5));
    const normal = outward.clone().addScaledVector(along, 0.19)
      .addScaledVector(across, 0.2 * (random(seed, channel + 6) - 0.5)).normalize();
    dabs.push({ cluster, position: point.toArray(), major: major.toArray(), minor: minor.toArray(), normal: normal.toArray() });
  }
}

/** Sparse second-scale leaf tufts on the larger native mass clumps. */
export function treeSpecGaussianSurfaceTufts(packet: ScenePacket): TreeSpecGaussianTuftDab[] {
  const buffers = packet.batches;
  const batch = packet.manifest.batches?.find(entry => entry.foliage?.family === "leaves" && entry.foliage.level === 0);
  if (!buffers || !batch) return [];
  const clusterRecord = batch.attributes?.find(attribute => attribute.domain === "instance" && attribute.name === "cluster");
  const dabRecord = batch.attributes?.find(attribute => attribute.domain === "instance" && attribute.name === "dab");
  if (!clusterRecord || !dabRecord) return [];
  const clusters = batchAttribute(buffers, batch, clusterRecord);
  const mask = batchAttribute(buffers, batch, dabRecord);
  const groups = new Map<number, MassClump>();
  const rotation = new Quaternion();
  for (let index = 0; index < batch.instanceCount; index++) {
    const cluster = clusters[index];
    if (cluster >= 0 || mask[index] < 0.5 || mask[index] > 1.5) continue;
    let group = groups.get(cluster);
    if (!group) {
      group = { cluster, sites: [], bounds: new Box3() };
      groups.set(cluster, group);
    }
    const offset = (batch.instanceStart + index) * PLACEMENT_FLOATS;
    rotation.fromArray(buffers.placements, offset + 3);
    const outward = new Vector3(0, 1, 0).applyQuaternion(rotation).normalize();
    const position = new Vector3().fromArray(buffers.placements, offset)
      .addScaledVector(outward, buffers.placements[offset + 8] * 0.5);
    group.bounds.expandByPoint(position);
    group.sites.push({ position, outward, seed: index ^ Math.imul(cluster, 0x9e3779b1) });
  }

  const ranked = [...groups.values()].filter(group => group.sites.length >= 12)
    .map(group => ({ ...group, span: group.bounds.min.distanceTo(group.bounds.max) }))
    .sort((a, b) => b.span - a.span || a.cluster - b.cluster);
  if (!ranked.length) return [];
  const minimumSpan = Math.max(0.42, ranked[Math.floor(0.45 * (ranked.length - 1))].span);
  const anchors: Vector3[] = [];
  const dabs: TreeSpecGaussianTuftDab[] = [];
  const center = new Vector3(), radial = new Vector3();
  for (const group of ranked) {
    if (group.span < minimumSpan || anchors.length >= 120) break;
    group.bounds.getCenter(center);
    const limit = group.span > 1.4 * minimumSpan ? 2 : 1;
    let placed = 0;
    group.sites.sort((a, b) => random(a.seed, 81) - random(b.seed, 81));
    for (const site of group.sites) {
      radial.copy(site.position).sub(center);
      if (radial.dot(site.outward) < 0.32 * radial.length()) continue;
      if (anchors.some(anchor => anchor.distanceToSquared(site.position) < 0.26 ** 2)) continue;
      appendTuft(dabs, group.cluster, site);
      anchors.push(site.position);
      placed++;
      if (placed === limit || anchors.length === 120) break;
    }
  }
  return dabs;
}
