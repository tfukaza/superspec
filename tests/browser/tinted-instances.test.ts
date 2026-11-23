import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import { MeshoptEncoder } from "meshoptimizer";
import { createRenderGltfLoader } from "../../adapters/three/src/gltfLoader.ts";
import { bindWorldSpecScene } from "../../adapters/three/src/bindScene.ts";
import { CityVisibility, mergeCityInstances } from "../../adapters/three/src/cityVisibility.ts";
import { clusterCityGeometry } from "../../adapters/three/src/cityClusters.ts";
import { CityFarHardware } from "../../adapters/three/src/cityFarHardware.ts";
import type { CityLodLevels } from "../../adapters/three/src/cityLods.ts";
import { DistantGlazingDraws } from "../../adapters/three/src/distantGlazing.ts";
import { GlazingRange } from "../../adapters/three/src/glazingRange.ts";
import { CityTextureResidency } from "../../adapters/three/src/cityTextureResidency.ts";
import { publishedCityGeometry } from "../../adapters/three/src/publishedGeometry.ts";
import { resolveRenderDiagnostics } from "../../adapters/three/src/renderProfile.ts";
import { prepareFirstColor, shadowDepthStage } from "../../adapters/three/src/worldWarmup.ts";
import { GlazingSides, glazingSideStage } from "../../adapters/three/src/glazing.ts";
import { applyInstanceRoles, hasInstanceRoles, INSTANCE_ROLE_ATTRIBUTE } from "../../adapters/three/src/worldSpecMaterials.ts";
import { recordingRenderer } from "./first-frame-programs.fixture.ts";
import { defaultPanes, defaultParts, tintedPartsGlb, type Batch, type Vec3 } from "./tinted-instances.fixture.ts";

const place = (translation: Vec3) => translation.join(",");
/** What the fixture authored for each placement of the part: its tint, or null where the batch has none. */
function authoredTints(parts: Batch[] = defaultParts) {
  const tints = new Map<string, Vec3 | null>();
  for (const batch of parts) batch.translations.forEach((at, i) => tints.set(place(at), batch.tints?.[i] ?? null));
  return tints;
}

async function load(parts: Batch[] = defaultParts, panes: Batch[] = defaultPanes, options?: Parameters<typeof tintedPartsGlb>[2]) {
  const gltf = await createRenderGltfLoader().parseAsync(tintedPartsGlb(parts, panes, options), "") as GLTF;
  gltf.scene.updateMatrixWorld(true);
  const batch = (name: string) => gltf.scene.getObjectByName(name) as THREE.InstancedMesh;
  return { gltf, root: gltf.scene, batch, geometry: batch(parts[0].node).geometry, material: batch(parts[0].node).material as THREE.MeshStandardMaterial };
}

/** Every instance a visible draw submits: where it stands, its tint as the GPU will read it, and its draw. */
function submitted(root: THREE.Object3D, family: (mesh: THREE.InstancedMesh) => boolean) {
  const instances: { at: string; tint: number[] | null; draw: THREE.InstancedMesh }[] = [];
  const matrix = new THREE.Matrix4(), position = new THREE.Vector3();
  root.traverseVisible(object => {
    if (!(object instanceof THREE.InstancedMesh) || !family(object)) return;
    for (let i = 0; i < object.count; i++) {
      object.getMatrixAt(i, matrix);
      position.setFromMatrixPosition(matrix.premultiply(object.matrixWorld));
      const tint = object.instanceColor;
      instances.push({ at: place(position.toArray() as Vec3), draw: object,
        tint: tint ? Array.from(tint.array.subarray(i * tint.itemSize, (i + 1) * tint.itemSize)) : null });
    }
  });
  return instances;
}

function assertTints(instances: ReturnType<typeof submitted>, expected: Map<string, Vec3 | null>, message: string) {
  for (const { at, tint } of instances) {
    assert.ok(expected.has(at), `${message}: an instance was drawn at ${at}, where none was authored`);
    const wanted = expected.get(at)!;
    if (!wanted) assert.equal(tint, null, `${message}: the untinted instance at ${at} gained a tint`);
    else {
      assert.ok(tint, `${message}: the instance at ${at} lost its tint`);
      wanted.forEach((value, channel) => assert.equal(tint[channel], Math.fround(value), `${message}: the tint at ${at}`));
    }
  }
}

