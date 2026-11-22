# WorldSpec LODs

WorldSpec composes the levels supplied by an asset owner. An asset with a single rendition remains valid. Levels affect the camera view. Collision, navigation, source selection and shadow casting use their canonical inputs.

| Owner | Declaration | Result |
| --- | --- | --- |
| ItemSpec | A root item definition can supply `lods` with increasing distances in metres and overrides for its geometry controls. | WorldSpec compiles the same placed item at each setting. Each alternative can contain multiple meshes and material slots. |
| TreeSpec | The tree or grass builder supplies levels of a foliage patch, including the parts and switch distance of each level. | WorldSpec groups related batches so the stem and crown can switch together. The same group can be exported with instancing or as plain glTF nodes. |
| MaterialSpec | A Studio project or authored world can supply `materialLods` keyed by material entry module. | WorldSpec attaches complete alternate materials to each receiver. Pigment, properties, normals, emission and live material metadata can all change at a transition. |

## An item with three geometry levels

A round pipe with a declared integer geometry parameter named `segments` can retain 24 sides nearby and use eight and four sides in the distance.

```json
{
  "lods": [
    { "distance": 20, "parameters": { "segments": 8 } },
    { "distance": 50, "parameters": { "segments": 4 } }
  ]
}
```

The item's normal parameter values define level zero. Each alternate starts from that same placed item, including any overridden length, transform or material. See the [ItemSpec definition reference](../crates/engine/src/itemspec/definition/README.md).

TreeSpec definitions use their own `lods` array. For example, the shipped birch sets switch distances of 0, 22 and 45 metres. It reduces bark sides from eight to five to four and changes the coarsest canopy to an impostor. [Birch definition](../crates/engine/src/treespec/fixtures/birch.tree.json)

## A material with a cheaper distant finish

```json
{
  "materialLods": {
    "wall.material.ts": [
      { "screenCoverage": 0.18, "parameters": { "chipAmount": 0.1 } },
      { "screenCoverage": 0.05, "entry": "distant-wall.ms", "resolution": 64 }
    ]
  }
}
```

Both entry modules belong in the project's `modules`. The first level inherits the owner's parameters and layers. A different entry starts with its own defaults and accepts its own parameter overrides. Each recipe uses the same receiver and UV chart. The optional resolution caps the longest image edge *after* baking, so changing the raster size preserves UV registration. It must be between 16 and the owner's resolution. Coverage entries strictly decrease. For a typical front-facing receiver at 1080p, `0.05` corresponds to a longest projected dimension of about 54 pixels.

## Delivery and playback

