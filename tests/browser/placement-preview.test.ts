import test from "node:test";
import assert from "node:assert/strict";
import { BoxGeometry, BufferGeometry, Float32BufferAttribute, InstancedMesh, Matrix4, Mesh, MeshBasicMaterial, Vector3 } from "three";
import type { Manifest, ScenePacket } from "../../browser/model/packet.ts";
import { disposeFootprintGeometry, footprintGeometry } from "../../browser/renderer/footprint-geometry.ts";
import { PlacementPreview } from "../../editor/placement-preview.ts";
import { renamedSelection } from "../../editor/rename-preview.ts";
import { PlacementTransforms } from "../../browser/renderer/placement-preview.ts";
import { itemShapePreview, onlyItemShapeChanged } from "../../editor/studio/item-shape.ts";
import type { AuthoringProject, Parameter } from "../../browser/runtime/authoring.ts";

test("a selected mesh follows a single placement ID edit through row suffixes and unicode source offsets", () => {
  const pre = '<architecture version="1" id="map"><!--街🏢--><city>';
  const source = pre + '<building id="lot" position="0 0 0" count="2" step="12 0 0"/></city></architecture>';
  const selected = { id: "city/lot/2/part/roof", objectId: "city/lot/2", source: { offset: pre.length } };
  assert.equal(renamedSelection(source, source.replace('id="lot"', 'id="teahouse"'), selected), "city/teahouse/2/part/roof");
  for (const draft of [source, source.replace("lot", ""), source.replace("lot", "&lt;"),
    source.replace("lot", "new").replace("12 0 0", "14 0 0")]) {
    assert.equal(renamedSelection(source, draft, selected), undefined);
  }
});

test("only a valid change inside the accepted position range previews; successive drafts use the same accepted origin", () => {
  const prefix = '<architecture version="1" id="map"><!--街🏢--><city>';
  const tag = "<building seed='5' position = '2 3 0' id=\"lot\" count='2' step='12 0 0'/>";
  const source = prefix + tag + "</city></architecture>";
  const preview = new PlacementPreview();
  preview.accept(source, { architecture: { metadata: [{ kind: "city", children: [
    { kind: "building", id: "lot", attributes: { position: [2, 3, 0], count: 2 }, source: { offset: prefix.length, end: prefix.length + tag.length } },
  ] }] } } as unknown as Manifest);
  const moved = (xyz: string) => source.replace("2 3 0", xyz);
  assert.deepEqual(preview.moves(moved("2 3 -0.0005")), [{ owners: ["city/lot/1", "city/lot/2"], translation: [0, 0, -0.001] }]);
  assert.deepEqual(preview.moves(moved("5 5 0")), [{ owners: ["city/lot/1", "city/lot/2"], translation: [3, 2, 0] }]);
  for (const other of [source, moved("1 3"), moved("0xA 3 0"), moved("1 2 3e999"), moved("\uFEFF0 0 0"),
    moved("2 3 1000001"), moved("5 4e-1 2").replace("seed='5'", "seed='4'"), moved("2 3 4").replace("</city>", "<")]) {
    assert.deepEqual(preview.moves(other), []);
  }
});

