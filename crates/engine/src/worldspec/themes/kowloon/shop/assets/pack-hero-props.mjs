// Publish the three approved Material Studio props as a shared PBR catalog atlas.
// Run from SuperSpec: node crates/engine/src/worldspec/themes/kowloon/shop/assets/pack-hero-props.mjs <props.glb>
import {readFile, writeFile} from 'node:fs/promises';
import {PNG} from 'pngjs';

const glb = await readFile(process.argv[2]);
const end = 20 + glb.readUInt32LE(12);
const document = JSON.parse(glb.toString('utf8', 20, end));
const bytes = glb.subarray(end + 8);
const tile = 512;
const inset = 4;
const chart = tile - 2 * inset;
const side = tile * 3;
const materials = new Set();
const layerNames = ['market-hero-props.png', 'market-hero-props-normal.png', 'market-hero-props-properties.png'];
const layers = layerNames.map(() => new PNG({width: side, height: side}));

function accessor(index, components) {
  const a = document.accessors[index];
  const view = document.bufferViews[a.bufferView];
  const [elementBytes, read] = {
    5123: [2, 'readUInt16LE'], 5125: [4, 'readUInt32LE'], 5126: [4, 'readFloatLE'],
  }[a.componentType];
  const stride = view.byteStride ?? components * elementBytes;
  const offset = (view.byteOffset ?? 0) + (a.byteOffset ?? 0);
  return Array.from({length: a.count}, (_, row) =>
    Array.from({length: components}, (_, component) =>
      bytes[read](offset + row * stride + component * elementBytes)));
}

function copyTexture(texture, layer, left, top) {
  const image = document.images[document.textures[texture].source];
  const {byteOffset = 0, byteLength} = document.bufferViews[image.bufferView];
  const source = PNG.sync.read(bytes.subarray(byteOffset, byteOffset + byteLength));
  // Resample each chart independently. Extend its outermost texels through the gutter.
  const clamp = (n, end) => Math.min(Math.max(n, 0), end);
  for (let y = 0; y < tile; y++) {
    const sy = clamp((y - inset + 0.5) * source.height / chart - 0.5, source.height - 1);
    const y0 = Math.floor(sy), y1 = Math.min(y0 + 1, source.height - 1);
    for (let x = 0; x < tile; x++) {
      const sx = clamp((x - inset + 0.5) * source.width / chart - 0.5, source.width - 1);
      const x0 = Math.floor(sx), x1 = Math.min(x0 + 1, source.width - 1);
      const tx = sx - x0, ty = sy - y0;
      const dst = 4 * ((top + y) * side + left + x);
      for (let channel = 0; channel < 4; channel++) {
        const a = source.data[4 * (y0 * source.width + x0) + channel];
        const b = source.data[4 * (y0 * source.width + x1) + channel];
        const c = source.data[4 * (y1 * source.width + x0) + channel];
        const d = source.data[4 * (y1 * source.width + x1) + channel];
        layer.data[dst + channel] = Math.round((a * (1-tx) + b * tx) * (1-ty) + (c * (1-tx) + d * tx) * ty);
      }
    }
  }
}

const groups = new Map([['wooden-stool', {center: -.48, vertices: []}],
  ['jade-stool', {center: 0, vertices: []}], ['bucket', {center: .45, vertices: []}]]);
for (const node of document.nodes.filter(node => node.mesh !== undefined)) {
  const group = groups.get(node.extras.meshId.split('/')[0]);
  if (!group || node.translation || node.rotation || node.scale) throw Error('Unexpected studio prop transform');
  for (const primitive of document.meshes[node.mesh].primitives) {
    const mat = document.materials[primitive.material];
    if (mat.emissiveTexture || mat.emissiveFactor?.some(value => value !== 0) || mat.extensions) {
      throw Error('Expected ordinary reflective PBR channels');
    }
    if (materials.has(primitive.material) || materials.size === 9) throw Error('Unexpected atlas material layout');
    const left = materials.size % 3 * tile;
    const top = Math.floor(materials.size / 3) * tile;
    materials.add(primitive.material);
    const pbr = mat.pbrMetallicRoughness;
    [pbr.baseColorTexture.index, mat.normalTexture.index, pbr.metallicRoughnessTexture.index]
      .forEach((texture, channel) => copyTexture(texture, layers[channel], left, top));
    const xyz = accessor(primitive.attributes.POSITION, 3);
    const normals = accessor(primitive.attributes.NORMAL, 3);
    const uv = accessor(primitive.attributes.TEXCOORD_0, 2);
    for (const [index] of accessor(primitive.indices, 1)) {
      const [x, y, z] = xyz[index];
      const [u, v] = uv[index];
      group.vertices.push(x - group.center, y, z, ...normals[index],
        (left + inset + chart * u) / side, 1 - (top + inset + chart * v) / side);
    }
  }
}
for (const [name, {vertices}] of groups) {
  if (!vertices.length || vertices.length % 24) throw Error('Expected triangles for ' + name);
  const binary = Buffer.alloc(vertices.length * 4);
  vertices.forEach((value, index) => binary.writeFloatLE(value, index * 4));
  await writeFile(new URL(name + '.triangles.bin', import.meta.url), binary);
}
for (const [i, layer] of layers.entries()) {
  await writeFile(new URL('../../../../../../../../assets/materials/' + layerNames[i], import.meta.url),
    PNG.sync.write(layer, {colorType: 2}));
}
