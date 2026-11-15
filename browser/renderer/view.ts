import * as THREE from "three";

export interface SceneView {
  position: [number, number, number];
  target: [number, number, number];
  projection: "perspective" | "orthographic";
  fov: number;
  height: number;
  roll: number;
  fixed: boolean;
  wireframe: boolean;
}

/** Parses reproducible camera settings in domain Z-up coordinates. */
export function parseView(parameters: URLSearchParams): SceneView {
  const vector = (key: string, fallback: [number, number, number]): [number, number, number] => {
    const raw = parameters.get(key);
    if (raw === null) return fallback;
    const values = raw.trim().split(/[\s,]+/).map(Number);
    if (values.length !== 3 || values.some(value => !Number.isFinite(value))) throw new Error(`${key} requires three finite coordinates.`);
    return values as [number, number, number];
  };
  const number = (key: string, fallback: number): number => {
    const raw = parameters.get(key);
    const value = raw === null ? fallback : Number(raw);
    if (!Number.isFinite(value)) throw new Error(`${key} must be finite.`);
    return value;
  };
  const projection = parameters.get("projection") ?? "perspective";
  if (projection !== "perspective" && projection !== "orthographic") throw new Error("Unknown camera projection.");
  const position = vector("camera", [11, -12, 10]);
  const target = vector("target", [3, 1, 1]);
  if (position.every((value, axis) => value === target[axis])) throw new Error("Camera and target must differ.");
  const fov = number("fov", 42);
  const height = number("view-height", 20);
  if (fov <= 0 || fov >= 180 || height <= 0) throw new Error("Camera field of view and height are outside their supported range.");
  return { position, target, projection, fov, height, roll: number("roll", 0), fixed: parameters.has("camera") || parameters.has("target"), wireframe: parameters.get("appearance") === "wireframe" };
}

/** Creates the selected Three.js camera while preserving metre units and Z-up orientation. */
export function viewCamera(view: SceneView): THREE.PerspectiveCamera | THREE.OrthographicCamera {
  const camera = view.projection === "perspective"
    ? new THREE.PerspectiveCamera(view.fov, 1, 0.01, 1000)
    : new THREE.OrthographicCamera(-view.height / 2, view.height / 2, view.height / 2, -view.height / 2, 0.01, 1000);
  camera.up.set(0, 0, 1);
  camera.position.fromArray(view.position);
  const forward = new THREE.Vector3().fromArray(view.target).sub(camera.position).normalize();
  camera.up.applyAxisAngle(forward, THREE.MathUtils.degToRad(view.roll));
  return camera;
}