const once = (instances: ReturnType<typeof submitted>) => {
  const places = instances.map(instance => instance.at);
  assert.equal(new Set(places).size, places.length, "no placement is drawn twice");
  return places.sort();
};
const triangles = (geometry: THREE.BufferGeometry) => geometry.index!.count / 3;

/** Two reduced levels of the part without error records: the legacy distance bands, which switch at 24 m and 50 m. */
function legacyLevels(geometry: THREE.BufferGeometry) {
  const level = (indices: number[]) => {
    const reduced = new THREE.BufferGeometry();
    for (const [name, attribute] of Object.entries(geometry.attributes)) reduced.setAttribute(name, attribute);
    reduced.setIndex(indices);
    reduced.boundingBox = geometry.boundingBox; reduced.boundingSphere = geometry.boundingSphere;
    return reduced;
  };
  return [level([0, 1, 2, 4, 5, 6]), level([0, 1, 2])];
}

test("the loader delivers _COLOR_0 as instanceColor on the shared prototype geometry and _MS_ROLE as a vertex attribute", async () => {
  const { batch, geometry } = await load();
  for (const { node, translations, tints } of defaultParts) {
    const mesh = batch(node);
    assert.ok(mesh instanceof THREE.InstancedMesh, node);
    assert.equal(mesh.count, translations.length);
    assert.equal(mesh.geometry, geometry, "tinted and untinted batches of one prototype share its geometry");
    if (!tints) { assert.equal(mesh.instanceColor, null, node); continue; }
    assert.ok(mesh.instanceColor instanceof THREE.InstancedBufferAttribute);
    assert.equal(mesh.instanceColor.itemSize, 3);
    assert.deepEqual(Array.from(mesh.instanceColor.array), tints.flat().map(Math.fround));
  }
  // No per-instance attribute lands on the geometry, which would bar the batch from instance subsets.
  assert.ok(Object.values(geometry.attributes).every(attribute => !(attribute instanceof THREE.InstancedBufferAttribute)));
  const role = geometry.getAttribute(INSTANCE_ROLE_ATTRIBUTE);
  assert.ok(hasInstanceRoles(geometry));
  assert.deepEqual([role.itemSize, role.normalized, Array.from(role.array)], [1, false, [1, 1, 1, 1, 0, 0, 0, 0]]);
  assert.equal(hasInstanceRoles(batch(defaultPanes[0].node).geometry), false);
});

test("merged batches keep per-instance colour, and tinted copies never merge with untinted ones", async () => {
  const { root, geometry, material } = await load();
  mergeCityInstances(root);
  const parts = root.children.filter((mesh): mesh is THREE.InstancedMesh => mesh instanceof THREE.InstancedMesh && mesh.geometry === geometry);
  assert.equal(parts.length, 2, "one draw for the tinted copies and one for the untinted");
  const expected = authoredTints();
  for (const mesh of parts) {
    assert.equal(mesh.material, material);
    const instances = submitted(root, draw => draw === mesh);
    assertTints(instances, expected, "merged");
    const tinted = !!mesh.instanceColor;
    assert.deepEqual(once(instances), [...expected].filter(([, tint]) => !!tint === tinted).map(([at]) => at).sort());
    if (tinted) assert.equal(mesh.instanceColor!.count, mesh.count, "one tint for each merged instance");
  }
});

