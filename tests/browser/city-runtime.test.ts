import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { WebGLObjects } from "three/src/renderers/webgl/WebGLObjects.js";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { loadCityLods, tagCityLodPrimitives } from "../../adapters/three/src/cityLods.ts";
import type { CityLodLevels } from "../../adapters/three/src/cityLods.ts";
import { CITY_LOD_MERGE_TRIANGLES, CityVisibility, mergeCityInstances, selectCityLod, omitCityDetail } from "../../adapters/three/src/cityVisibility.ts";
import { clusterCityGeometry } from "../../adapters/three/src/cityClusters.ts";
import { CityFarHardware } from "../../adapters/three/src/cityFarHardware.ts";

function sidecar(primitives: unknown[]) {
  const manifest = new TextEncoder().encode(JSON.stringify({ primitives }));
  const bytes = new Uint8Array(12 + Math.ceil(manifest.length / 4) * 4);
  bytes.set([83, 76, 79, 68, 1, 0, 0, 0]);
  new DataView(bytes.buffer).setUint32(8, manifest.length, true);
  bytes.set(manifest, 12);
  return bytes;
}

test("far hardware shares a camera draw across cells and restores each original caster", () => {
  const root = new THREE.Group(), material = new THREE.MeshStandardMaterial();
  material.userData.worldSpecSurface = { family: "Building atlases", surface: "Mixed surfaces with metal", recipe: "weathered-steel" };
  const geometry = new THREE.BoxGeometry(), reduced = geometry.clone();
  reduced.userData.cityLodErrorMeters = .4;
  const source = new THREE.InstancedMesh(geometry, material, 3);
  for (const [i, z] of [-5, -50, -100].entries()) source.setMatrixAt(i, new THREE.Matrix4().makeTranslation(0, 0, z));
  source.castShadow = true;
  const cells = [source.clone(), source.clone()];
  root.add(source, ...cells);
  const far = new CityFarHardware();
  far.register(source, cells, [reduced, reduced]);
  source.removeFromParent();
  const visibility = new CityVisibility(root, true, new Map([[geometry, [reduced, reduced]]]), { farHardware: far });
  const camera = new THREE.PerspectiveCamera(70, 1.78, .1, 300);
  visibility.frame(camera, 1080);
  const visible = () => root.children.filter(child => child.visible) as THREE.InstancedMesh[];
  const farDraw = visible().find(mesh => mesh.geometry === reduced)!;
  assert.ok(farDraw && !cells.includes(farDraw));
  assert.equal(farDraw.material, material);
  assert.deepEqual(Array.from(farDraw.instanceMatrix.array).filter((_, i) => i % 16 === 14).slice(0, farDraw.count), [-50, -100]);
  assert.equal(farDraw.castShadow, false);
  assert.equal(visible().filter(mesh => mesh.geometry === geometry).length, 2);
  assert.ok(visible().filter(mesh => mesh.geometry === geometry).every(mesh => mesh.count === 1));
  visibility.restore();
  assert.deepEqual(visible(), cells);
  assert.ok(visible().every(mesh => mesh.count === 3 && mesh.castShadow));
  visibility.frame(camera, 1080);
  assert.ok(farDraw.visible);
  camera.position.z = -48;
  visibility.frame(camera, 1080);
  assert.equal(farDraw.count, 1);
  assert.equal(farDraw.instanceMatrix.array[14], -100);
  visibility.dispose();
  assert.deepEqual(root.children, cells);
});

test("LOD loader resolves beside the GLB and binds grouped instanced primitives without copying attributes", async t => {
  const root = new THREE.Group(), batch = new THREE.Group(), geometry = new THREE.BoxGeometry();
  batch.add(new THREE.Mesh(new THREE.PlaneGeometry()), new THREE.InstancedMesh(geometry, new THREE.MeshBasicMaterial(), 1));
  root.add(batch);
  root.userData.cityLod = "city-data/lod-cafe.bin";
  const gltf = { scene: root, parser: { associations: new Map([[batch, { nodes: 0 }]]),
    json: { nodes: [{ mesh: 0 }], meshes: [{ primitives: [{}, {}] }] } } };
  tagCityLodPrimitives(gltf as unknown as GLTF);
  const nearCount = geometry.index!.count;
  const bytes = sidecar([{ mesh: 0, primitive: 1, vertexCount: geometry.attributes.position.count, nearCount,
    levels: [nearCount, 0].map(count => ({ offset: 0, bytes: 0, count, componentBytes: 2, errorMeters: 0.02 })) }]);
  const fetcher = t.mock.method(globalThis, "fetch", async (url: unknown) => {
    assert.equal(String(url), "https://viewer.example/artifacts/city-data/lod-cafe.bin");
    return new Response(bytes);
  });
  const loaded = await loadCityLods(root, "https://viewer.example/artifacts/city.glb");
  const [mid, far] = loaded.get(geometry)!;
  assert.equal(fetcher.mock.callCount(), 1);
  assert.equal(mid, geometry);
  assert.equal(far.index!.count, 0);
  assert.equal(far.attributes.position, geometry.attributes.position);
  assert.equal(far.userData.cityLodErrorMeters, 0.02);
  assert.equal(geometry.index!.count, nearCount);
});

test("missing, unsafe and malformed LOD assets retain the original scene", async t => {
  const root = new THREE.Group();
  const fetcher = t.mock.method(globalThis, "fetch", async () => new Response(new Uint8Array([83, 76])));
  t.mock.method(console, "warn", () => {});
  assert.equal((await loadCityLods(root, "https://viewer.example/city.glb")).size, 0);
  root.userData.cityLod = "../city-data/lod-cafe.bin";
  assert.equal((await loadCityLods(root, "https://viewer.example/city.glb")).size, 0);
  assert.equal(fetcher.mock.callCount(), 0);
  root.userData.cityLod = "city-data/lod-cafe.bin";
  assert.equal((await loadCityLods(root, "https://viewer.example/city.glb")).size, 0);
  assert.equal(fetcher.mock.callCount(), 1);
});

test("screen error preserves close detail, increases detail with resolution and zoom, and uses hysteresis", () => {
  const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 200);
  const mid = new THREE.BoxGeometry(), far = new THREE.BoxGeometry();
  mid.userData.cityLodErrorMeters = 0.035;
  far.userData.cityLodErrorMeters = 0.11;
  const levels: CityLodLevels = [mid, far];
  assert.equal(selectCityLod(levels, 5, camera, 1080, 0), 0);
  assert.equal(selectCityLod(levels, 40, camera, 1080, 0), 1);
  assert.equal(selectCityLod(levels, 80, camera, 1080, 0), 2);
  assert.equal(selectCityLod(levels, 80, camera, 2160, 0), 1);
  camera.zoom = 2;
  camera.updateProjectionMatrix();
  assert.equal(selectCityLod(levels, 80, camera, 1080, 0), 1);
  camera.zoom = 1;
  camera.updateProjectionMatrix();
  assert.equal(selectCityLod(levels, 63, camera, 1080, 0), 1);
  assert.equal(selectCityLod(levels, 63, camera, 1080, 2), 2);
  assert.equal(selectCityLod(levels, 55, camera, 1080, 2), 1);
});

test("both occlusion grid sizes preserve doorways and cutouts and restore shadow casters", () => {
  for (const occlusionSize of [[80, 48], [160, 96]] as const) for (const alphaTest of [0, 0.5]) {
    const root = new THREE.Group();
    const wall = new THREE.Mesh(new THREE.PlaneGeometry(6, 12), new THREE.MeshBasicMaterial({ alphaTest }));
    wall.position.set(-5, 0, -5);
    root.add(wall);
    const target = (x: number) => {
      const mesh = new THREE.InstancedMesh(new THREE.BoxGeometry(0.3, 0.3, 0.3), new THREE.MeshBasicMaterial(), 1);
      mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(x, 1.5, -9));
      root.add(mesh);
      return mesh;
    };
    const behindWall = target(-7), doorway = target(0);
    const camera = new THREE.PerspectiveCamera(78, 1.6, 0.05, 100);
    const visibility = new CityVisibility(root, true, new Map(), { occlusionSize });
    visibility.frame(camera);
    assert.equal(behindWall.visible, alphaTest > 0);
    assert.equal(doorway.visible, true);
    visibility.restore();
    assert.equal(behindWall.visible, true);
    visibility.frame(camera);
    assert.equal(behindWall.visible, alphaTest > 0);
    visibility.dispose();
    assert.equal(behindWall.visible, true);
  }
});

test("a wall clipped by the GPU near plane cannot hide a visible street-level receiver", () => {
  for (const coordinateSystem of [THREE.WebGLCoordinateSystem, THREE.WebGPUCoordinateSystem] as const) {
    const root = new THREE.Group();
    const wall = new THREE.Mesh(new THREE.PlaneGeometry(8, 8), new THREE.MeshBasicMaterial());
    wall.position.z = -.075;
    const receiver = new THREE.InstancedMesh(new THREE.BoxGeometry(), new THREE.MeshBasicMaterial(), 1);
    receiver.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -10));
    root.add(wall, receiver);
    const camera = new THREE.PerspectiveCamera(70, 1.6, .1, 100);
    camera.coordinateSystem = coordinateSystem;
    camera.updateProjectionMatrix();
    const visibility = new CityVisibility(root);
    visibility.frame(camera);
    assert.equal(receiver.visible, true);
    // Move back enough to see the near wall. It should now hide the receiver.
    camera.position.z = .1;
    visibility.frame(camera);
    assert.equal(receiver.visible, false);
    visibility.dispose();
  }
});

test("per-placement selection retains original geometry and picking and disposes selection buffers", () => {
  const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, 0.1, 100);
  const geometry = new THREE.BoxGeometry(), material = new THREE.MeshBasicMaterial();
  const batch = new THREE.InstancedMesh(geometry, material, 2);
  batch.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -8));
  batch.setMatrixAt(1, new THREE.Matrix4().makeTranslation(30, 0, -8));
  batch.castShadow = true;
  root.add(batch);
  const visibility = new CityVisibility(root);
  visibility.frame(camera);
  const subset = root.children.find(object => object !== batch) as THREE.InstancedMesh;
  assert.equal(batch.visible, false);
  assert.equal(subset.count, 1);
  assert.equal(subset.castShadow, false);
  assert.equal(subset.geometry, geometry);
  assert.equal(subset.material, material);
  const ray = new THREE.Raycaster(new THREE.Vector3(), new THREE.Vector3(0, 0, -1));
  assert.ok(ray.intersectObject(root).every(hit => hit.object === batch));
  visibility.restore();
  assert.equal(batch.visible, true);
  assert.equal(subset.visible, false);
  visibility.dispose();
  assert.deepEqual(root.children, [batch]);
});

