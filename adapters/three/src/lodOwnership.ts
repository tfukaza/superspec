import type * as THREE from "three";

// Runtime ownership is not serialized into user extras. Multiple views can
// retain the same source without one view's disposal clearing another's guard.
const owners = new WeakMap<THREE.Object3D, number>();
export function retainDeclaredLodOwner(object: THREE.Object3D): void { owners.set(object, (owners.get(object) ?? 0) + 1); }
export function releaseDeclaredLodOwner(object: THREE.Object3D): void {
  const count = owners.get(object) ?? 0;
  if (count <= 1) owners.delete(object); else owners.set(object, count - 1);
}
export function hasDeclaredLodOwner(object: THREE.Object3D): boolean {
  for (let at: THREE.Object3D | null = object; at; at = at.parent) if (owners.has(at)) return true;
  return false;
}
