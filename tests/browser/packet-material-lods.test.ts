import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { validatePacket, type SceneMaterial, type ScenePacket } from "../../browser/model/packet.ts";
import { configurePacketMaterial, PacketMaterialLods } from "../../browser/renderer/packet-material-lods.ts";
import { PacketLods } from "../../browser/renderer/packet-lods.ts";
import { TexturePool } from "../../browser/renderer/textures.ts";

function material(id = "near"): SceneMaterial {
  return { id, color: "#a0b0c0", repeat: 1, roughness: .7, metalness: .2, emissive: null, emissiveStrength: null,
    texture: null, normalTexture: null, normalScale: null, metallicRoughnessTexture: null,
    heightTexture: null, heightScale: null, heightOffset: null, opacity: null, alphaCutoff: null };
}
function packet(): ScenePacket {
  const m = material(); m.lods = [{ screenCoverage: .2, material: material("far") }];
  return { manifest: { schemaVersion: 3, revision: 1, success: true, id: "material-lod", name: "material-lod",
    units: "meters", axes: "right-handed-z-up", geometryValidation: "not-run", navigationValidation: "not-run", diagnostics: [],
    materials: [m], assets: [], assetByteCount: 0, meshes: [], colliders: [], vertexCount: 0, indexCount: 0 },
    positions: new Float32Array(), normals: new Float32Array(), colors: new Float32Array(), uvs: new Float32Array(),
    indices: new Uint32Array(), assets: new Uint8Array() };
}
function view(z: number) {
  const camera = new THREE.PerspectiveCamera(70, 16 / 9, .1, 1000); camera.position.z = z; camera.updateMatrixWorld(); return camera;
}

test("preview filtering updates resident and newly decoded mipmaps and restores the Medium sampler", async () => {
  const old = globalThis.createImageBitmap;
  globalThis.createImageBitmap = async () => ({ close() {} }) as ImageBitmap;
  const pool = new TexturePool({ capabilities: { getMaxAnisotropy: () => 8 } } as THREE.WebGLRenderer);
  const leases = new Set<string>(), bytes = new Uint8Array([0]);
  try {
    const resident = await pool.acquire("resident", bytes, "image/png", true, leases);
    assert.equal(resident.anisotropy, 1);
    pool.setAnisotropy(16);
    const incoming = await pool.acquire("incoming", bytes, "image/png", false, leases);
    assert.deepEqual([resident.anisotropy, incoming.anisotropy], [8, 8]);
    pool.setAnisotropy();
    await Promise.resolve();
    assert.deepEqual([resident.anisotropy, incoming.anisotropy], [1, 1]);
    pool.release(leases);
    await Promise.resolve();
    assert.equal(resident.version, 0);
  } finally {
    pool.dispose();
    if (old) globalThis.createImageBitmap = old;
    else Reflect.deleteProperty(globalThis, "createImageBitmap");
  }
});

test("packet validation checks every flat authored material alternative before texture allocation", () => {
  assert.equal(validatePacket(packet()).manifest.materials[0].lods?.length, 1);
  const changes: Array<(m: SceneMaterial) => void> = [
    m => { m.lods![0].screenCoverage = 0; }, m => { m.lods![0].screenCoverage = 1; },
    m => { m.lods![0].screenCoverage = NaN; }, m => { m.lods!.push({ screenCoverage: .3, material: material("third") }); },
    m => { m.lods![0].material.id = m.id; }, m => { m.lods![0].material.id = ""; },
    m => { m.lods![0].material.lods = [{ screenCoverage: .1, material: material("nested") }]; },
    m => { m.lods![0].material.texture = "missing-image"; },
    m => { m.lods![0].material.roughness = 2; },
    m => { m.lods![0].material.program = { version: 99 } as never; },
    m => { m.lods = Array.from({ length: 17 }, (_, i) => ({ screenCoverage: .5 ** (i + 1), material: material("far" + i) })); },
  ];
  for (const change of changes) {
    const p = packet(); change(p.manifest.materials[0]);
    assert.throws(() => validatePacket(p), /material/i, change.toString());
  }
  const maximum = packet(); maximum.manifest.materials[0].lods = Array.from({ length: 16 }, (_, i) => ({ screenCoverage: .5 ** (i + 1), material: material("far" + i) }));
  assert.doesNotThrow(() => validatePacket(maximum));
});

