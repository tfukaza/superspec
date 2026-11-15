import * as THREE from "three";
import { RustScene } from "../../browser/renderer/scene.ts";
import { decodePacket } from "../../browser/model/packet.ts";

const host = document.querySelector<HTMLElement>("#viewport")!;
const scene = new RustScene(host, () => {});
// This fixture bridge observes the actual renderer boundary without adding a production test API.
const renderer = (scene as unknown as { renderer: THREE.WebGLRenderer }).renderer;
const originalRender = renderer.render;
let rendered: THREE.Scene;
renderer.render = function(scene, camera): void {
  rendered = scene as THREE.Scene;
  originalRender.call(this, scene, camera);
};
let revision = 0;
let pending: Promise<boolean> | undefined;
let release: ((fail: boolean) => void) | undefined;
const imageSource = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, "src")!;

/** Holds the next actual browser image decode, optionally rejecting it with an invalid image. */
function gate(): void {
  Object.defineProperty(HTMLImageElement.prototype, "src", {
    ...imageSource,
    set(this: HTMLImageElement, value: string) {
      Object.defineProperty(HTMLImageElement.prototype, "src", imageSource);
      release = fail => imageSource.set!.call(this, fail ? "data:image/png;base64,bm90LWFuLWltYWdl" : value);
    },
  });
}

/** Captures publicly observable Three material settings and rendered resource counts. */
function snapshot() {
  /** Reads the decoded image itself, so swapping channel references cannot pass on state alone. */
  const firstPixel = (texture: THREE.Texture | null): number[] | undefined => {
    if (!texture) return undefined;
    const canvas = document.createElement("canvas"); canvas.width = canvas.height = 1;
    const context = canvas.getContext("2d")!;
    context.drawImage(texture.image as HTMLImageElement, 0, 0);
    return [...context.getImageData(0, 0, 1, 1).data];
  };
  const materials: Record<string, unknown> = {};
  rendered.traverse(object => {
    if (!(object instanceof THREE.Mesh) || !object.visible) return;
    const material = object.material as THREE.MeshStandardMaterial;
    materials[object.name] = {
      vertexColors: material.vertexColors,
      vertexColorSize: object.geometry.getAttribute("color").itemSize,
      vertexColorCount: object.geometry.getAttribute("color").count,
      positionCount: object.geometry.getAttribute("position").count,
      uvCount: object.geometry.getAttribute("uv").count,
      uvData: Array.from(object.geometry.getAttribute("uv").array),
      vertexColorsData: Array.from(object.geometry.getAttribute("color").array),
      materialColor: material.color.toArray(),
      colorSpace: material.map?.colorSpace,
      colorPixel: firstPixel(material.map), normalPixel: firstPixel(material.normalMap), dataPixel: firstPixel(material.roughnessMap),
      normalColorSpace: material.normalMap?.colorSpace,
      dataColorSpace: material.roughnessMap?.colorSpace,
      sharedDataMap: material.roughnessMap === material.metalnessMap,
      repeatMaps: [material.map, material.normalMap, material.roughnessMap].every(texture =>
        texture?.wrapS === THREE.RepeatWrapping && texture.wrapT === THREE.RepeatWrapping),
      distinctMaps: material.map !== material.normalMap && material.normalMap !== material.roughnessMap,
      normalScale: material.normalScale.toArray(),
      bump: material.bumpMap !== null,
      opacity: material.opacity, transparent: material.transparent, depthWrite: material.depthWrite,
      side: material.side, alphaTest: material.alphaTest, alphaToCoverage: material.alphaToCoverage,
      polygonOffset: material.polygonOffset, polygonOffsetFactor: material.polygonOffsetFactor, polygonOffsetUnits: material.polygonOffsetUnits,
    };
  });
  return { materials, reflectionLighting: rendered.environment !== null, meshes: host.dataset.meshes, revision: host.dataset.revision,
    canvases: host.querySelectorAll("canvas").length, memory: { ...renderer.info.memory } };
}

/** Exercises only the adapter's public lifecycle; packet bytes come from the actual native compiler. */
const driver = {
  async apply(bytes: ArrayLike<number>) {
    const packet = decodePacket(Uint8Array.from(bytes).buffer);
    const ownRevision = ++revision;
    packet.manifest.revision = ownRevision;
    return scene.apply(packet, () => ownRevision === revision);
  },
  start(bytes: ArrayLike<number>) { gate(); pending = this.apply(bytes); pending.catch(() => {}); },
  gated: () => release !== undefined,
  async release(fail = false) {
    const done = pending!;
    release!(fail); release = undefined;
    try { return { applied: await done, error: false }; } catch { return { applied: false, error: true }; }
  },
  snapshot,
  focus(name?: string, direction: [number, number, number] = [0.25, -1.5, 0.2]) {
    const bounds = new THREE.Box3();
    if (name) bounds.setFromObject(rendered.getObjectByName(name)!);
    else rendered.traverse(object => {
      if (object instanceof THREE.Mesh && object.visible) bounds.expandByObject(object);
    });
    const center = bounds.getCenter(new THREE.Vector3());
    const view = scene as unknown as { camera: THREE.PerspectiveCamera; controls: { target: THREE.Vector3; update(): void } };
    view.controls.target.copy(center);
    view.camera.position.copy(center).add(new THREE.Vector3(...direction).multiplyScalar(bounds.getSize(new THREE.Vector3()).length()));
    view.controls.update();
    renderer.render(rendered, view.camera);
    return { center: center.toArray(), camera: view.camera.position.toArray() };
  },
  close() { scene.dispose(); },
};
Object.assign(window, { driver });
window.addEventListener("pagehide", () => {
  scene.dispose();
  renderer.render = originalRender;
  Object.defineProperty(HTMLImageElement.prototype, "src", imageSource);
}, { once: true });
