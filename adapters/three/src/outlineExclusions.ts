import * as THREE from "three";
import { compileWorldStage, warmupMesh } from "./worldWarmup.ts";

// Instancing, spatial clustering and LOD subsets copy the source mesh's layers.
const excludedLayer = 29;
const depthSafe = new WeakMap<THREE.Material, THREE.Material["onBeforeCompile"]>();

/** The scene binder admits only ordinary glTF surfaces before adding its known
 * color/lighting operations. Later shader edits fall back to the full material. */
export function registerOutlineDepth(material: THREE.Material): void {
  depthSafe.set(material, material.onBeforeCompile);
}

function ordinaryDepth(material: THREE.Material): boolean {
  return material instanceof THREE.MeshStandardMaterial && depthSafe.get(material) === material.onBeforeCompile
    && !material.transparent && material.alphaTest === 0 && !material.alphaHash && !material.alphaToCoverage
    && !material.displacementMap && !material.wireframe && !material.userData.grassField
    && material.userData.paintedStroke === undefined;
}

export function outlineDepthMaterial(material: THREE.Material,
  cache: Map<THREE.Material, THREE.MeshDepthMaterial>): THREE.Material {
  if (!ordinaryDepth(material)) return material;
  let depth = cache.get(material);
  if (!depth) {
    depth = new THREE.MeshDepthMaterial();
    cache.set(material, depth);
  }
  // Preserve raster state without copying the material's large recipe metadata.
  Object.assign(depth, {
    colorWrite: false, side: material.side, visible: material.visible,
    depthTest: material.depthTest, depthWrite: material.depthWrite, depthFunc: material.depthFunc,
    polygonOffset: material.polygonOffset, polygonOffsetFactor: material.polygonOffsetFactor,
    polygonOffsetUnits: material.polygonOffsetUnits,
    clippingPlanes: material.clippingPlanes, clipIntersection: material.clipIntersection,
    stencilWrite: material.stencilWrite, stencilWriteMask: material.stencilWriteMask,
    stencilFunc: material.stencilFunc, stencilRef: material.stencilRef, stencilFuncMask: material.stencilFuncMask,
    stencilFail: material.stencilFail, stencilZFail: material.stencilZFail, stencilZPass: material.stencilZPass,
  });
  if (depth.userData.sourceVersion !== material.version) {
    depth.userData.sourceVersion = material.version;
    depth.needsUpdate = true;
  }
  return depth;
}

/** outlineSizeFade is zero beyond a five-centimetre pixel footprint. A plane's
 * largest tangent footprint is at least the smaller unprojected pixel size. */
export function outlineDepthLimit(camera: THREE.PerspectiveCamera, width: number, height: number): number {
  const p = camera.projectionMatrix.elements;
  return 0.025 * Math.max(width * Math.abs(p[0]), height * Math.abs(p[5]));
}

function pastOutlineDepth(mesh: THREE.Mesh, camera: THREE.Camera, limit: number, matrix: THREE.Matrix4): boolean {
  if (mesh.children.length || mesh instanceof THREE.SkinnedMesh || mesh.geometry.morphAttributes.position?.length) return false;
  const materials = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
  if (!materials.every(ordinaryDepth)) return false;
  const source = mesh instanceof THREE.InstancedMesh ? mesh : mesh.geometry;
  if (!source.boundingBox) source.computeBoundingBox();
  const box = source.boundingBox;
  if (!box || box.isEmpty()) return false;
  const m = matrix.multiplyMatrices(camera.matrixWorldInverse, mesh.matrixWorld).elements;
  // Exact nearest camera depth of the transformed local box, including shear.
  const depth = -m[14] - (m[2] * (box.min.x + box.max.x) + m[6] * (box.min.y + box.max.y)
    + m[10] * (box.min.z + box.max.z)) * 0.5;
  const extent = (Math.abs(m[2]) * (box.max.x - box.min.x) + Math.abs(m[6]) * (box.max.y - box.min.y)
    + Math.abs(m[10]) * (box.max.z - box.min.z)) * 0.5;
  return depth - extent > limit;
}

export function markOutlineExclusions(root: THREE.Object3D): boolean {
  let found = false;
  root.traverse(object => {
    if (!(object instanceof THREE.Mesh)) return;
    if (object.userData.worldSpecOutline !== false && object.geometry.userData.worldSpecOutline !== false) return;
    object.layers.enable(excludedLayer);
    found = true;
  });
  return found;
}

