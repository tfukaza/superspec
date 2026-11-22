# WorldSpec city layout MVP

The previous avenue study combines a corner building, street surfaces, planting
and furniture in a studio preview project. Its avenue record and placement code
are the dimensional reference. WorldSpec owns the arrangement of these objects.
ItemSpec owns a building and its attached parts, a segment of paving, a tree or
a lamp in local coordinates.

1. Admit a city section in the shipped AML profile and schema. Give every placed
   object a source ID, world position and heading. Use a count and a world-space
   displacement for straight rows. Bound expansion during source admission.
2. Compile the architectural base through the existing physical pipeline. Resolve
   each role to a theme-owned ItemSpec definition and place the resulting local
   resources in source order. Carry the AML location onto each delivered mesh.
   Both the native CLI and the browser worker consume the same Rust implementation.
3. Port the corner building, carriageway, pedestrian avenue, far sidewalk, median
   trees and lamp rows. The avenue and sidewalk reuse the existing stone generators.
   The tree and shrub placements use the street study’s illustrated TreeSpec foliage batches.
   The [compiler fixture](../../crates/engine/src/worldspec/fixtures/corner-avenue.aml)
   exercises the reference positions and dimensions.
4. Verify rotated strip bounds, row spacing and ancestry, source errors and recovery,
   nested building assets, physical independence, and native/Wasm delivery.

The MVP handles explicit straight segments and repeated placements. The AML
preview uses the shop-house material recipes for city receivers, the avenue
median's planted groundcover, and the study's lighting and shadow settings.
Mesh contours are off in the city preview.

Sunlit corners of city geometry catch a narrow highlight. The pass reads geometric
normals, depth and the sun's shadow map. Use `?edgeboost=off` to compare the city
without it, or `?edgeboost=on` to try it on a standalone ItemSpec or material project.

Add `?edgeblur=on` to the preview URL to soften strong color transitions
within one device pixel. `edgethreshold` sets the minimum display-color
difference on a scale from 0 to 1. The default is `0.12`.

Path-following and intersections require further placement and physical contracts.

## Proposed rendering architecture for unique buildings

Design for a city in which every building has a distinct footprint, massing or façade layout. Keep each building as an editable composition. Let the exporter choose rendering units by shared geometry, compatible materials and spatial visibility. Reusing a complete building remains an optional shortcut.

The Kowloon recipes already place floors, windows and air conditioners as nested items. ItemSpec currently flattens a child item's meshes when adopting the child. The native city compiler looks for repeated whole designs, while the Stoa packer joins geometry within each prototype before instancing its placements. To retain reusable parts, carry the nested item references and their local transforms through those steps. [Floor recipe](../../crates/engine/src/worldspec/themes/kowloon/items/kowloon-floor.item.json) · [Item assembly](../../crates/engine/src/itemspec/definition/parts.rs) · [City compiler](../../crates/engine/src/worldspec/city.rs) · [Stoa packer](../../tools/kowloon-export/pack.mjs)

### Generation and export

- **ItemSpec** identifies a local mesh by its effective geometry and vertex attributes. Preserve source part IDs and a stable placement ID with its owning building, transform and appearance overrides. The existing part rebuild key includes placement and feature data for editing. Use a separate content key for sharing geometry across buildings.
- **MaterialSpec and the Kowloon theme** provide shared finish definitions, tiling maps and trim sheets. Carry tint and compatible variation as instance inputs. A weathering bake keyed to a particular receiver keeps its own content identity and detail density. A different material binding can reference the same geometry.
- **WorldSpec** partitions static placements into bounded spatial cells. Instance identical parts with compatible bindings. Merge the opaque triangles of differently shaped walls and roofs into cell-local buffers by compatible material, vertex layout and pass state. Keep stable source mappings for instances and merged ranges. Preserve tangents for normal-mapped surfaces when selecting a batch format. Sorted glass and moving sashes retain their own rendering paths. [Existing batch contract](../../crates/model/src/batches.rs)
- **The exporter** writes a local hierarchy with bounds, projected-error inputs, child membership and a coarse exterior for each replacement group. A façade or block proxy uses the actual buildings' silhouettes, setbacks, window rhythms and large color regions. Shallow details can transfer to shared material pages. Give each cell's near and coarse resources separate references so either can be loaded, and share common assets by content.

### Runtime and editing

The engine adapter first tests a group's bounds and projected error. When the exterior is sufficient, it submits that exterior and skips visiting the detailed children for the color pass. Near a street corner it can descend into the visible frontage while neighboring cells stay coarse. Use hysteresis at the transition and keep the current rendition resident until its replacement is ready. Independently select shadow casters from the light's view. Collision, navigation and validation continue to use their canonical data.

An AML edit invalidates the affected placements, receiver-dependent finishes and containing proxies. Moving a building also invalidates its old cell and any neighboring group whose declared dependencies changed. Publish updated references together, retaining unaffected cells and shared part resources. Portable records support this hierarchy across exporters and engine adapters.

### Acceptance

Use the published city as the control. In the LevelSpec harness, author a second map with the same building count, lot density, camera routes and comparable near-view detail. Require every building's assembled shape or layout to be distinct and verify that common parts actually share mesh and texture resources. Compare the original and proposed delivery of that varied map.

