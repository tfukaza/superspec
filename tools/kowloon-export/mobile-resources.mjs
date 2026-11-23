function compact(array, used, kind) {
  const ids = new Map();
  const values = array.filter((_, id) => {
    if (!used.has(id)) return false;
    ids.set(id, ids.size);
    return true;
  });
  const remap = id => {
    if (!ids.has(id)) throw Error("Invalid retained city " + kind + " index " + id);
    return ids.get(id);
  };
  return { values, remap };
}

function textureInfos(material) {
  const pbr = material.pbrMetallicRoughness;
  return [pbr?.baseColorTexture, pbr?.metallicRoughnessTexture, material.normalTexture,
    material.occlusionTexture, material.emissiveTexture].filter(Boolean);
}

/** Drop resources outside the remaining static visual graph. Keep node IDs
 * stable because detached declared LOD nodes use the same ID space. */
export function pruneMobileResources(json) {
  const reachable = new Set(), meshIds = new Set(), materialIds = new Set(), accessorIds = new Set();
  const textureIds = new Set(), imageIds = new Set();
  const visitNode = id => {
    if (reachable.has(id)) return;
    const node = json.nodes[id];
    if (!node) throw Error("Invalid city node " + id);
    reachable.add(id);
    if (node.mesh !== undefined) meshIds.add(node.mesh);
    if (node.mesh !== undefined) {
      Object.values(node.extensions?.EXT_mesh_gpu_instancing?.attributes ?? {}).forEach(id => accessorIds.add(id));
    }
    for (const child of node.children ?? []) visitNode(child);
    for (const alternate of node.extensions?.MSFT_lod?.ids ?? []) visitNode(alternate);
  };
  for (const scene of json.scenes) for (const root of scene.nodes ?? []) visitNode(root);
  const mesh = compact(json.meshes, meshIds, "mesh");
  const oldMeshes = json.meshes.length;
  json.meshes = mesh.values;
  json.nodes = json.nodes.map((node, id) => {
    if (!reachable.has(id)) return {};
    if (node.mesh !== undefined) node.mesh = mesh.remap(node.mesh);
    return node;
  });
  const visitMaterial = id => {
    if (materialIds.has(id)) return;
    materialIds.add(id);
    const material = json.materials?.[id];
    if (!material) throw Error("Invalid city material " + id);
    for (const info of textureInfos(material)) textureIds.add(info.index);
    const detail = material.extras?.worldSpecSurface?.detailTexture;
    if (detail !== undefined) textureIds.add(detail);
    for (const alternate of material.extensions?.MSFT_lod?.ids ?? []) visitMaterial(alternate);
  };
  for (const mesh of json.meshes) for (const part of mesh.primitives) {
    if (part.material !== undefined) visitMaterial(part.material);
    if (part.indices !== undefined) accessorIds.add(part.indices);
    for (const attrs of [part.attributes, ...(part.targets ?? [])]) {
      Object.values(attrs).forEach(id => accessorIds.add(id));
    }
  }
  for (const scene of json.scenes) {
    const texture = scene.extras?.worldSpecConcreteDetail?.texture;
    if (texture !== undefined) textureIds.add(texture);
  }
  const material = compact(json.materials ?? [], materialIds, "material");
  const accessor = compact(json.accessors, accessorIds, "accessor");
  const texture = compact(json.textures ?? [], textureIds, "texture");
  const oldImages = json.images?.length ?? 0;
  json.materials = material.values;
  json.accessors = accessor.values;
  json.textures = texture.values;
  for (const mesh of json.meshes) for (const part of mesh.primitives) {
    if (part.material !== undefined) part.material = material.remap(part.material);
    if (part.indices !== undefined) part.indices = accessor.remap(part.indices);
    for (const attrs of [part.attributes, ...(part.targets ?? [])])
      for (const key of Object.keys(attrs)) attrs[key] = accessor.remap(attrs[key]);
  }
  for (const node of json.nodes) {
    const attrs = node.extensions?.EXT_mesh_gpu_instancing?.attributes;
    if (attrs) for (const key of Object.keys(attrs)) attrs[key] = accessor.remap(attrs[key]);
  }
  for (const item of json.materials) {
    for (const info of textureInfos(item)) info.index = texture.remap(info.index);
    const surface = item.extras?.worldSpecSurface;
    if (surface?.detailTexture !== undefined) surface.detailTexture = texture.remap(surface.detailTexture);
    const chain = item.extensions?.MSFT_lod;
    if (chain) chain.ids = chain.ids.map(material.remap);
  }
  for (const item of json.textures) {
    if (item.source !== undefined) imageIds.add(item.source);
    if (item.extensions?.EXT_texture_webp) imageIds.add(item.extensions.EXT_texture_webp.source);
  }
  const image = compact(json.images ?? [], imageIds, "image");
  json.images = image.values;
  for (const item of json.textures) {
    if (item.source !== undefined) item.source = image.remap(item.source);
    const webp = item.extensions?.EXT_texture_webp;
    if (webp) webp.source = image.remap(webp.source);
  }
  for (const scene of json.scenes) {
    const concrete = scene.extras?.worldSpecConcreteDetail;
    if (concrete?.texture !== undefined) concrete.texture = texture.remap(concrete.texture);
  }
  const views = [
    ...json.images,
    ...json.accessors.flatMap(item => item.sparse ? [item, item.sparse.indices, item.sparse.values] : [item]),
    ...json.scenes.map(scene => scene.extras?.worldSpecBounce).filter(Boolean),
  ].filter(value => value.bufferView !== undefined);
  const raw = json.scenes.map(scene => scene.extras?.paintedLawn).filter(record => record?.buffer !== undefined);
  return { views, raw, prunedMeshes: oldMeshes - json.meshes.length, prunedImages: oldImages - json.images.length };
}
