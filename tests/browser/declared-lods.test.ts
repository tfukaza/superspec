import test, { type TestContext } from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";

const hooks = registerHooks({ resolve(specifier, context, nextResolve) {
  return nextResolve(specifier.startsWith(".") && !/\.[a-z]+$/i.test(specifier) ? specifier + ".ts" : specifier, context);
} });
const { loadDeclaredLods } = await import("../../adapters/three/src/declaredLods.ts");
const { hasDeclaredLodOwner } = await import("../../adapters/three/src/lodOwnership.ts");
const { mergeCityInstances, CityVisibility } = await import("../../adapters/three/src/cityVisibility.ts");
const { clusterCityGeometry } = await import("../../adapters/three/src/cityClusters.ts");
const { markOutlineExclusions } = await import("../../adapters/three/src/outlineExclusions.ts");
const { publishedCityGeometry } = await import("#render-engine");
const { registerCityTextureLod } = await import("../../adapters/three/src/cityTextureLoader.ts");
const { resolveRenderDiagnostics } = await import("../../adapters/three/src/renderProfile.ts");
hooks.deregister();

function fixture() {
  const json: any = { asset: { version: "2.0" }, scene: 0, scenes: [{ nodes: [0] }], extensionsUsed: ["MSFT_lod"],
    buffers: [{ byteLength: 0 }], bufferViews: [], accessors: [],
    nodes: [{ name: "near-group", children: [1], extensions: { MSFT_lod: { ids: [2, 3] } },
      extras: { worldSpecLod: { version: 1, distances: [0, 10, 30] } } },
      { name: "near-mesh", mesh: 0 }, { name: "middle-mesh", mesh: 1 }, { name: "far-mesh", mesh: 2 }],
    materials: [{ name: "near-material", doubleSided: true, pbrMetallicRoughness: { baseColorFactor: [1, .1, .1, 1] },
      extensions: { MSFT_lod: { ids: [1] } }, extras: { MSFT_screencoverage: [.2, 0] } },
      { name: "far-material", pbrMetallicRoughness: { baseColorFactor: [.1, 1, .1, 1] } },
      { name: "own-node-material", pbrMetallicRoughness: { baseColorFactor: [.1, .1, 1, 1] } }], meshes: [] };
  const chunks: Buffer[] = []; let length = 0;
  function accessor(array: Float32Array | Uint16Array, type: string, min?: number[], max?: number[]) {
    const data = Buffer.from(array.buffer, array.byteOffset, array.byteLength), aligned = (data.length + 3) & ~3;
    const bytes = Buffer.alloc(aligned); data.copy(bytes); chunks.push(bytes);
    const view = json.bufferViews.push({ buffer: 0, byteOffset: length, byteLength: data.length }) - 1; length += aligned;
    const width = type === "VEC3" ? 3 : type === "VEC2" ? 2 : 1;
    return json.accessors.push({ bufferView: view, componentType: array instanceof Float32Array ? 5126 : 5123,
      count: array.length / width, type, ...(min ? { min, max } : {}) }) - 1;
  }
  const position = accessor(new Float32Array([-1,-1,0, 1,-1,0, 1,1,0, -1,1,0]), "VEC3", [-1,-1,0], [1,1,0]);
  const normal = accessor(new Float32Array([0,0,1, 0,0,1, 0,0,1, 0,0,1]), "VEC3");
  const color = accessor(new Float32Array([1,1,1, .5,.5,.5, 1,1,1, .5,.5,.5]), "VEC3");
  const uv = accessor(new Float32Array([0,0, 1,0, 1,1, 0,1]), "VEC2");
  const near = accessor(new Uint16Array([0,1,2, 0,2,3]), "SCALAR"), far = accessor(new Uint16Array([0,1,2]), "SCALAR");
  json.meshes = [0, 1, 2].map(i => ({ primitives: [{ attributes: { POSITION: position, NORMAL: normal, COLOR_0: color, TEXCOORD_0: uv },
    indices: i === 0 ? near : far, material: i === 2 ? 2 : 0 }] }));
  return { json, accessor,
    bytes() {
      json.buffers[0].byteLength = length;
      const text = Buffer.from(JSON.stringify(json)), size = (text.length + 3) & ~3;
      const header = Buffer.alloc(20 + size, 0x20); header.write("glTF"); header.writeUInt32LE(2, 4);
      header.writeUInt32LE(header.length + 8 + length, 8); header.writeUInt32LE(size, 12); header.write("JSON", 16); text.copy(header, 20);
      const bin = Buffer.alloc(8); bin.writeUInt32LE(length); bin.writeUInt32LE(0x004e4942, 4);
      const result = Buffer.concat([header, bin, ...chunks]); return result.buffer.slice(result.byteOffset, result.byteOffset + result.length);
    } };
}
async function parse(f = fixture()) { return new GLTFLoader().parseAsync(f.bytes(), ""); }
function view(z: number, fov = 70) {
  const camera = new THREE.PerspectiveCamera(fov, 16 / 9, .1, 1000);
  camera.position.z = z; camera.updateMatrixWorld(true); return camera;
}
function shown(root: THREE.Object3D): THREE.Mesh[] {
  const meshes: THREE.Mesh[] = []; root.traverseVisible(object => { if (object instanceof THREE.Mesh) meshes.push(object); }); return meshes;
}