test("a source place moves its exact ordinary and batched draws while all other instances and original bounds are preserved", () => {
  const preview = new PlacementTransforms();
  const geometry = new BoxGeometry(), material = new MeshBasicMaterial();
  const batch = new InstancedMesh(geometry, material, 2);
  batch.userData.source = { objectId: "city/template", template: "city/template/body" };
  const original = new Matrix4().makeRotationZ(Math.PI / 2).setPosition(3, 4, 5);
  batch.setMatrixAt(0, new Matrix4());
  batch.setMatrixAt(1, original);
  const first = Array.from(batch.instanceMatrix.array);
  batch.computeBoundingBox();
  batch.computeBoundingSphere();
  const { boundingBox, boundingSphere } = batch;
  const ordinary = new Mesh(geometry, material);
  ordinary.position.set(7, 8, 9);
  ordinary.userData.source = { objectId: "city/lot" };
  const meshes = [ordinary, batch];
  const owners = { "city/template": ["city/template", "city/lot"] };
  const move = [{ owners: ["city/lot"], translation: [5, 6, 7] }];
  assert.equal(preview.apply(meshes, owners, move), true);
  const matrix = new Matrix4();
  batch.getMatrixAt(0, matrix);
  assert.deepEqual(matrix.elements, new Matrix4().elements);
  batch.getMatrixAt(1, matrix);
  assert.deepEqual(matrix.elements.slice(12, 15), [8, 10, 12]);
  assert.deepEqual(matrix.elements.slice(0, 12), first.slice(16, 28));
  assert.deepEqual(ordinary.position.toArray(), [12, 14, 16]);
  assert.equal(batch.boundingBox, null);
  batch.computeBoundingBox();
  batch.computeBoundingSphere();
  assert.equal(preview.apply(meshes, owners, [{ owners: ["city/lot"], translation: [1, 0, 0] }]), true);
  assert.deepEqual(ordinary.position.toArray(), [8, 8, 9]);
  assert.equal(preview.clear(), true);
  assert.deepEqual(ordinary.position.toArray(), [7, 8, 9]);
  assert.deepEqual(Array.from(batch.instanceMatrix.array), first);
  assert.equal(batch.boundingBox, boundingBox);
  assert.equal(batch.boundingSphere, boundingSphere);
  // A packet missing instance provenance or a missing row member restores the whole draft.
  assert.equal(preview.apply(meshes, {}, move), false);
  assert.equal(preview.apply(meshes, owners, [{ owners: ["city/lot", "city/missing"], translation: [1, 0, 0] }]), false);
  assert.deepEqual(ordinary.position.toArray(), [7, 8, 9]);
  assert.deepEqual(Array.from(batch.instanceMatrix.array), first);
});

test("adding dimensions and replacing them with a convex four-corner lot previews one source row", () => {
  const prefix = '<architecture version="1" id="test"><city>';
  const tag = '<building id="lot" position="7 2 0" heading="90" count="2" step="0 15 0" />';
  const base = prefix + tag + "</city></architecture>";
  const accepted = (source: string, attributes: Record<string, unknown> = {}) => {
    const preview = new PlacementPreview();
    preview.accept(source, { architecture: { metadata: [{ kind: "city", children: [{ id: "lot", kind: "building",
      attributes: { position: [7, 2, 0], heading: 90, count: 2, step: [0, 15, 0], ...attributes },
      source: { offset: prefix.length } }] }] } } as unknown as Manifest);
    return preview;
  };
  const withAttrs = (attrs: string) => base.replace(" />", " " + attrs + " />");
  const wide = withAttrs('width="12" depth="8"');
  const first = accepted(base).moves(wide);
  assert.deepEqual(first[0].footprint!.from, [[0,0],[9.8,0],[9.8,7],[0,7]]);
  assert.deepEqual(first[0].footprint!.to, [[0,0],[12,0],[12,8],[0,8]]);
  assert.deepEqual(first[0].footprint!.origins, [[7,2,0],[7,17,0]]);
  const preview = accepted(wide, { width:12, depth:8 });
  const trap = withAttrs('footprint="0 0; 12 0; 10 8; 0 8"');
  assert.deepEqual(preview.moves(trap)[0].footprint!.to, [[0,0],[12,0],[10,8],[0,8]]);
  for (const bad of [
    trap.replace("12 0; 10 8", "10 8; 12 0"), trap.replace("0 8", "0 1"),
    trap.replace("footprint=", 'width="12" footprint='), wide.replace('width="12"', 'width="12" width="8"'),
    trap.replace('id="test"', 'id="new"'), wide.replace('width="12"', 'width="4"'),
    trap.replace(" />", ">"), trap.replace('count="2"', 'count="3"'),
  ]) assert.deepEqual(preview.moves(bad), [], bad);
});

test("a courtyard placement translates immediately and uses the compiler for footprint edits", () => {
  const prefix = '<architecture version="1" id="test"><city>';
  const source = prefix + '<building id="court" position="0 0 0" footprint="0 0; 12 0; 12 10; 0 10">'
    + '<hole points="4 3; 4 7; 8 7; 8 3"/></building></city></architecture>';
  const preview = new PlacementPreview();
  preview.accept(source, { architecture: { metadata: [{ kind: "city", children: [{
    id: "court", kind: "building", attributes: { position: [0, 0, 0] },
    children: [{ kind: "hole" }], source: { offset: prefix.length },
  }] }] } } as unknown as Manifest);
  assert.deepEqual(preview.moves(source.replace('position="0 0 0"', 'position="2 3 0"')),
    [{ owners: ["city/court"], translation: [2, 3, 0] }]);
  assert.deepEqual(preview.moves(source.replace("12 0; 12 10", "13 0; 13 10")), []);
});

