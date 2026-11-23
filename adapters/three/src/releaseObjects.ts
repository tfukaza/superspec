import * as THREE from "three";

type Resource = { dispose(): void };

function objectResources(roots: THREE.Object3D[], detached: Iterable<Resource> = []): Set<Resource> {
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
  return resources;
}

/** Release glTF resources once, preserving resources still borrowed by retained roots. */
export function releaseObjects(roots: THREE.Object3D[], detached: Iterable<Resource> = [], retained: THREE.Object3D[] = []) {
  const resources = objectResources(roots, detached);
  if (!resources.size) return;
  if (retained.length) for (const resource of objectResources(retained)) resources.delete(resource);
  for (const resource of resources) resource.dispose();
}