async function streamedFixture(t: TestContext, movable: boolean) {
  const f = fixture();
  f.json.nodes[0].extras.worldSpecLod.distances = [0, 1, 30];
  f.json.materials[0].extras = { worldSpecLod: { version: 1, distances: [0, 1] } };
  f.json.materials[movable ? 0 : 1].pbrMetallicRoughness.baseColorTexture = { index: 5 };
  f.json.scenes[0].extras = { worldSpecTextureLod: { version: 1, pairs: [{ near: 5, far: 6 }] } };
  if (movable) f.json.scenes[0].extras.modernIndustrialCableDrag = 1;
  f.json.textures = Array.from({ length: 7 }, (_, source) => ({ source }));
  f.json.images = f.json.textures.map(({ source }: { source: number }) => ({ uri: source + ".png" }));
  const images: string[] = [];
  const manager = new THREE.LoadingManager();
  manager.addHandler(/\.png$/, { load(url: string, loaded: (texture: THREE.Texture) => void) {
    images.push(url);
    loaded(new THREE.DataTexture(new Uint8Array([1, 2, 3, 255]), 1, 1));
  } } as THREE.Loader);
  const oldSelf = Object.getOwnPropertyDescriptor(globalThis, "self");
  Object.defineProperty(globalThis, "self", { configurable: true, value: globalThis });
  t.after(() => {
    if (oldSelf) Object.defineProperty(globalThis, "self", oldSelf);
    else Reflect.deleteProperty(globalThis, "self");
  });
  const gltf = await registerCityTextureLod(new GLTFLoader(manager)).parseAsync(f.bytes(), "");
  const declared = movable ? undefined : await loadDeclaredLods(gltf);
  const renderer = { domElement: { height: 1080, width: 1920 }, initTexture() {} } as THREE.WebGLRenderer;
  const controller = await publishedCityGeometry(gltf, renderer, resolveRenderDiagnostics(), { declared });
  return { gltf, controller, images };
}

test("a close declared node and its selected material stream the full texture, then release it on departure", async t => {
  const { gltf, controller, images } = await streamedFixture(t, false);
  try {
    assert.deepEqual(images, ["6.png"]);
    controller.beginFrame(view(2));
    const mesh = shown(gltf.scene).find(m => m.name === "middle-mesh")!;
    const material = mesh.material as THREE.MeshStandardMaterial, low = material.map;
    await controller.settle();
    assert.deepEqual(images, ["6.png", "5.png"]);
    assert.notEqual(material.map, low);
    assert.equal(controller.textureStats()?.high, 1);
    controller.beginFrame(view(50));
    await controller.settle();
    assert.equal(material.map, low);
    assert.equal(controller.textureStats()?.high, 0);
  } finally { controller.dispose(); }
});

test("a movable connector streams by its current world position while static culling is disabled", async t => {
  const { gltf, controller, images } = await streamedFixture(t, true);
  const parent = gltf.scene.getObjectByName("near-group")!, camera = view(2);
  try {
    parent.position.x = 100;
    controller.beginFrame(camera);
    await controller.settle();
    assert.deepEqual(images, ["6.png"]);
    parent.position.x = 0;
    controller.beginFrame(camera);
    await controller.settle();
    assert.deepEqual(images, ["6.png", "5.png"]);
    assert.equal(controller.textureStats()?.high, 1);
  } finally { controller.dispose(); }
});