test("shared geometry selects occluder faces per placement scale and static or instance budget in either scene order", () => {
  const run = (reverse: boolean, largeScale: number, offscreenScale: number, instancedOffscreen: boolean) => {
    const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, .1, 100);
    const panel = new THREE.PlaneGeometry(1, 1), material = new THREE.MeshBasicMaterial();
    const on = new THREE.Mesh(panel, material);
    on.position.z = -4;
    on.scale.setScalar(largeScale);
    const off = instancedOffscreen ? new THREE.InstancedMesh(panel, material, 1) : new THREE.Mesh(panel, material);
    off.position.set(40, 0, -4);
    off.scale.setScalar(offscreenScale);
    root.add(...(reverse ? [on, off] : [off, on]));
    const receiver = new THREE.Mesh(new THREE.BoxGeometry(.2, .2, .2), material);
    receiver.position.z = -8;
    root.add(receiver);
    const visibility = new CityVisibility(root);
    visibility.frame(camera);
    assert.equal(receiver.visible, false);
    visibility.restore();
    assert.equal(receiver.visible, true);
    visibility.dispose();
  };
  for (const reverse of [false, true]) {
    run(reverse, 8, .5, false);
    run(reverse, 2.4, 2.4, true);
  }
});

test("rebuilding published visibility after a parent edit refreshes color bounds, LOD and shadow selection", () => {
  const root = new THREE.Group(), parent = new THREE.Group();
  const camera = new THREE.PerspectiveCamera(70, 1.78, .1, 300);
  const source = new THREE.BoxGeometry(), far = new THREE.BoxGeometry();
  const lods = new Map([[source, [far]]]);
  const mesh = new THREE.InstancedMesh(source, new THREE.MeshBasicMaterial(), 1);
  mesh.castShadow = true;
  parent.add(mesh);
  root.add(parent);
  parent.position.set(100, 0, -80);
  const before = new CityVisibility(root, true, lods);
  before.frame(camera);
  assert.equal(mesh.visible, false);
  before.dispose();
  assert.equal(mesh.matrixAutoUpdate, true);
  parent.position.x = 0;
  const after = new CityVisibility(root, true, lods);
  after.frame(camera);
  const drawn = () => parent.children.filter(object => object.visible) as THREE.Mesh[];
  assert.deepEqual(drawn().map(object => object.geometry), [far]);
  after.shadowFrame(camera);
  assert.deepEqual(drawn(), [mesh]);
  after.dispose();
});

test("placement cells share current occlusion across materials and retain instance order and shadows", () => {
  const root = new THREE.Group(), parent = new THREE.Group();
  parent.position.set(.25, .5, -.3);
  parent.rotation.z = .025;
  const wall = new THREE.Mesh(new THREE.PlaneGeometry(3, 8), new THREE.MeshBasicMaterial());
  wall.position.set(2, 0, -4);
  wall.layers.set(1);
  root.add(parent, wall);
  const geometry = new THREE.BoxGeometry(.2, .2, .2);
  const batches = [0.3, 0.8].map(y => {
    const batch = new THREE.InstancedMesh(geometry, new THREE.MeshBasicMaterial({ alphaTest: y }), 4);
    for (const [i, x] of [2, -1, 3, -2].entries()) batch.setMatrixAt(i, new THREE.Matrix4().makeTranslation(x, y, -6));
    batch.castShadow = true;
    parent.add(batch);
    return batch;
  });
  const visibility = new CityVisibility(root);
  const camera = new THREE.PerspectiveCamera(70, 1.6, .1, 100);
  const visible = () => parent.children.filter(object => object.visible) as THREE.InstancedMesh[];
  const members = (mesh: THREE.InstancedMesh) =>
    Array.from({ length: mesh.count }, (_, i) => mesh.instanceMatrix.array[16 * i + 12]);

  for (const occluderEnabled of [true, false, true]) {
    if (occluderEnabled) camera.layers.enable(1); else camera.layers.disable(1);
    visibility.frame(camera);
    const expected = occluderEnabled ? [-1, -2] : [2, -1, 3, -2];
    assert.deepEqual(visible().map(members), [expected, expected]);
    if (!occluderEnabled) assert.deepEqual(visible(), batches);
    visibility.shadowFrame(camera, camera);
    assert.deepEqual(visible(), batches);
    assert.ok(visible().every(mesh => mesh.castShadow && mesh.count === 4));
  }
  visibility.dispose();
  assert.deepEqual(parent.children, batches);
});

test("placement cells defer near and far plane crossings to the original leaf queries", () => {
  for (const y of [0, 2 ** 24]) {
    const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(70, 1.6, .1, 6);
    const wall = new THREE.Mesh(new THREE.PlaneGeometry(4, 4), new THREE.MeshBasicMaterial());
    wall.position.z = -.75;
    const geometry = new THREE.BoxGeometry(.2, .2, .2);
    const batch = new THREE.InstancedMesh(geometry, new THREE.MeshBasicMaterial({ alphaTest: .5 }), 4);
    for (const [i, z] of [-.14, -2, -4, -5.94].entries())
      batch.setMatrixAt(i, new THREE.Matrix4().makeTranslation(.04, .04, z));
    root.add(wall, batch);
    root.position.y = camera.position.y = y;
    const visibility = new CityVisibility(root);
    visibility.frame(camera);
    const subset = root.children.find(mesh => mesh instanceof THREE.InstancedMesh && mesh.visible) as THREE.InstancedMesh;
    assert.ok(subset);
    assert.equal(subset.count, 2);
    assert.ok(Math.abs(subset.instanceMatrix.array[14] + .14) < 1e-6);
    assert.ok(Math.abs(subset.instanceMatrix.array[30] + 5.94) < 1e-6);
    visibility.dispose();
  }
});

test("the placement cell sample follows current camera layers", () => {
  const root = new THREE.Group();
  const wall = new THREE.Mesh(new THREE.PlaneGeometry(18, 13), new THREE.MeshBasicMaterial());
  wall.position.z = -10;
  wall.layers.set(1);
  root.add(wall);
  const positions = [-22, -10, -3, 3, 10, 22];
  const geometry = new THREE.BoxGeometry(.25, .25, .25);
  const sources = Array.from({ length: 5 }, (_, batch) => {
    const mesh = new THREE.InstancedMesh(geometry, new THREE.MeshBasicMaterial({ alphaTest: .25 + batch / 20 }), positions.length);
    for (const [i, x] of positions.entries()) mesh.setMatrixAt(i, new THREE.Matrix4().makeTranslation(x, 1 + batch / 10, -20));
    mesh.castShadow = true;
    root.add(mesh);
    return mesh;
  });
  const camera = new THREE.PerspectiveCamera(70, 16 / 9, .1, 100);
  const visibility = new CityVisibility(root);
  for (const closed of [false, true, false, true]) {
    if (closed) camera.layers.enable(1); else camera.layers.disable(1);
    visibility.frame(camera);
    assert.equal((visibility as unknown as { usePlacementCells: boolean }).usePlacementCells, closed);
    for (const source of sources) {
      const selected = root.children.find((object): object is THREE.InstancedMesh =>
        object instanceof THREE.InstancedMesh && object.material === source.material && object.visible);
      assert.ok(selected);
      const shown = Array.from({ length: selected.count }, (_, i) => selected.instanceMatrix.array[i * 16 + 12]);
      assert.deepEqual(shown, closed ? [-22, 22] : positions);
    }
    visibility.shadowFrame(camera, camera);
    assert.ok(sources.every(source => source.visible && source.count === positions.length));
  }
  visibility.dispose();
  assert.deepEqual(root.children, [wall, ...sources]);
});

test("large occlusion queries match every covered cell across tile and viewport edges", () => {
  const width = 19, height = 15;
  const camera = new THREE.PerspectiveCamera(90, width / height, .1, 100);
  const root = new THREE.Group(), material = new THREE.MeshBasicMaterial();
  const leftWall = new THREE.Mesh(new THREE.PlaneGeometry(17, 24), material);
  leftWall.position.set(-5, 0, -10);
  const rightWall = new THREE.Mesh(new THREE.PlaneGeometry(22, 24), material);
  rightWall.position.set(10, 0, -10);
  rightWall.layers.set(1);
  root.add(leftWall, rightWall);
  const visibility = new CityVisibility(root, true, new Map(), { occlusionSize: [width, height] });
  const point = (x: number, y: number) => new THREE.Vector3(
    (2 * x / width - 1) * 20 * camera.aspect, (2 * y / height - 1) * 20, -20);
  const rectangle = (x: number, y: number, endX: number, endY: number) =>
    new THREE.Box3(point(x + .25, y + .25), point(endX - .25, endY - .25));
  for (const includeRight of [false, true, false]) {
    if (includeRight) camera.layers.enable(1); else camera.layers.disable(1);
    visibility.frame(camera);
    assert.equal(visibility.occludesBounds(rectangle(13, 2, 19, 14)), includeRight);
    const covered = Array.from({ length: height }, (_, y) => Array.from({ length: width }, (_, x) =>
      visibility.occludesBounds(new THREE.Box3(point(x + .5, y + .5), point(x + .5, y + .5)))));
    for (let left = 0; left < width; left++) for (let bottom = 0; bottom < height; bottom++) {
      for (const size of [5, 8, 13, 20]) {
        const right = Math.min(width, left + size), top = Math.min(height, bottom + size);
        const expected = covered.slice(bottom, top).every(row => row.slice(left, right).every(Boolean));
        assert.equal(visibility.occludesBounds(rectangle(left, bottom, right, top)), expected,
          `left=${left} bottom=${bottom} right=${right} top=${top} includeRight=${includeRight}`);
      }
    }
  }
  visibility.dispose();
});

