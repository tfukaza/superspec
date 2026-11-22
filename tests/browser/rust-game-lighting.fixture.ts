/**
 * A minimal three.js "game": loads a SuperSpec GLB with three's own GLTFLoader into a Y-up scene and lights it from
 * the record the file carries, through the game runtime module. `tests/lighting-washroom-browser.mjs` compares its
 * picture with the Studio's view of the same project.
 */
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { GameLighting, gltfLighting } from "../../browser/renderer/game-lighting.ts";

interface View {
  /** Canvas size in CSS pixels. */
  width: number; height: number;
  /** The Studio's camera, in its Z-up coordinates. */
  position: [number, number, number]; target: [number, number, number]; fov: number;
  /** Session overrides in page-parameter form, as the Studio page had them. */
  overrides?: string;
}

let current: { renderer: THREE.WebGLRenderer; lighting: GameLighting; scene: THREE.Scene; camera: THREE.Camera } | undefined;

/**
 * Draws a mask of the shown scene instead of its picture: white where a mesh not named in `ids` (by the exporter's
 * `meshId`) is seen, black on the named meshes and the backdrop. `show` again draws the picture.
 */
function mask(ids: string[]): void {
  if (!current) throw new Error("Nothing is shown.");
  const { renderer, scene, camera } = current;
  const kept = new Map<THREE.Mesh, THREE.Material | THREE.Material[]>();
  const white = new THREE.MeshBasicMaterial({ color: 0xffffff, toneMapped: false, side: THREE.DoubleSide });
  const black = new THREE.MeshBasicMaterial({ color: 0x000000, toneMapped: false, side: THREE.DoubleSide });
  scene.traverse(object => {
    const mesh = object as THREE.Mesh;
    if (!mesh.isMesh) return;
    kept.set(mesh, mesh.material);
    mesh.material = ids.includes(String(mesh.userData.meshId)) ? black : white;
  });
  const background = scene.background;
  scene.background = new THREE.Color(0x000000);
  renderer.render(scene, camera);
  scene.background = background;
  for (const [mesh, material] of kept) mesh.material = material;
  white.dispose(); black.dispose();
}

/** The domain's Z-up coordinates in the loaded scene's Y-up world, as the exporter's root node turns them. */
const yUp = ([x, y, z]: [number, number, number]): [number, number, number] => [x, z, -y];

async function show(url: string, view: View) {
  current?.lighting.dispose(); current?.renderer.dispose(); current?.renderer.domElement.remove();
  // The host's own choices, the same as the Studio viewer's: an antialiased sRGB canvas on its backdrop color.
  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.setSize(view.width, view.height);
  document.body.append(renderer.domElement);
  const scene = new THREE.Scene();
  scene.background = new THREE.Color("#d5d7ce");
  const camera = new THREE.PerspectiveCamera(view.fov, view.width / view.height, 0.01, 1000);
  camera.position.fromArray(yUp(view.position));
  camera.lookAt(new THREE.Vector3().fromArray(yUp(view.target)));

  const gltf = await new GLTFLoader().loadAsync(url);
  scene.add(gltf.scene);
  const punctual: { type: string; intensity: number; travel: number[] }[] = [];
  gltf.scene.updateMatrixWorld(true);
  gltf.scene.traverse(object => {
    const light = object as THREE.DirectionalLight;
    if (!light.isDirectionalLight) return;
    // Where the file's own light shines, as three.js reads it: from the light toward its target.
    const from = light.getWorldPosition(new THREE.Vector3()), to = light.target.getWorldPosition(new THREE.Vector3());
    punctual.push({ type: light.type, intensity: light.intensity, travel: to.sub(from).normalize().toArray() });
  });
  const stated = gltfLighting(gltf);
  const lighting = new GameLighting(renderer, scene, camera, stated, { overrides: new URLSearchParams(view.overrides ?? "") });
  lighting.adopt(gltf.scene);
  current = { renderer, lighting, scene, camera };
  // Programs compile at the first draw; the second is the picture.
  lighting.render();
  await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
  lighting.render();
  let lights = 0, meshes = 0, styled = 0;
  scene.traverseVisible(object => {
    if ((object as THREE.Light).isLight && camera.layers.test(object.layers)) lights++;
    if ((object as THREE.Mesh).isMesh) { meshes++; if (object.castShadow && object.receiveShadow) styled++; }
  });
  return { stated, lighting: lighting.lighting, punctual, lights, meshes, shadowed: styled, ambientOcclusion: lighting.view.ambientOcclusion !== undefined,
    styledShadows: lighting.view.styledShadows };
}

Object.assign(window, { gameLighting: { show, mask } });