test("a tinted batch selects LOD per instance and subset colours follow subset matrices", async () => {
  for (const lodMergeTriangles of [-1, undefined]) {
    const { root, geometry } = await load();
    const levels = [geometry, ...legacyLevels(geometry)], expected = authoredTints();
    mergeCityInstances(root);
    const part = (mesh: THREE.InstancedMesh) => levels.includes(mesh.geometry);
    const sources = root.children.filter(mesh => mesh instanceof THREE.InstancedMesh && part(mesh)) as THREE.InstancedMesh[];
    const visibility = new CityVisibility(root, true, new Map<THREE.BufferGeometry, CityLodLevels>([[geometry, levels.slice(1)]]), { lodMergeTriangles });
    const camera = new THREE.PerspectiveCamera(60, 1, .1, 200);
    // The camera first sees every placement, then stands past the first two rows and sees only the last.
    for (const [z, seen] of [[0, [...expected.keys()]], [-45, [...expected.keys()].filter(at => Number(at.split(",")[2]) < -45)]] as const) {
      camera.position.z = z;
      camera.updateMatrixWorld(true);
      visibility.frame(camera);
      const instances = submitted(root, part);
      assertTints(instances, expected, `camera at ${z}`);
      assert.deepEqual(once(instances), [...seen].sort(), "each placement in view is drawn exactly once");
      for (const { at, draw } of instances) {
        // Legacy levels switch on the distance to the placement, which here is within a metre of its depth.
        const depth = z - Number(at.split(",")[2]), selected = depth > 51 ? 2 : depth < 23 ? 0 : depth > 25 && depth < 45 ? 1 : undefined;
        if (selected === undefined) continue;
        if (lodMergeTriangles === -1) assert.equal(draw.geometry, levels[selected], `the placement at ${at} draws its own level from ${z}`);
        else assert.ok(triangles(draw.geometry) >= triangles(levels[selected]), "band merging never draws less detail than was selected");
      }
      const tintedLevels = new Set(instances.filter(instance => instance.tint).map(instance => instance.draw.geometry));
      if (lodMergeTriangles === -1 && z === 0) assert.equal(tintedLevels.size, 3, "one tinted batch draws three levels at once");
      for (const draw of new Set(instances.map(instance => instance.draw))) {
        const tint = draw.instanceColor;
        if (tint && !sources.includes(draw)) assert.ok(tint.version > 0, "a rewritten subset uploads its tints");
      }
      visibility.restore();
      assert.deepEqual(root.children.filter(mesh => mesh.visible && mesh instanceof THREE.InstancedMesh && part(mesh)), sources);
    }
    visibility.dispose();
  }
});

test("a light's caster subset of a tinted batch keeps each kept instance's colour", async () => {
  const { root, geometry } = await load();
  mergeCityInstances(root);
  root.traverse(object => { object.castShadow = true; });
  const visibility = new CityVisibility(root);
  // A light looking along -Z from the far end covers only the placements beyond it.
  const light = new THREE.PerspectiveCamera(60, 1, .1, 100);
  light.position.z = -40;
  visibility.shadowFrame(light);
  const casters = submitted(root, mesh => mesh.geometry === geometry && mesh.castShadow);
  const expected = authoredTints();
  assertTints(casters, expected, "shadow subset");
  assert.deepEqual(once(casters), [...expected.keys()].filter(at => Number(at.split(",")[2]) < -40).sort());
  assert.ok(casters.every(({ draw }) => draw.geometry === geometry), "casters keep full detail");
  visibility.dispose();
});

/** The part with enough triangles for the spatial splitter, and reduced levels of the same streams. */
async function clusteredParts() {
  const loaded = await load();
  const { root, geometry } = loaded;
  const indices = Array.from(geometry.index!.array);
  geometry.setIndex(Array.from({ length: 3001 }, () => indices).flat());
  const level = () => {
    const reduced = new THREE.BufferGeometry().setIndex(geometry.index);
    for (const [name, attribute] of Object.entries(geometry.attributes)) reduced.setAttribute(name, attribute);
    return reduced;
  };
  const lods = new Map<THREE.BufferGeometry, CityLodLevels>([[geometry, [level(), level()]]]);
  mergeCityInstances(root);
  const pairs = clusterCityGeometry(root, lods);
  const pieces = root.children.filter((mesh): mesh is THREE.InstancedMesh => mesh instanceof THREE.InstancedMesh && pairs.has(mesh));
  return { ...loaded, lods, pairs, pieces, part: (mesh: THREE.InstancedMesh) => mesh.geometry.getAttribute("position") === geometry.getAttribute("position") };
}

