import * as THREE from "three";

export type AuthoredLodHint = { kind: "distance" | "coverage"; values: readonly number[] };
const REFERENCE_HEIGHT = 1080, REFERENCE_FOCAL = 1 / Math.tan(35 * Math.PI / 180);

/** Shared viewport-aware policy for authored whole-object alternatives. The
 * bounds are the canonical object's full world-space bounds, not the last LOD. */
export function selectAuthoredLod(box: THREE.Box3, camera: THREE.Camera, height: number,
  hint: AuthoredLodHint, previous = 0, hysteresis = .15): number {
  if (!(height > 0) || !Number.isFinite(height) || box.isEmpty()
    || !(camera instanceof THREE.PerspectiveCamera || camera instanceof THREE.OrthographicCamera)) return 0;
  const values = hint.values;
  let level = Math.min(values.length - 1, Math.max(0, previous));
  const gap = Math.min(.45, Math.max(0, hysteresis));
  if (hint.kind === "distance") {
    const scale = height * Math.abs(camera.projectionMatrix.elements[5]) / (REFERENCE_HEIGHT * REFERENCE_FOCAL);
    const eye = new THREE.Vector3().setFromMatrixPosition(camera.matrixWorld);
    const distance = camera instanceof THREE.PerspectiveCamera ? box.distanceToPoint(eye) / scale : 1 / scale;
    while (level + 1 < values.length && distance >= values[level + 1] * (1 + gap)) level++;
    while (level > 0 && distance < values[level] * (1 - gap)) level--;
  } else {
    const projection = new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    const point = new THREE.Vector3(), view = new THREE.Vector3();
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity, coverage = Infinity;
    for (let i = 0; i < 8; i++) {
      point.set(i & 1 ? box.max.x : box.min.x, i & 2 ? box.max.y : box.min.y, i & 4 ? box.max.z : box.min.z);
      if (camera instanceof THREE.PerspectiveCamera && -view.copy(point).applyMatrix4(camera.matrixWorldInverse).z <= camera.near) break;
      point.applyMatrix4(projection);
      minX = Math.min(minX, point.x); maxX = Math.max(maxX, point.x);
      minY = Math.min(minY, point.y); maxY = Math.max(maxY, point.y);
      if (i === 7) {
        const aspect = Math.abs(camera.projectionMatrix.elements[5] / camera.projectionMatrix.elements[0]);
        coverage = Math.max((maxX - minX) * aspect, maxY - minY) * .5 * height / REFERENCE_HEIGHT;
      }
    }
    while (level + 1 < values.length && coverage < values[level] * (1 - gap)) level++;
    while (level > 0 && coverage >= values[level - 1] * (1 + gap)) level--;
  }
  return level;
}