test("a rotated textured lot keeps its UVs on both halves of a taper, maps its corners, and restores every draw", () => {
  const geometry = new BufferGeometry(), material = new MeshBasicMaterial(), mesh = new Mesh(geometry, material);
  geometry.setAttribute("position", new Float32BufferAttribute([0,0,14, 0,8,14, -7,8,14, -7,0,14], 3));
  geometry.setAttribute("uv", new Float32BufferAttribute([0,0, 1,0, 1,1, 0,1], 2));
  geometry.setIndex([0,1,2,0,2,3]);
  mesh.position.set(3, 4, 5);
  mesh.layers.set(2);
  mesh.userData.source = { objectId: "city/home", bounds: [[4,-4,5], [14,8,19]] };
  const meshes = [mesh], batches = {}, original = new Matrix4().makeTranslation(3,4,5);
  const width = { from: [[0,0],[8,0],[8,7],[0,7]], to: [[0,0],[12,0],[12,7],[0,7]],
    origins: [[3,4,5]], heading: 90 };
  const preview = new PlacementTransforms();
  const apply = (shape: typeof width) => preview.apply(meshes, batches, [
    { owners: ["city/home"], translation: [1,2,0], footprint: shape },
  ]);
  assert.ok(apply(width));
  const sample = new Vector3(0, 8, 0).applyMatrix4(mesh.matrix);
  assert.ok(sample.distanceTo(new Vector3(4,18,5)) < 1e-8);
  assert.equal(mesh.matrixAutoUpdate, false);
  const trap = { ...width, to: [[0,0],[12,0],[9,7],[0,7]] };
  assert.ok(apply(trap));
  assert.equal(mesh.layers.mask, 0);
  assert.equal(preview.hulls.children.length, 0);
  const halves = preview.pieces.children as Mesh[];
  assert.equal(halves.length, 2);
  const corners = new Map<string, Vector3>();
  for (const half of halves) {
    assert.equal(half.material, material);
    const pos = half.geometry.getAttribute("position"), uv = half.geometry.getAttribute("uv");
    for (const vertex of half.geometry.index!.array) {
      const key = [uv.getX(vertex),uv.getY(vertex)].join();
      const point = new Vector3().fromBufferAttribute(pos, vertex).applyMatrix4(half.matrix);
      if (corners.has(key)) assert.ok(point.distanceTo(corners.get(key)!) < 1e-6);
      corners.set(key, point);
    }
  }
  for (const [uv, xyz] of [["0,0",[4,6,19]], ["1,0",[4,18,19]], ["1,1",[-3,15,19]], ["0,1",[-3,6,19]],
    ["0.5,0.5",[0.5,12,19]]] as const) assert.ok(corners.get(uv)!.distanceTo(new Vector3(...xyz)) < 1e-6);
  preview.showRegenerated(new Set([mesh]));
  assert.equal(preview.pieces.visible, false);
  assert.deepEqual(mesh.matrix.elements, original.elements);
  assert.equal(mesh.layers.mask, 4);
  assert.equal(mesh.material, material);
  assert.ok(apply(width));
  assert.ok(halves.every(half => !half.visible));
  assert.equal(mesh.layers.mask, 4);
  assert.equal(preview.clear(), true);
  assert.equal(mesh.matrixAutoUpdate, true);
  assert.deepEqual(mesh.matrix.elements, original.elements);
  assert.equal(preview.hulls.children.length, 0);
  assert.equal(preview.pieces.children.length, 0);
  preview.dispose();
  geometry.dispose(); material.dispose();
});