test("spatial clusters of a tinted batch keep its colours and join only with equal colours", async () => {
  for (const wideClusters of [false, true]) for (const change of ["none", "one tint differs", "one piece untinted"] as const) {
    const { root, lods, pairs, pieces, part } = await clusteredParts();
    const expected = authoredTints();
    assert.equal(pieces.length, 4, "the tinted and the untinted batch each split in two");
    assertTints(submitted(root, mesh => pieces.includes(mesh)), expected, "cluster pieces");
    const tintedPieces = pieces.filter(piece => piece.instanceColor);
    assert.equal(tintedPieces.length, 2);
    // The second placement's green channel, in one half only.
    const changed = place(defaultParts[0].translations[1]);
    if (change === "one tint differs") tintedPieces[1].instanceColor!.array[4] = .5;
    if (change === "one piece untinted") tintedPieces[1].instanceColor = null;
    const visibility = new CityVisibility(root, true, lods, { coalesceClusters: true, clusterPairs: pairs, wideClusters, lodMergeTriangles: -1 });
    const camera = new THREE.PerspectiveCamera(60, 1, .1, 200);
    visibility.frame(camera);
    const draws = [...new Set(submitted(root, part).map(instance => instance.draw))];
    const joined = draws.filter(draw => !pairs.has(draw) && triangles(draw.geometry) === 2 * 3001 * 2);
    const untinted = joined.filter(draw => !draw.instanceColor), tinted = joined.filter(draw => draw.instanceColor);
    assert.ok(untinted.length > 0, "the untinted halves join into one draw for a level");
    if (change === "none") {
      // The tinted placements stand in three distance bands and the untinted ones in two.
      assert.deepEqual([tinted.length, untinted.length], [3, 2], "the halves of every level join, tinted as untinted");
      assert.deepEqual(draws.filter(draw => !joined.includes(draw)), [], "no half is drawn on its own");
      assertTints(submitted(root, mesh => tinted.includes(mesh)), expected, "joined cluster draw");
      for (const draw of tinted) assert.equal(draw.instanceColor!.count >= draw.count, true);
      assertTints(submitted(root, part), expected, "every cluster draw");
    } else if (change === "one piece untinted") assert.equal(tinted.length, 0, "a tinted half never joins an untinted one");
    else {
      // Only the level that draws the changed placement has halves that disagree.
      const halves = submitted(root, part).filter(instance => instance.at === changed).map(instance => instance.draw);
      assert.equal(halves.length, 2, "the placement whose halves disagree is drawn as two halves");
      assert.ok(halves.every(draw => triangles(draw.geometry) === 3001 * 2));
      assert.notDeepEqual(submitted(root, mesh => mesh === halves[0]).find(instance => instance.at === changed)!.tint,
        submitted(root, mesh => mesh === halves[1]).find(instance => instance.at === changed)!.tint);
      assert.deepEqual([tinted.length, untinted.length], [2, 2], "the other two tinted levels still join");
    }
    visibility.dispose();
    assert.ok(tintedPieces[0].instanceColor, "disposing a joined draw leaves its source's tints in place");
  }
});

test("a distant hardware draw of a tinted batch carries the tints of the placements it kept", async () => {
  const { root, geometry, material } = await load();
  material.userData.worldSpecSurface = { family: "Building atlases", surface: "Mixed surfaces with metal", recipe: "weathered-steel" };
  mergeCityInstances(root);
  const source = root.children.find(mesh => mesh instanceof THREE.InstancedMesh && mesh.geometry === geometry && mesh.instanceColor) as THREE.InstancedMesh;
  const reduced = geometry.clone();
  reduced.userData.cityLodErrorMeters = .4;
  const cells = [source.clone(), source.clone()];
  root.add(...cells);
  const far = new CityFarHardware();
  far.register(source, cells, [reduced, reduced]);
  source.removeFromParent();
  const visibility = new CityVisibility(root, true, new Map([[geometry, [reduced, reduced]]]), { farHardware: far });
  visibility.frame(new THREE.PerspectiveCamera(70, 1.78, .1, 300), 1080);
  const farDraws = submitted(root, mesh => mesh.geometry === reduced && !cells.includes(mesh));
  assert.ok(farDraws.length > 0, "distant placements share a far draw");
  assert.ok(farDraws.length < source.count, "near placements stay in their cells");
  assertTints(farDraws, authoredTints(), "far hardware");
  once(farDraws);
  visibility.dispose();
});