/** The exclusion pass changes camera layers. Its real-material fallback for
 * cutouts and decals therefore sees different lights than the world draw. */
export async function warmExcludedDepth(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera,
  map: THREE.Object3D, target: THREE.WebGLRenderTarget, depthMaterials?: Map<THREE.Material, THREE.MeshDepthMaterial>) {
  const stage = new THREE.Group(), view = camera.clone();
  view.layers.set(excludedLayer);
  map.traverse(object => {
    if (!(object instanceof THREE.Mesh) || !object.layers.test(view.layers)) return;
    let material = object.material;
    if (depthMaterials) material = Array.isArray(material)
      ? material.map(slot => outlineDepthMaterial(slot, depthMaterials)) : outlineDepthMaterial(material, depthMaterials);
    stage.add(warmupMesh(object, material));
  });
  if (stage.children.length) await compileWorldStage(renderer, stage, view, scene, target);
}

/** Three resolves the world color target before calling Scene.onAfterRender.
 * At that point a nested draw gets its own light state. The exclusion camera
 * can filter out world lights without invalidating every lit color program
 * on the next frame. Both passes use the host's scene and object transforms. */
export function renderColorAndExclusions(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera,
  drawExclusions: () => void): void {
  const originalBefore = scene.onBeforeRender, originalAfter = scene.onAfterRender;
  if (originalBefore !== THREE.Object3D.prototype.onBeforeRender || originalAfter !== THREE.Object3D.prototype.onAfterRender
    || renderer.xr?.isPresenting) {
    renderer.render(scene, camera);
    drawExclusions();
    return;
  }
  let depth = 0, drawn = false;
  let failure: { error: unknown } | undefined;
  // Reflections and other host effects can render this scene again while
  // the color pass is open. Wait for the outer color draw to finish.
  const before = () => { depth++; };
  const after = () => {
    if (--depth !== 0) return;
    restore();
    drawn = true;
    // Let the outer renderer finish releasing its state on failure.
    try { drawExclusions(); } catch (error) { failure = { error }; }
  };
  function restore() {
    if (scene.onBeforeRender === before) scene.onBeforeRender = originalBefore;
    if (scene.onAfterRender === after) scene.onAfterRender = originalAfter;
  }
  scene.onBeforeRender = before;
  scene.onAfterRender = after;
  try { renderer.render(scene, camera); } finally { restore(); }
  if (failure) throw failure.error;
  // A host renderer can omit scene hooks or replace them during the draw.
  if (!drawn) drawExclusions();
}

/** The real materials preserve cutouts and displacement. Comparing this depth
 * with the city depth masks only an excluded surface visible in the foreground. */
export function renderExcludedDepth(renderer: THREE.WebGLRenderer, scene: THREE.Scene,
  camera: THREE.Camera, target: THREE.WebGLRenderTarget,
  depthMaterials?: Map<THREE.Material, THREE.MeshDepthMaterial>, maximumDepth = Infinity): void {
  const layers = camera.layers.mask, background = scene.background;
  const { autoUpdate, needsUpdate } = renderer.shadowMap;
  const originals: [THREE.Mesh, THREE.Material | THREE.Material[]][] = [];
  const distant: THREE.Mesh[] = [];
  const matrix = new THREE.Matrix4();
  try {
    camera.layers.set(excludedLayer);
    if (depthMaterials || Number.isFinite(maximumDepth)) scene.traverseVisible(object => {
      if (!(object instanceof THREE.Mesh) || !object.layers.test(camera.layers)) return;
      if (Number.isFinite(maximumDepth) && pastOutlineDepth(object, camera, maximumDepth, matrix)) {
        distant.push(object);
        object.visible = false;
        return;
      }
      if (!depthMaterials) return;
      originals.push([object, object.material]);
      object.material = Array.isArray(object.material)
        ? object.material.map(material => outlineDepthMaterial(material, depthMaterials))
        : outlineDepthMaterial(object.material, depthMaterials);
    });
    scene.background = null;
    renderer.shadowMap.autoUpdate = renderer.shadowMap.needsUpdate = false;
    renderer.setRenderTarget(target);
    renderer.clearDepth();
    renderer.render(scene, camera);
  } finally {
    for (const mesh of distant) mesh.visible = true;
    for (const [mesh, material] of originals) mesh.material = material;
    renderer.shadowMap.autoUpdate = autoUpdate;
    renderer.shadowMap.needsUpdate = needsUpdate;
    camera.layers.mask = layers;
    scene.background = background;
  }
}