test("disposing a published world releases secondary scenes and releases shared resources once", async () => {
  const gltf = await parse();
  const mesh = shown(gltf.scene)[0];
  const texture = new THREE.Texture();
  const material = new THREE.MeshStandardMaterial({ map: texture });
  const second = new THREE.Group().add(new THREE.Mesh(mesh.geometry, material));
  gltf.scenes.push(second);
  const disposed = [0, 0, 0];
  [mesh.geometry, material, texture].forEach((resource, i) => resource.addEventListener("dispose", () => disposed[i]++));
  const renderer = { domElement: { height: 1080, width: 1920 } } as THREE.WebGLRenderer;
  const controller = await publishedCityGeometry(gltf, renderer, resolveRenderDiagnostics({ visibility: { cityCulling: false } }), {});
  controller.dispose(); controller.dispose();
  assert.deepEqual(disposed, [1, 1, 1]);
});

test("real GLTFLoader fallback keeps only the highest node; declared distances select whole authored alternatives", async () => {
  const gltf = await parse(), original = gltf.scene.getObjectByName("near-mesh") as THREE.Mesh;
  assert.deepEqual(shown(gltf.scene).map(mesh => mesh.name), ["near-mesh"]);
  const geometry = original.geometry, material = original.material;
  const lod = await loadDeclaredLods(gltf, { hysteresis: 0 });
  try {
    lod.frame(view(2), 1080); assert.equal(shown(gltf.scene)[0], original);
    assert.equal(original.geometry, geometry); assert.equal(original.material, material);
    lod.frame(view(15), 1080); assert.equal(shown(gltf.scene)[0].name, "middle-mesh");
    lod.frame(view(50), 1080); assert.equal(shown(gltf.scene)[0].name, "far-mesh");
    assert.equal((shown(gltf.scene)[0].material as THREE.Material).name, "own-node-material", "far node keeps its own material chain");
    lod.restore(); assert.equal(shown(gltf.scene)[0], original); assert.equal(original.geometry, geometry);
  } finally { lod.dispose(); }
});

test("authored screen thresholds honor small nearby geometry and zoom and drawing-buffer resolution", async () => {
  const f = fixture(); delete f.json.nodes[0].extras.worldSpecLod;
  f.json.nodes[0].extras.MSFT_screencoverage = [.6, .2, .01];
  const gltf = await parse(f), lod = await loadDeclaredLods(gltf, { hysteresis: 0 });
  try {
    const camera = view(3);
    lod.frame(camera, 1080); assert.equal(shown(gltf.scene)[0].name, "middle-mesh", "explicit nearby swatch may use its authored middle level");
    camera.zoom = 2; camera.updateProjectionMatrix(); lod.frame(camera, 1080);
    assert.equal(shown(gltf.scene)[0].name, "near-mesh");
    camera.zoom = 1; camera.updateProjectionMatrix(); lod.frame(camera, 2160);
    assert.equal(shown(gltf.scene)[0].name, "near-mesh");
    lod.frame(view(500), 1080); assert.equal(shown(gltf.scene)[0].name, "far-mesh", "last coverage hint does not silently cull the object");
    lod.frame(view(.01), 1080); assert.equal(shown(gltf.scene)[0].name, "near-mesh", "near-plane intersection retains original detail");
  } finally { lod.dispose(); }
});

test("geometry and material LOD thresholds accept independent effective pixel heights", async () => {
  const gltf = await parse(), lod = await loadDeclaredLods(gltf, { hysteresis: 0 }), camera = view(8);
  const displayed = () => {
    const [mesh] = shown(gltf.scene);
    return [mesh.name, (mesh.material as THREE.Material).name];
  };
  try {
    lod.frame(camera, 1080, 2160);
    assert.deepEqual(displayed(), ["near-mesh", "near-material"]);
    lod.frame(camera, 540, 2160);
    assert.deepEqual(displayed(), ["middle-mesh", "near-material"]);
    lod.frame(camera, 540, 1080);
    assert.deepEqual(displayed(), ["middle-mesh", "far-material"]);
    lod.frame(camera, 1080, 1080);
    assert.deepEqual(displayed(), ["near-mesh", "far-material"]);
  } finally { lod.dispose(); }
});

