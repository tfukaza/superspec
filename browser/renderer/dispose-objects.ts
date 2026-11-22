import * as THREE from "three";

/** Release glTF objects once across all scenes, including shared mesh and material resources. */
export function releaseObjects(roots: THREE.Object3D[], detached: Iterable<{ dispose(): void }> = []) {
  const resources = new Set(detached);
  for (const root of roots) root.traverse(object => {
    if (object instanceof THREE.Light || object instanceof THREE.InstancedMesh) resources.add(object);
    if (object instanceof THREE.SkinnedMesh) resources.add(object.skeleton);
    if (!(object instanceof THREE.Mesh || object instanceof THREE.Line || object instanceof THREE.Points)) return;
    resources.add(object.geometry);
    for (const material of Array.isArray(object.material) ? object.material : [object.material]) {
      resources.add(material);
      for (const value of Object.values(material)) if (value instanceof THREE.Texture) resources.add(value);
    }
  });
  for (const resource of resources) resource.dispose();
}