test("clustering retains triangle streams, attributes and world error bounds at every LOD", () => {
  const source = new THREE.BufferGeometry();
  source.setAttribute("position", new THREE.Float32BufferAttribute([-5, 0, 0, -4, 0, 0, -4, 1, 0, 4, 0, 0, 5, 0, 0, 5, 1, 0], 3));
  source.setIndex(Array.from({ length: 12002 }, (_, i) => i % 2 ? [3, 4, 5] : [0, 1, 2]).flat());
  const mid = source.clone(), far = source.clone(), distant = source.clone();
  mid.setIndex([3, 4, 5, 0, 1, 2]);
  far.setIndex([0, 1, 2]);
  distant.setIndex([3, 4, 5]);
  mid.userData = { cityLodErrorMeters: 0.035 };
  far.userData = { cityLodErrorMeters: 0.11 };
  distant.userData = { cityLodErrorMeters: 0.35 };
  const lods = new Map<THREE.BufferGeometry, CityLodLevels>([[source, [mid, far, distant]]]);
  const root = new THREE.Group(), material = new THREE.MeshStandardMaterial();
  for (const x of [-20, 20]) {
    const mesh = new THREE.InstancedMesh(source, material, 1);
    mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(x, 0, 0));
    root.add(mesh);
  }
  mergeCityInstances(root);
  clusterCityGeometry(root, lods);
  const parts = root.children as THREE.InstancedMesh[];
  assert.equal(parts.length, 2);
  for (const part of parts) {
    assert.equal(part.count, 2);
    assert.equal(part.geometry.attributes.position, source.attributes.position);
    assert.equal(part.geometry.userData.lodDistanceBounds, undefined);
    assert.equal(part.geometry.boundingBox!.max.x - part.geometry.boundingBox!.min.x, 1);
    assert.equal(part.geometry.userData.cityLodComponentDiameter, Math.SQRT2);
    assert.ok(Math.abs(part.geometry.userData.cityLodComponentArea - Math.PI / 2) < 1e-12,
      "area counts the component once despite repeated triangles and UV/normal seams");
    assert.equal(lods.get(part.geometry)![1].userData.cityLodErrorMeters, 0.11);
  }
  const triangles = (indices: number[]) => Array.from({ length: indices.length / 3 }, (_, i) => indices.slice(i * 3, i * 3 + 3).join("/")).sort();
  for (const [level, original] of [source, mid, far, distant].entries()) {
    const actual = parts.flatMap(part => Array.from((level ? lods.get(part.geometry)![level - 1] : part.geometry).index!.array));
    assert.deepEqual(triangles(actual), triangles(Array.from(original.index!.array)));
  }
});

test("quantized clustering welds normalized seams and includes vertices introduced by reduced levels", () => {
  const position = new THREE.InterleavedBufferAttribute(new THREE.InterleavedBuffer(new Int16Array([
    1, -32768, 0, 0,
    1, -32768, 100, 0,
    1, -32768, 0, 100,
    1, -32767, 0, 100,
    1, -29000, 1200, 100,
    1, -30000, 100, 150,
    1, 12000, 0, 0,
    1, 14000, 100, 0,
    1, 15000, 0, 100,
  ]), 4), 3, 1, true);
  const original = new THREE.BufferGeometry().setAttribute("position", position)
    .setIndex(Array.from({ length: 12002 }, (_, i) => i % 2 ? [6, 7, 8] : [0, 1, 2]).flat());
  const reduced = new THREE.BufferGeometry().setAttribute("position", position).setIndex([3, 4, 5, 6, 7, 8]);
  const alias = new THREE.BufferGeometry().setAttribute("position", position).setIndex(reduced.index);
  reduced.userData.cityLodErrorMeters = .1;
  alias.userData.cityLodErrorMeters = .2;
  const lods = new Map<THREE.BufferGeometry, CityLodLevels>([[original, [original, reduced, alias]]]);
  const root = new THREE.Group();
  root.add(new THREE.Mesh(original, new THREE.MeshBasicMaterial()));

  clusterCityGeometry(root, lods);
  assert.equal(root.children.length, 2);
  const left = (root.children[0] as THREE.Mesh).geometry;
  const [near, mid, far] = lods.get(left)!;
  const expected = new THREE.Box3(new THREE.Vector3(-1, 0, 0), new THREE.Vector3(-29000 / 32767, 1200 / 32767, 150 / 32767));
  const diameter = expected.getSize(new THREE.Vector3()).length();
  assert.equal(left.userData.cityLodIndependent, true);
  assert.equal(left.userData.cityLodComponentDiameter, diameter);
  assert.equal(left.userData.cityLodComponentArea, Math.PI / 4 * diameter * diameter);
  assert.equal(near, left);
  assert.notEqual(mid, far);
  assert.deepEqual([mid.userData.cityLodErrorMeters, far.userData.cityLodErrorMeters], [.1, .2]);
  for (const part of [left, mid, far]) {
    assert.equal(part.attributes.position, position);
    assert.deepEqual(part.boundingBox, expected);
  }
  assert.deepEqual(Array.from(mid.index!.array), [3, 4, 5]);
  assert.deepEqual(Array.from(far.index!.array), [3, 4, 5]);
});

test("legacy LOD distances retain return bands and full original shadow geometry", () => {
  const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, 0.1, 200);
  const original = new THREE.BoxGeometry(), mid = original.clone(), far = original.clone();
  mid.setIndex([0, 1, 2, 2, 3, 0]);
  far.setIndex([0, 1, 2]);
  const mesh = new THREE.InstancedMesh(original, new THREE.MeshBasicMaterial(), 1);
  mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -10));
  mesh.castShadow = true;
  root.add(mesh);
  const visibility = new CityVisibility(root, true, new Map([[original, [mid, far]]]));
  for (const [z, wanted] of [[0, original], [18, mid], [38, mid], [45, far], [38, far], [35, mid], [13, mid], [10, original]] as const) {
    camera.position.z = z;
    visibility.frame(camera);
    assert.deepEqual(root.children.filter(object => object.visible).map(object => (object as THREE.Mesh).geometry), [wanted]);
    visibility.restore();
    assert.deepEqual(root.children.filter(object => object.visible), [mesh]);
    assert.equal(mesh.geometry, original);
    assert.equal(mesh.castShadow, true);
  }
});

test("turning away resets projected LOD and preserves legacy distance hysteresis", () => {
  for (const projected of [false, true]) {
    const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, .1, 200);
    const original = new THREE.BoxGeometry(), mid = original.clone(), far = original.clone();
    mid.userData = {cityLodErrorMeters: .02, cityLodIndependent: true};
    far.userData = {};
    // Exercise a change between partially declared legacy levels and a complete error record.
    if (!projected) far.userData.cityLodErrorMeters = .083;
    const mesh = new THREE.InstancedMesh(original, new THREE.MeshBasicMaterial(), 1);
    mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -55));
    root.add(mesh);
    const visibility = new CityVisibility(root, true, new Map([[original, [mid, far]]]));
    const visibleGeometry = () => root.children.filter(o => o.visible).map(o => (o as THREE.Mesh).geometry);
    visibility.frame(camera);
    assert.deepEqual(visibleGeometry(), [far]);
    if (projected) far.userData.cityLodErrorMeters = .083;
    else delete far.userData.cityLodErrorMeters;
    camera.lookAt(0, 0, 1);
    visibility.frame(camera);
    camera.position.z = -7;
    camera.lookAt(0, 0, -55);
    visibility.frame(camera);
    // At 47.5 m both policies can retain the far level if they entered it earlier.
    assert.deepEqual(visibleGeometry(), [projected ? mid : far]);
    visibility.dispose();
  }
});

test("near-clipped walls cover only their visible front-facing footprint", () => {
  const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(75, 1.5, 0.2, 100);
  const positions = new THREE.Float32BufferAttribute([
    -1, -4, 1, -1, -4, -7, -1, 4, -7,
    -1, -4, 1, -1, 4, -7, -1, 4, 1,
  ], 3);
  const wall = new THREE.Mesh(new THREE.BufferGeometry().setAttribute("position", positions), new THREE.MeshBasicMaterial());
  root.add(wall);
  const targets = [];
  for (const y of [-1.5, 0, 1.5]) for (const x of [-9, -7, -4, -0.6, 0.6, 4]) {
    const mesh = new THREE.InstancedMesh(new THREE.BoxGeometry(0.12, 0.12, 0.12), new THREE.MeshBasicMaterial(), 1);
    mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(x, y, -13));
    root.add(mesh);
    targets.push({ x, y, mesh });
  }
  const visibility = new CityVisibility(root);
  visibility.frame(camera);
  assert.ok(targets.filter(target => !target.mesh.visible).length >= 6);
  // Independently verify all corners of every hidden box lie behind the actual wall triangles.
  const ray = new THREE.Raycaster();
  for (const { x, y, mesh } of targets) {
    if (x > -1) assert.equal(mesh.visible, true);
    if (mesh.visible) continue;
    for (let corner = 0; corner < 8; corner++) {
      const point = new THREE.Vector3(x + (corner & 1 ? 0.06 : -0.06), y + (corner & 2 ? 0.06 : -0.06), -13 + (corner & 4 ? 0.06 : -0.06));
      ray.set(camera.position, point.clone().sub(camera.position).normalize());
      ray.far = camera.position.distanceTo(point);
      assert.ok(ray.intersectObject(wall).length > 0);
    }
  }
  camera.near = 2;
  camera.updateProjectionMatrix();
  visibility.frame(camera);
  assert.ok(targets.filter(target => target.x === -9).every(target => target.mesh.visible));
  assert.ok(targets.filter(target => target.x === -4).some(target => !target.mesh.visible));
  camera.position.x = -1.2;
  visibility.frame(camera);
  assert.ok(targets.every(target => target.mesh.visible));
});