test("authored metre ranges scale with FOV and resolution and apply hysteresis", async () => {
  const gltf = await parse(), lod = await loadDeclaredLods(gltf);
  try {
    lod.frame(view(10.5), 1080); assert.equal(shown(gltf.scene)[0].name, "near-mesh");
    lod.frame(view(12), 1080); assert.equal(shown(gltf.scene)[0].name, "middle-mesh");
    lod.frame(view(9), 1080); assert.equal(shown(gltf.scene)[0].name, "middle-mesh");
    lod.frame(view(8), 1080); assert.equal(shown(gltf.scene)[0].name, "near-mesh");
    lod.frame(view(15), 2160); assert.equal(shown(gltf.scene)[0].name, "near-mesh");
    lod.frame(view(15, 35), 1080); assert.equal(shown(gltf.scene)[0].name, "near-mesh");
  } finally { lod.dispose(); }
});

test("late reattachment updates the whole chosen LOD before color or outline reads child transforms", async () => {
  const f = fixture();
  f.json.nodes[3] = { name: "far-root", translation: [0, 2, 0], children: [4] };
  f.json.nodes.push({ mesh: 2, name: "far-child", translation: [3, 0, 0] });
  const gltf = await parse(f);
  const lod = await loadDeclaredLods(gltf, { hysteresis: 0 });
  try {
    const source = gltf.scene.getObjectByName("near-group")!;
    const camera = view(100);
    for (const x of [5, -4]) {
      source.position.x = x;
      lod.frame(camera, 1080);
      const child = gltf.scene.getObjectByName("far-child")!;
      assert.deepEqual(new THREE.Vector3().setFromMatrixPosition(child.matrixWorld).toArray(), [x + 3, 2, 0]);
      lod.restore(); // Canonical shadow pass can intervene between selections.
      assert.equal(gltf.scene.getObjectByName("far-child"), undefined);
    }
  } finally { lod.dispose(); }
});

test("material alternatives are selected per receiving mesh with glTF vertex-color and normal conventions", async () => {
  const f = fixture(); delete f.json.nodes[0].extensions;
  f.json.nodes.push({ name: "other-placement", mesh: 0, translation: [0, 0, -30] }); f.json.scenes[0].nodes.push(4);
  const gltf = await parse(f), original = gltf.scene.getObjectByName("near-mesh") as THREE.Mesh;
  const distant = gltf.scene.getObjectByName("other-placement") as THREE.Mesh;
  const near = original.material as THREE.MeshStandardMaterial;
  assert.equal(distant.material, near);
  const lod = await loadDeclaredLods(gltf, { hysteresis: 0 });
  try {
    lod.frame(view(2), 1080);
    assert.equal(original.material, near); assert.notEqual(distant.material, near);
    const far = distant.material as THREE.MeshStandardMaterial;
    assert.equal(far.name, "far-material"); assert.equal(far.vertexColors, true);
    assert.equal(far.normalScale.y, near.normalScale.y, "assignFinalMaterial applies derivative-tangent normal convention");
    assert.equal(original.geometry, distant.geometry);
    lod.restore(); assert.equal(distant.material, near);
  } finally { lod.dispose(); }
});

test("canonical picking retains original geometry/material sides while alternatives are visible", async () => {
  const f = fixture(); delete f.json.nodes[0].extensions;
  f.json.materials[0].extras = { worldSpecLod: { version: 1, distances: [0, 1] } };
  const gltf = await parse(f), source = gltf.scene.getObjectByName("near-mesh") as THREE.Mesh, raycast = source.raycast;
  const lod = await loadDeclaredLods(gltf, { hysteresis: 0 });
  try {
    lod.frame(view(20), 1080); assert.equal((source.material as THREE.Material).side, THREE.FrontSide);
    const picker = new THREE.Raycaster(new THREE.Vector3(0, 0, -3), new THREE.Vector3(0, 0, 1));
    assert.ok(picker.intersectObject(gltf.scene, true).some(hit => hit.object === source), "back picking uses the authored double-sided near material");
    assert.equal((source.material as THREE.Material).side, THREE.FrontSide, "picking restores the displayed material");
  } finally { lod.dispose(); }
  assert.equal(source.raycast, raycast);

  const nodes = await parse(), controller = await loadDeclaredLods(nodes, { hysteresis: 0 });
  try {
    controller.frame(view(50), 1080);
    const picker = new THREE.Raycaster(new THREE.Vector3(-.8, .8, 3), new THREE.Vector3(0, 0, -1));
    const hits = picker.intersectObject(nodes.scene, true);
    assert.ok(hits.some(hit => hit.object.name === "near-mesh"), "triangle omitted by far mesh remains canonically pickable");
    assert.ok(hits.every(hit => hit.object.name !== "far-mesh"));
  } finally { controller.dispose(); }
});