test("an instanced, untinted pane is still batched in the distance and left out beyond the glazing range", async () => {
  const { root, batch } = await load();
  const panes = defaultPanes.map(pane => batch(pane.node));
  for (const pane of panes) assert.equal(pane.instanceColor, null, "blended prototype primitives carry no tint");
  const camera = new THREE.PerspectiveCamera(60, 1, .1, 500);
  camera.updateMatrixWorld(true);
  const distant = new DistantGlazingDraws(root);
  distant.frame(camera, 1080, 1080);
  const count = panes.reduce((sum, pane) => sum + pane.count, 0);
  assert.equal(distant.stats.farPlacements, count, "every distant pane joins the shared draw");
  assert.equal(distant.stats.savedDraws, panes.length - 1);
  assert.ok(panes.every(pane => !pane.visible));
  const shared = submitted(root, mesh => mesh.material === panes[0].material);
  assert.deepEqual(once(shared), defaultPanes.flatMap(pane => pane.translations.map(place)).sort());
  // A tinted pane keeps its own draw: one shared draw could not carry its instances' colours.
  distant.dispose();
  panes[0].setColorAt(0, new THREE.Color(1, 0, 0));
  const withTint = new DistantGlazingDraws(root);
  withTint.frame(camera, 1080, 1080);
  assert.ok(panes[0].visible, "a tinted pane stays on its own draw");
  withTint.dispose();
  panes[0].instanceColor = null;

  // Range: the camera stands 2 m in front of the first pane. The next pane of its batch is 2.5 m away.
  const range = new GlazingRange(root, 2.2);
  camera.position.set(-2, 4.25, -78);
  camera.updateMatrixWorld(true);
  range.frame(camera);
  const kept = submitted(root, mesh => mesh.material === panes[0].material);
  assert.deepEqual(once(kept), [place(defaultPanes[0].translations[0])], "only the pane within range is drawn, as a subset of its batch");
  range.dispose();
  assert.ok(panes.every(pane => pane.visible));
});

test("texture residency measures every placement of a tinted batch", async () => {
  const { root, material } = await load();
  const low = new THREE.Texture(), high = new THREE.Texture();
  material.map = low;
  const residency = new CityTextureResidency(root, surface => surface === material ? [{ slot: "map", low, loadHigh: async () => high }] : []);
  const camera = new THREE.PerspectiveCamera(70, 16 / 9, .1, 500);
  // 300 m away nothing is near. Beside the farthest tinted placement, its material is.
  camera.position.set(0, 0, 300);
  camera.updateMatrixWorld(true);
  residency.frame(camera, 1080);
  await residency.settle();
  assert.equal(material.map, low);
  camera.position.set(7, 0, -58);
  camera.updateMatrixWorld(true);
  residency.frame(camera, 1080);
  await residency.settle();
  assert.equal(material.map, high, "the last instance of the tinted batch brings its material near");
  residency.dispose();
});

/** A sidecar giving the part two reduced levels with error records, as a published pack does. */
function lodSidecar(): ArrayBuffer {
  const streams = [[0, 1, 2, 4, 5, 6], [0, 1, 2]].map(indices => MeshoptEncoder.encodeIndexBuffer(new Uint8Array(new Uint16Array(indices).buffer), indices.length, 2));
  let offset = 0;
  const levels = streams.map((stream, i) => {
    const level = { offset, bytes: stream.length, count: [6, 3][i], componentBytes: 2, errorMeters: [.02, .2][i] };
    offset += stream.length;
    return level;
  });
  const manifest = new TextEncoder().encode(JSON.stringify({ primitives: [{ mesh: 0, primitive: 0, vertexCount: 8, nearCount: 12, levels }] }));
  const start = 12 + Math.ceil(manifest.length / 4) * 4;
  const bytes = new Uint8Array(start + offset);
  bytes.set([83, 76, 79, 68, 1, 0, 0, 0]);
  new DataView(bytes.buffer).setUint32(8, manifest.length, true);
  bytes.set(manifest, 12);
  let at = start;
  for (const stream of streams) { bytes.set(stream, at); at += stream.length; }
  return bytes.buffer;
}

