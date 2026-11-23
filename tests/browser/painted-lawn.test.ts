import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { cullPaintedStrokes, PAINTED_STROKE_REACH } from "../../adapters/three/src/paintedLawn.ts";

function bed(ranks: number[]) {
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute("lawnStroke", new THREE.InstancedBufferAttribute(new Float32Array(ranks.flatMap(rank => [.2, .1, 3, rank])), 4));
  geometry.instanceCount = ranks.length;
  // Roots within one metre of the bed's origin.
  geometry.boundingBox = new THREE.Box3(new THREE.Vector3(-1, 0, -1), new THREE.Vector3(1, .3, 1));
  return geometry;
}
const drawn = (mesh: THREE.Mesh, geometry: THREE.InstancedBufferGeometry, x: number) => {
  const camera = new THREE.PerspectiveCamera();
  camera.position.set(x, 0, 0);
  camera.updateMatrixWorld(true);
  mesh.updateMatrixWorld(true);
  mesh.onBeforeRender(null as never, null as never, camera, geometry, null as never, null as never);
  const count = geometry.instanceCount;
  mesh.onAfterRender(null as never, null as never, camera, geometry, null as never, null as never);
  return count;
};

test("a bed beyond the strokes' fade submits no brushes and returns when approached", () => {
  // The stroke shader keeps a brush while rank <= 1 - smoothstep(18, 30, distance): none from 30 m.
  assert.equal(PAINTED_STROKE_REACH, 30);
  const geometry = bed([.25, .5, 1.75]), mesh = new THREE.Mesh(geometry);
  cullPaintedStrokes(mesh, geometry);
  // The bed's near edge is at x = 1, so an eye at x = 31 is exactly 30 m away.
  assert.equal(drawn(mesh, geometry, 30.9), 3);
  assert.equal(drawn(mesh, geometry, 31), 0);
  assert.equal(geometry.instanceCount, 3, "other placements of the geometry start from the full count");
  assert.equal(drawn(mesh, geometry, 5), 3);
  // A moved or scaled placement is measured where it now stands.
  mesh.position.x = 20;
  assert.equal(drawn(mesh, geometry, -11), 0);
  mesh.scale.setScalar(4);
  assert.equal(drawn(mesh, geometry, -11), 3);
});

test("a brush the shader never fades keeps its bed submitted", () => {
  const geometry = bed([.25, 2]), mesh = new THREE.Mesh(geometry), before = mesh.onBeforeRender;
  cullPaintedStrokes(mesh, geometry);
  assert.equal(mesh.onBeforeRender, before);
});