test("dragging every copy in a batched row leaves its GPU instance buffer untouched", () => {
  const batch = new InstancedMesh(new BoxGeometry(), new MeshBasicMaterial(), 2);
  batch.userData.source = { objectId: "city/lot/1", template: "city/lot/1/body" };
  batch.setMatrixAt(0, new Matrix4().makeTranslation(2, 0, 0));
  batch.setMatrixAt(1, new Matrix4().makeTranslation(17, 0, 0));
  const before = Array.from(batch.instanceMatrix.array), version = batch.instanceMatrix.version;
  const preview = new PlacementTransforms(), meshes = [batch], ids = ["city/lot/1", "city/lot/2"];
  const batches = { "city/lot/1": ids };
  for (const x of [2, 3, 4]) {
    assert.ok(preview.apply(meshes, batches, [{ owners: ids, translation: [x,1,0] }]));
    assert.deepEqual(batch.position.toArray(), [x,1,0]);
    assert.equal(batch.instanceMatrix.version, version);
  }
  preview.clear();
  assert.deepEqual(batch.position.toArray(), [0,0,0]);
  assert.deepEqual(Array.from(batch.instanceMatrix.array), before);
  preview.dispose(); batch.geometry.dispose(); batch.material.dispose(); batch.dispose();
});

test("regenerated provisional geometry uses only the selected visible triangles and preserves packet vertex colors and material groups", async () => {
  const mesh = (objectId: string, start: number, material: string, visible = true) => ({
    objectId, vertexStart: start, indexStart: start, vertexCount: 3, indexCount: 3, visible, material, bounds: [[0,0,0],[1,1,1]],
  });
  const descriptors = [mesh("ground", 0, "a"), mesh("house", 3, "a"), mesh("house", 6, "b"),
    mesh("other", 9, "b"), mesh("house", 12, "a"), mesh("house", 15, "a", false)];
  const packet = { manifest: { meshes: descriptors, materials: [
    { id: "a", color: "#ffffff", baseColorFactor: [.2,.4,.7] }, { id: "b", color: "#ffffff", alphaMode: "BLEND", opacity: .6 },
  ] }, positions: new Float32Array(54), normals: new Float32Array(54), colors: new Float32Array(54),
    indices: Uint32Array.from({ length: 18 }, (_, i) => i % 3) } as unknown as ScenePacket;
  const owner = new Set(["house"]);
  const provisional = await footprintGeometry(packet, owner, () => true);
  assert.ok(provisional);
  const geometry = provisional.geometry;
  assert.deepEqual(geometry.groups, [{ start: 0, count: 6, materialIndex: 0 }, { start: 6, count: 3, materialIndex: 1 }]);
  assert.deepEqual([...geometry.index!.array], [0,1,2,9,10,11,3,4,5]);
  assert.deepEqual(provisional.material[0].color.toArray(), [.2,.4,.7]);
  assert.equal(provisional.material[1].opacity, .6);
  assert.equal(geometry.getAttribute("color").array.buffer, packet.colors.buffer);
  assert.equal(geometry.getAttribute("position").array.byteOffset, 3 * 3 * 4);
  disposeFootprintGeometry(provisional);
  const glass = new MeshBasicMaterial({ transparent: true, opacity: .42 });
  glass.userData.programUniforms = {};
  let released = false;
  glass.addEventListener("dispose", () => { released = true; });
  packet.uvs = Float32Array.from({ length: 36 }, (_, i) => i * .15);
  const textured = await footprintGeometry(packet, owner, () => true, source => source.material === "b" ? glass : undefined);
  assert.ok(textured);
  assert.deepEqual(textured.userData.coverage, { triangles: 3, shaded: 1, retainedAtlas: 0, procedural: 1, texture: 0, flat: 0, fallback: 2 });
  const drawnGlass = textured.children[0] as Mesh;
  assert.equal(drawnGlass.material, glass);
  assert.equal(drawnGlass.geometry.getAttribute("uv").array.buffer, packet.uvs.buffer);
  assert.deepEqual(drawnGlass.geometry.drawRange, { start: 6, count: 3 });
  disposeFootprintGeometry(textured);
  assert.equal(released, false, "discarding a draft must retain the accepted shader");
  const walls = new Map(descriptors.map((record, index) => [record.vertexStart, new Mesh(new BoxGeometry(), glass)]));
  const baked = await footprintGeometry(packet, owner, () => true, source => ({ retained: walls.get(source.vertexStart)! }));
  assert.ok(baked);
  assert.deepEqual([...baked.userData.retained], [walls.get(3), walls.get(6), walls.get(12)]);
  assert.equal(baked.userData.coverage.retainedAtlas, 3);
  assert.equal(baked.geometry.index!.count, 0);
  assert.equal(baked.children.length, 0, "the original draws own the accepted charts");
  let chartDisposed = false;
  for (const wall of walls.values()) wall.geometry.addEventListener("dispose", () => { chartDisposed = true; });
  disposeFootprintGeometry(baked);
  assert.equal(chartDisposed, false);
  for (const wall of walls.values()) wall.geometry.dispose();
  glass.dispose();
  assert.equal(await footprintGeometry(packet, owner, () => false), undefined);
  assert.equal(await footprintGeometry(packet, new Set(["absent"]), () => true), undefined);
  packet.manifest.batches = [{ objectId: "other", template: "other/body" }] as typeof packet.manifest.batches;
  assert.equal(await footprintGeometry(packet, owner, () => true), undefined);
  packet.manifest.batchPlacementOwners = { other: ["house"] };
  assert.equal(await footprintGeometry(packet, owner, () => true), undefined);
});