test("software depth covers only whole cells across fractional edges and viewport boundaries", () => {
  const grid = 16, wallDepth = 128;
  const camera = new THREE.PerspectiveCamera(90, 1, 0.1, 1000);
  const world = (x: number, y: number, depth: number) =>
    new THREE.Vector3((2 * x / grid - 1) * depth, (2 * y / grid - 1) * depth, -depth);
  const faces = [
    [[-.25, 2.25], [6.875, 3.5], [1.25, 10.25]],
    [[3, 2], [13, 2], [13, 8], [3, 8]],
    [[5.125, 3.75], [14, 2.75], [16.25, 15.25], [8.5, 12.5]],
    [[6.25, 2], [6.875, 2], [6.875, 13], [6.25, 13]],
    [[1.25, 5.25], [12, 5.25], [12, 6.125], [1.25, 6.125]],
    [[2.875, 4.875], [4.125, 4.875], [4.125, 6.125], [2.875, 6.125]],
  ];
  for (const polygon of faces) {
    const positions = polygon.flatMap(([x, y]) => world(x, y, wallDepth).toArray());
    const geometry = new THREE.BufferGeometry().setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
    geometry.setIndex(polygon.length === 3 ? [0, 1, 2] : [0, 1, 2, 0, 2, 3]);
    const root = new THREE.Group();
    root.add(new THREE.Mesh(geometry, new THREE.MeshBasicMaterial()));
    const visibility = new CityVisibility(root, true, new Map(), { occlusionSize: [grid, grid] });
    visibility.frame(camera);
    for (let y = 0; y < grid; y++) for (let x = 0; x < grid; x++) {
      // Independently test the four cell corners against every directed edge.
      const covered = polygon.every(([ax, ay], edge) => {
        const [bx, by] = polygon[(edge + 1) % polygon.length];
        return [[x, y], [x + 1, y], [x, y + 1], [x + 1, y + 1]].every(([cx, cy]) =>
          (bx - ax) * (cy - ay) - (by - ay) * (cx - ax) > .01);
      });
      const point = world(x + .5, y + .5, wallDepth * 2);
      assert.equal(visibility.occludesBounds(new THREE.Box3(point, point)), covered,
        `(${x}, ${y}) for polygon ${JSON.stringify(polygon)}`);
    }
    visibility.dispose();
  }
});

test("occluders join a shared triangle edge only when it has no crack or reversed face", () => {
  const camera = new THREE.PerspectiveCamera(65, 1, 0.1, 100);
  const hidden = (separation: number, reverseSecond: boolean) => {
    const root = new THREE.Group();
    const first = [-3, -3, -5, 3, -3, -5, 3, 3, -5];
    const second = [[-3, -3, -5], [3, 3, -5], [-3, 3, -5]].map(([x, y, z]) => [x - separation, y + separation, z]);
    if (reverseSecond) second.reverse();
    root.add(new THREE.Mesh(new THREE.BufferGeometry().setAttribute("position", new THREE.Float32BufferAttribute([...first, ...second.flat()], 3)), new THREE.MeshBasicMaterial()));
    const target = new THREE.InstancedMesh(new THREE.BoxGeometry(0.09, 0.09, 0.09), new THREE.MeshBasicMaterial(), 1);
    target.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -10));
    root.add(target);
    new CityVisibility(root).frame(camera);
    return !target.visible;
  };
  assert.equal(hidden(0, false), true);
  assert.equal(hidden(0.002, false), false);
  assert.equal(hidden(0, true), false);
});

test("standalone lawn occlusion restores the full caster for shadow rendering", () => {
  const root = new THREE.Group();
  const wall = new THREE.Mesh(new THREE.PlaneGeometry(10, 10), new THREE.MeshBasicMaterial());
  wall.position.z = -4;
  const lawn = new THREE.Mesh(new THREE.PlaneGeometry(0.5, 0.5), new THREE.MeshBasicMaterial({ transparent: true }));
  lawn.material.userData.grassField = true;
  lawn.position.z = -8;
  root.add(wall, lawn);
  const visibility = new CityVisibility(root);
  visibility.frame(new THREE.PerspectiveCamera(60, 1, 0.1, 40));
  assert.equal(lawn.visible, false);
  visibility.restore();
  assert.equal(lawn.visible, true);
});

test("hidden ancestors and camera-excluded layers cannot occlude visible objects", () => {
  for (const hiddenAncestor of [false, true]) {
    const root = new THREE.Group(), parent = new THREE.Group();
    parent.visible = !hiddenAncestor;
    root.add(parent);
    const wall = new THREE.Mesh(new THREE.PlaneGeometry(10, 10), new THREE.MeshBasicMaterial());
    wall.position.z = -4;
    wall.layers.set(1);
    parent.add(wall);
    const target = new THREE.InstancedMesh(new THREE.BoxGeometry(0.5, 0.5, 0.5), new THREE.MeshBasicMaterial(), 1);
    target.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -8));
    root.add(target);
    const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 40);
    const visibility = new CityVisibility(root);
    visibility.frame(camera);
    assert.equal(target.visible, true);
    camera.layers.enable(1);
    visibility.frame(camera);
    assert.equal(target.visible, hiddenAncestor);
    camera.layers.disable(1);
    visibility.frame(camera);
    assert.equal(target.visible, true);
  }
});

test("ordinary static props behind a wall are culled with full shadow restoration", () => {
  const root = new THREE.Group();
  const wall = new THREE.Mesh(new THREE.PlaneGeometry(10, 10), new THREE.MeshBasicMaterial());
  wall.position.z = -4;
  const hidden = new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.5, 0.5), new THREE.MeshStandardMaterial());
  hidden.position.z = -8;
  hidden.castShadow = true;
  const visible = hidden.clone();
  visible.position.set(0, 0, -2);
  const deformed = hidden.clone();
  deformed.morphTargetInfluences = [0.5];
  root.add(wall, hidden, visible, deformed);
  const visibility = new CityVisibility(root);
  const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 40);
  visibility.frame(camera);
  assert.equal(hidden.visible, false);
  assert.equal(visible.visible, true);
  assert.equal(deformed.visible, true);
  visibility.restore();
  assert.equal(hidden.visible, true);
  assert.equal(hidden.castShadow, true);
  visibility.frame(camera);
  assert.equal(hidden.visible, false);
  visibility.dispose();
  assert.equal(hidden.visible, true);
});

test("both occlusion grid sizes cover roofs and respect underside material sidedness", () => {
  for (const occlusionSize of [[80, 48], [160, 96]] as const) for (const fromBelow of [false, true]) for (const doubleSided of [false, true]) {
    const root = new THREE.Group();
    const roof = new THREE.Mesh(new THREE.PlaneGeometry(10, 10), new THREE.MeshBasicMaterial({ side: doubleSided ? THREE.DoubleSide : THREE.FrontSide }));
    roof.rotation.x = -Math.PI / 2;
    roof.position.y = 5;
    const prop = new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshStandardMaterial());
    prop.position.y = fromBelow ? 8 : 0;
    root.add(roof, prop);
    const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 40);
    camera.position.y = fromBelow ? -10 : 10;
    camera.up.set(0, 0, -1);
    camera.lookAt(0, 5, 0);
    const visibility = new CityVisibility(root, true, new Map(), { occlusionSize });
    visibility.frame(camera);
    assert.equal(prop.visible, fromBelow && !doubleSided);
    visibility.restore();
    assert.equal(prop.visible, true);
  }
});

test("connected surface UV and normal seams keep synchronized cluster LOD", () => {
  const geometry = new THREE.BufferGeometry();
  // The shared diagonal has separate attribute vertices, as at an atlas seam.
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([-5, 0, 0, 5, 0, 0, 5, 1, 0, -5, 0, 0, 5, 1, 0, -5, 1, 0], 3));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute([0, 0, 1, 0, 1, 1, 0.2, 0.2, 0.8, 0.8, 0.2, 0.8], 2));
  geometry.setIndex(Array.from({ length: 12002 }, (_, i) => i % 2 ? [3, 4, 5] : [0, 1, 2]).flat());
  const mid = geometry.clone(), far = geometry.clone();
  mid.setIndex([0, 1, 2, 3, 4, 5]);
  far.setIndex([0, 1, 2]);
  const lods = new Map<THREE.BufferGeometry, CityLodLevels>([[geometry, [mid, far]]]);
  const root = new THREE.Group();
  root.add(new THREE.InstancedMesh(geometry, new THREE.MeshStandardMaterial(), 1));
  clusterCityGeometry(root, lods);
  assert.equal(root.children.length, 2);
  for (const object of root.children) {
    const mesh = object as THREE.Mesh;
    assert.deepEqual(mesh.geometry.userData.lodDistanceBounds, geometry.boundingBox);
    assert.equal(mesh.geometry.attributes.uv, geometry.attributes.uv);
  }
});

test("larger prop error budget applies only to disconnected clusters beyond close range", () => {
  const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 200);
  const mid = new THREE.BoxGeometry(), far = new THREE.BoxGeometry();
  const levels: CityLodLevels = [mid, far];
  mid.userData.cityLodErrorMeters = 0.035;
  far.userData.cityLodErrorMeters = 0.11;
  assert.equal(selectCityLod(levels, 40, camera, 1080, 0, 40, 4), 1);
  for (const geometry of levels) geometry.userData.cityLodIndependent = true;
  assert.equal(selectCityLod(levels, 40, camera, 1080, 0), 1);
  assert.equal(selectCityLod(levels, 40, camera, 1080, 0, 40, 4), 2);
  assert.equal(selectCityLod(levels, 5, camera, 1080, 0, 5, 4), 0);
  assert.equal(selectCityLod(levels, 24, camera, 1080, 0, 24, 4), 1);
  assert.equal(selectCityLod(levels, 24, camera, 1080, 2, 24, 4), 2);
});

test("instanced shaders, displaced meshes and child subtrees keep their authored visibility", () => {
  for (const mode of ["shader", "displacement", "children", "morph", "overlay"]) {
    const root = new THREE.Group();
    const wall = new THREE.Mesh(new THREE.PlaneGeometry(10, 10), new THREE.MeshBasicMaterial());
    wall.position.z = -4;
    const material = mode === "shader" ? new THREE.ShaderMaterial() : new THREE.MeshStandardMaterial();
    if (mode === "displacement") (material as THREE.MeshStandardMaterial).displacementMap = new THREE.Texture();
    if (mode === "overlay") material.depthTest = false;
    const target = new THREE.InstancedMesh(new THREE.BoxGeometry(), material, 1);
    target.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -8));
    if (mode === "children") target.add(new THREE.Group());
    if (mode === "morph") target.morphTexture = new THREE.DataTexture();
    root.add(wall, target);
    new CityVisibility(root).frame(new THREE.PerspectiveCamera(60, 1, 0.1, 40));
    assert.equal(target.visible, true, mode);
  }
});

