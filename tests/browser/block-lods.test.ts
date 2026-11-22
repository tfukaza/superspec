import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { BlockLods } from "../../adapters/three/src/blockLods.ts";
import { CityVisibility, mergeCityInstances, type CityInstanceGroups } from "../../adapters/three/src/cityVisibility.ts";
import { clusterCityGeometry } from "../../adapters/three/src/cityClusters.ts";
import { CityFarHardware } from "../../adapters/three/src/cityFarHardware.ts";
import { CityTextureResidency } from "../../adapters/three/src/cityTextureResidency.ts";

function model() {
  const scene = new THREE.Group(), geometry = new THREE.BoxGeometry(), material = new THREE.MeshStandardMaterial();
  const source = new THREE.InstancedMesh(geometry, material, 1), neighbor = source.clone();
  neighbor.setMatrixAt(0, new THREE.Matrix4().makeTranslation(4, 0, 0));
  source.castShadow = neighbor.castShadow = true;
  const detailed = new THREE.Group().add(source), proxy = new THREE.Mesh(geometry.clone(), material);
  scene.add(detailed, neighbor);
  const block = { id: "North", sources: [0], proxy: 2, distance: 45, bounds: { min: [-1,-1,-1], max: [1,1,1] } };
  scene.userData.worldSpecBlockLods = { version: 1, blocks: [block] };
  const parser = { json: { scenes: [{nodes:[0,1]}], nodes: [{mesh:0}, {mesh:0}, {mesh:1}],
    meshes: [{primitives:[{material:0}]}, {primitives:[{material:1}]}], materials: [{}, {}] },
    associations: new Map<THREE.Object3D, {nodes:number}>([[detailed,{nodes:0}], [neighbor,{nodes:1}]]),
    getDependency: async () => proxy };
  return { block, scene, detailed, source, neighbor, proxy, parser, gltf: { scene, scenes:[scene], parser } as unknown as GLTF };
}

test("a block proxy selected near the camera can promote its initially hidden material", async () => {
  const { scene, proxy, gltf, block } = model();
  block.distance = 1;
  const low = new THREE.Texture(), high = new THREE.Texture();
  proxy.material = new THREE.MeshStandardMaterial({ map: low });
  const blocks = (await BlockLods.load(gltf))!;
  const residency = new CityTextureResidency(scene, m => m === proxy.material ? [{ slot: "map", low, loadHigh: async () => high }] : []);
  const camera = new THREE.PerspectiveCamera(70, 1, .1, 100);
  try {
    assert.equal(proxy.visible, false);
    camera.position.z = 6;
    blocks.select(camera, 1080);
    assert.equal(proxy.visible, true);
    residency.frame(camera, 1080);
    await residency.settle();
    assert.equal(proxy.material.map, high);
    blocks.restore();
    residency.frame(camera, 1080);
    await residency.settle();
    assert.equal(proxy.material.map, low);
  } finally {
    residency.dispose(); blocks.dispose();
    proxy.material.dispose(); low.dispose();
  }
});

test("far block replaces whole source roots while keeping nearby detail, picks and canonical shadows", async () => {
  const { scene, source, neighbor, proxy, gltf } = model();
  const blocks = (await BlockLods.load(gltf))!;
  assert.ok(blocks);
  mergeCityInstances(scene, blocks.groupFor);
  assert.ok(scene.getObjectById(source.id) && scene.getObjectById(neighbor.id), "batching must retain block membership");
  proxy.castShadow = true;
  blocks.keepCanonicalShadows();
  assert.equal(proxy.castShadow, false);
  const camera = new THREE.PerspectiveCamera(70, 16 / 9, .1, 1000);
  camera.position.z = 50;
  assert.equal(blocks.select(camera, 1080), true);
  assert.equal(blocks.groupFor(source)!.visible, false);
  assert.equal(blocks.stats().active, true);
  assert.equal(blocks.stats().distance, 49);
  assert.equal(proxy.visible, true);
  assert.equal(neighbor.visible, true);
  scene.updateWorldMatrix(false, true);
  const hits = new THREE.Raycaster(new THREE.Vector3(0,0,8), new THREE.Vector3(0,0,-1)).intersectObject(scene);
  assert.ok(hits.some(hit => hit.object === source));
  assert.ok(hits.every(hit => hit.object !== proxy));
  blocks.restore();
  assert.ok(blocks.groupFor(source)!.visible && source.castShadow && !proxy.visible);
  assert.equal(blocks.select(camera,1080), false, "shadow restoration leaves LOD history intact");
  assert.ok(proxy.visible);
  camera.position.z = 47;
  blocks.select(camera,1080);
  assert.ok(proxy.visible, "receding and approaching thresholds are separated");
  camera.position.z = 45;
  blocks.select(camera,1080);
  assert.equal(proxy.visible, false);
  camera.position.z = 55;
  assert.equal(blocks.select(camera,2160), false, "higher pixel density retains the original");
  assert.equal(blocks.select(camera,1080), true);
  blocks.setEnabled(false);
  assert.equal(blocks.select(camera,1080), true);
  assert.equal(proxy.visible, false);
  assert.deepEqual(blocks.dispose(), [proxy]);
  assert.equal(proxy.parent,null);
  assert.equal(source.parent?.parent,scene);
});

