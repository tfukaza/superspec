/**
 * The illustrated tree lab (tests/anime-tree-browser.mjs drives it): the lane's plants (`lane-plants.json`, written by
 * `treespec_batches::write_lane_plants`) alone on a plain ground under the avenue view's sun, their bark as written and
 * their masses drawn in one of the looks of browser/renderer/anime-tree.ts (or `painted`, the painted mass), with the
 * sun's shadow map, for quick pictures, orbits and frame times without the Studio or a bake.
 */
import * as THREE from "three";
import type { FoliageMass } from "../../../browser/model/packet.ts";
import { animeTree, type AnimeTreeLook } from "../../../browser/renderer/anime-tree.ts";
import { paintedMass } from "../../../browser/renderer/painted-mass.ts";
import { programSun } from "../../../browser/renderer/shade.ts";

interface Batch { id: string; mesh: { positions: number[]; normals: number[]; indices: number[] }; instances: { translation: number[]; rotation: number[]; scale: number[] }[];
  foliage: { family: string; patch: string; level: number; mass?: FoliageMass } }

const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
renderer.setPixelRatio(1);
renderer.setSize(innerWidth, innerHeight);
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;
document.body.append(renderer.domElement);
const scene = new THREE.Scene();
scene.background = new THREE.Color("#d2ccc4");
const camera = new THREE.PerspectiveCamera(30, innerWidth / innerHeight, 0.1, 400);
camera.up.set(0, 0, 1);
const time = { value: 0 };
const sunLight = new THREE.DirectionalLight(new THREE.Color(1, 0.9, 0.77), 2.2);
sunLight.castShadow = true;
sunLight.shadow.mapSize.set(4096, 4096);
sunLight.shadow.bias = -0.0006;
sunLight.shadow.normalBias = 0.03;
scene.add(sunLight, sunLight.target, new THREE.HemisphereLight(new THREE.Color(1, 0.98, 0.95), new THREE.Color(0.42, 0.4, 0.35), 2.2));
const ground = new THREE.Mesh(new THREE.PlaneGeometry(200, 200), new THREE.MeshLambertMaterial({ color: "#8f8b82" }));
ground.receiveShadow = true;
scene.add(ground);
// A plain wall behind the row, where the building's side stands in the lane.
const wall = new THREE.Mesh(new THREE.BoxGeometry(60, 0.4, 16), new THREE.MeshLambertMaterial({ color: "#c2ae82" }));
wall.receiveShadow = true;
scene.add(wall);
const plants = new THREE.Group();
scene.add(plants);

let batches: Batch[] = [];
let shown: { patch: string; centre: THREE.Vector3; base: THREE.Vector3; height: number }[] = [];
const placement = (batch: Batch) => { const i = batch.instances[0]; return new THREE.Matrix4().compose(new THREE.Vector3(...i.translation), new THREE.Quaternion(...i.rotation), new THREE.Vector3(...i.scale)); };

function setSun(azimuth: number, elevation: number): void {
  const az = azimuth * Math.PI / 180, el = elevation * Math.PI / 180;
  programSun.value.set(Math.cos(el) * Math.cos(az), Math.cos(el) * Math.sin(az), Math.sin(el));
  const middle = shown.reduce((sum, plant) => sum.add(plant.centre), new THREE.Vector3()).multiplyScalar(1 / Math.max(1, shown.length));
  sunLight.target.position.copy(middle);
  sunLight.position.copy(middle).addScaledVector(programSun.value, 60);
  const reach = 16 + 4 * shown.length;
  Object.assign(sunLight.shadow.camera, { left: -reach, right: reach, top: reach, bottom: -reach, near: 1, far: 140 });
  sunLight.shadow.camera.updateProjectionMatrix();
}

