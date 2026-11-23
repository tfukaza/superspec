# SuperSpec Three.js render engine

`@superspec/three` runs SuperSpec's render optimizations on a host-owned
Three.js renderer, scene, and camera. It can consume a published GLB without the
compiler, Wasm module, or editor. Three.js is a peer dependency.

```js
import { attachWorldSpecScene, createRenderGltfLoader, disposeRenderGltfLoader, resolveRenderProfile } from "@superspec/three";

const profile = resolveRenderProfile({ version: 1, preset: "medium" });
const loader = createRenderGltfLoader(renderer, { transcoderPath: "basis/" });
const gltf = await loader.loadAsync(mapUrl.href);
scene.add(gltf.scene);
const world = await attachWorldSpecScene(gltf, {
  renderer, scene, sun,
  entryUrl: mapUrl,
  diagnostics: profile.diagnostics,
  graphics: profile.settings,
});

function draw() {
  if (world) world.render(camera);
  else renderer.render(scene, camera);
}

// On map replacement, dispose the controller. Release transcoder workers
// when shutting down the renderer.
world?.dispose();
disposeRenderGltfLoader(renderer);
```

Pass the host's directional light as `sun`. Hosts can keep refreshing its
shadow map for moving characters or fit it to the play area for a static
viewer. The controller selects geometry and material LODs, manages draw
clusters and texture residency, binds published lighting and material effects,
and applies finishing passes. The same geometry controller is available to
other Three.js backends. Plain glTF stays on the host's normal render path.
On successful attachment the controller owns the map geometry and texture
resources. The host still owns its renderer, camera, player objects, and UI.

Serve `basis_transcoder.js` and `basis_transcoder.wasm` from the supplied
`transcoderPath` for KTX2 maps. Large published packs use browser image elements
for PNG/JPEG decoding to avoid ImageBitmap stalls. The host can also set
`imageDecoder: "html"` or `"three"` explicitly.

In SuperSpec, `#render-engine` resolves directly to the package's public API.
Run `npm run pack:three` at the repository root to build the installable tarball
under `generated/render-engine/`. Install that tarball in another project with
`npm install --save-dev /path/to/superspec-three-0.3.0.tgz`. The tarball contains
compiled JavaScript and TypeScript declarations.