test("variant binding exposes all materials and restoration preserves full-detail shadow casters", async () => {
  const f = fixture(); f.json.nodes[3].extras = { worldSpecOutline: false };
  const gltf = await parse(f), near = gltf.scene.getObjectByName("near-mesh") as THREE.Mesh;
  const lod = await loadDeclaredLods(gltf, { hysteresis: 0 }), found = new Set<string>();
  try {
    await lod.withVariants(async () => {
      gltf.scene.traverse(node => {
        if (!(node instanceof THREE.Mesh)) return;
        node.castShadow = true;
        for (const m of Array.isArray(node.material) ? node.material : [node.material]) { found.add(m.name); m.userData.bound = true; }
      });
      assert.equal(markOutlineExclusions(gltf.scene), true, "binding discovers exclusions on detached far node levels");
    });
    assert.ok(found.has("near-material") && found.has("far-material") && found.has("own-node-material"));
    assert.equal(gltf.scene.children.length, 1);
    lod.frame(view(50), 1080); assert.equal(shown(gltf.scene)[0].castShadow, false);
    assert.equal(shown(gltf.scene)[0].layers.isEnabled(29), true, "selected variant retains its outline exclusion");
    assert.equal((shown(gltf.scene)[0].material as THREE.Material).userData.bound, true);
    lod.restore(); assert.equal(shown(gltf.scene)[0], near); assert.equal(near.castShadow, true);
    lod.frame(view(50), 1080); assert.equal(shown(gltf.scene)[0].name, "far-mesh");
    await assert.rejects(lod.withVariants(async () => { throw Error("binding failed"); }), /binding failed/);
    assert.equal(gltf.scene.children.length, 1); assert.equal(shown(gltf.scene)[0], near);
  } finally { lod.dispose(); }
});

test("diagnostics count selected and displayed node/material tiers only when requested", async () => {
  const gltf = await parse(), lod = await loadDeclaredLods(gltf, { hysteresis: 0 });
  try {
    lod.frame(view(15), 1080);
    const middle = lod.stats();
    assert.deepEqual(middle.nodes, { total: 1, byTier: [0, 1], visibleByTier: [0, 1] });
    assert.equal(middle.materials.meshes, 2); assert.equal(middle.materials.slots, 2);
    assert.deepEqual(middle.materials.visibleByTier, [0, 1], "only the selected node's material contributes");
    lod.frame(view(50), 1080);
    assert.deepEqual(lod.stats().nodes.visibleByTier, [0, 0, 1]);
    assert.deepEqual(lod.stats().materials.visibleByTier, [], "unselected alternatives are not displayed receivers");
    lod.restore();
    assert.deepEqual(lod.stats().nodes.visibleByTier, [1]);
    assert.deepEqual(lod.stats().materials.visibleByTier, [1], "shadow restoration reports the canonical near binding");
  } finally { lod.dispose(); }
  assert.deepEqual(lod.stats().nodes, { total: 0, byTier: [], visibleByTier: [] });
});

test("invalid chains fall back independently and unknown glTF declarations are never inferred from names", async () => {
  const f = fixture(); f.json.nodes[0].extensions.MSFT_lod.ids = [999];
  const gltf = await parse(f), warnings: string[] = [], lod = await loadDeclaredLods(gltf, { hysteresis: 0, onWarning: message => warnings.push(message) });
  try {
    lod.frame(view(50), 1080);
    assert.equal(shown(gltf.scene)[0].name, "near-mesh");
    assert.equal((shown(gltf.scene)[0].material as THREE.Material).name, "far-material", "independent valid material chain still works");
    assert.match(warnings[0], /nodes\[0\]/);
  } finally { lod.dispose(); }
  for (const mutate of [
    (json: any) => { json.nodes[0].extras.worldSpecLod.distances = [0, 10, 9]; },
    (json: any) => { json.nodes[0].extensions.MSFT_lod.ids = [2, 2]; },
    (json: any) => { json.nodes[2].extensions = { MSFT_lod: { ids: [0] } }; },
    (json: any) => { json.scenes[0].nodes.push(2); },
    (json: any) => { json.nodes[2].children = [1]; },
    (json: any) => { json.nodes[2].children = [2]; },
  ]) {
    const data = fixture(); mutate(data.json); const parsed = await parse(data);
    const controller = await loadDeclaredLods(parsed, { onWarning: () => {} });
    assert.equal(controller.managedNodes.size, 0);
    assert.equal(parsed.scene.getObjectByName("near-mesh")?.parent?.name, "near-group", "malformed lower nodes cannot steal canonical children");
    controller.dispose();
  }
});

