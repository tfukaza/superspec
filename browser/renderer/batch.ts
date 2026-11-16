import * as THREE from "three";
import { PLACEMENT_FLOATS, batchAttribute, type SceneBatch, type ScenePacket } from "../model/packet.ts";

/**
 * Instanced batches in the viewer (`superspec_model::batches`): one geometry, one material and one `InstancedMesh` per
 * batch, so a batch is one draw call per pass whatever its instance count. The batch's mesh stays in its own space
 * (the space its material is baked in); each instance's placement is its instance matrix.
 *
 * Per-instance attributes: tints are three.js's instance colors (they multiply the base color of every look, baked or
 * per pixel, and are never part of a bake); `instance` values are the instanced attribute `msInstance`, which a
 * per-pixel program material passes to its fragment shader (shade.ts). Named attributes (`SceneBatch.attributes`) are
 * bound under `attributeName`: per-vertex ones as attributes of the batch's geometry, per-instance ones as instanced
 * attributes, for materials that read them (no program does yet).
 */

/** The geometry attribute a named batch attribute is bound as: `msVertex_<name>` or `msInstance_<name>`. */
export function attributeName(domain: "instance" | "vertex", name: string): string {
  return `${domain === "instance" ? "msInstance" : "msVertex"}_${name}`;
}

/** The instanced vertex attribute that holds a batch's `instance` values (four floats per instance). */
export const INSTANCE_ATTRIBUTE = "msInstance";

/** A batch's mesh as a geometry, in the batch's own space. */
export function batchGeometry(packet: ScenePacket, batch: SceneBatch): THREE.BufferGeometry {
  const data = packet.batches!;
  const vertices = (array: Float32Array, size: number) => new THREE.BufferAttribute(array.slice(batch.vertexStart * size, (batch.vertexStart + batch.vertexCount) * size), size);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", vertices(data.positions, 3));
  geometry.setAttribute("normal", vertices(data.normals, 3));
  geometry.setAttribute("uv", vertices(data.uvs, 2));
  geometry.setAttribute("color", vertices(data.colors, 3));
  geometry.setIndex(new THREE.BufferAttribute(data.indices.slice(batch.indexStart, batch.indexStart + batch.indexCount), 1));
  for (const attribute of batch.attributes ?? []) {
    if (attribute.domain === "vertex") geometry.setAttribute(attributeName("vertex", attribute.name), new THREE.BufferAttribute(batchAttribute(data, batch, attribute).slice(), attribute.components));
  }
  geometry.computeBoundingBox();
  geometry.computeBoundingSphere();
  return geometry;
}

/** What a batch's instances are drawn from: placements (`PLACEMENT_FLOATS` each) and the optional per-instance attributes. */
export interface BatchInstances { count: number; placements: Float32Array; values?: Float32Array; tints?: Float32Array;
  /** Named per-instance attributes (not carried by `stressInstances`). */
  named?: { name: string; components: number; values: Float32Array }[] }

/** A batch's own instances, as its packet holds them. */
export function batchInstances(packet: ScenePacket, batch: SceneBatch): BatchInstances {
  const data = packet.batches!, count = batch.instanceCount;
  const slice = (array: Float32Array, start: number, size: number) => array.slice(start * size, (start + count) * size);
  return {
    count, placements: slice(data.placements, batch.instanceStart, PLACEMENT_FLOATS),
    values: batch.valueStart === undefined ? undefined : slice(data.values, batch.valueStart, 4),
    tints: batch.tintStart === undefined ? undefined : slice(data.tints, batch.tintStart, 3),
    named: (batch.attributes ?? []).filter(attribute => attribute.domain === "instance")
      .map(attribute => ({ name: attribute.name, components: attribute.components, values: batchAttribute(data, batch, attribute).slice() })),
  };
}

/**
 * TEST ONLY (`?batchstress=N`): `count` instances made from a batch's own, for measuring how the viewer draws many.
 * Instance `i` has the rotation, scale and attributes of the batch's instance `i mod n` and stands on a regular lattice
 * that fills `volume` (the scene's bounds), so every instance is in view. `center` is the middle of the batch's mesh
 * in its own space: placements move the mesh, whose own positions need not be around its origin.
 */
export function stressInstances(source: BatchInstances, count: number, volume: THREE.Box3, center: THREE.Vector3): BatchInstances {
  const placements = new Float32Array(count * PLACEMENT_FLOATS);
  const values = source.values && new Float32Array(count * 4), tints = source.tints && new Float32Array(count * 3);
  const side = Math.ceil(Math.cbrt(count)), size = volume.getSize(new THREE.Vector3());
  const rotation = new THREE.Quaternion(), offset = new THREE.Vector3();
  for (let index = 0; index < count; index++) {
    const from = index % source.count, at = index * PLACEMENT_FLOATS;
    placements.set(source.placements.subarray(from * PLACEMENT_FLOATS, (from + 1) * PLACEMENT_FLOATS), at);
    const cell = [index % side, Math.floor(index / side) % side, Math.floor(index / (side * side))];
    // The mesh's middle lands on the lattice point: translation = point - R (S center).
    rotation.set(placements[at + 3], placements[at + 4], placements[at + 5], placements[at + 6]);
    offset.set(center.x * placements[at + 7], center.y * placements[at + 8], center.z * placements[at + 9]).applyQuaternion(rotation);
    placements[at] = volume.min.x + (cell[0] + 0.5) / side * size.x - offset.x;
    placements[at + 1] = volume.min.y + (cell[1] + 0.5) / side * size.y - offset.y;
    placements[at + 2] = volume.min.z + (cell[2] + 0.5) / side * size.z - offset.z;
    values?.set(source.values!.subarray(from * 4, from * 4 + 4), index * 4);
    tints?.set(source.tints!.subarray(from * 3, from * 3 + 3), index * 3);
  }
  return { count, placements, values, tints };
}