test("original and alternate packet looks share the exact map, alpha, live-operation and normal binding path", async () => {
  const definition = material(), textures = new Map(["color", "normal", "mr", "ao", "emission"].map(id => [id, new THREE.Texture()]));
  Object.assign(definition, { texture: "color", normalTexture: "normal", metallicRoughnessTexture: "mr",
    occlusionTexture: "ao", emissiveTexture: "emission", normalScale: .4, alphaMode: "MASK", alphaCutoff: .3,
    doubleSided: false, occlusionStrength: .7, emissiveStrength: 2, baseColorFactor: [.2, .4, .6],
    live: [{ id: "future-unknown-effect", stage: "surface", requires: [], arguments: {} }] });
  const requests: string[] = [];
  const context = { batch: false, visible: true, wireframe: false,
    async texture(id: string | null, color: boolean) { requests.push(`${id}:${color}`); return id === null ? null : textures.get(id)!; },
    lookup: (id: string) => textures.get(id) };
  const near = new THREE.MeshStandardMaterial(), far = new THREE.MeshStandardMaterial();
  assert.deepEqual(await configurePacketMaterial(near, definition, context), []);
  await configurePacketMaterial(far, definition, context);
  for (const key of ["map", "normalMap", "roughnessMap", "metalnessMap", "aoMap", "emissiveMap"] as const) assert.equal(near[key], far[key]);
  assert.equal(far.map, textures.get("color")); assert.equal(far.roughnessMap, far.metalnessMap);
  assert.deepEqual(far.normalScale.toArray(), [.4, .4]); assert.deepEqual(far.color.toArray(), [.2, .4, .6]);
  assert.equal(far.alphaTest, .3); assert.equal(far.transparent, false); assert.equal(far.side, THREE.FrontSide);
  assert.equal(far.aoMapIntensity, .7); assert.equal(far.emissiveIntensity, 2); assert.equal(far.vertexColors, true);
  assert.deepEqual(requests.slice(0, 5), ["color:true", "normal:false", "mr:false", "emission:true", "ao:false"]);
  near.dispose(); far.dispose(); textures.forEach(texture => texture.dispose());
});

test("packet materials select per mesh with resolution-aware hysteresis and retain edited near look", () => {
  const geometry = new THREE.PlaneGeometry(2, 2), near = new THREE.MeshStandardMaterial(), far = new THREE.MeshStandardMaterial();
  const a = new THREE.Mesh(geometry, near), b = new THREE.Mesh(geometry, near); b.position.z = -40;
  const lod = new PacketMaterialLods([a, b].map(mesh => ({ mesh, levels: [far], screenCoverage: [.2] })));
  const camera = view(2);
  try {
    lod.frame(camera, 1080, false); assert.equal(a.material, near); assert.equal(b.material, far);
    camera.position.z = 20; lod.frame(camera, 1080, false); assert.equal(a.material, far);
    camera.position.z = 8; lod.frame(camera, 1080, false); assert.equal(a.material, far, "hysteresis retains prior distant selection");
    camera.position.z = 5; lod.frame(camera, 1080, false); assert.equal(a.material, near);
    camera.position.z = 20; lod.frame(camera, 4320, false); assert.equal(a.material, near);
    lod.frame(camera, 1080, false); assert.equal(a.material, far);
    lod.frame(camera, 1080, true); assert.equal(a.material, near); assert.equal(b.material, near);
    const edited = new THREE.MeshStandardMaterial({ color: "#123456" }); a.material = edited;
    lod.frame(camera, 1080, true); assert.equal(a.material, edited, "intentional shader/edit replacement is the current canonical look");
    lod.frame(camera, 1080, false); assert.equal(a.material, far);
    lod.restore(); assert.equal(a.material, edited); edited.dispose();
    const baked = geometry.clone(); a.geometry = baked;
    lod.frame(camera, 1080, false); assert.equal(a.material, edited, "a custom baked UV geometry retains its matching look"); baked.dispose();
  } finally { lod.dispose(); geometry.dispose(); near.dispose(); }
});

test("material-only packet shadows and picking use original sides and preserve source ownership", () => {
  const scene = new THREE.Scene(), root = new THREE.Group(); scene.add(root);
  const geometry = new THREE.PlaneGeometry(2, 2), near = new THREE.MeshStandardMaterial({ side: THREE.DoubleSide }), far = new THREE.MeshStandardMaterial();
  const source = new THREE.Mesh(geometry, near); source.castShadow = true; root.add(source);
  let farDisposals = 0, nearDisposals = 0;
  far.addEventListener("dispose", () => farDisposals++); near.addEventListener("dispose", () => nearDisposals++);
  const materials = new PacketMaterialLods([{ mesh: source, levels: [far], screenCoverage: [.2] }]);
  const geometryLods = new PacketLods(scene, root, [], { restoreMaterials: () => materials.restore(),
    frameMaterials: (camera, height, editing) => materials.frame(camera, height, editing) });
  try {
    geometryLods.frame(view(30), 1080, false, 9000); assert.equal(source.material, near);
    geometryLods.frame(view(30), 9000, false, 1080); assert.equal(source.material, far);
    const picker = new THREE.Raycaster(new THREE.Vector3(0,0,-3), new THREE.Vector3(0,0,1));
    assert.ok(picker.intersectObject(source).length); assert.equal(source.material, far);
    const before = scene.children[0] as THREE.Mesh, after = scene.children.at(-1) as THREE.Mesh;
    before.onBeforeShadow({} as never, {} as never, {} as never, {} as never, {} as never, {} as never);
    assert.equal(source.material, near); assert.equal(source.castShadow, true);
    after.onBeforeShadow({} as never, {} as never, {} as never, {} as never, {} as never, {} as never);
    assert.equal(source.material, far);
    geometryLods.dispose(); materials.dispose(); materials.dispose();
    assert.equal(source.material, near); assert.equal(farDisposals, 1); assert.equal(nearDisposals, 0);
  } finally { geometryLods.dispose(); materials.dispose(); geometry.dispose(); near.dispose(); }
});