test("only surfaces with solid GPU coverage become software occluders", () => {
  for (const mode of ["wireframe", "clipping", "depthTest", "neverDepth", "stencil", "offset", "alphaCoverage",
    "depthOnly", "additive", "transmission"]) {
    const root = new THREE.Group();
    const material = new THREE.MeshPhysicalMaterial();
    if (mode === "wireframe") material.wireframe = true;
    if (mode === "clipping") material.clippingPlanes = [new THREE.Plane(new THREE.Vector3(1, 0, 0), 0)];
    if (mode === "depthTest") material.depthTest = false;
    if (mode === "neverDepth") material.depthFunc = THREE.NeverDepth;
    if (mode === "stencil") { material.stencilWrite = true; material.stencilFunc = THREE.NeverStencilFunc; }
    if (mode === "offset") material.polygonOffset = true;
    if (mode === "alphaCoverage") material.alphaToCoverage = true;
    if (mode === "depthOnly") material.colorWrite = false;
    if (mode === "additive") material.blending = THREE.AdditiveBlending;
    if (mode === "transmission") material.transmission = 1;
    const wall = new THREE.Mesh(new THREE.PlaneGeometry(10, 10), material);
    wall.position.z = -4;
    const target = new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshStandardMaterial());
    target.position.z = -8;
    root.add(wall, target);
    new CityVisibility(root).frame(new THREE.PerspectiveCamera(60, 1, 0.1, 40));
    assert.equal(target.visible, true, mode);
    assert.equal(wall.visible, true, mode);
  }
});

test("occlusion preserves receivers with independent depth comparisons and stencil operations", () => {
  const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, .1, 40);
  const wall = new THREE.Mesh(new THREE.PlaneGeometry(10, 10), new THREE.MeshBasicMaterial());
  wall.position.z = -4;
  root.add(wall);
  const receivers = [
    new THREE.MeshBasicMaterial(),
    new THREE.MeshBasicMaterial({ depthFunc: THREE.AlwaysDepth }),
    new THREE.MeshBasicMaterial({ stencilWrite: true, stencilZFail: THREE.ReplaceStencilOp }),
  ].map(material => {
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(), material);
    mesh.position.z = -8;
    root.add(mesh);
    return mesh;
  });
  const visibility = new CityVisibility(root);
  visibility.frame(camera);
  assert.deepEqual(receivers.map(mesh => mesh.visible), [false, true, true]);
  visibility.dispose();
});

test("additional distant LOD levels render by pixel error and retain near geometry for shadows", () => {
  const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, 0.1, 500);
  const original = new THREE.BoxGeometry();
  const levels = [0.035, 0.11, 0.35].map((error, i) => {
    const geometry = original.clone();
    geometry.setIndex(Array.from(original.index!.array).slice(0, 18 - i * 6));
    geometry.userData = { cityLodErrorMeters: error, cityLodIndependent: true };
    return geometry;
  });
  const mesh = new THREE.InstancedMesh(original, new THREE.MeshBasicMaterial(), 1);
  mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -120));
  mesh.castShadow = true;
  root.add(mesh);
  const visibility = new CityVisibility(root, true, new Map([[original, levels]]), { disconnectedErrorPixels: 4 });
  visibility.frame(camera);
  assert.deepEqual(root.children.filter(object => object.visible).map(object => (object as THREE.Mesh).geometry), [levels[2]]);
  visibility.restore();
  assert.deepEqual(root.children.filter(object => object.visible), [mesh]);
  assert.equal(mesh.castShadow, true);
  camera.position.z = -115;
  visibility.frame(camera);
  assert.deepEqual(root.children.filter(object => object.visible), [mesh]);
  assert.equal(mesh.geometry, original);
  visibility.dispose();
});

test("distant small components use the stronger budget while large and nearby surfaces retain detail", () => {
  const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 500);
  const levels = [0.035, 0.11, 0.35].map(error => {
    const geometry = new THREE.BoxGeometry();
    geometry.userData = { cityLodErrorMeters: error, cityLodIndependent: true };
    return geometry;
  });
  assert.equal(selectCityLod(levels, 45, camera, 1080, 0, 45, 4, 0.1, 10, 3), 3);
  assert.equal(selectCityLod(levels, 45, camera, 1080, 0, 45, 4, 15, 10, 3), 2);
  assert.equal(selectCityLod(levels, 8, camera, 1080, 0, 8, 4, 0.1, 10, 3), 0);
  for (const geometry of levels) geometry.userData.cityLodIndependent = false;
  assert.equal(selectCityLod(levels, 40, camera, 1080, 0, 40, 4, 0.1, 10, 3), 1);
  assert.equal(selectCityLod(levels, 120, camera, 1080, 0, 120, 4, 0.1, 10), 2);
  assert.equal(selectCityLod(levels, 120, camera, 1080, 0, 120, 4, 0.1, 10, 3), 3);
});

test("placement uses component diameter rather than the aggregate batch footprint", () => {
  for (const componentDiameter of [undefined, 0.1]) {
    const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, 0.1, 500);
    const original = new THREE.BoxGeometry(20, 1, 1);
    original.userData.cityLodComponentDiameter = componentDiameter;
    const levels = [0.035, 0.11, 0.35].map(error => {
      const geometry = original.clone();
      geometry.userData = { cityLodErrorMeters: error, cityLodIndependent: true };
      return geometry;
    });
    const mesh = new THREE.Mesh(original, new THREE.MeshBasicMaterial());
    mesh.position.z = -45;
    root.add(mesh);
    const visibility = new CityVisibility(root, true, new Map([[original, levels]]), {
      disconnectedErrorPixels: 4, distantDetailErrorPixels: 10,
    });
    visibility.frame(camera);
    const wanted = componentDiameter === undefined ? levels[1] : levels[2];
    assert.deepEqual(root.children.filter(object => object.visible).map(object => (object as THREE.Mesh).geometry), [wanted]);
    visibility.dispose();
  }
});

test("detail omission bounds coverage, resolution, off-axis perspective, near range and hysteresis", () => {
  const camera = new THREE.PerspectiveCamera(90, 1, .1, 500);
  camera.updateMatrixWorld();
  const box = new THREE.Box3(new THREE.Vector3(-.1, -.1, -100), new THREE.Vector3(.1, .1, -100));
  const budget = { maxComponentPixels: 2, maxCoveragePixels: 4, minDistance: 40 };
  const omit = (diameter = .3, area = .12, height = 1000, distance = 100, hidden = false) =>
    omitCityDetail(box, diameter, area, distance, 100, camera, height, hidden, budget);
  assert.equal(omit(), true);
  assert.equal(omit(.3, .3), false, "many individually tiny components cannot exceed total coverage");
  assert.equal(omit(.3, .12, 2000), false, "larger drawing buffers preserve more detail");
  assert.equal(omit(.3, .12, 1000, 39), false);
  assert.equal(omit(.3, .12, 1000, 39, true), true);
  assert.equal(omit(.3, .12, 1000, 35, true), false);
  assert.equal(omit(.45), false);
  assert.equal(omit(.45, .12, 1000, 100, true), true);
  box.translate(new THREE.Vector3(100, 0, 0));
  assert.equal(omit(), false, "off-axis perspective has larger possible coverage");
  box.translate(new THREE.Vector3(-100, 0, 0));
  camera.zoom = 2; camera.updateProjectionMatrix();
  assert.equal(omit(), false);
  assert.equal(omitCityDetail(box, .1, .01, 100, -.1, camera, 1000, false, budget), false);
});

test("optional distant detail omission removes draws while preserving near instances, picks and full casters", () => {
  for (const instanced of [false, true]) for (const enabled of [false, true]) {
    const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, .1, 200);
    const geometry = new THREE.BoxGeometry(.1, .1, .1);
    geometry.userData = { cityLodIndependent: true, cityLodComponentDiameter: Math.sqrt(.03), cityLodComponentArea: Math.PI * .03 / 4 };
    const material = new THREE.MeshStandardMaterial();
    material.userData.worldSpecSurface = { family: "Shop inventory", recipe: "stock-hardware" };
    const mesh = instanced ? new THREE.InstancedMesh(geometry, material, 2) : new THREE.Mesh(geometry, material);
    if (mesh instanceof THREE.InstancedMesh) {
      mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -80));
      mesh.setMatrixAt(1, new THREE.Matrix4().makeTranslation(1, 0, -8));
    } else mesh.position.z = -80;
    mesh.castShadow = true; root.add(mesh);
    const visibility = new CityVisibility(root, true, new Map([[geometry, [geometry, geometry]]]), enabled ? { detailCull: {} } : {});
    const visiblePlacements = () => root.children.filter(object => object.visible)
      .reduce((sum, object) => sum + (object instanceof THREE.InstancedMesh ? object.count : 1), 0);
    const totalPlacements = instanced ? 2 : 1;
    const expectedVisible = enabled ? totalPlacements - 1 : totalPlacements;
    visibility.frame(camera);
    assert.equal(visiblePlacements(), expectedVisible);
    visibility.frame(camera, 1080, 4);
    assert.equal(visiblePlacements(), expectedVisible, "visual LOD bias leaves the actual pixel omission threshold intact");
    const ray = new THREE.Raycaster(new THREE.Vector3(), new THREE.Vector3(0, 0, -1));
    assert.equal(ray.intersectObject(root)[0].object, mesh);
    visibility.restore(); assert.deepEqual(root.children.filter(object => object.visible), [mesh]);
    visibility.shadowFrame(camera, camera);
    assert.deepEqual(root.children.filter(object => object.visible), [mesh]);
    if (mesh instanceof THREE.InstancedMesh) assert.equal(mesh.count, 2);
    assert.equal(mesh.castShadow, true);
    camera.position.z = -75; visibility.frame(camera);
    const near = root.children.filter(object => object.visible) as THREE.Mesh[];
    assert.ok(near.some(object => !(object instanceof THREE.InstancedMesh) || object.count >= 1));
    visibility.dispose(); assert.deepEqual(root.children, [mesh]);
  }
});

