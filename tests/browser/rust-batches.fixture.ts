import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { validateBytes } from "gltf-validator";
import { RustScene } from "../../browser/renderer/scene.ts";
import { decodePacket } from "../../browser/model/packet.ts";
import { SceneUpdates } from "../../browser/model/updates.ts";
import { libraryAsset } from "../../browser/runtime/library.ts";

/** Drives the viewer and three.js's stock glTF loader over instanced batches the Rust exporter wrote. */
const host = document.querySelector<HTMLElement>("#viewport")!;
const picked: { id?: string; instance?: number }[] = [];
const scene = new RustScene(host, (mesh, instance) => picked.push({ id: mesh?.id, instance }));
const view = scene as unknown as { renderer: THREE.WebGLRenderer; camera: THREE.PerspectiveCamera; models: THREE.Group;
  controls: { target: THREE.Vector3; update(): void } };

const driver = {
  /** Shows a packet and reports how its batches are drawn. */
  async apply(bytes: ArrayLike<number>, update = false) {
    const buffer = Uint8Array.from(bytes).buffer;
    const packet = update ? new SceneUpdates().decode(buffer) : decodePacket(buffer);
    await scene.apply(packet, () => true, true);
    return view.models.children.map(object => {
      const mesh = object as THREE.InstancedMesh;
      const instanced = mesh.isInstancedMesh === true;
      const material = mesh.material as THREE.MeshStandardMaterial;
      return {
        name: mesh.name, instanced, count: instanced ? mesh.count : 1,
        bounds: instanced ? [mesh.boundingBox!.min.toArray(), mesh.boundingBox!.max.toArray()] : null,
        sphere: instanced ? [...mesh.boundingSphere!.center.toArray(), mesh.boundingSphere!.radius] : null,
        castShadow: mesh.castShadow, receiveShadow: mesh.receiveShadow, frustumCulled: mesh.frustumCulled,
        tints: instanced && mesh.instanceColor ? Array.from(mesh.instanceColor.array) : null,
        values: mesh.geometry.hasAttribute("msInstance") ? Array.from(mesh.geometry.getAttribute("msInstance").array) : null,
        valuesInstanced: (mesh.geometry.getAttribute("msInstance") as THREE.InstancedBufferAttribute | undefined)?.isInstancedBufferAttribute === true,
        roughness: material.roughness, metalness: material.metalness, vertices: mesh.geometry.getAttribute("position").count,
        // The second instance's matrix, column-major.
        matrix: instanced ? Array.from(mesh.instanceMatrix.array.slice(16, 32)) : null,
      };
    });
  },
  /** Whether the batch is inside the view frustum of a camera at `position` looking at `target`. */
  culled(name: string, position: number[], target: number[]) {
    const camera = new THREE.PerspectiveCamera(30, 1.5, 0.01, 1000);
    camera.up.set(0, 0, 1);
    camera.position.fromArray(position); camera.lookAt(new THREE.Vector3().fromArray(target));
    camera.updateMatrixWorld(true);
    const frustum = new THREE.Frustum().setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    return !frustum.intersectsObject(view.models.getObjectByName(name)!);
  },
  /** Clicks where instance `index` of a batch is on screen and returns what the viewer reported. */
  pick(name: string, index: number) {
    const mesh = view.models.getObjectByName(name) as THREE.InstancedMesh;
    const matrix = new THREE.Matrix4();
    mesh.getMatrixAt(index, matrix);
    // A point inside the instance's mesh: the middle of its first triangle.
    const positions = mesh.geometry.getAttribute("position"), center = new THREE.Vector3();
    for (let corner = 0; corner < 3; corner++) center.add(new THREE.Vector3().fromBufferAttribute(positions, mesh.geometry.index!.getX(corner)));
    center.multiplyScalar(1 / 3).applyMatrix4(matrix);
    view.controls.target.copy(center);
    view.camera.position.copy(center).add(new THREE.Vector3(0.6, -1.5, 2.5));
    view.controls.update();
    view.camera.updateMatrixWorld(true);
    const projected = center.clone().project(view.camera);
    const canvas = view.renderer.domElement, bounds = canvas.getBoundingClientRect();
    picked.length = 0;
    canvas.dispatchEvent(new MouseEvent("click", { bubbles: true,
      clientX: bounds.left + (projected.x + 1) / 2 * bounds.width, clientY: bounds.top + (1 - projected.y) / 2 * bounds.height }));
    return { reported: picked.at(-1), dataset: JSON.parse(host.dataset.picked ?? "null") };
  },
  frame: () => scene.measureFrame(),
  rendering: () => JSON.parse(host.dataset.rendering ?? "null"),
  /** Turns the time-driven mode on or off and reports its state after `wait` milliseconds. */
  async animate(on: boolean, wait: number) {
    scene.setContinuous(on);
    const before = scene.time;
    await new Promise(done => setTimeout(done, wait));
    return { before, after: scene.time, continuous: host.dataset.continuous, budget: JSON.parse(host.dataset.frameBudget ?? "null"),
      readout: host.querySelector(".frame-budget")?.textContent ?? null };
  },
  time: () => scene.time,
  /** Fires the page's visibility change as the browser does when a tab is hidden or shown. */
  hide(hidden: boolean) {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => hidden ? "hidden" : "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
    return host.dataset.continuousPaused;
  },
  /** Validates a GLB and loads it with three.js's stock loader: no SuperSpec code reads the file. */
  async load(url: string) {
    const bytes = await (await fetch(url)).arrayBuffer();
    const report = await validateBytes(new Uint8Array(bytes), { maxIssues: 100 });
    const gltf = await new GLTFLoader().parseAsync(bytes, "");
    gltf.scene.updateMatrixWorld(true);
    const meshes: unknown[] = [];
    gltf.scene.traverse(object => {
      const mesh = object as THREE.InstancedMesh;
      if (!(object as THREE.Mesh).isMesh) return;
      const instanced = mesh.isInstancedMesh === true;
      // Where each instance puts the mesh's first vertex, in the loaded (Y-up) world.
      const first = new THREE.Vector3().fromBufferAttribute(mesh.geometry.getAttribute("position"), 1);
      const placed: number[][] = [];
      for (let index = 0; index < (instanced ? mesh.count : 1); index++) {
        const matrix = new THREE.Matrix4();
        if (instanced) mesh.getMatrixAt(index, matrix);
        placed.push(first.clone().applyMatrix4(matrix).applyMatrix4(mesh.matrixWorld).toArray());
      }
      const attribute = (name: string) => mesh.geometry.hasAttribute(name) ? Array.from(mesh.geometry.getAttribute(name).array) : null;
      meshes.push({ name: mesh.name, instanced, count: instanced ? mesh.count : 1, placed, geometry: mesh.geometry.uuid,
        instance: attribute("_INSTANCE"), tint: attribute("_TINT"), extras: mesh.userData,
        material: (mesh.material as THREE.MeshStandardMaterial).name });
    });
    return { issues: report.issues, used: gltf.parser.json.extensionsUsed ?? [], meshes };
  },
  /**
   * Delivers a saved Studio project through the production Wasm module as the Studio's worker does (the preview packet,
   * then a PNG GLB export) and returns the SHA-256 of each, for comparison with the native tool's output.
   */
  async deliver(url: string) {
    const { assets, ...inputs } = await (await fetch(url)).json() as { assets: Record<string, string> } & Record<string, unknown>;
    const module = "/superspec_wasm.js";
    const wasm = await import(/* @vite-ignore */ module);
    await wasm.default({ module_or_path: "/superspec_wasm_bg.wasm" });
    await wasm.prepareTextures();
    const session = new wasm.Authoring();
    try {
      for (const [name, encoded] of Object.entries(assets)) session.set_asset(name, Uint8Array.from(atob(encoded), character => character.charCodeAt(0)));
      const source = JSON.stringify(inputs);
      // The catalog's own images, as the Studio's worker supplies them.
      for (const name of JSON.parse(session.project_library_assets(source)) as string[]) session.set_asset(name, await libraryAsset(name));
      const preview = JSON.parse(session.preview_project(source, 0));
      if (!preview.success) throw new Error(JSON.stringify(preview.diagnostics));
      const packet: Uint8Array = session.packet();
      const glb: Uint8Array = session.export_project(source, 0, false);
      const digest = async (bytes: Uint8Array) => [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes as Uint8Array<ArrayBuffer>))].map(byte => byte.toString(16).padStart(2, "0")).join("");
      const manifest = decodePacket(packet.slice().buffer).manifest;
      return { packet: await digest(packet), packetBytes: packet.length, glb: await digest(glb), glbBytes: glb.length,
        meshes: manifest.meshes.map(mesh => mesh.id), batches: manifest.batches ?? [] };
    } finally { session.free(); }
  },
};
Object.assign(window, { driver });