/**
 * The instanced mesh of a batch: `geometry` drawn once per placement. The mesh's bounding box and sphere cover every
 * instance (three.js derives them from the instance matrices), so frustum culling, shadow fitting and camera framing
 * see the whole batch.
 */
export function instancedBatch(geometry: THREE.BufferGeometry, material: THREE.Material, instances: BatchInstances): THREE.InstancedMesh {
  const mesh = new THREE.InstancedMesh(geometry, material, instances.count);
  const matrix = new THREE.Matrix4(), translation = new THREE.Vector3(), rotation = new THREE.Quaternion(), scale = new THREE.Vector3();
  const p = instances.placements;
  for (let index = 0; index < instances.count; index++) {
    const at = index * PLACEMENT_FLOATS;
    translation.set(p[at], p[at + 1], p[at + 2]);
    rotation.set(p[at + 3], p[at + 4], p[at + 5], p[at + 6]);
    scale.set(p[at + 7], p[at + 8], p[at + 9]);
    matrix.compose(translation, rotation, scale).toArray(mesh.instanceMatrix.array, index * 16);
  }
  mesh.instanceMatrix.needsUpdate = true;
  // Tints are linear RGB multipliers, as vertex colors are.
  if (instances.tints) mesh.instanceColor = new THREE.InstancedBufferAttribute(instances.tints, 3);
  if (instances.values) geometry.setAttribute(INSTANCE_ATTRIBUTE, new THREE.InstancedBufferAttribute(instances.values, 4));
  for (const attribute of instances.named ?? []) geometry.setAttribute(attributeName("instance", attribute.name), new THREE.InstancedBufferAttribute(attribute.values, attribute.components));
  mesh.computeBoundingBox();
  mesh.computeBoundingSphere();
  return mesh;
}

/** The named per-instance vector a batch without a material is lit by, when it has one (foliage's shade normal). */
export const SHADE_NORMAL = "shadeNormal";

/**
 * Lights a batch's instances by their per-instance `shadeNormal` (a direction in the scene's space, the same for every
 * vertex of an instance) instead of the mesh's normals: what a leaf cluster's painted material will do, done here for
 * the standard material a batch without a material is shown with. Chains onto what is installed (the shadow style).
 */
export function lightByShadeNormal(material: THREE.Material): void {
  const previous = material.onBeforeCompile, key = material.customProgramCacheKey;
  material.onBeforeCompile = (shader, renderer) => {
    previous.call(material, shader, renderer);
    shader.vertexShader = shader.vertexShader
      .replace("#include <common>", `#include <common>\nattribute vec3 ${attributeName("instance", SHADE_NORMAL)};`)
      // The mesh normal that the instance's placement takes to the shade normal, for everything that reads the normal.
      .replace("#include <beginnormal_vertex>", `#include <beginnormal_vertex>\n  objectNormal = transpose(mat3(instanceMatrix)) * ${attributeName("instance", SHADE_NORMAL)};`)
      .replace("#include <defaultnormal_vertex>", `#include <defaultnormal_vertex>\n  transformedNormal = normalize(normalMatrix * ${attributeName("instance", SHADE_NORMAL)});`);
  };
  material.customProgramCacheKey = () => `${key.call(material)}|shadeNormal`;
}

/** Gives `target` the per-instance attributes of `source`: another geometry the same instances are drawn with. */
export function shareInstanceAttributes(source: THREE.BufferGeometry, target: THREE.BufferGeometry): void {
  for (const [name, attribute] of Object.entries(source.attributes)) {
    if ((attribute as THREE.InstancedBufferAttribute).isInstancedBufferAttribute) target.setAttribute(name, attribute);
  }
}

/**
 * A stand-in for `mesh` with another geometry and material, for compiling the material as the mesh will draw it: an
 * instanced mesh's programs differ from an ordinary mesh's, so the stand-in is instanced in the same way.
 */
export function compileTwin(mesh: THREE.Mesh, geometry: THREE.BufferGeometry, material: THREE.Material): THREE.Mesh {
  const instanced = mesh as THREE.InstancedMesh;
  let twin: THREE.Mesh;
  if (instanced.isInstancedMesh) {
    const made = twin = new THREE.InstancedMesh(geometry, material, 1);
    if (instanced.instanceColor) made.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array([1, 1, 1]), 3);
  } else twin = new THREE.Mesh(geometry, material);
  twin.castShadow = mesh.castShadow; twin.receiveShadow = mesh.receiveShadow;
  return twin;
}
