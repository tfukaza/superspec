// The renderer culls one bounding volume per instanced draw. Keep houses across
// town out of a street's draw, sharing the same geometry, materials and textures.
export function groupStreetInstances(doc) {
  const extension = "EXT_mesh_gpu_instancing";
  for (const scene of doc.getRoot().listScenes()) {
    for (const node of scene.listChildren()) {
      const batch = node.getExtension(extension);
      const positions = batch?.getAttribute("TRANSLATION");
      if (!positions) continue;
      const cells = new Map();
      for (let i = 0; i < positions.getCount(); i++) {
        const [x, , z] = positions.getElement(i, []);
        const key = Math.floor(x / 28) + "," + Math.floor(z / 28);
        if (!cells.has(key)) cells.set(key, []);
        cells.get(key).push(i);
      }
      if (cells.size < 2) continue;
      for (const [key, indices] of cells) {
        const part = batch.clone();
        for (const semantic of batch.listSemantics()) {
          const original = batch.getAttribute(semantic);
          const array = original.getArray(), width = original.getElementSize();
          const slice = new array.constructor(indices.length * width);
          indices.forEach((index, i) => slice.set(array.subarray(index * width, (index + 1) * width), i * width));
          part.setAttribute(semantic, original.clone().setArray(slice));
        }
        scene.addChild(doc.createNode("Street " + key)
          .setMesh(node.getMesh()).setMatrix(node.getMatrix()).setExtension(extension, part));
      }
      node.dispose();
      batch.dispose();
    }
  }
}