At 1920×1080 measure uncapped completed frames/s and paced late frames on the first-person walk and overhead pan. Count groups visited, complete draw calls retired after adding the proxy draws, and triangles per pass. Record export bytes, peak resident geometry and texture bytes, cold uploads and pauses on entering a cell. Compare matched near views, silhouettes and transitions, glass from both sides, canonical shadows and normalized physical outputs. Edit one building and check the affected cells, reuse and equivalence to a clean build. [Benchmark workflow](../LOD.md#measuring-runtime-lod-changes)

### Rendering backend evidence

On Stoa at 1920×1080 in Chrome 157 on an RTX 5060 Ti, WebGL draw submission consumed 8.78 ms of the 15.98 ms synchronous overhead frame, including material setup and driver waits. Three.js spent 3.11 ms updating transforms and building and sorting render lists. WorldSpec spent 3.68 ms preparing the view. At street level those stages took 1.99, 1.03 and 2.70 ms respectively. Median draw counts were 1,407 overhead and 221 on the street. This makes retiring whole draws the first priority overhead, while faster visibility selection has more leverage at street level.

Two isolated experiments tested where to run that work.

- **CPU occlusion.** Replaying 32 captured poses in Windows Node, Rust/Wasm saved a median paired 0.10 ms per street pose and 0.19 ms overhead on the 160×96 conservative raster. Both implementations produced identical depth buffers. Timings include copying the changing inputs and one Wasm call per pose.
- **GPU selection.** A WebGPU compute pass culled 5,407 primitive placements and partitioned survivors into three distance tiers. Three indexed indirect draws consumed the resulting instance lists directly. IDs and counts matched the CPU reference at 15 poses per route. Across two 240-frame trials per route, GPU timestamps averaged 2.46–3.28 µs for compute and 138–208 µs for the proxy draws. These timings cover simple proxy geometry and exclude the city's materials, shadows and post-processing. [WebGPU indirect drawing](https://www.w3.org/TR/webgpu/#dom-gpurendercommandsmixin-drawindexedindirect)

Ubisoft used a similar division of work on the modular city in *Assassin’s Creed Unity*. The CPU selected coarse spatial regions and grouped compatible render states. The GPU tested instances and clusters, compacted visible geometry and supplied indirect draw arguments. For WorldSpec, one logical placement can reference any compatible façade part or cell-local mesh. Keep the selection output on the device through drawing. Reading IDs back to update individual Three.js objects would reintroduce synchronization and per-object submission. [Ubisoft SIGGRAPH presentation, pipeline overview](https://advances.realtimerendering.com/s2015/aaltonenhaar_siggraph2015_combined_final_footer_220dpi.pdf#page=13)

Unreal’s Nanite builds a hierarchy of triangle clusters at import and chooses cluster detail from the camera view at runtime. WorldSpec can apply the same principle at the façade or block scale. Build bounded alternatives once during export, then let the renderer skip a distant façade’s children as soon as its silhouette and projected detail budget permit. [Epic’s Nanite overview](https://dev.epicgames.com/documentation/en-us/unreal-engine/nanite-virtualized-geometry-in-unreal-engine#howdoesnanitework?)

The published viewer has WebGL and WebGPU backends that share CPU-side city geometry selection. Auto selects WebGL for scenes requiring illustrated effects that still need a WebGPU implementation. The Wasm raster and GPU-driven draw lists remain experiments. To evaluate GPU submission on the full city, export compatible draw groups with stable placement IDs, maintain separate color and shadow selections, and port the required shader and post-processing effects. Compare completed-frame timings and matched images with those effects active on both backends. [Published viewer](../../editor/native-entry.ts) · [WebGPU city integration](../../editor/native-webgpu.ts) · [Three.js migration guide](https://threejs.org/manual/pages/webgpurenderer)

On the warmed 1080p overhead route with portable materials, Three.js r186 spent 23.63 ms per frame preparing and submitting the WebGPU draw lists. GPU timestamps covered 11.25 ms across the color and output passes. At each of five camera positions, over 1,100 separate render-uniform buffers held only three distinct combinations of layout and contents. Keeping the material and binding preparation while omitting all city draw commands still cost 23.53–23.87 ms per frame. Omitting the same draws before their per-object preparation took 3.15–5.64 ms. These controls isolate a substantial cost before rasterization.

For a full-city GPU-driven trial, define camera and lighting uniforms once per compatible view and pass. Share material pipelines by layout and render state, and put variation in material and instance data. Prove sharing through field semantics and resource lifetimes so changes in lights, materials and shadow cameras propagate correctly. Count distinct pipelines and buffer updates at matched poses alongside CPU submission time, GPU time and completed FPS. [WebGPU pipeline model](https://developer.chrome.com/docs/web-platform/webgpu/from-webgl-to-webgpu#global_state)

The current phase measures the illustrated TypeScript/WebGL path at street level and overhead. Separate investigations cover CPU preparation and submission, GPU geometry and shading, memory, and first-use hitches. The handoff includes open research questions, primary engine references and common comparison routes. [Parallel profiling brief](city-rendering-profile.md)