test("continuous masses, signage, lights, cutouts and explicit keep metadata bypass omission", () => {
  for (const protection of ["continuous", "architecture", "emissive", "emissive-map", "sign", "poster-recipe", "marking-recipe", "tile-recipe", "cutout", "keep", "malformed", "legacy"]) {
    const root = new THREE.Group(), geometry = new THREE.BoxGeometry(.1, .1, .1), material = new THREE.MeshStandardMaterial();
    geometry.userData = { cityLodIndependent: true, cityLodComponentDiameter: .2, cityLodComponentArea: .04 };
    material.userData.worldSpecSurface = { family: protection === "architecture" ? "Architecture" : "Shop inventory",
      recipe: ({ architecture: "concrete", "poster-recipe": "wall-posters", "marking-recipe": "cabinet-markings", "tile-recipe": "restaurant-tiles" } as Record<string, string>)[protection] ?? "stock-hardware" };
    if (protection === "continuous") geometry.userData.cityLodIndependent = false;
    if (protection === "legacy") delete geometry.userData.cityLodComponentArea;
    if (protection === "emissive") material.emissive.setRGB(.1, .05, 0);
    if (protection === "emissive-map") material.emissiveMap = new THREE.Texture();
    if (protection === "cutout") material.alphaTest = .5;
    if (protection === "sign") material.userData.sourceFinishes = ["room / posters"];
    if (protection === "keep") material.userData.cityDetailKeep = true;
    if (protection === "malformed") material.userData.sourceFinishes = {};
    const mesh = new THREE.Mesh(geometry, material); mesh.position.z = -100; root.add(mesh);
    const visibility = new CityVisibility(root, true, new Map([[geometry, [geometry, geometry]]]), { detailCull: {} });
    visibility.frame(new THREE.PerspectiveCamera(60, 1, .1, 200));
    assert.equal(mesh.visible, true, protection);
    visibility.dispose();
  }
});

test("unclustered stock uses whole bounds and omitted geometry cannot remain an occluder", () => {
  for (const instanced of [false, true]) {
    const root = new THREE.Group(), geometry = new THREE.BoxGeometry(.1, .1, .1), material = new THREE.MeshStandardMaterial();
    material.userData.worldSpecSurface = { family: "Shop inventory", recipe: "stock-hardware" };
    const mesh = instanced ? new THREE.InstancedMesh(geometry, material, 1) : new THREE.Mesh(geometry, material);
    if (mesh instanceof THREE.InstancedMesh) mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -100));
    else mesh.position.z = -100;
    root.add(mesh);
    const camera = new THREE.PerspectiveCamera(60, 1, .1, 200), visibility = new CityVisibility(root, true, new Map(), { detailCull: {} });
    visibility.frame(camera); assert.equal(mesh.visible, false);
    camera.position.z = -95; visibility.frame(camera); assert.equal(mesh.visible, true);
    visibility.dispose();
  }
  for (const enabled of [false, true]) {
    const root = new THREE.Group(), material = new THREE.MeshStandardMaterial();
    material.userData.worldSpecSurface = { family: "Shop inventory", recipe: "stock-hardware" };
    const surface = new THREE.Mesh(new THREE.PlaneGeometry(10, 10), material); surface.position.z = -50;
    const behind = new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshStandardMaterial()); behind.position.z = -60;
    root.add(surface, behind);
    const visibility = new CityVisibility(root, true, new Map(), enabled ? { detailCull: { maxComponentPixels: 1000, maxCoveragePixels: 1e6 } } : {});
    visibility.frame(new THREE.PerspectiveCamera(60, 1, .1, 200));
    assert.equal(surface.visible, !enabled);
    assert.equal(behind.visible, enabled, "a camera-omitted surface must leave the software occlusion buffer open");
    visibility.dispose();
  }
});

function clusterPair(instances?: readonly [number[], number[]]) {
  const root = new THREE.Group(), material = new THREE.MeshBasicMaterial();
  const clusterPairs = new WeakMap<THREE.Mesh, object>(), pair = {};
  const position = new THREE.Float32BufferAttribute([-5, 0, 0, -4, 0, 0, -4, 1, 0, 4, 0, 0, 5, 0, 0, 5, 1, 0], 3);
  const sources = [0, 1].map(part => {
    const geometry = new THREE.BufferGeometry().setAttribute("position", position);
    geometry.setIndex(part ? [3, 4, 5] : [0, 1, 2]);
    geometry.boundingBox = new THREE.Box3(new THREE.Vector3(part ? 4 : -5, 0, 0), new THREE.Vector3(part ? 5 : -4, 1, 0));
    geometry.boundingSphere = geometry.boundingBox.getBoundingSphere(new THREE.Sphere());
    const mesh = instances ? new THREE.InstancedMesh(geometry, material, instances[part].length) : new THREE.Mesh(geometry, material);
    if (mesh instanceof THREE.InstancedMesh) instances![part].forEach((x, i) => mesh.setMatrixAt(i, new THREE.Matrix4().makeTranslation(x, 0, 0)));
    mesh.position.z = -8;
    mesh.castShadow = true;
    clusterPairs.set(mesh, pair);
    root.add(mesh);
    return mesh;
  });
  return { root, sources, material, position, clusterPairs, camera: new THREE.PerspectiveCamera(70, 2, 0.1, 100) };
}

test("exact cluster coalescing preserves index winding, attributes, material, picking and shadow restoration", () => {
  for (const instanced of [false, true]) {
    const { root, sources, material, position, clusterPairs, camera } = clusterPair(instanced ? [[0, 1], [0, 1]] : undefined);
    const visibility = new CityVisibility(root, true, new Map(), { coalesceClusters: true, clusterPairs });
    visibility.frame(camera);
    const drawn = root.children.filter(object => object.visible) as THREE.Mesh[];
    assert.equal(drawn.length, 1);
    assert.deepEqual(Array.from(drawn[0].geometry.index!.array), [0, 1, 2, 3, 4, 5]);
    assert.equal(drawn[0].geometry.attributes.position, position);
    assert.equal(drawn[0].material, material);
    assert.equal(drawn[0].castShadow, false);
    if (instanced) assert.equal((drawn[0] as THREE.InstancedMesh).count, 2);
    const ray = new THREE.Raycaster(new THREE.Vector3(-4.2, 0.3, 0), new THREE.Vector3(0, 0, -1));
    assert.ok(ray.intersectObject(root).length > 0);
    assert.ok(ray.intersectObject(root).every(hit => sources.includes(hit.object as THREE.Mesh)));
    visibility.restore();
    assert.deepEqual(root.children.filter(object => object.visible), sources);
    visibility.frame(camera);
    assert.deepEqual(root.children.filter(object => object.visible), drawn);
    visibility.dispose();
    assert.deepEqual(root.children, sources);
    assert.ok(sources.every(mesh => mesh.visible && mesh.castShadow));
  }
});

test("coalescing preserves differing instance subsets and ordering", () => {
  for (const wideClusters of [false, true]) {
    for (const instances of [[[0, 10], [0, 10]], [[0, 1], [1, 0]]] as const) {
      const { root, camera, clusterPairs } = clusterPair(instances);
      const visibility = new CityVisibility(root, true, new Map(), { coalesceClusters: true, clusterPairs, wideClusters });
      visibility.frame(camera);
      assert.equal(root.children.filter(object => object.visible).length, 2);
      visibility.dispose();
    }
  }
});

test("optional detailed LOD streams override legacy levels while legacy sidecars retain two levels", async t => {
  const root = new THREE.Group(), geometry = new THREE.BoxGeometry();
  const mesh = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial());
  root.add(mesh);
  root.userData.cityLod = "city-data/lod-cafe.bin";
  tagCityLodPrimitives({ scene: root, parser: { associations: new Map([[mesh, { nodes: 0 }]]),
    json: { nodes: [{ mesh: 0 }], meshes: [{ primitives: [{}] }] } } } as unknown as GLTF);
  const level = (errorMeters: number) => ({ offset: 0, bytes: 0, count: 0, componentBytes: 2, errorMeters });
  const record = { mesh: 0, primitive: 0, vertexCount: geometry.attributes.position.count, nearCount: geometry.index!.count,
    levels: [level(0.035), level(0.11)], detailLevels: [0.02, 0.04, 0.08, 0.15, 0.3].map(level) };
  let bytes = sidecar([record]);
  t.mock.method(globalThis, "fetch", async () => new Response(bytes));
  const detailed = await loadCityLods(root, "https://viewer.example/city.glb");
  assert.deepEqual(detailed.get(geometry)!.map(g => g.userData.cityLodErrorMeters), [0.02, 0.04, 0.08, 0.15, 0.3]);
  const { detailLevels, ...legacy } = record;
  bytes = sidecar([legacy]);
  assert.equal((await loadCityLods(root, "https://viewer.example/city.glb")).get(geometry)!.length, 2);
});

test("custom object callbacks retain their source mesh through merging, clustering and LOD selection", () => {
  for (const callback of ["onBeforeRender", "onAfterRender", "onBeforeShadow", "onAfterShadow", "raycast"] as const) {
    const root = new THREE.Group(), geometry = new THREE.BoxGeometry();
    const original = Array.from(geometry.index!.array);
    geometry.setIndex(Array.from({ length: 1100 }, () => original).flat());
    const mesh = new THREE.InstancedMesh(geometry, new THREE.MeshBasicMaterial(), 1);
    mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -100));
    const hook = () => {};
    mesh[callback] = hook;
    root.add(mesh);
    const lods = new Map<THREE.BufferGeometry, CityLodLevels>([[geometry, [geometry.clone(), geometry.clone()]]]);
    mergeCityInstances(root);
    const clusterPairs = clusterCityGeometry(root, lods);
    const visibility = new CityVisibility(root, true, lods, { coalesceClusters: true, clusterPairs });
    visibility.frame(new THREE.PerspectiveCamera(60, 1, 0.1, 200));
    assert.deepEqual(root.children, [mesh], callback);
    assert.equal(mesh[callback], hook);
    assert.equal(mesh.visible, true);
    visibility.dispose();
  }
});

test("merges and clusters preserve admitted depth and point-shadow materials", () => {
  const root = new THREE.Group(), geometry = new THREE.BoxGeometry(), material = new THREE.MeshBasicMaterial();
  const indices = Array.from(geometry.index!.array);
  geometry.setIndex(Array.from({ length: 1100 }, () => indices).flat());
  const depth = new THREE.MeshDepthMaterial(), distance = new THREE.MeshDistanceMaterial();
  for (let i = 0; i < 2; i++) {
    const mesh = new THREE.InstancedMesh(geometry, material, 1);
    mesh.customDepthMaterial = depth;
    mesh.customDistanceMaterial = distance;
    root.add(mesh);
  }
  mergeCityInstances(root);
  assert.equal(root.children.length, 1);
  assert.equal((root.children[0] as THREE.Mesh).customDistanceMaterial, distance);
  const lods = new Map<THREE.BufferGeometry, CityLodLevels>([[geometry, [geometry.clone(), geometry.clone()]]]);
  const pairs = clusterCityGeometry(root, lods);
  assert.ok(root.children.length > 1);
  for (const object of root.children) {
    const mesh = object as THREE.Mesh;
    assert.equal(mesh.customDepthMaterial, depth);
    assert.equal(mesh.customDistanceMaterial, distance);
    assert.ok(pairs.has(mesh));
    assert.equal(mesh.userData.cityClusterPair, undefined);
  }
});

