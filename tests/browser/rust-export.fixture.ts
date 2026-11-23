import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { validateBytes } from "gltf-validator";
import { createRenderGltfLoader } from "#render-engine";

const renderer = new THREE.WebGLRenderer({ antialias: true });
renderer.setSize(960,640);
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
document.body.append(renderer.domElement);
const scene = new THREE.Scene();
scene.background = new THREE.Color("#d5d7ce");
scene.add(new THREE.HemisphereLight(0xffffff,0x777768,2.2));
const sun = new THREE.DirectionalLight(0xffefdb,3);
sun.position.set(-3,8,5);scene.add(sun);
const camera = new THREE.PerspectiveCamera(42,1.5,0.01,10000);
let model: THREE.Group | undefined;

/** Releases actual loader-owned resources between format acceptance samples. */
function release(): void {
  if (!model) return;
  const textures = new Set<THREE.Texture>();
  model.traverse(object => {
    if (!(object instanceof THREE.Mesh)) return;
    object.geometry.dispose();
    const materials = Array.isArray(object.material) ? object.material : [object.material];
    for (const material of materials) {
      for (const value of Object.values(material)) if (value instanceof THREE.Texture) textures.add(value);
      material.dispose();
    }
  });
  for (const texture of textures) texture.dispose();
  scene.remove(model);model = undefined;
}

/** Reads decoded pixels rather than trusting the declared GLB channel identifiers. */
function pixel(texture: THREE.Texture | null): number[] | undefined {
  if (!texture || (texture as THREE.CompressedTexture).isCompressedTexture) return undefined;
  const canvas = document.createElement("canvas");canvas.width=canvas.height=1;
  const context = canvas.getContext("2d")!;
  context.drawImage(texture.image as CanvasImageSource,0,0);
  return Array.from(context.getImageData(0,0,1,1).data);
}

/** Validates native bytes, loads them through Three's production glTF parser, then renders. */
async function load(url: string, ktx2 = false) {
  const bytes = await (await fetch(url)).arrayBuffer();
  const report = await validateBytes(new Uint8Array(bytes), { maxIssues: 100 });
  // SuperSpec exports store KTX2 textures by default.
  const result = await (ktx2 ? createRenderGltfLoader(renderer, { transcoderPath: "basis/" }) : new GLTFLoader()).parseAsync(bytes, "");
  release();model = result.scene;scene.add(model);model.updateMatrixWorld(true);
  const bounds = new THREE.Box3().setFromObject(model);
  const center = bounds.getCenter(new THREE.Vector3());
  const radius = Math.max(1,bounds.getSize(new THREE.Vector3()).length());
  camera.position.copy(center).add(new THREE.Vector3(.8,.9,1.2).multiplyScalar(radius));
  camera.lookAt(center);renderer.render(scene,camera);
  const meshes: Record<string, unknown> = {};
  model.traverse(object => {
    if (!(object instanceof THREE.Mesh)) return;
    const positions = object.geometry.getAttribute("position");
    const world: number[] = [];
    for (let i=0;i<positions.count;i++) world.push(...new THREE.Vector3().fromBufferAttribute(positions,i).applyMatrix4(object.matrixWorld).toArray());
    const material = object.material as THREE.MeshStandardMaterial;
    meshes[String(object.userData.meshId)] = {
      objectId: object.userData.objectId,
      world, indices: Array.from(object.geometry.index!.array),
      color: material.color.toArray(), emissive: material.emissive.toArray(),emissiveIntensity:material.emissiveIntensity,
      colorPixel:pixel(material.map),normalPixel:pixel(material.normalMap),dataPixel:pixel(material.roughnessMap),
      colorSpace:material.map?.colorSpace,normalColorSpace:material.normalMap?.colorSpace,
      dataColorSpace:material.roughnessMap?.colorSpace,normalScale:material.normalScale.toArray(),
      hasTangents:object.geometry.hasAttribute("tangent"),opacity:material.opacity,alphaTest:material.alphaTest,
      source:object.userData.source,
      compressed:[material.map,material.normalMap,material.roughnessMap].map(map=>Boolean((map as THREE.CompressedTexture | null)?.isCompressedTexture)),
    };
  });
  return {report,meshes,bounds:[bounds.min.toArray(),bounds.max.toArray()],memory:{...renderer.info.memory}};
}
Object.assign(window,{exportDriver:{load}});
window.addEventListener("pagehide",()=>{release();renderer.dispose();},{once:true});