/** The published path as `attachWorldSpecScene` runs it: bind the materials, then build the camera selection. */
async function publishedParts(t: test.TestContext, renderer: THREE.WebGLRenderer, withholdTinted = false) {
  await MeshoptEncoder.ready;
  t.mock.method(globalThis, "fetch", async () => new Response(lodSidecar()));
  const gltf = await createRenderGltfLoader().parseAsync(tintedPartsGlb(defaultParts, defaultPanes, { lod: "parts-data/lod-0a.bin" }), "") as GLTF;
  const position = (gltf.scene.getObjectByName(defaultParts[0].node) as THREE.Mesh).geometry.getAttribute("position");
  const scene = new THREE.Scene(), sun = new THREE.DirectionalLight(0xffffff, 2);
  sun.position.set(2, 30, -20);
  sun.target.position.set(2, 0, -30);
  sun.castShadow = true;
  Object.assign(sun.shadow.camera, { left: -40, right: 40, top: 40, bottom: -40, near: 1, far: 80 });
  sun.shadow.camera.updateProjectionMatrix();
  scene.add(sun, sun.target, gltf.scene);
  scene.updateMatrixWorld(true);
  const diagnostics = resolveRenderDiagnostics();
  const caster = new THREE.MeshDepthMaterial();
  assert.equal(await bindWorldSpecScene(gltf, { renderer, sun, caster, isCurrent: () => true, diagnostics }), true);
  // A control can keep the tinted batches out of the selection, and so out of the preparation.
  const withheld: THREE.InstancedMesh[] = [];
  if (withholdTinted) {
    gltf.scene.traverse(object => { if (object instanceof THREE.InstancedMesh && object.instanceColor) withheld.push(object); });
    for (const mesh of withheld) mesh.removeFromParent();
  }
  const geometry = await publishedCityGeometry(gltf, renderer, diagnostics, { entryUrl: new URL("https://viewer.example/parts.glb"), lodBandMergeTriangles: -1 });
  /** The part's draws, whichever level, subset or joined draw submits them. */
  const part = (mesh: THREE.InstancedMesh) => mesh.geometry.getAttribute("position") === position;
  return { gltf, scene, sun, geometry, caster, withheld, part };
}

test("after first-frame preparation, tinted batches link no program in the shadow pass or the colour pass", async t => {
  for (const prepareTinted of [true, false]) {
    const { renderer, stats } = recordingRenderer();
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFShadowMap;
    // The control prepares the same scene without its tinted batches, as if their variants were not covered.
    const { gltf, scene, sun, geometry, caster, withheld } = await publishedParts(t, renderer, !prepareTinted);
    const camera = new THREE.PerspectiveCamera(60, 1, .1, 200);
    scene.add(camera);
    camera.updateMatrixWorld(true);
    assert.equal(withheld.length > 0, !prepareTinted);
    // Shadow casters are read canonical, the colour submission with the camera's selection applied.
    geometry.restoreCanonical();
    const shadows = shadowDepthStage(renderer, scene);
    geometry.prepareView(camera);
    const sides = new GlazingSides();
    const prepared = await prepareFirstColor(renderer, scene, camera, null, undefined, undefined,
      { variants: glazingSideStage(gltf.scene, sides), shadowCasters: shadows.stage });
    geometry.finishView();
    assert.ok(prepared && prepared.programs > 0);
    for (const mesh of withheld) gltf.scene.add(mesh);
    const linkedBefore = stats.linked;
    const known = new Set(renderer.info.programs);

    // What each pass submits, and with which material.
    const depthDraws: THREE.InstancedMesh[] = [], colourDraws: THREE.InstancedMesh[] = [];
    const direct = renderer.renderBufferDirect.bind(renderer);
    renderer.renderBufferDirect = (view, fog, drawn, material, object, group) => {
      if (object instanceof THREE.InstancedMesh && object.instanceColor)
        ((material as THREE.MeshDepthMaterial).isMeshDepthMaterial ? depthDraws : colourDraws).push(object);
      return direct(view, fog, drawn, material, object, group);
    };
    // The sun's pass draws canonical casters culled to the light. The camera's selection follows.
    geometry.restoreCanonical();
    sun.shadow.updateMatrices(sun);
    geometry.shadowFrame(sun.shadow.camera, camera);
    sides.render(renderer, () => renderer.render(scene, camera));
    assert.ok(depthDraws.length > 0, "the shadow pass draws the tinted batch");
    assert.ok(depthDraws.every(mesh => mesh.count > 0 && mesh.castShadow));
    renderer.shadowMap.autoUpdate = false;
    depthDraws.length = colourDraws.length = 0;
    for (const z of [0, -20, -45]) {
      camera.position.z = z;
      camera.updateMatrixWorld(true);
      geometry.beginFrame(camera);
      sides.render(renderer, () => renderer.render(scene, camera));
      geometry.finishView();
    }
    assert.equal(depthDraws.length, 0);
    assert.ok(colourDraws.length > 0, "the colour pass draws the tinted batch");
    const firstFrames = stats.linked - linkedBefore;
    const late = (renderer.info.programs ?? []).filter(program => !known.has(program)).map(program => program.cacheKey.split(",")[0]);
    if (prepareTinted) {
      assert.ok(new Set(colourDraws.map(mesh => mesh.geometry)).size > 1, "at more than one level");
      assert.equal(firstFrames, 0, "every program the tinted batches draw with was prepared");
    } else {
      // Three names a program's shader first in its key.
      assert.deepEqual(late.sort(), ["depth", "physical"], "the unprepared control links the tinted depth and colour programs while drawing");
      assert.equal(firstFrames, 2);
    }
    geometry.dispose();
    sides.dispose();
    for (const material of shadows.materials) material.dispose();
    caster.dispose();
    renderer.dispose();
  }
});