The portable model records geometry in `geometryLods`, with a group ID and near-first levels referencing placement and batch IDs. A material carries its ordered lower materials in `lods`. A visual GLB puts the near node in the active scene and references the alternatives through optional `MSFT_lod.ids`. For a material, the ordinary glTF primitive references the near material and its extension lists the lower materials. A core-only loader draws the original exactly once. The collision export excludes alternate visuals. [glTF extension specification](https://github.com/KhronosGroup/glTF/blob/main/extensions/2.0/Vendor/MSFT_lod/README.md)

The near node can carry `extras.worldSpecLod = { "version": 1, "distances": [0, 20, 50] }`. Authored screen thresholds use `extras.MSFT_screencoverage`, one entry per level. SuperSpec writes a final zero for material chains. The Three adapter uses projected bounds, camera zoom and drawing-buffer height, with hysteresis to keep small movements from repeatedly flipping levels. The reference view is 1080 pixels high with a 70-degree vertical field of view. The published viewer loads static authored alternatives when opening the GLB and selects a level for each view. The same adapter is exported by `@superspec/three` as `loadDeclaredLods`.

Kowloon also publishes generated index streams for city geometry. Each chain belongs to a glTF mesh and primitive index. Two buildings can share their full-detail vertex and index buffers and still select different authored reductions. An authored group takes precedence on geometry it owns. Optional texture streaming promotes a surface's original maps when approached, including the material and geometry alternatives active for the current camera. Enable this publication option with `SUPERSPEC_CITY_TEXTURE_LODS=1`.

Open published GLBs directly in the viewer to inspect their levels. Reopening a GLB as editable ItemSpec source uses a narrower static-mesh importer that rejects LOD extensions. Keep the item, tree or material definition for further authoring. The Three adapter supports static node chains and keeps the near rendition when a chain contains unsupported animated or skinned geometry.

### Preparing a level for play

On WebGL with parallel shader compilation, the packaged viewer prepares the opening camera's color shaders before drawing its first world frame. It uses the real viewport, lighting and render target, and limits the optional wait to eight seconds. The viewer then warms other reachable variants before enabling camera controls. Set `shaderwarmup=serial` to compare the original first-draw schedule. A game using the Three adapter can await `WorldSpecRenderPass.warmup(renderer, scene, camera, map, textureBudgetBytes)` after rendering its first world frame with the intended lighting. When the map has declared LODs, wrap that call with `declared.withVariants(...)` so detached alternatives participate. Resume normal camera LOD selection after the promise resolves. The prepared programs belong to the current WebGL context.

Preparation covers the actual world color target, mesh and instancing layouts, camera-dependent glazing, and the outline exclusion pass. Texture uploads have a separate budget. The default prepares up to 32 MiB of additional decoded maps, ordered by the distance to their material's placements. The viewer accepts `texturewarmupmib` to test a platform-specific budget. The value limits estimated new upload bytes during loading. Total GPU memory also includes visible textures, render targets, shadows, and geometry.

To measure first-use costs, pass `--first-traversal --views spawn-walk,city-pan` to `npm run bench:published-city -- path/to/stoacity.glb`. This starts at the street camera and measures each route twice in the same renderer. Results include time to Ready, time to settled starting textures, frames over 50 ms, new programs and texture counts. Use `--viewer-query warmup=off` for a control and `--compare path/to/control` to compare the matching screenshots.

The published viewer combines visible sibling clusters that share a material, vertex buffers and the same set of instance transforms. Each cluster keeps its selected LOD indices. The default and `clusterdraws=wide` use this grouping. Use `clusterdraws=pairs` for fixed sibling pairs or `clusterdraws=off` to disable index merging.

### Measuring runtime LOD changes

Open a published GLB with `native.html?gltf=…&renderer=auto` or use the viewer's Renderer menu. Auto tries WebGPU and uses WebGL when the device is unavailable or the scene requires a SuperSpec effect awaiting a WebGPU implementation. Select WebGPU to inspect that scene with portable materials and lighting. The status line shows the active backend and identifies portable previews. Selecting WebGL opens the existing illustrated renderer directly. The renderer choice applies to published GLBs.

Run the published viewer at the target drawing-buffer size with hardware acceleration. At 1080p the default route set includes both the moving overhead pan and the street walk. Default measurement (`--throughput`) reports GPU-completed render frames per wall-clock second in `completedFps`. The harness drives the same renderer and camera poses independently of animation callbacks. It allows up to three frames in flight, drains earlier work before timing and waits for the final WebGL fence or WebGPU queue completion. This measures sustained capacity across CPU submission and GPU execution. The display refresh rate does not cap this metric.

Use `--paced` to measure playback cadence separately. Its `deliveredFps` comes from browser animation-frame intervals. Compare mean interval, p95 and frames over 25 ms. At 60 Hz a missed refresh usually shows up as a 33 ms interval. For both modes, ordinary moving routes get a full replay before measurement. Use identical camera steps, alternate the order of the variants, retain complete runs as trials, and compare screenshots from matching poses. Each route advances one pose per frame, so changing `--frames` also changes the camera displacement per frame and the frequency of visibility and LOD changes. Repeat a promising result in a fresh browser. The first-traversal mode above measures cold playback.

```sh
npm run bench:published-city -- maps/stoacity.glb --views city-pan,spawn-walk --out generated/city-throughput
npm run bench:published-city -- maps/stoacity.glb --renderer webgpu --views city-pan,spawn-walk --out generated/city-webgpu
npm run bench:published-city -- maps/stoacity.glb --paced --views city-pan,spawn-walk --out generated/city-pacing
npm run bench:published-city -- maps/stoacity.glb --calibrate --out generated/city-calibration
```

The report records the selected `timingMode`. Ordinary throughput and pacing runs leave GPU timer queries off. The paced diagnostics include `--passes` for pass attribution and `--gpu-sample-every 12` for periodic whole-frame GPU timing. Calibration replays two identical query-free controls and an instrumented run in all six orders. Its report includes control-to-control drift, the timing change with queries enabled, and how many camera poses submitted different draw or triangle counts. A smaller effect than the observed control variation calls for more trials. CPU submission and GPU execution overlap, so their durations cannot be added to predict FPS.

Reports record the requested renderer, active backend and appearance. Explicit backend benchmarks fail if the viewer falls back. Pass attribution, timer sampling, calibration and first-traversal instrumentation use WebGL. Compare results with matching effects, camera paths and resolution to evaluate a speedup.

Before building a distant replacement, trace the actual color submissions at multiple route poses. Count a draw as removable only when every visible instance in it qualifies for the replacement. Measure the candidate by temporarily suppressing those submissions alongside a control that runs the same selection logic. This measures the work that can be saved at that point in the renderer. A finished replacement adds draws of its own and needs a fresh timing and visual comparison.

### Distant building blocks

The Kowloon publisher can bake one street cell's static building shells into a shared distant mesh and material atlas set. To include the experimental rendition in a new city pack, pass `--block-proxy=X,Z` to `tools/kowloon-export/pack.mjs`. The indices identify a 28-metre street cell in glTF's horizontal XZ plane. For an already published pack, the following command writes a sibling entry and reuses its original companion files.

```sh
node tools/kowloon-export/block-proxy.mjs maps/stoacity.glb maps/stoacity-block.glb --cell=-2,0
```

Open `/native.html?gltf=/artifacts/stoacity-block.glb&blockproxy=on` in the published viewer. The default 45-metre distance is measured from the nearest point of the block bounds. Perspective zoom, viewport height and hysteresis also affect the transition. Use `--distance=METRES` on the standalone bake to adjust this boundary. Close views and orthographic cameras draw the original buildings.

The scene declaration `extras.worldSpecBlockLods` has `version: 1` and a `blocks` array. Each block names its source root node indices in `sources`, a detached glTF node index in `proxy`, its `id`, metric `distance` and glTF-space `bounds.min` and `bounds.max`. The ordinary glTF scene contains the source nodes. A supporting viewer can load the detached node for the color pass. Shadows use the canonical casters. Independently placed props and foliage retain their own LOD decisions.

The city plan proposes part instancing, spatial batches and hierarchical selection for neighborhoods with unique building layouts. [Unique-building rendering proposal](plans/worldspec-city.md#proposed-rendering-architecture-for-unique-buildings)

## Verify a delivery

After building the packaged viewer, run `node tests/published-webgpu-browser.mjs` with a WebGPU-capable browser. The washroom exercises renderer selection, image output and recovery to WebGL. Set `WEBGPU_WASHROOM` to a baked washroom GLB to include its delivered textures in the check.

Build the native Studio CLI and run `npm run build:native-viewer`. With `STUDIO_CLI` pointing to the binary, run `node tests/exported-lods-browser.mjs`. The test exports the standard washroom with an additional pipe and a distant concrete finish, opens it in the packaged viewer, and checks the active pipe and wall material at near, middle and far distances and on return. It repeats with the core fallback and opens a separate CLI export of the shipped birch to verify its three levels. Screenshots and selection records are written under `generated/exported-lod-review/`.
