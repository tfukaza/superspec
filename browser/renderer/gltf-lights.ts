import * as THREE from "three";
import { sunOf, type Light, type Lighting } from "./lighting.ts";

const exportFrame = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), -Math.PI / 2);

/** The native exporter puts the record's sun under its Z-up root, with the
 * record ID as the node name and local +Z pointing toward the light. */
function mirrorsRecordedSun(light: THREE.Light, root: THREE.Object3D, sun: Light): boolean {
  const frame = light.parent;
  if (!(light instanceof THREE.DirectionalLight) || !frame || (frame !== root && frame.parent !== root)
      || frame.quaternion.angleTo(exportFrame) > 1e-6 || light.userData.name !== sun.id
      || light.intensity !== sun.intensity || !light.color.equals(new THREE.Color(...sun.color))) return false;
  const direction = new THREE.Vector3(0, 0, 1).applyQuaternion(light.quaternion);
  return direction.distanceToSquared(new THREE.Vector3(...sun.direction).normalize()) < 1e-10;
}

/** Both backends share ownership of glTF lights in the same way. Suppressing
 * only a light's layers keeps its attached meshes and children renderable. */
export class GltfLights {
  private readonly suppressed = new Map<THREE.Light, number>();
  private readonly recorded = new WeakSet<THREE.Light>();

  select(roots: Iterable<THREE.Object3D>, record?: Lighting) {
    this.restore();
    const embedded = new Set<THREE.Light>();
    const sun = record && sunOf(record);
    for (const root of roots) root.traverse(object => {
      if (object instanceof THREE.Light) {
        embedded.add(object);
        if (sun && mirrorsRecordedSun(object, root, sun)) this.recorded.add(object);
      }
    });
    const authored: THREE.Light[] = [];
    for (const light of embedded) {
      if (record && this.recorded.has(light)) {
        this.suppressed.set(light, light.layers.mask);
        light.layers.disableAll();
      } else authored.push(light);
    }
    return { useRig: !!record || embedded.size === 0, authored };
  }

  restore(): void {
    for (const [light, mask] of this.suppressed) if (light.layers.mask === 0) light.layers.mask = mask;
    this.suppressed.clear();
  }
}