test("an unavailable or malformed block keeps every source attached with no replacement", async t => {
  const warning = t.mock.method(console,"warn",() => {});
  const fixture = model();
  fixture.block.proxy = 1;
  const get = t.mock.method(fixture.parser,"getDependency");
  assert.equal(await BlockLods.load(fixture.gltf),undefined);
  assert.equal(get.mock.callCount(),0, "never load an active source as a proxy");
  fixture.block.proxy = 2;
  fixture.source.visible = false;
  assert.equal(await BlockLods.load(fixture.gltf),undefined);
  fixture.source.visible = true;
  fixture.parser.json.materials[1] = {pbrMetallicRoughness:{baseColorTexture:{index:0}}};
  assert.equal(await BlockLods.load(fixture.gltf),undefined, "GLTFLoader can return a material when its texture failed");
  fixture.parser.getDependency = async () => { throw Error("decode failed"); };
  assert.equal(await BlockLods.load(fixture.gltf),undefined);
  assert.equal(warning.mock.callCount(),4);
  assert.deepEqual(fixture.scene.children,[fixture.detailed,fixture.neighbor]);
});

test("a replaced facade cannot occlude a neighboring object through the former silhouette", () => {
  const scene = new THREE.Group(), block = new THREE.Group();
  const wall = new THREE.Mesh(new THREE.PlaneGeometry(10,10),new THREE.MeshStandardMaterial());
  wall.position.z = -4;
  block.add(wall);
  const target = new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshStandardMaterial());
  target.position.z = -9;
  scene.add(block, target);
  const visibility = new CityVisibility(scene, true, new Map(), {groupFor: mesh => mesh === wall ? block : undefined});
  const camera = new THREE.PerspectiveCamera(70,16/9,.1,100);
  visibility.frame(camera);
  assert.equal(target.visible,false);
  block.visible = false;
  visibility.setHiddenGroups(new Set([block]));
  visibility.frame(camera);
  assert.ok(target.visible, "stationary camera must invalidate its software depth when the block changes");
  block.visible = true;
  visibility.setHiddenGroups(new Set());
  visibility.frame(camera);
  assert.equal(target.visible,false);
  visibility.dispose();
});

test("a shared hardware batch keeps its neighbor through clustering, distant LOD and shadow restoration", async () => {
  const { scene, source, neighbor, gltf } = model();
  source.geometry = neighbor.geometry = new THREE.BoxGeometry(1, 1, 1, 35, 35, 35);
  source.geometry.userData.cityLodErrorMeters = 0;
  source.material.userData.worldSpecSurface = { family: "Building atlases", surface: "Mixed surfaces with metal", recipe: "weathered-steel" };
  const blocks = (await BlockLods.load(gltf))!;
  const groups: CityInstanceGroups = new WeakMap();
  mergeCityInstances(scene, blocks.groupFor, groups);
  const combined = scene.children.find(o => o instanceof THREE.InstancedMesh) as THREE.InstancedMesh;
  assert.equal(combined.count, 2);
  assert.equal(groups.get(combined)![1], undefined);
  const hardware = new CityFarHardware();
  const lods = new Map([[source.geometry, [source.geometry, source.geometry]]]);
  const pairs = clusterCityGeometry(scene, lods, hardware, groups);
  assert.equal(combined.parent, null, "fixture must exercise real atlas clustering");
  const visibility = new CityVisibility(scene, true, lods, { groupFor: blocks.groupFor,
    instanceGroups: groups, farHardware: hardware, coalesceClusters: true, clusterPairs: pairs, wideClusters: true });
  const camera = new THREE.PerspectiveCamera(70, 16 / 9, .1, 200);
  const draws = () => {
    const xs: number[] = [], transform = new THREE.Matrix4();
    scene.traverseVisible(o => {
      if (o instanceof THREE.InstancedMesh) for (let i = 0; i < o.count; i++) {
        o.getMatrixAt(i, transform); xs.push(transform.elements[12]);
      }
    });
    return xs;
  };
  const frame = () => {
    blocks.restore();
    if (blocks.select(camera, 1080)) visibility.setHiddenGroups(blocks.hidden);
    visibility.frame(camera);
  };
  camera.position.z = 60;
  blocks.setEnabled(false);
  frame();
  assert.deepEqual(draws(), [0, 4], "ordinary distant hardware uses both placements in one draw");
  blocks.setEnabled(true);
  frame();
  assert.deepEqual(draws(), [4], "only the proxy-owned placement is removed");
  blocks.restore(); visibility.restore();
  const canonical = draws();
  assert.ok(canonical.includes(0) && canonical.includes(4), "original cluster casters include both placements");
  frame();
  assert.deepEqual(draws(), [4]);
  const plan = new THREE.OrthographicCamera(-5, 5, 5, -5, .1, 200);
  plan.position.z = 100;
  if (blocks.select(plan, 1080)) visibility.setHiddenGroups(blocks.hidden);
  visibility.frame(plan);
  assert.equal(blocks.stats().active, false);
  assert.ok(draws().includes(0) && draws().includes(4), "orthographic views retain the complete batch");
  camera.position.z = 20;
  frame();
  assert.ok(draws().includes(0) && draws().includes(4));
  visibility.dispose();
  blocks.dispose();
});
