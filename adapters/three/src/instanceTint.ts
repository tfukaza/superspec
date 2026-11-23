import * as THREE from "three";

/** A batch's per-instance tint is Three's `instanceColor`, which GLTFLoader fills from
 * `_COLOR_0` of `EXT_mesh_gpu_instancing` with the accessor's own array, item size and
 * normalization. These helpers copy whole items so that any such layout stays intact. */

/** Whether two batches can share instances: both untinted, or tints of one layout. */
export function sameInstanceTintLayout(a: THREE.InstancedMesh, b: THREE.InstancedMesh): boolean {
  const left = a.instanceColor, right = b.instanceColor;
  if (!left || !right) return !left && !right;
  return left.itemSize === right.itemSize && left.normalized === right.normalized
    && left.array.constructor === right.array.constructor;
}

/** Names the tint layout in a grouping key. Untinted batches share the empty name. */
export function instanceTintLayout(mesh: THREE.InstancedMesh): string {
  const tint = mesh.instanceColor;
  return tint ? `${tint.array.constructor.name}/${tint.itemSize}/${tint.normalized}` : "";
}

/** Copy the tint of instance `from` to slot `to` of a batch with the same tint layout.
 * Does nothing for untinted batches. Mark the target with `uploadInstanceTints`. */
export function copyInstanceTint(source: THREE.InstancedMesh, from: number, target: THREE.InstancedMesh, to: number): void {
  const a = source.instanceColor, b = target.instanceColor;
  if (!a || !b) return;
  const size = a.itemSize, input = a.array, output = b.array;
  for (let k = 0; k < size; k++) output[to * size + k] = input[from * size + k];
}

/** Upload the first `count` tints of a batch whose slots were rewritten. */
export function uploadInstanceTints(mesh: THREE.InstancedMesh, count: number): void {
  const tint = mesh.instanceColor;
  if (!tint) return;
  tint.clearUpdateRanges();
  tint.addUpdateRange(0, count * tint.itemSize);
  tint.needsUpdate = true;
}

/** Whether the first `count` instances of two batches carry equal tints, or neither is tinted. */
export function equalInstanceTints(a: THREE.InstancedMesh, b: THREE.InstancedMesh, count: number): boolean {
  if (!sameInstanceTintLayout(a, b)) return false;
  const left = a.instanceColor, right = b.instanceColor;
  if (!left || !right || left === right) return true;
  const x = left.array, y = right.array;
  for (let i = 0, end = count * left.itemSize; i < end; i++) if (x[i] !== y[i]) return false;
  return true;
}
