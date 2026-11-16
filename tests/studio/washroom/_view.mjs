// Renders a GLB with plain three.js from the concept's viewpoint. Usage: node view.mjs <glb> <png>
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { resolve, extname } from "node:path";
import { chromium } from "playwright";

const repo = "C:/Users/tomok/OneDrive/code/superspec";
const [glb, out] = process.argv.slice(2);
const html = `<!doctype html><body style="margin:0;background:#b9b2b4"><script type="importmap">{"imports":{"three":"/node_modules/three/build/three.module.js","three/addons/":"/node_modules/three/examples/jsm/"}}</script>
<script type="module">
import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
renderer.setSize(1100, 1100); document.body.append(renderer.domElement);
renderer.toneMapping = THREE.ACESFilmicToneMapping; renderer.toneMappingExposure = 0.8; renderer.shadowMap.enabled = true; renderer.shadowMap.type = THREE.PCFSoftShadowMap;
const scene = new THREE.Scene(); scene.background = new THREE.Color(0xb9b2b4);
const camera = new THREE.OrthographicCamera(-2.3, 2.3, 2.3, -2.3, 0.1, 50);
camera.position.set(9, 6.4, 9); camera.lookAt(1.0, 0.95, 1.0);
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
scene.environment = new THREE.PMREMGenerator(renderer).fromScene(new RoomEnvironment(), 0.04).texture; scene.environmentIntensity = 0.55;
scene.add(new THREE.HemisphereLight(0xdfe6ff, 0x8a7a6a, 0.5));
const sun = new THREE.DirectionalLight(0xffd29a, 3.4); sun.position.set(7, 8, 1.5); sun.castShadow = true; sun.shadow.mapSize.set(2048, 2048); Object.assign(sun.shadow.camera, { left: -4, right: 4, top: 4, bottom: -4, near: 1, far: 30 }); sun.shadow.bias = -0.0005; scene.add(sun);
const gltf = await new GLTFLoader().loadAsync("/model.glb");
gltf.scene.traverse(node => { if (node.isMesh) { node.castShadow = true; node.receiveShadow = true; } }); scene.add(gltf.scene); renderer.render(scene, camera); window.done = true;
</script>`;
const server = createServer(async (request, response) => {
  const path = new URL(request.url, "http://x").pathname;
  try {
    if (path === "/") { response.setHeader("Content-Type", "text/html"); response.end(html); return; }
    const file = path === "/model.glb" ? resolve(glb) : resolve(repo, "." + path);
    response.setHeader("Content-Type", extname(file) === ".js" ? "text/javascript" : "application/octet-stream");
    response.end(await readFile(file));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1100, height: 1100 } });
page.on("console", message => console.log(message.text()));
page.on("pageerror", error => console.log(String(error)));
await page.goto(`http://127.0.0.1:${server.address().port}/`);
await page.waitForFunction(() => window.done, {}, { timeout: 60000 });
await writeFile(out, await page.locator("canvas").screenshot());
await browser.close(); server.close();