test("a standalone item shape stretches the last atlas only across matching triangles", async () => {
  const triangle = (height: number, rotation: number, sharedMiddle = 0) => ({
    manifest: { revision: height, meshes: [{ id: "pipe", material: rotation ? "baked-metal" : "metal-slot", visible: true, indexStart: 0, indexCount: 3, vertexStart: 0, bounds: [[0, 0, 0], [1, height, 0]] }] },
    positions: new Float32Array([0, 0, 0, 1, 0, 0, sharedMiddle, height, 0]),
    normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
    colors: new Float32Array(9).fill(1),
    indices: new Uint32Array([0, 1, 2].map(v => (v + rotation) % 3)),
    uvs: new Float32Array([0.25, 0.25, 0.5, 0.25, 0.25, 0.5]), assets: new Uint8Array([1]),
  } as unknown as ScenePacket);
  const previous = triangle(1, 1);
  const shape = triangle(2, 0);
  const preview = await itemShapePreview(shape, previous, () => true);
  assert.equal(preview.assets, previous.assets);
  assert.equal(preview.uvs, previous.uvs);
  assert.equal(preview.indices, previous.indices);
  assert.equal(preview.manifest.revision, 2);
  assert.deepEqual(preview.positions, shape.positions);
  assert.equal(previous.positions[7], 1);
  const tapered = triangle(2, 0, 0.5);
  assert.equal(await itemShapePreview(tapered, previous, () => true), tapered);
  previous.manifest.meshes[0].material = shape.manifest.meshes[0].material = "builtin:trim";
  shape.uvs[3] += 0.125;
  assert.equal(await itemShapePreview(shape, previous, () => true), shape, "the trim keeps its new physical texture scale");
  shape.uvs[3] = previous.uvs[3];
  previous.manifest.meshes[0].vertexCount = 3;
  shape.manifest.meshes[0].vertexCount = 4;
  assert.equal(await itemShapePreview(shape, previous, () => true), shape, "a newly split stock normal keeps its own vertex");
});

test("an outstanding resize cannot reuse the previous finish after a simultaneous material edit", () => {
  const previous = { kind: "item", modules: { "metal.ms": "old source" }, assets: { image: new Uint8Array([10]) }, parameters: {}, mapping: "object" } as unknown as AuthoringProject;
  const resized = { ...previous, parameters: { length: 3 } };
  const parameters: Parameter[] = [{ name: "length", type: "number", default: 2, role: "geometry" }, { name: "wear", type: "number", default: 0, role: "finish" }];
  assert.ok(onlyItemShapeChanged(previous, resized, parameters));
  assert.ok(!onlyItemShapeChanged(previous, { ...resized, modules: { "metal.ms": "new source" } }, parameters));
  assert.ok(!onlyItemShapeChanged(previous, { ...resized, assets: { image: new Uint8Array([11]) } }, parameters));
  assert.ok(!onlyItemShapeChanged(previous, { ...resized, mapping: "uv" }, parameters));
  assert.ok(!onlyItemShapeChanged(previous, { ...resized, parameters: { length: 3, wear: 1 } }, parameters));
});