const lab = {
  renderer, scene, camera, time,
  async load(url: string): Promise<number> { batches = (await (await fetch(url)).json()).batches; return batches.length; },
  /** Draws the plants `indexes` (their patches are `lane-plant-<index>`) in `look`; returns what was built and how long it took. */
  show(indexes: number[], look: AnimeTreeLook | "painted" | "bare", sun: [number, number] = [135, 40]) {
    for (const child of [...plants.children]) { plants.remove(child); child.traverse(node => { const mesh = node as THREE.Mesh; mesh.geometry?.dispose(); }); }
    shown = [];
    const built: Record<string, unknown>[] = [];
    let ms = 0;
    for (const index of indexes) {
      const patch = `lane-plant-${index}`, mine = batches.filter(batch => batch.foliage.patch === patch);
      const bark = mine.filter(batch => batch.foliage.family === "bark").sort((a, b) => a.foliage.level - b.foliage.level)[0];
      const carrier = mine.find(batch => batch.foliage.mass);
      if (!bark || !carrier) throw new Error(`No plant ${patch}.`);
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute("position", new THREE.Float32BufferAttribute(bark.mesh.positions, 3));
      geometry.setAttribute("normal", new THREE.Float32BufferAttribute(bark.mesh.normals, 3));
      geometry.setIndex(bark.mesh.indices);
      const trunk = new THREE.Mesh(geometry, new THREE.MeshLambertMaterial({ color: "#6a5a48" }));
      trunk.applyMatrix4(placement(bark));
      trunk.castShadow = trunk.receiveShadow = true;
      trunk.name = `${patch}/bark`;
      plants.add(trunk);
      const mass = carrier.foliage.mass!, at = placement(carrier);
      const started = performance.now();
      const parts = look === "bare" ? [] : look === "painted" ? paintedMass(mass, at) : animeTree(mass, at, look, programSun, time);
      ms += performance.now() - started;
      for (const part of parts) {
        part.userData.foliage = true;
        (part.userData.animeTree?.bake as ((renderer: THREE.WebGLRenderer) => void) | undefined)?.(renderer);
        plants.add(part);
        built.push({ name: part.name, ...(part.userData.animeTree ?? {}), bake: undefined });
      }
      const box = new THREE.Box3();
      for (const twig of mass.twigs) for (const [x, y, z] of twig) box.expandByPoint(new THREE.Vector3(x, y, z).applyMatrix4(at));
      const base = new THREE.Vector3().setFromMatrixPosition(at);
      shown.push({ patch, centre: box.getCenter(new THREE.Vector3()), base, height: box.max.z - base.z });
    }
    const floor = Math.min(...shown.map(plant => plant.base.z));
    ground.position.set(shown[0].base.x, shown[0].base.y, floor);
    wall.position.set(shown[0].base.x, shown[0].base.y - 5.5, floor + 8);
    setSun(...sun);
    return { built, ms, plants: shown.map(plant => ({ patch: plant.patch, centre: plant.centre.toArray(), base: plant.base.toArray(), height: plant.height })) };
  },
  setSun,
  view(position: number[], target: number[], fov = 30): void {
    camera.fov = fov; camera.aspect = innerWidth / innerHeight; camera.updateProjectionMatrix();
    camera.position.fromArray(position); camera.lookAt(new THREE.Vector3().fromArray(target));
    renderer.render(scene, camera);
  },
  wall(visible: boolean): void { wall.visible = visible; },
  /** Median milliseconds a frame over `frames` draws (each finished by reading a pixel back), and what a frame draws. */
  measure(frames = 60, shadows = true) {
    const gl = renderer.getContext(), pixel = new Uint8Array(4), runs: number[] = [];
    renderer.shadowMap.autoUpdate = shadows;
    for (let i = 0; i < frames + 8; i++) {
      const start = performance.now();
      renderer.render(scene, camera);
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
      if (i >= 8) runs.push(performance.now() - start);
    }
    renderer.shadowMap.autoUpdate = true;
    runs.sort((a, b) => a - b);
    return { ms: runs[runs.length >> 1], calls: renderer.info.render.calls, triangles: renderer.info.render.triangles };
  },
};
Object.assign(window, { lab });
document.body.dataset.state = "ready";
