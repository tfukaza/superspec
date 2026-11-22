import test, { before, after } from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import { narrowDataTextureChannels } from "../../adapters/three/src/textureChannels.ts";

const bitmapDescriptor = Object.getOwnPropertyDescriptor(globalThis, "ImageBitmap");
class Bitmap { width = 8; height = 4; }
before(() => Object.defineProperty(globalThis, "ImageBitmap", { value: Bitmap, configurable: true }));
after(() => {
  if (bitmapDescriptor) Object.defineProperty(globalThis, "ImageBitmap", bitmapDescriptor);
  else Reflect.deleteProperty(globalThis, "ImageBitmap");
});

function texture() { return new THREE.Texture(new Bitmap() as unknown as ImageBitmap); }
function fixture(materials: THREE.Material[], json: object = {}) {
  const scene = new THREE.Group(), geometry = new THREE.BoxGeometry();
  for (const material of materials) scene.add(new THREE.Mesh(geometry, material));
  const associations = new Map();
  const gltf = { scene, scenes: [scene], parser: { json, associations } } as unknown as GLTF;
  return { gltf, associations, dispose() { geometry.dispose(); for (const material of materials) material.dispose(); } };
}

test("edge masks retain RG, scalar AO retains R and all sampling settings stay intact", () => {
  const edge = texture(), ao = texture();
  edge.repeat.set(2, 3); edge.offset.set(0.2, 0.4); edge.rotation = 0.7; edge.channel = 1; edge.anisotropy = 8;
  const image = edge.image, source = edge.source;
  const first = new THREE.MeshStandardMaterial({ aoMap: edge }), second = new THREE.MeshStandardMaterial({ aoMap: ao });
  first.userData.materialSpecEdge = { from: 0.2, to: 0.8 };
  const f = fixture([first, second]);
  try {
    assert.equal(narrowDataTextureChannels(f.gltf, new Set([first, second])), 2);
    assert.equal(edge.format, THREE.RGFormat); assert.equal(edge.internalFormat, "RG8");
    assert.equal(ao.format, THREE.RedFormat); assert.equal(ao.internalFormat, "R8");
    assert.equal(edge.image, image); assert.equal(edge.source, source);
    assert.deepEqual(edge.repeat.toArray(), [2, 3]); assert.deepEqual(edge.offset.toArray(), [0.2, 0.4]);
    assert.equal(edge.rotation, 0.7); assert.equal(edge.channel, 1); assert.equal(edge.anisotropy, 8);
    assert.equal(edge.generateMipmaps, true); assert.equal(edge.minFilter, THREE.LinearMipmapLinearFilter);
    assert.equal(narrowDataTextureChannels(f.gltf, new Set([first, second])), 0, "repeated binding is stable");
  } finally { f.dispose(); edge.dispose(); ao.dispose(); }
});

test("shared sources take the widest AO requirement and conflicting material or shader uses block narrowing", () => {
  const edge = texture(), scalar = edge.clone(), conflict = texture(), custom = texture();
  const a = new THREE.MeshStandardMaterial({ aoMap: edge }), b = new THREE.MeshStandardMaterial({ aoMap: scalar });
  a.userData.materialSpecEdge = {};
  const c = new THREE.MeshStandardMaterial({ aoMap: conflict, roughnessMap: conflict.clone() });
  const d = new THREE.MeshStandardMaterial({ aoMap: custom });
  const shader = new THREE.ShaderMaterial({ uniforms: { hiddenSampler: { value: custom } } });
  const f = fixture([a, b, c, d, shader]);
  try {
    assert.equal(narrowDataTextureChannels(f.gltf, new Set([a, b, c, d])), 2);
    assert.equal(edge.format, THREE.RGFormat); assert.equal(scalar.format, THREE.RGFormat);
    assert.equal(conflict.format, THREE.RGBAFormat); assert.equal(custom.format, THREE.RGBAFormat);
  } finally { f.dispose(); edge.dispose(); scalar.dispose(); conflict.dispose(); custom.dispose(); c.roughnessMap?.dispose(); }
});

test("serialized extras and unused same-image color bindings preserve full-channel storage", () => {
  const ao = texture(), extra = texture();
  const a = new THREE.MeshStandardMaterial({ aoMap: ao }), b = new THREE.MeshStandardMaterial({ aoMap: extra });
  const f = fixture([a, b], {
    textures: [{ source: 0 }, { source: 0 }, { source: 1 }],
    materials: [{ occlusionTexture: { index: 0 } }, { pbrMetallicRoughness: { baseColorTexture: { index: 1 } } }],
    scenes: [{ extras: { futureEffect: { detailTexture: 2 } } }],
  });
  f.associations.set(ao, { textures: 0 }); f.associations.set(extra, { textures: 2 });
  try { assert.equal(narrowDataTextureChannels(f.gltf, new Set([a, b])), 0); }
  finally { f.dispose(); ao.dispose(); extra.dispose(); }
});

test("unadmitted shaders, sRGB data and typed arrays keep their existing upload formats", () => {
  const unknown = texture(), colored = texture(), data = new THREE.DataTexture(new Uint8Array(16), 2, 2);
  colored.colorSpace = THREE.SRGBColorSpace;
  const a = new THREE.MeshStandardMaterial({ aoMap: unknown }), b = new THREE.MeshStandardMaterial({ aoMap: colored });
  const c = new THREE.MeshStandardMaterial({ aoMap: data });
  const f = fixture([a, b, c]);
  try { assert.equal(narrowDataTextureChannels(f.gltf, new Set([b, c])), 0); }
  finally { f.dispose(); unknown.dispose(); colored.dispose(); data.dispose(); }
});