test("authored user data cannot nominate meshes for coalescing", () => {
  const { root, sources, camera } = clusterPair();
  for (const mesh of sources) mesh.userData.cityClusterPair = "authored-pair";
  const visibility = new CityVisibility(root, true, new Map(), { coalesceClusters: true });
  visibility.frame(camera);
  assert.deepEqual(root.children.filter(object => object.visible), sources);
  visibility.dispose();
});

test("light-frustum subsets retain off-camera full-detail casters without mutating queued view buffers", () => {
  const root = new THREE.Group(), geometry = new THREE.BoxGeometry();
  const material = new THREE.MeshBasicMaterial({ alphaTest: 0.5 });
  const mesh = new THREE.InstancedMesh(geometry, material, 3);
  for (let i = 0; i < 3; i++) mesh.setMatrixAt(i, new THREE.Matrix4().makeTranslation(i * 20, 0, 0));
  mesh.position.set(5, 0, -8);
  mesh.castShadow = true;
  mesh.customDepthMaterial = new THREE.MeshDepthMaterial();
  mesh.customDistanceMaterial = new THREE.MeshDistanceMaterial();
  root.add(mesh);
  const view = new THREE.PerspectiveCamera(40, 1, 0.1, 100);
  view.position.x = 5;
  const visibility = new CityVisibility(root);
  visibility.frame(view);
  const cameraDraw = root.children.find(object => object.visible) as THREE.InstancedMesh;
  assert.equal(cameraDraw.count, 1);
  const queuedMatrices = cameraDraw.instanceMatrix.array.slice();
  const uploaded = new Map<THREE.BufferAttribute, number[]>(), released: THREE.Object3D[] = [];
  // Exercise Three's actual once-per-object upload gate with both lights in the
  // same renderer frame. Reusing one subset object silently leaves stale GPU data.
  const objects = new WebGLObjects({ ARRAY_BUFFER: 34962 } as never,
    { get: (_object: THREE.Object3D, source: THREE.BufferGeometry) => source, update: () => {} } as never,
    { update: (attribute: THREE.BufferAttribute) => uploaded.set(attribute, Array.from(attribute.array)),
      remove: (attribute: THREE.BufferAttribute) => uploaded.delete(attribute) } as never,
    { releaseStatesOfObject: (object: THREE.Object3D) => released.push(object) } as never,
    { render: { frame: 1 } } as never);
  const lightDraws: THREE.InstancedMesh[] = [];
  const lightCameras: THREE.Camera[] = [];
  for (const x of [25, 45]) {
    const light = new THREE.PerspectiveCamera(40, 1, 0.1, 100);
    lightCameras.push(light);
    light.position.x = x;
    visibility.shadowFrame(light, view);
    const drawn = root.children.filter(object => object.visible) as THREE.InstancedMesh[];
    assert.equal(drawn.length, 1);
    assert.notEqual(drawn[0], cameraDraw);
    assert.equal(drawn[0].count, 1);
    assert.equal(drawn[0].geometry, geometry);
    assert.equal(drawn[0].material, material);
    assert.equal(drawn[0].castShadow, true);
    assert.equal(drawn[0].customDepthMaterial, mesh.customDepthMaterial);
    assert.equal(drawn[0].customDistanceMaterial, mesh.customDistanceMaterial);
    assert.deepEqual(drawn[0].matrixWorld.elements, mesh.matrixWorld.elements);
    assert.equal(drawn[0].instanceMatrix.array[12], x - 5);
    assert.ok(!lightDraws.includes(drawn[0]));
    assert.deepEqual(drawn[0].instanceMatrix.updateRanges, [{ start: 0, count: 16 }]);
    objects.update(drawn[0]);
    assert.equal(uploaded.get(drawn[0].instanceMatrix)![12], x - 5);
    lightDraws.push(drawn[0]);
    assert.deepEqual(cameraDraw.instanceMatrix.array, queuedMatrices);
    assert.equal(mesh.count, 3);
  }
  assert.equal(lightDraws[0].instanceMatrix.array[12], 20, "the second light does not mutate the first draw buffer");
  assert.equal(uploaded.size, 2, "both lights upload independently in the same render frame");
  visibility.shadowFrame(lightCameras[0], view);
  assert.deepEqual(root.children.filter(object => object.visible), [lightDraws[0]], "each light reuses its own stable object");
  visibility.frame(view);
  assert.deepEqual(root.children.filter(object => object.visible), [cameraDraw]);
  assert.deepEqual(cameraDraw.instanceMatrix.array, queuedMatrices);
  visibility.restore();
  assert.deepEqual(root.children.filter(object => object.visible), [mesh]);
  visibility.dispose();
  assert.deepEqual(root.children, [mesh]);
  assert.deepEqual(released, lightDraws);
  assert.equal(uploaded.size, 0);
  objects.dispose();
});

test("transient shadow cameras have bounded allocation and preserve full casters on overflow", () => {
  const root = new THREE.Group(), mesh = new THREE.InstancedMesh(new THREE.BoxGeometry(), new THREE.MeshBasicMaterial(), 2);
  mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -10));
  mesh.setMatrixAt(1, new THREE.Matrix4().makeTranslation(30, 0, -10));
  mesh.castShadow = true;
  root.add(mesh);
  const visibility = new CityVisibility(root);
  for (let i = 0; i < 100; i++) visibility.shadowFrame(new THREE.PerspectiveCamera(40, 1, 0.1, 100));
  assert.equal(root.children.length, 9, "at most eight cameras own a separate subset");
  assert.deepEqual(root.children.filter(object => object.visible), [mesh], "extra cameras retain all full-detail casters");
  visibility.dispose();
  assert.deepEqual(root.children, [mesh]);
});

test("custom shadow deformation bypasses original-geometry bounds", () => {
  for (const shadowMaterial of [new THREE.ShaderMaterial(), new THREE.MeshDepthMaterial({ displacementMap: new THREE.Texture() })]) {
    const root = new THREE.Group(), mesh = new THREE.InstancedMesh(new THREE.BoxGeometry(), new THREE.MeshBasicMaterial(), 2);
    mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(30, 0, -10));
    mesh.setMatrixAt(1, new THREE.Matrix4().makeTranslation(60, 0, -10));
    mesh.castShadow = true;
    mesh.customDepthMaterial = shadowMaterial;
    root.add(mesh);
    const visibility = new CityVisibility(root);
    visibility.shadowFrame(new THREE.PerspectiveCamera(40, 1, 0.1, 100));
    assert.deepEqual(root.children.filter(object => object.visible), [mesh]);
    visibility.dispose();
  }
});

test("shadow selection retains boundary intersections and honors full-visibility overrides", () => {
  const root = new THREE.Group(), mesh = new THREE.InstancedMesh(new THREE.BoxGeometry(), new THREE.MeshBasicMaterial(), 2);
  mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(10.4, 0, -10));
  mesh.setMatrixAt(1, new THREE.Matrix4().makeTranslation(30, 0, -10));
  mesh.castShadow = true;
  root.add(mesh);
  const light = new THREE.PerspectiveCamera(90, 1, 0.1, 100), view = light.clone();
  const visibility = new CityVisibility(root);
  visibility.shadowFrame(light, view);
  const subset = root.children.find(object => object.visible) as THREE.InstancedMesh;
  assert.equal(subset.count, 1);
  assert.ok(Math.abs(subset.instanceMatrix.array[12] - 10.4) < 1e-5);
  light.position.x = -100;
  visibility.shadowFrame(light, view);
  assert.equal(root.children.filter(object => object.visible).length, 0);
  visibility.restore();
  mesh.frustumCulled = false;
  visibility.shadowFrame(light, view);
  assert.deepEqual(root.children.filter(object => object.visible), [mesh]);
  visibility.dispose();
});

test("a single mesh uses its published LOD, material and full-detail picking and caster", () => {
  const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, 0.1, 200);
  const source = new THREE.BoxGeometry(), far = source.clone();
  far.setIndex([0, 1, 2]);
  far.userData.cityLodErrorMeters = 0.05;
  const material = new THREE.MeshStandardMaterial({ metalness: 0.75 });
  const mesh = new THREE.Mesh(source, material);
  mesh.position.z = -80;
  mesh.castShadow = true;
  root.add(mesh);
  const visibility = new CityVisibility(root, true, new Map([[source, [far, far]]]));
  visibility.frame(camera);
  const alternate = root.children.find(child => child.visible) as THREE.Mesh;
  assert.equal(alternate.geometry, far);
  assert.equal(alternate.material, material);
  assert.equal(alternate.castShadow, false);
  assert.equal(mesh.visible, false);
  visibility.frame(camera, 1080, 4);
  assert.deepEqual(root.children.filter(child => child.visible), [mesh], "changing the bias alone refreshes projected LOD selection");
  visibility.frame(camera, 1080);
  assert.deepEqual(root.children.filter(child => child.visible), [alternate]);
  const ray = new THREE.Raycaster(new THREE.Vector3(), new THREE.Vector3(0, 0, -1));
  assert.equal(ray.intersectObject(root)[0].object, mesh);
  visibility.restore();
  assert.deepEqual(root.children.filter(child => child.visible), [mesh]);
  camera.position.z = -74;
  visibility.frame(camera);
  assert.deepEqual(root.children.filter(child => child.visible), [mesh]);
  visibility.dispose();
  assert.deepEqual(root.children, [mesh]);
});