test("material-only instanced placements retain near detail if any instance is near and preserve canonical buffers", async () => {
  const f = fixture(); delete f.json.nodes[0].extensions;
  const translation = f.accessor(new Float32Array([0,0,0, 0,0,-40]), "VEC3");
  f.json.extensionsUsed.push("EXT_mesh_gpu_instancing");
  f.json.nodes[1].extensions = { EXT_mesh_gpu_instancing: { attributes: { TRANSLATION: translation } } };
  const gltf = await parse(f), source = gltf.scene.getObjectByName("near-mesh") as THREE.InstancedMesh;
  assert.equal(source.isInstancedMesh, true);
  const near = source.material, geometry = source.geometry, matrices = source.instanceMatrix, values = matrices.array.slice();
  source.setColorAt(0, new THREE.Color(1, 0, 0));
  source.userData.worldSpecOutline = false;
  const lod = await loadDeclaredLods(gltf, { hysteresis: 0 });
  try {
    await lod.withVariants(() => {
      markOutlineExclusions(gltf.scene);
      let holder: THREE.Mesh | undefined;
      gltf.scene.traverse(node => {
        if (node instanceof THREE.Mesh && node.material.name === "far-material") holder = node;
      });
      assert.ok(holder instanceof THREE.InstancedMesh);
      assert.equal(holder.instanceColor, source.instanceColor);
      assert.equal(holder.layers.mask, source.layers.mask);
    });
    lod.frame(view(2), 1080); assert.equal(source.material, near);
    lod.frame(view(50), 1080); assert.notEqual(source.material, near);
    assert.equal(source.geometry, geometry); assert.equal(source.instanceMatrix, matrices);
    assert.deepEqual(matrices.array, values); assert.equal(source.count, 2);
    const hits = new THREE.Raycaster(new THREE.Vector3(0,0,3), new THREE.Vector3(0,0,-1)).intersectObject(source);
    assert.ok(hits.some(hit => hit.instanceId === 0));
    lod.restore(); assert.equal(source.material, near);
  } finally { lod.dispose(); }
});

test("sixteen material alternatives are accepted and explicit coverage takes precedence over distance", async () => {
  const f = fixture(); delete f.json.nodes[0].extensions;
  f.json.materials = Array.from({ length: 17 }, (_, i) => ({ name: "level-" + i,
    pbrMetallicRoughness: { baseColorFactor: [1, 1, 1, 1] } }));
  f.json.materials[0].extensions = { MSFT_lod: { ids: Array.from({ length: 16 }, (_, i) => i + 1) } };
  f.json.materials[0].extras = { worldSpecLod: { version: 1, distances: Array.from({ length: 17 }, (_, i) => i * 10) },
    MSFT_screencoverage: Array.from({ length: 17 }, (_, i) => .5 * .4 ** i) };
  const gltf = await parse(f), lod = await loadDeclaredLods(gltf, { hysteresis: 0 });
  try {
    lod.frame(view(8), 1080); assert.equal((shown(gltf.scene)[0].material as THREE.Material).name, "level-2");
    lod.frame(view(1e10), 1080); assert.equal((shown(gltf.scene)[0].material as THREE.Material).name, "level-16");
  } finally { lod.dispose(); }
});

test("a failed optional load preserves the canonical scene and cleans successful sibling dependencies", async () => {
  const f = fixture(); f.json.nodes[3].mesh = 999;
  const gltf = await parse(f), warnings: string[] = [];
  const lod = await loadDeclaredLods(gltf, { onWarning: message => warnings.push(message) });
  assert.equal(lod.managedNodes.size, 0); assert.match(warnings[0], /nodes\[0\]/);
  const loaded = await gltf.parser.getDependency("node", 2) as THREE.Mesh;
  let disposed = 0; loaded.geometry.addEventListener("dispose", () => disposed++);
  lod.frame(view(50), 1080); assert.equal(shown(gltf.scene)[0].name, "near-mesh");
  lod.dispose(); assert.equal(disposed, 1);
});