test("the published path draws tinted instances at their own levels with their own colours", async t => {
  const { renderer } = recordingRenderer();
  const { gltf, geometry, part } = await publishedParts(t, renderer);
  const expected = authoredTints();
  const camera = new THREE.PerspectiveCamera(60, 1, .1, 200);
  camera.updateMatrixWorld(true);
  geometry.beginFrame(camera);
  const instances = submitted(gltf.scene, part);
  assertTints(instances, expected, "published");
  assert.deepEqual(once(instances), [...expected.keys()].sort());
  // Detail never increases with distance, the nearest placements are complete, and the batch is not at one level.
  const depth = ({ at }: { at: string }) => -Number(at.split(",")[2]);
  for (const a of instances) for (const b of instances)
    if (depth(a) < depth(b) - 5) assert.ok(triangles(a.draw.geometry) >= triangles(b.draw.geometry), `${a.at} is nearer than ${b.at}`);
  for (const instance of instances) if (depth(instance) < 10) assert.equal(triangles(instance.draw.geometry), 4);
  assert.ok(new Set(instances.filter(instance => instance.tint).map(instance => triangles(instance.draw.geometry))).size > 1);
  geometry.dispose();
  renderer.dispose();
});

test("the role material changes only its tinted instanced program", async t => {
  const { renderer } = recordingRenderer();
  const { gltf, geometry } = await publishedParts(t, renderer);
  const materials = new Set<THREE.Material>();
  gltf.scene.traverse(object => { if (object instanceof THREE.Mesh) materials.add(object.material); });
  const stock = new THREE.MeshStandardMaterial();
  for (const material of materials) {
    const roles = material.name === "parts";
    assert.equal(/instance-roles/.test(material.customProgramCacheKey()), roles, `${material.name} ${roles ? "reads" : "does not read"} roles`);
  }
  // The same binding of a pack whose part has no role attribute leaves the material's key as it was.
  geometry.dispose();
  const plain = await createRenderGltfLoader().parseAsync(tintedPartsGlb(defaultParts, defaultPanes, { roles: false }), "") as GLTF;
  const sun = new THREE.DirectionalLight();
  await bindWorldSpecScene(plain, { renderer, sun, caster: new THREE.MeshDepthMaterial(), isCurrent: () => true, diagnostics: resolveRenderDiagnostics() });
  plain.scene.traverse(object => {
    if (object instanceof THREE.Mesh) assert.equal(/instance-roles/.test(object.material.customProgramCacheKey()), false);
  });

  // The addition is confined to the tinted instanced variant of the vertex stage.
  const shader = { vertexShader: THREE.ShaderLib.standard.vertexShader, fragmentShader: THREE.ShaderLib.standard.fragmentShader, uniforms: {} };
  applyInstanceRoles(stock);
  stock.onBeforeCompile(shader as never, renderer);
  assert.equal(shader.fragmentShader, THREE.ShaderLib.standard.fragmentShader);
  const lines = (source: string) => source.split("\n").map(line => line.trim()).filter(Boolean);
  const stockLines = lines(THREE.ShaderLib.standard.vertexShader);
  let guarded = 0;
  for (const line of lines(shader.vertexShader)) {
    if (line.startsWith("#ifdef USE_INSTANCING_COLOR")) guarded++;
    else if (line.startsWith("#endif")) guarded = Math.max(0, guarded - 1);
    else if (!stockLines.includes(line)) assert.ok(guarded > 0, `"${line}" is compiled only with instance colours`);
  }
  assert.deepEqual(lines(shader.vertexShader).filter(line => stockLines.includes(line) && !/^#(ifdef USE_INSTANCING_COLOR|endif)/.test(line)),
    stockLines.filter(line => !/^#(ifdef USE_INSTANCING_COLOR|endif)/.test(line)), "the stock vertex stage is kept whole and in order");
  assert.ok(shader.vertexShader.includes("#include <color_vertex>"), "Three's own chunk still applies the colour");
  renderer.dispose();
});