test("spatial index batches join different selected LODs and restore the ordinary shadow and picking owners", () => {
  const root = new THREE.Group(), material = new THREE.MeshStandardMaterial();
  const positions = new THREE.Float32BufferAttribute(
    [-8, -35, -70].flatMap(z => [-.3, 0, z, 0, .5, z, .3, 0, z, 0, -.5, z]), 3);
  const levels = new Map<THREE.BufferGeometry, CityLodLevels>(), clusterPairs = new WeakMap<THREE.Mesh, object>(), origin = {};
  const source = [-8, -35, -70].map((z, i) => {
    const geom = (indices: number[]) => {
      const g = new THREE.BufferGeometry().setAttribute("position", positions).setIndex(indices.map(id => 4 * i + id));
      g.boundingBox = new THREE.Box3(new THREE.Vector3(-.3, -.5, z), new THREE.Vector3(.3, .5, z));
      g.boundingSphere = g.boundingBox.getBoundingSphere(new THREE.Sphere()); return g;
    };
    const near = geom([0, 2, 1, 0, 3, 2]), mid = geom([0, 2, 1]), far = geom([3, 2, 0]);
    levels.set(near, [mid, far]);
    const mesh = new THREE.Mesh(near, material); mesh.castShadow = true;
    root.add(mesh); clusterPairs.set(mesh, origin); return mesh;
  });
  const view = new CityVisibility(root, true, levels, { coalesceClusters: true, clusterPairs, wideClusters: true });
  const camera = new THREE.PerspectiveCamera(60, 1, .1, 500);
  view.frame(camera);
  const wide = root.children.filter(m => m.visible) as THREE.Mesh[];
  assert.equal(wide.length, 1);
  assert.deepEqual(Array.from(wide[0].geometry.index!.array), [0, 2, 1, 0, 3, 2, 4, 6, 5, 11, 10, 8]);
  assert.equal(wide[0].geometry.attributes.position, positions);
  assert.equal(wide[0].castShadow, false);
  view.setSubmission("ordinary");
  view.frame(camera); assert.equal(root.children.filter(m => m.visible).length, 3);
  view.setSubmission("clusters");
  view.frame(camera); assert.deepEqual(root.children.filter(m => m.visible), wide);
  view.restore(); assert.deepEqual(root.children.filter(m => m.visible), source);
  view.dispose(); assert.deepEqual(root.children, source);
  assert.ok(source.every(m => m.castShadow && m.geometry.attributes.position === positions));
});

/** Four placements of a 12-triangle box with 4- and 2-triangle legacy levels: one near, two mid, one far. */
function lodBandScene(lodMergeTriangles?: number, far?: THREE.BufferGeometry) {
  const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(60, 1, 0.1, 200);
  const original = new THREE.BoxGeometry(), mid = original.clone();
  far ??= original.clone().setIndex([0, 1, 2, 2, 3, 0]);
  mid.setIndex(Array.from(original.index!.array).slice(0, 12));
  const mesh = new THREE.InstancedMesh(original, new THREE.MeshBasicMaterial(), 4);
  for (const [i, [x, z]] of [[-2, -8], [-1, -30], [1, -31], [3, -70]].entries())
    mesh.setMatrixAt(i, new THREE.Matrix4().makeTranslation(x, 0, z));
  mesh.castShadow = true;
  root.add(mesh);
  const visibility = new CityVisibility(root, true, new Map([[original, [mid, far]]]),
    lodMergeTriangles === undefined ? {} : { lodMergeTriangles });
  const draws = () => (root.children.filter(object => object.visible) as THREE.InstancedMesh[])
    .map(draw => [draw.geometry === original ? "near" : draw.geometry === mid ? "mid" : "far", draw.count]);
  return { root, camera, mesh, visibility, draws };
}

test("LOD bands of one batch join the next finer selected level within the triangle budget", () => {
  // Far to mid adds 1 * (4 - 2) triangles. Mid to near then adds 3 * (12 - 4).
  const expected: [number | undefined, (string | number)[][]][] = [
    [-1, [["near", 1], ["mid", 2], ["far", 1]]],
    [1, [["near", 1], ["mid", 2], ["far", 1]]],
    [2, [["near", 1], ["mid", 3]]],
    [23, [["near", 1], ["mid", 3]]],
    [24, [["near", 4]]],
    [undefined, [["near", 4]]],
  ];
  assert.ok(CITY_LOD_MERGE_TRIANGLES >= 24, "the default budget admits this 26-triangle promotion");
  for (const [budget, wanted] of expected) {
    const { root, camera, mesh, visibility, draws } = lodBandScene(budget);
    visibility.frame(camera);
    assert.deepEqual(draws().sort(), wanted.slice().sort(), `budget ${budget}`);
    if (wanted.length === 1) assert.deepEqual(root.children.filter(object => object.visible), [mesh], "one band is the original batch");
    visibility.restore();
    assert.deepEqual(root.children.filter(object => object.visible), [mesh]);
    assert.ok(mesh.castShadow && mesh.count === 4);
    visibility.dispose();
    assert.deepEqual(root.children, [mesh]);
  }
});

test("a new graphics preset changes the band budget of the running world", () => {
  const { camera, visibility, draws } = lodBandScene(2);
  const bands = () => { visibility.frame(camera); return draws().sort(); };
  assert.deepEqual(bands(), [["mid", 3], ["near", 1]]);
  visibility.setLodMergeTriangles(-1);
  assert.deepEqual(bands(), [["far", 1], ["mid", 2], ["near", 1]], "the unchanged camera is selected again");
  visibility.setLodMergeTriangles(24);
  assert.deepEqual(bands(), [["near", 4]]);
  visibility.setLodMergeTriangles(undefined);
  assert.deepEqual(bands(), [["near", 4]], "no stated budget is the default one");
  visibility.dispose();
});

test("a promoted placement keeps its selected level's hysteresis and an omitted level stays omitted", () => {
  const { camera, visibility, draws } = lodBandScene(2);
  visibility.frame(camera);
  assert.deepEqual(draws().sort(), [["mid", 3], ["near", 1]]);
  // At 47 m only a placement that was selected at the far level stays there.
  // The near band now costs 12 - 2 triangles to join, which exceeds the budget.
  camera.position.z = -23;
  visibility.frame(camera);
  assert.deepEqual(draws().sort(), [["far", 1], ["near", 2]]);
  visibility.dispose();

  const omitted = lodBandScene(Infinity, new THREE.BoxGeometry().setIndex([]));
  omitted.visibility.frame(omitted.camera);
  assert.deepEqual(omitted.draws(), [["near", 3]], "the part omitted at the far level is not drawn again");
  omitted.visibility.dispose();
});

test("software occlusion follows the level a promoted placement draws", () => {
  for (const [lodMergeTriangles, seen] of [[-1, false], [2, true]] as const) {
    const root = new THREE.Group(), camera = new THREE.PerspectiveCamera(78, 1.6, 0.05, 100);
    // The detailed wall has a 2 m doorway. Its reduced level closes the opening.
    const doorway = new THREE.BufferGeometry().setAttribute("position", new THREE.Float32BufferAttribute(
      [-6, -6, 0, -1, -6, 0, -1, 6, 0, -6, 6, 0, 1, -6, 0, 6, -6, 0, 6, 6, 0, 1, 6, 0], 3));
    doorway.setIndex([0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
    const closed = doorway.clone().setIndex([0, 5, 6, 0, 6, 3]);
    const walls = new THREE.InstancedMesh(doorway, new THREE.MeshBasicMaterial(), 2);
    walls.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -10));
    walls.setMatrixAt(1, new THREE.Matrix4().makeTranslation(0, 0, -40));
    const target = new THREE.InstancedMesh(new THREE.BoxGeometry(.3, .3, .3), new THREE.MeshBasicMaterial(), 1);
    target.setMatrixAt(0, new THREE.Matrix4().makeTranslation(0, 0, -45));
    root.add(walls, target);
    const visibility = new CityVisibility(root, true, new Map([[doorway, [closed]]]), { lodMergeTriangles });
    visibility.frame(camera);
    const drawn = root.children.filter(object => object.visible && object !== target) as THREE.InstancedMesh[];
    assert.deepEqual(drawn.map(draw => draw.geometry === doorway).sort(), seen ? [true] : [false, true]);
    assert.equal(target.visible, seen, seen ? "the open doorway is drawn, so it cannot hide the receiver"
      : "the closed far level is drawn and hides the receiver");
    visibility.dispose();
  }
});

test("parts split from one primitive promote together so their joined draws remain", () => {
  // Promotion adds 4 - 2 triangles to one part and 6 - 2 to the other. A part
  // which its reduced level omits stays omitted there while its sibling is promoted.
  for (const [lodMergeTriangles, firstReduced, wanted] of [[5, [0, 1, 2, 0, 2, 3], [[12, 1], [30, 1]]],
    [6, [0, 1, 2, 0, 2, 3], [[30, 2]]], [6, [], [[12, 1], [18, 2]]]] as const) {
    const root = new THREE.Group(), material = new THREE.MeshStandardMaterial(), origin = {};
    const positions = new THREE.Float32BufferAttribute([-1, 0, 0, 0, 0, 0, 0, 1, 0, -1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0], 3);
    const part = (indices: number[]) => new THREE.BufferGeometry().setAttribute("position", positions).setIndex(indices);
    const levels = new Map<THREE.BufferGeometry, CityLodLevels>(), clusterPairs = new WeakMap<THREE.Mesh, object>();
    for (const [near, reduced] of [[[0, 1, 2, 0, 2, 3, 0, 1, 2, 0, 2, 3], [...firstReduced]],
      [[4, 5, 6, 4, 6, 7, 4, 5, 6, 4, 6, 7, 4, 5, 6, 4, 6, 7], [4, 5, 6, 4, 6, 7]]]) {
      const geometry = part(near);
      levels.set(geometry, [part(reduced)]);
      const mesh = new THREE.InstancedMesh(geometry, material, 2);
      mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(-2, 0, -10));
      mesh.setMatrixAt(1, new THREE.Matrix4().makeTranslation(2, 0, -40));
      root.add(mesh); clusterPairs.set(mesh, origin);
    }
    const visibility = new CityVisibility(root, true, levels,
      { coalesceClusters: true, clusterPairs, wideClusters: true, lodMergeTriangles });
    visibility.frame(new THREE.PerspectiveCamera(60, 1, .1, 200));
    const draws = (root.children.filter(object => object.visible) as THREE.InstancedMesh[])
      .map(draw => [draw.geometry.index!.count, draw.count]).sort((a, b) => a[0] - b[0]);
    assert.deepEqual(draws, wanted, `budget ${lodMergeTriangles}`);
    visibility.dispose();
  }
});