test("empty declarations have no frame traversal and disposal does not release source geometry or material", async () => {
  const f = fixture(); delete f.json.nodes[0].extensions; delete f.json.materials[0].extensions;
  const gltf = await parse(f), lod = await loadDeclaredLods(gltf), camera = view(50);
  let updates = 0;
  gltf.scene.updateWorldMatrix = () => { updates++; }; camera.updateWorldMatrix = () => { updates++; };
  lod.frame(camera, 1080); assert.equal(updates, 0); lod.dispose();
  const data = await parse(), source = data.scene.getObjectByName("near-mesh") as THREE.Mesh;
  let geometries = 0, materials = 0;
  source.geometry.addEventListener("dispose", () => geometries++);
  (source.material as THREE.Material).addEventListener("dispose", () => materials++);
  const controller = await loadDeclaredLods(data); controller.frame(view(50), 1080); controller.dispose(); controller.dispose();
  assert.equal(geometries, 0); assert.equal(materials, 0); assert.equal(shown(data.scene)[0], source);
});

test("disposing alternate geometry does not delete glTF accessor buffers shared with the canonical mesh", async () => {
  const gltf = await parse(), near = gltf.scene.getObjectByName("near-mesh") as THREE.Mesh;
  const lod = await loadDeclaredLods(gltf, { hysteresis: 0 });
  lod.frame(view(50), 1080);
  const far = shown(gltf.scene)[0].geometry;
  assert.notEqual(far, near.geometry);
  assert.equal(far.getAttribute("position"), near.geometry.getAttribute("position"));
  const attributes = far.attributes, index = far.index;
  let disposals = 0;
  far.addEventListener("dispose", () => {
    disposals++;
    assert.deepEqual(Object.keys(far.attributes), [], "WebGLGeometries cannot remove borrowed accessor storage");
    assert.equal(far.index, index, "the alternate-only index is still released");
  });
  lod.dispose(); lod.dispose();
  assert.equal(disposals, 1);
  assert.equal(far.attributes, attributes); assert.equal(far.index, index);
  assert.equal(near.geometry.getAttribute("position"), attributes.position);
});

test("moving the source parent preserves authored relative placement and independent object visibility", async () => {
  const f = fixture(); f.json.nodes[0].translation = [3, 0, 0]; f.json.nodes[2].translation = [3, 0, 0]; f.json.nodes[3].translation = [3, 0, 0];
  const gltf = await parse(f), lod = await loadDeclaredLods(gltf, { hysteresis: 0 }), source = gltf.scene.children[0];
  try {
    source.position.x += 5; gltf.scene.position.y = 7;
    lod.frame(view(50), 1080);
    const point = shown(gltf.scene)[0].getWorldPosition(new THREE.Vector3());
    assert.deepEqual(point.toArray(), [8, 7, 0]);
    lod.restore(); source.visible = false; lod.frame(view(50), 1080); assert.equal(shown(gltf.scene).length, 0);
    lod.restore(); assert.equal(source.visible, false);
  } finally { lod.dispose(); }
});

test("declared sources are protected from city geometry replacement and view-culling overrides", async () => {
  const gltf = await parse(), lod = await loadDeclaredLods(gltf), source = gltf.scene.getObjectByName("near-mesh") as THREE.Mesh;
  const original = source.geometry, far = original.clone(), map = new Map([[original, [far, far]]]);
  try {
    assert.equal(lod.ownsNode(source), true); assert.equal(lod.ownsMesh(source), true); assert.equal(hasDeclaredLodOwner(source), true);
    const clusters = clusterCityGeometry(gltf.scene, map); assert.equal(source.geometry, original); assert.equal(source.children.length, 0);
    const visibility = new CityVisibility(gltf.scene, true, map, { clusterPairs: clusters });
    visibility.frame(view(50), 1080); assert.equal(source.visible, true); assert.equal(source.geometry, original); visibility.dispose();
    const a = new THREE.InstancedMesh(original, source.material, 1), b = new THREE.InstancedMesh(original, source.material, 1);
    gltf.scene.children[0].add(a, b); mergeCityInstances(gltf.scene);
    assert.equal(a.parent, gltf.scene.children[0]); assert.equal(b.parent, gltf.scene.children[0]);
  } finally { lod.dispose(); far.dispose(); }
  assert.equal(hasDeclaredLodOwner(source), false);
});
