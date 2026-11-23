# Native Studio pipelines

Launch the local service with `npm run dev`. The first run builds the viewer and the release Rust binary. For repeated starts use `superspec-studio serve`. A packaged executable discovers the adjacent `native-viewer` directory. `--ui` selects an explicit viewer build and `--port` selects a loopback port. `npm run dev:rust` retains the Vite development path for the reference browser compiler.

| Domain | Native input and work | Viewer |
| --- | --- | --- |
| MaterialSpec | Saved Studio JSON or ZIP, material sources, recipe checking and edits, receiver fields, shader and layout descriptions, CPU-baked scene and GLB export | Material editor, interactive parameters, GPU atlas rendering |
| ItemSpec | Item definitions, parameters, instantiation, companions and scene composition | Item editor, picking and handles |
| TreeSpec | Standalone tree, or grass with a ground site supplied to the CLI, compiled through the existing foliage pipeline | Open a tree or compiled packet on the landing page |
| WorldSpec | AML and bundled companions, scene compilation, validation and export | World editor, native-backed city shading, navigation and inspection |

The HTTP service owns a warm compiler per session. Revisions supersede stale work. Saving and export rebuild the admitted project. The input transport uploads companion bytes once per set and refers to the accepted revision on subsequent edits. A missing set triggers a full upload. The generic scene endpoint retains its last successful packet across rejected edits. At capacity, the service reclaims the least recently used idle compiler. Active and queued requests keep their compiler alive. The engine shares its content-keyed occlusion cache with the Wasm reference. Renderer effects and immediate interactive feedback run in the browser.

## Old Hong Kong

Compare the LevelSpec `projects/klali-stoa/scene.aml` against Klali's `public/maps/stoacity.glb` and its adjacent resources. In the market comparison, the game camera is at `(22.5, 1.72, 15.8)`, looking at `(18.8, 1.65, 7)` with a 64-degree field of view. The corresponding SuperSpec coordinates are `(22.5, -15.8, 1.72)` and `(18.8, -7, 1.65)`. The captures use 1440 × 960 pixels.

Opening the AML directly loses the arena finishing pass that authors paving and concrete charts in the LevelSpec packer. Loading the shipped GLB restores those surfaces. The market still appears dark in a standard glTF preview. Klali applies shop lights, probe lighting, concrete grain, and a sky and fog record after loading. Published-world inspection now consumes those records. To inspect the delivered result on a static host, open `native.html?gltf=URL` and serve the referenced resources beside the GLB. The switches `fixtures=off`, `gi=off` and `grain=off` isolate these effects. Material Studio also uses WorldSpec's city appearance when opening a city AML base and preserves explicitly replaced material receivers.

## Local verification

Windows and Linux service tests exercised cancellation, revision conflicts, recovery, tree LOD batches, artifact confinement, and PNG and KTX2 exports. The AML reference fixture produced byte-identical scene packets on Windows native, Linux native and Wasm. A grass build using the shipped meadow definition and a four-metre ground site produced 12 batches.

Three alternating Windows native and Wasm runs used the 64 px washroom, Windows Chromium, ANGLE D3D11 and an RTX 5060 Ti. Times below are medians in milliseconds. Each run started a fresh browser. Filesystem and graphics driver caches were shared across runs.

| Timing | Windows native | Wasm reference |
| --- | ---: | ---: |
| First visible preview | 1913 | 2184 |
| Full preview ready | 2191 | 2227 |
| GPU-baked maps ready | 2226 | 2264 |
| Tile edit through bake | 255 | 244 |
| Concrete-wall edit through bake | 345 | 336 |

One run at the default 256 px reached the full preview in 1818 ms on native and 1967 ms on Wasm. In the 64 px native test, editing a source without changing its program reused all twelve mesh bakes. These timings include renderer work, client serialization, binding and upload. They do not isolate compiler throughput or establish an improvement for warm edits.

The published market comparison used one local copy of the GLB (SHA-256 `4a4da10f4ecc7698b51fb16807d1ba2075a9d491678cee091fe62ab7b39634dd`) and its referenced buffers, images and LOD sidecars. At the market eye the final viewer and Klali differed by 5.35 channel levels on average on an 8-bit RGB scale. At the closer camera they differed by 5.38. Visible differences remain in cast-shadow edges and the fluorescent highlights. The AML view also lacks the separate packer's arena surface finish.

## Headless artifact builds

LevelSpec's Stoa building and arena producers call the native Studio CLI with project archives on stdin. A separate Node GPU host runs the shared MaterialSpec shaders through ANGLE, reads the texture channels directly, and exports them with the original geometry. The browser remains available for interactive preview and as a reference renderer. The Stoa project README documents the build commands.

On Windows D3D11 the complete 25-sheet arena GLB matched the browser bake byte for byte. Prototype building-0 retained its 334,830 triangles, 21 texture pages, eight emissive charts and ten original mesh groups. Its decoded material textures differed by 0.0223 of one 8-bit channel level on average. Individual texels can differ across canvas implementations. LevelSpec's `verify-bake-parity.mjs` checks the geometry and material channels against a supplied reference GLB. Keep consumer pins through independent review. Native CLI validation, collision and sampled navigation retain their existing meanings.

The full native Stoa pack finished all six building prototypes and baked bounced sunlight from 78,410 reflectors. A warm arena run reused all 75 cached channel images and reproduced the same GLB. At the two cameras above, the same new map in SuperSpec and Klali differed by 6.12 and 6.30 RGB levels respectively. The remaining visible differences include the cast-shadow edge across the paving and the glow around the fluorescent tubes. The older shipped map also differs in surface finishes, so comparisons to that delivery include content changes.

### Native Stoa pipeline timings

On a Ryzen 5 9600X (12 logical CPUs) with an RTX 5060 Ti, the complete AML export to the Windows workspace from WSL took 8.36 and 7.46 seconds serially. Four concurrent prototype exports took 3.64 and 3.44 seconds. All 38 compiler outputs matched byte for byte. Prototype names and placement order are assigned before scheduling. `compile.py --jobs 1` selects serial export.

For the 150,523-triangle building-3, two Windows headless control bakes took 31.53 and 31.99 seconds. Overlapping atlas-channel encoders within a single page and feeding raw pixels to the JPEG encoder took 22.81 and 22.70 seconds. These were fresh processes with warm filesystem and driver caches, in control/candidate/candidate/control order. The GLB hashes matched. Sampled peak Node working set increased from 2.05 to 2.21 GiB. This measures the prototype bake through GLB publication, excluding the final city pack.

WorldSpec city preview and the offline bakers can request shading and layout records for an existing scene without returning another geometry packet. The building-3 report fell from 91,334,566 to 68,486,892 bytes. The shader metadata and binary layout records matched. The paired command times overlapped at 3.57–3.75 seconds.

### City edits and repeated game exports

WorldSpec retains successful native shading layouts by project slice, including module sources, placements and companion contents. A rename can bind an existing layout to the new revision. Moving a building changes the slice and requests a fresh layout. In one native editor run per version, Stoa building-3 (595 objects) opened in 23.31 s before and 16.45 s after. An AML rename reached ready in 15.56 s before and 2.70 s after, with zero layout requests after the change. Moving the placement took 7.58 s before and 4.93 s after, with one layout request.

In the washroom, a tile parameter edit reached its finished bake in 275 ms before and 252 ms after. An item wall-width edit took 1268 ms before and 1033 ms after. Changing the tile shader source took 392 ms before and 469 ms after. These are individual end-to-end observations under variable load.

Three repeated copies of Stoa building-3 produced byte-identical scene packets and PNG GLBs. With fresh CLI processes and warm filesystem caches, three-run median times were 0.670 s before and 0.680 s after for AML to scene, and 0.702 s before and 0.713 s after for AML to GLB. Removing repeated batch expansion and temporary exporter buffers did not measurably improve either standalone command on this fixture.

Using already baked Stoa prototypes, the game pack took 121.64 s before the atlas PNG cache, 108.26 s filling it, and 43.28 s reusing all 263 pages. The GLB and 44 sidecars matched the control by SHA-256 on both runs.

### Stoa pipeline review, November 2026

The current Stoa AML produces 578 placements from 38 placed prototypes. The
Windows production path first emits prototype geometry, placements and collision.
Seven building prototypes then receive GPU-baked materials. The packer finishes
the arena, merges geometry, instances repeated objects, verifies compressed
textures, generates bounced lighting and builds the runtime LOD sidecar.
Klali navigation and game-load verification are timed separately.

| Production stage | Earlier fresh output | Reviewed fresh output | Repeat using generated caches |
| --- | ---: | ---: | ---: |
| AML through native geometry and placements | 6.21 s | 5.13 s | 4.44 s |
| Building GPU bakes and atlas export | 313.61 s | 308.30 s | 6.63 s |
| Scene finishing, packing, GI and LOD | 196.96 s | 137.43 s | 80.18 s |
| Total | 516.78 s | 450.86 s | 91.25 s |

These are individual runs with prebuilt Windows binaries and uncontrolled OS and
driver caches. The reviewed fresh run overlapped a Studio compiler build. The
repeat used the same baker executable as the fresh run. It verified and reused
all seven baked GLBs and all 296 compressed atlas pages. The 175 published
files matched the fresh run by SHA-256. The fresh run preceded the scheduling
change for independent arena and dressing exports.

In a separate interleaved scheduling comparison, the former four-worker
generation path took 4.49–9.65 s (median 8.23 s). Running the arena visual and
collision exports independently and scheduling the four dressing jobs in the
prototype pool took 4.12–7.05 s (median 6.89 s). Six workers took 5.60–9.46 s
(median 7.70 s). All 82 native-stage files matched across the three schedules.
Four workers remain the default. These timings include filesystem writes.

The building baker keeps a receipt beside each baked GLB. It checks the source
AML, native receiver selection and balcony checks, Studio executable, headless
bundle, producer code, dependency locks, GPU and shader image inputs. The
catalog fingerprint is conservative across materials. An output hash detects
damaged or mismatched products. The packer defaults to lossless PNG level 6.
Use `pack.mjs directory destination.glb --compact` for the prior level 9.
Both validate decoded pixels before replacing an original PNG. The resulting
package grew from 829.38 MB to 848.37 MB. Across the two fresh builds the
mesh, node, accessor and material tables and 1,062 non-image buffer views
matched. Seven image differences were already present in the independently
baked prototype inputs to packing. The largest per-image mean difference was
0.015 on the 0–255 channel scale.

The native AML editor now asks for a delta against its decoded scene revision.
The compiler compares mesh and asset buffers exactly. It sends a self-contained
update when the client has no matching baseline. Invalid AML returns source
diagnostics, and a later valid edit can still reuse the last successful scene.
The compiler still evaluates the AML and the browser still reconstructs the
accepted packet. Delta bytes measure transport reuse.

Two alternating native editor runs per version opened Stoa building-3, renamed
the scene and moved the building. The 778-object fixture uses the same WorldSpec
editor as other AML. Elapsed time includes input debounce, compilation, transfer,
appearance preparation and two animation frames after the editor reported ready.

| Edit | Earlier response | Reviewed response | Earlier ready | Reviewed ready |
| --- | ---: | ---: | ---: | ---: |
| Rename | 62.49 MB | 0.35 MB | 2.26–4.41 s | 1.93–2.13 s |
| Move building | 62.49 MB | 39.43 MB | 8.95–10.22 s | 8.87–10.10 s |

The move still requests a full layout for the affected building. The native
compiler and packet transfer account for roughly one second. For the next editor
iteration, retain the prototype mesh in local coordinates, submit a placement
transform to preview, and track which receivers depend on world position.
Measure the first frame showing the move and the later frame showing its
finished shading. Compare that result with a clean build, including collision
and any adjacent routes that the move invalidates. This follows the
[WorldSpec live session contract](spec-modularization.md#worldspec-live-session-interface).

The complete city currently exceeds the single-preview-packet budget. The
native benchmark reached 6,002,918 vertices at the second grass-field placement
against a 6,000,000-vertex allowance. The publisher succeeds by splitting out
prototypes. Sharing this representation with the native and Wasm preview hosts
would let the editor open the city in sections and apply a transform independently
of untouched prototype buffers. The building edit measurements above cover one
section. Whole-city edit-to-frame latency remains unmeasured.

For cold publication the largest stage is still the seven building bakes.
Instrument native layout generation, GPU shading, readback, atlas assembly and
encoding before assigning more workers to a single GPU. For repeat publication,
the remaining 80 s is concentrated in the pack command. Measure its per-prototype
mesh transforms, scene assembly, GI and LOD separately before selecting the next
cache boundary. A placement change can reuse an intrinsic material bake and still
change the required lighting volume or world-scale LOD error.

The staged game export loaded in Klali on the RTX 5060 Ti. At the fixed market
camera, 960 × 540, 120 warm frame intervals measured 16.7 ms median and 33.3 ms
p95. With renderer counters accumulated across passes, one draw of the view
issued 1,111 calls and 4.62 million triangles. Counters read after the final
postprocessing pass alone undercount the scene. The next runtime measurement
should use the joined-game camera route and separate shadow and main-view cost
before changing LOD thresholds.

### Published viewer frame performance, November 2026

The target is 60 FPS at native 1920 × 1080, prioritizing first-person street,
shop and terrace views. The elevated city view is a secondary stress case.
The measurements below use the staged Stoa export on an RTX 5060 Ti through
Windows Chromium and ANGLE D3D11. Each view includes a 90-frame warmup followed
by 180–240 frames with small camera movements. Other development processes
were active on the host. These are viewer measurements without game actors,
weapons, networking or simulation.

| View | GPU median | GPU p95 | Median draws across passes | Median triangles across passes |
| --- | ---: | ---: | ---: | ---: |
| Market shop | 15.22 ms | 15.54 ms | 887 | 3.54 M |
| Close shop | 10.08 ms | 10.48 ms | 514 | 1.82 M |
| Spawn lane | 13.75 ms | 14.10 ms | 906 | 3.14 M |
| Street | 10.85 ms | 11.40 ms | 735 | 2.79 M |
| Raised terrace | 16.43 ms | 16.77 ms | 1,056 | 4.33 M |
| Elevated city | 39.42 ms | 40.83 ms | 1,870 | 14.30 M |

The five first-person views delivered approximately 60 FPS during the measured
windows. The terrace has little headroom and occasional missed frames. The
elevated view averaged about 25 FPS, with CPU/frame-time outliers. Sustained
60 FPS across all views and a complete moving-player route remains open.
Before this work, the published viewer's market view submitted about 47.4 M
triangles and took 134.7 ms GPU time. Its close shop and elevated city took
97.5 ms and 186.2 ms respectively. The previous game-view timing above used a
different renderer path and a smaller resolution.

The Three.js adapter now owns the city LOD loader, instance merging, spatial
clustering and conservative visibility implementation. The published viewer
loads the exported sidecar relative to the GLB and applies these features.
The game can consume the adapter API in a subsequent integration.

LOD selection uses exported world-space error projected through the camera,
including FOV, zoom and drawing-buffer height. Full detail is retained nearby,
with a 12 m entry distance and 10 m return distance. Continuous surfaces use
a 1.5-pixel geometric-error budget. The viewer allows 4 pixels for clusters of
disconnected components, with hysteresis. Components are joined across all
LOD index streams and welded positions, so UV seams and shared physical edges
stay together. Building texture density and shadow-map resolution retain their
existing settings. The viewer uses a 160 × 96 software occlusion buffer and
tests ordinary props, individual instances, walls, floors and roofs.

Static spotlight maps are refreshed when fixtures change, after explicit
invalidation and after context restoration. The shadow cache restores complete
casters during shadow rendering and reapplies camera visibility afterward.
Eligible opaque outline exclusions use depth-only materials. Transparent,
cutout, displaced and custom vertex materials retain their compatible path.
The controller assumes static published geometry. Recreate visibility and
invalidate the shadow cache after modifying transforms or caster geometry.

The matched market image was pixel-identical after the depth-only exclusion
and ordinary-prop culling changes. Across the subsequent component/grid/LOD
changes, mean RGB differences from the earlier culled image were 0.0006/255
at the market, 0.0093/255 in the close shop and 0.0835/255 overhead. These
are fixed-camera comparisons and do not establish temporal LOD quality for
every route. An opaque depth-prepass experiment increased GPU time in the
first-person views and was removed.

Local evidence is under `generated/runtime-performance/`, including
`bench.mjs`, `baseline`, `fine-occlusion`, `fps-ground` and the rejected
`depth-prepass` experiment. The benchmark records full-pass draw counts,
CPU submission time, GPU timer queries and frame intervals separately.
`npm run test:city-runtime` covers the runtime contracts. Debug comparisons
use `citycull=off`, `lod=off`, `propdetail=full`, `occlusiongrid=coarse` or
`exclusiondepth=full` on the published viewer. `gpuprobe` exposes the scene,
renderer, camera, controls and render function for local measurements.

Remaining production work is concentrated in dense stock and hardware
geometry, visibility of enclosed interiors, and broad views with many nearby
buildings. Producer-owned occluder surfaces and semantic component records
would allow more reliable interior rejection and selective detail reduction.
Keep these records in the shared model/export contract so each engine adapter
can consume them with its own runtime visibility implementation.

Verification completed with 61 standard browser contract tests, 25 city-runtime
tests, the full TypeScript check, the native viewer build, the Three.js adapter
package/declaration build, the refreshed Wasm build and the architecture check
across 387 Rust modules. Native packet tests used a self-contained compiler copy
under `generated/runtime-performance/compiler` because their environment clears
PATH. The benchmark images and reports contain no browser exceptions.

### Distance detail in the published Stoa viewer

The publisher emits five optional index streams for hardware and shop inventory.
The simplifier weighs normals, colors, UVs and chart aspect along with geometric
error. Small hardware components can be pruned. Shop stock uses a separate policy
that favors retaining the bulk of the items. Other eligible architecture gets one
additional far level. The new streams reuse the original vertices and materials.
The Stoa sidecar grew from about 11 to 22 MiB and took 23 seconds to generate on
the Windows benchmark host. All 654 original compatibility streams matched the
previous publication byte for byte.

The viewer uses the projected size of the largest connected component to distinguish
a cluster of small fittings from a wall of similar overall size. Beyond 20 m it
can relax geometric error for small details, reaching a 10-pixel ceiling for the
smallest components. Continuous architecture has a separate ceiling of 3 pixels
at 100 m. The near gate and hysteresis protect first-person views. The same
rules apply to individual meshes and repeated instances. Adjacent opaque clusters
with matching selections can share a draw. Small distant panes can share their
two-sided draw once the entire pane group passes the distance and size tests.

The original normal maps, base colors and shadow resolutions are used throughout.
Single-purpose edge masks and AO maps use two or one GPU channels after checking
all bindings sharing the image. Across the measured city views this saved 783 MiB
of uploaded mip storage. On a fixture change, the viewer retains existing lights
in their shadow slots and draws full-resolution city instances within the new
spotlight's frustum. The shadow instance buffers belong to individual light
cameras, so a second light can upload its own selection in the same render frame.

On the 48 m spawn-lane sweep the largest submission fell from 107.74 million
triangles to 6.60 million. The comparison used the same 720 camera poses.
Eighteen 512 × 512 spotlight depths across six additional camera poses matched
the full-caster control at every sampled texel. In matching 1080p captures,
the median triangle count in the oblique city view fell from 14.25 million
to 3.93 million. Mean absolute RGB differences were 0.003/255 in the market,
0.146/255 on the terrace and 0.984/255 across the oblique city view.

On the RTX 5060 Ti with D3D11, the control and revised viewer used the same
1920 × 1080 camera sweep and all render passes. The revised measurements below
were taken after the concurrent editor GPU run ended. FPS is the reciprocal
of the mean delivered frame interval. GPU timing is a separate timer query.

| Stoa view | Previous FPS | Revised FPS | Median GPU time | Frame interval, p95 |
| --- | ---: | ---: | ---: | ---: |
| Market | 59.0 | 59.0 | 10.1 ms | 16.8 ms |
| Terrace | 57.1 | 60.0 | 10.7 ms | 16.8 ms |
| Oblique city | 24.8 | 47.6 | 16.1 ms | 33.4 ms |
| Top-down city | — | 54.3 | 13.1 ms | 33.3 ms |
| 48 m spawn walk | — | 59.8 | 5.1 ms | 16.7 ms |

The close shop, spawn lane and street averaged 59.7–60.0 FPS. The walk
contained an occasional 33.4 ms interval. At 2560 × 1440, the maintained
benchmark's stationary market, terrace and oblique city cameras averaged
59.7, 59.7 and 36.0 FPS respectively. Broad city views still exceed the
16.67 ms delivered-frame budget. Local reports are in
`generated/runtime-performance/distance-clear-control` and `distance-1440p`.

To compare individual optimizations, use `clusterdraws=off`,
`glazingsinglepass=exact`, `texturechannels=full` or `shadowinstancecull=off`.
The Three.js adapter leaves the distant glazing approximation and stable fixture
slots to the host to enable. The staged game distribution uses its existing pin.

After `npm run build:native-viewer`, benchmark a published entry with
`npm run bench:published-city -- path/to/stoacity.glb --views market,terrace,city,top-down,spawn-walk`.
The runner serves the packaged viewer and the entry's neighboring resources
over loopback. The default run records GPU-completed throughput, drawing-buffer
size, per-frame submission time and screenshots. Add `--paced` for delivered
animation-frame intervals or `--passes` for GPU timer diagnostics. Use
`--resolution 2560x1440` for the higher-resolution pass. The supplied cameras
use Stoa coordinates. A different publication can provide camera positions and
moving endpoints with `--views cameras.json`.

The optional city delivery stage stores large, exclusively metallic/roughness
atlas pages as KTX2 alongside their core glTF fallback. To stage an existing
publication, run
`node tools/kowloon-export/city-data-textures.mjs path/to/stoacity.glb --out generated/compact/stoacity.glb --cli path/to/superspec-materials`.
The native encoder uses the exporter's linear UASTC data recipe and generates
mips at the authored resolution. Block encoding can change the sampled
roughness and metallic values. Pigment, normal and emitter maps retain their
original bytes. The staged entry references immutable companion files in its
own directory. Repeated builds verify cached payloads before reuse.

The Stoa trial encoded 158 pages in 448 seconds with four native workers.
A verified warm repeat took 3.07 seconds. The RTX uploaded 808.7 MiB of BC7
mips for those pages, saving 2.37 GiB relative to RGBA8. Comparing rendered
frames against the revised viewer with the original data maps gave mean
absolute differences of 0.043/255 in the market, 0.174/255 on the terrace and
0.179/255 in the oblique city. Tests of three individual textures found larger
local errors, particularly in filtered hardware roughness. The compact pack
averaged 58.8 FPS on the 1080p walk and 37.4 FPS in the oblique city in its
separate run. The measured advantage was GPU memory. The standard publication
continues to use its original metallic/roughness data. The staged entry and
integrity checks are under `generated/runtime-performance/mr-ktx2`.

### Smaller web downloads

Stage a browser delivery of an already published city with:

```sh
node tools/kowloon-export/city-web-delivery.mjs maps/stoacity.glb --out generated/web-map/stoacity.glb
```

The stage replaces eligible PNGs with lossless WebP only when the decoded
dimensions and every RGBA byte match and the replacement saves space. It keeps
geometry, material parameters and LOD index payloads, rebuilds the affected
content-hashed buffers, and writes a whole-pack size report. JPEG images keep
their original bytes. The web pack requires a glTF loader with
`EXT_texture_webp` support. Keep the core PNG/JPEG publication for consumers
that need it. Use a separate delivery directory for each variant.

Serve the GLB entry with revalidation and give content-hashed companion
resources an immutable cache lifetime. HTTP Brotli can still reduce bytes in
the binary buffers. Compare transfer sizes with the same HTTP encoding policy
on both packs. This stage leaves texture dimensions and LOD selection at
their authored values. GPU-memory and texture residency changes have their
own delivery paths. The stage accepts the core external-buffer city pack
before optional texture-residency pairs or GPU-compressed variants are applied.

### Interactive edit measurements

Run `npm run bench:world-edits -- initial.aml edited.aml next-edit.aml` against
the built native viewer. Each edited file contains the complete draft at that
step. Set `AML_FOLDER` to the initial project's folder when it has companion
assets, or `STUDIO_URL` to use an already running viewer. Set `BROWSER_ARGS`
to `--use-angle=d3d11 --ignore-gpu-blocklist` for the Windows GPU.

Use three clocks for WorldSpec edits. The first measures input through the first
frame showing the requested change. The second ends when the compiler's revision
is installed with its geometry and picking data. The third waits for the
receiver maps to finish baking and for another animation frame. The second clock
can stop several seconds before the third on a detailed building. Collision and
sampled navigation validation keep their own status.

The world editor can show a position-only edit using the displayed GPU meshes
and instance matrices. The compiler supplies the source owner of each instance
in a repeated building batch. The editor keeps the last accepted matrices and
bounds so a superseding input or rejected draft can restore them. Mixed edits
and edits without unambiguous placement provenance take the authoritative path.
Source-only changes pass through the shared Rust preview session. It compares
admitted AML trees, rebases source locations, and verifies the content of
companions it read. A matching displayed revision can keep its shaders, baked
maps, geometry and camera.

WorldSpec reuses ItemSpec design work and intrinsic tree geometry across edits.
Placements, local designs and material receivers keep separate identities.
For painted plants, the renderer can keep an unchanged patch's GPU resources,
including its matching shadow representation, after checking all its levels.
Native and Wasm clients can negotiate batch deltas. Each numeric channel sends
the interval from its first changed value to its last, compared by exact bits.
Unchanged channels share the previous buffer. A changed count falls back to a
complete batch payload, and a missing revision gets a standalone update.
Clients can independently request canopy references. Where a batch keeps its
stable ID, shape, twig coordinates and material parameters, it refers to its
previous canopy description. A newly planted or reshaped tree still carries
its complete description. On a batch-only edit, the browser also retains the
ordinary geometry and image buffers for neighboring buildings.

In MaterialSpec, material source, receiver layout, CPU fields and GPU map passes
have different dependencies. The shared authoring engine prepares an ItemSpec
placement once per project preparation and keeps a bounded cache of chart
records across revisions. A cache hit in the preview defers GPU bake-geometry
construction until a pass actually draws. Hybrid field textures are identified
by dimensions and RGBA content. Receivers with different names can share
identical output. Uniform material channels can reuse their map when chart
coverage and all inputs to that channel stay the same.

The default ItemSpec washroom has 115 receivers. Increasing the wall width from
2.700 to 2.701 m changes the front, back and edge charts. The reveal and sill
keep their charts but read changed weathering features. At 256 px, five map
sets were redrawn and 110 reused. The wall's concrete reads world coordinates,
normal, footprint, bounds and features. A blanket size tolerance in a raster
key would let new features display an old material and make the result depend
on edit history.

On the Ryzen 5 9600X and RTX 5060 Ti, using native Studio and the D3D11
browser, the washroom produced the following input-to-finished-frame timings:

| Edit | First after opening | Subsequent edits | Reused material receivers |
| --- | ---: | ---: | ---: |
| Add 1 mm of wall width, 256 px cap | 383 ms | 211–294 ms | 110 of 115 |
| Add 1 mm of wall width, 1024 px cap at 160 texels/m | 406 ms | 207–277 ms | 110 of 115 |
| Adjust tile or concrete parameters, 256 px cap | | 246 ms tile, 343 ms concrete | Only the two assigned receivers redrew |
| Shorten the washroom pipe by 0.3 m, 256 px cap | | 479 ms | 111 of 115 |

Each subsequent wall measurement used a new width. The first 1024 px wall
front was 1024 × 578 and a later width produced 1024 × 579. The standard
washroom material test and the ItemSpec fixture also completed from fresh
opens. Seven complete native shade/layout reports for different wall widths
matched the control byte for byte. The six small native responses alone had
a median of 86 ms in the candidate and 65 ms in the control during this run.
Before retaining the editor's handles and compiled GPU programs, the first
1 mm edit after opening the same project took 1.74 s at 256 px and 1.73 s at
1024 px. The handle now follows the resized wall with its original drawable;
incoming materials acquire their programs before the old materials release
them. Three consecutive edited frames recorded zero WebGL program links.

On the 778-object Stoa building, three raw native placement updates took
296–327 ms, down from 1,022–1,066 ms. A novel building width took 1.10–1.19 s
to produce the geometry update alone. Moving that resized design reused it and
took 297 ms. All nine decoded states matched the control.

| Same 778-object Stoa building in the viewport | Before, 3 runs | Final build, 2 edits |
| --- | ---: | ---: |
| Rename the AML document, finished frame | 3.28–3.71 s | 210–231 ms |
| Move the building, first visible frame | 9.03–10.82 s | 19–28 ms |
| Move the building, compiler revision installed | 9.03–10.82 s | 7.79–11.86 s |
| Move the building, finished frame | 15.96–17.78 s | 14.50–18.85 s |

The first visible move uses the previous material maps. An invalid subsequent
draft restored the previous placement and selection revision. When another
large render job overlapped the browser and the machine was paging, move
previews took 33–224 ms and finished materials took 51–61 s. This makes
memory pressure a practical consideration for running both tools together.

In a 20-tree Stoa planting, moving one tree took 297, 297, 333, 299 and 300 ms
from input through two frames after the accepted revision, compared with
1,308 ms on the earlier viewer. The browser received 134,136 bytes per move,
down from 56,249,680 bytes. Twenty canopy references replaced the repeated
shape descriptions and only 32 placement floats needed new values. The editor
kept the existing 76 draw objects of the 19 untouched trees. Images from three
camera stops, including moving away and returning, matched a fresh build of
the edited planting pixel for pixel. Opening that planting from scratch took
2.97–3.48 s.

An intrinsic TreeSpec edit also went through the compiler. Changing leaf
spacing from 0.15 to 0.1125 m increased instance count from 3,140 to 3,884 in
nine batches. Warm native responses took 57–76 ms through the complete 2.94 MB
packet and matched a fresh compile. This measurement ends before GPU upload.

The packed Stoa used for this review already holds 1,450 instance transforms
in 557 glTF instancing groups. The publisher partitions groups into street cells
to preserve culling. An intrinsic design can therefore be shared while runtime
draws remain spatially separate. Capture main and shadow passes separately and
retain instance identity through spatial splits when adding publisher records
for game picking, culling or streaming.

In that version, the main cost on a large placement edit was material preparation. A
translated Stoa shop house produced a 124,442,729-byte shading response for 739
receivers, including 45 MB of shading metadata, 23 MB of chart descriptions and
44 MB of triangle records. The preview rebuilt this response even
when the final delivery reused the building as a prototype. The following
measurements include the editor's placement reuse. A shared game
interface should carry a prototype's local geometry and surface charts once,
then associate stable placement IDs, transforms and world-dependent material
inputs with each copy. Stable texture-page IDs and texel density would also
let a game adapter stream mip levels from the same prototype. An adapter
could update only the affected contacts,
ambient occlusion and world-mapped passes. Spatial groups and independent
main-view and shadow visibility would still let the game cull each street.

The full Stoa AML still exceeds the single-scene editor's six-million-vertex
allowance at the second grass field (6,002,918 vertices). The published Stoa
uses spatially grouped prototypes. A whole-city editor benchmark will need
that representation as an input to incremental editing. Raising the allocation
limit alone would keep the same decoding, material and GPU costs.

### Interactive building edits

During a width drag the AML editor transforms the existing textured building.
For a supported four-corner taper it deforms the existing finish with the
two halves of the footprint. When the compiler returns the regenerated
triangles, unchanged surfaces keep their finished maps. Changed surfaces
can use their stock maps or procedural materials at physical scale while
MaterialSpec computes their final appearance. A temporary mass remains
available for unsupported geometry. Picking, collision and export use the
accepted revision. A newer draft supersedes either preview.

The earlier flat-geometry preview provides a baseline. On the four-storey
Stoa shop house (778 draw objects), native Studio's D3D11
editor viewport was 794 × 656 on an RTX 5060 Ti. Once the scene and its
materials had loaded, each gesture sent 60 AML drafts, one per browser
animation frame. Geometry and settlement times start at the last input.

| Edit | Input to drawn preview, median / p95 | Distinct preview draws | Regenerated triangles visible | Finished maps |
| --- | ---: | ---: | ---: | ---: |
| Move the building 2 m | 16.8 / 19.6 ms | 60 of 60 | Existing textured geometry follows the drag | 0.51 s |
| Grow lot width from 9.8 to 10.8 m | 16.7 / 18.7 ms | 60 of 60 | 1.07 s | 15.08 s |
| Taper one back corner by 1.5 m | 16.6 / 17.8 ms | 60 of 60 | 0.98 s | 11.39 s |
| Rename the selected building | 5.8 ms to the updated inspector | One discrete edit | | 1.41 s |

The move, width and taper gestures averaged 59.1, 60.1 and 61.3 drawn frames/s
over their respective sampling windows. Their first previews appeared after
20, 26 and 37 ms. The earlier viewer drew zero intermediate widths. The width
and taper geometry updates took 803 and 745 ms through the native endpoint.
An independent rollback test drew all 470,706 visible native triangles of its
test trapezoid in five temporary draw groups. The separate material responses for the two benchmark edits
were 105.3 and 100.9 MB. Ordinary triangle charts can reconstruct their surface
mapping from existing triangle records. On the original lot, sending explicit
mappings only for lattice charts reduced the report from 125.25 to 99.73 MB,
with identical triangle records.

The longest animation-frame gap after release was 46 ms on the move, 294 ms
on the width gesture and 291 ms on the taper. In a second run overlapping a
game benchmark, GPU memory use reached 15,626 of 16,311 MiB at 100% GPU
utilization. Width and taper dropped to 52 and 42 drawn frames/s and their
final maps took 59 and 41 s. The slowest background frame gaps in that run
were 5.66 and 4.55 s. All 60 distinct inputs still drew for each gesture.

These measurements cover the four-storey shop house and a pair of two-storey
buildings. The 12-storey, 14 × 14 m, seed-31 infill fixture generated 1,001,925
vertices against the then-current 1,000,000 placed-item limit. Two earlier
native builds also rejected it. Welding vertices with identical attributes
and derived tangents would save 425 vertices on this asset.

#### Building generation and portable delivery

The full-resolution building remains in the portable model, with its own
material slots, source ownership and collision. The native and browser compiler
share the building mesh conversion. The glTF writer can share identical
attribute and index streams across primitives, including when the imported
engine draws ordinary nodes. Use `--portable` for standard mesh nodes and PNG
or JPEG textures. The 11-storey tower's PNG GLB went from 71.89 to 59.03 MB
with identical referenced triangle attributes, placements, material images and
collision in the foundation check. The 12-storey fixture now builds under the
1.5-million-vertex ItemSpec ceiling and exports a 60.55 MB portable GLB.
[Portable export](../EXPORTS.md#portable-gltf) · [Resource limits](../AUTHORING.md#resource-limits-by-stage)

Four native Windows debug CLI launches used the 11-storey fixture in
before, after, after, before order. Mean cold generation fell from 10.06
to 9.35 seconds. Warm preview-ready and a placement edit stayed near 1.9
seconds. All corresponding scene packet and source-asset hashes matched.
[Native timing records](../../generated/runtime-performance/building-optimization/building-compile.json)

On the seven Stoa building prototypes, the publisher's receiver metadata read
used 9.33 MB of JSON compared with 463.54 MB of complete source GLBs. The median
read-and-parse time fell from 150.7 to 25.5 ms in five alternating local runs.
On building 0, packed PNGs went from 42.10 to 39.24 MB. The retained channels
matched across every page. These measurements cover the offline publisher's
metadata reads and texture packing.
[Material measurements](../../generated/runtime-performance/building-optimization/material-pack.json)

#### Material reuse during footprint edits

A flat plaster face can now read the common material tile even when its mesh
also contains a bevel. The bevel evaluates the original material in its own
surface coordinates. On the four-storey house this makes 133 of 136 plaster
receivers eligible for the tiled path, up from 41. The paint, repair, dirt
and contact passes keep their own dependencies. A wall can change its final
composite while the underlying stock tile stays resident. On changed faces
the provisional finish can look greyer than the settled facade while the
streaks below ledges and other receiver-dependent details are being rebuilt.

The same warm 60-draft D3D11 benchmark measured the following after these
changes. The drag column times each input through its draw submission. The
remaining columns start at the final input. Both shape gestures drew 60
distinct previews. All visible triangles in the regenerated geometry used
an existing finished map, a procedural surface, a stock texture or the
material's authored flat color.

| Edit | Drag median / p95 | Regenerated textured geometry | All maps settled, previous → current | Material response, previous → current |
| --- | ---: | ---: | ---: | ---: |
| Move 2 m | 16.2 / 21.6 ms | Existing geometry throughout | 0.51 → 0.45 s | No request |
| Widen by 1 m | 16.6 / 18.9 ms | 0.97 s | 15.08 → 8.61 s | 105.27 → 45.36 MB |
| Taper by 1.5 m | 16.5 / 19.1 ms | 0.91 s | 11.39 → 6.05 s | 100.92 → 26.32 MB |

The slowest animation-frame gaps during background work were 312 ms for
width and 222 ms for taper. Width recomposed 618 receiver maps and reused
144 finished receivers and 2,602 material graph nodes. Taper recomposed
402 maps, reused 331 finished receivers and reused 2,613 graph nodes.
Moving reused all 739 receivers. The editor claims maps for unchanged charts
first so that large new walls do not evict them from the bounded cache.
A separate one-millimeter width edit drew in 11 ms. Its regenerated geometry
arrived at 0.97 s, and final maps at 8.17 s. It reused 144 finished receivers
and 2,506 graph nodes. Before correcting the mapping key and bake order,
all 739 receivers required raster work on the same edit.

Native Studio can send just the chart and shader blocks the editor is
missing. Contact and crease coordinates use an exact binary encoding. A
cold client or an expired cache entry can request the entire description.
In a separate native replay, width and taper chart layout fell from 2.39 and
2.22 s to 0.17 and 0.11 s. The complete material requests in that replay
took 1.56 and 1.01 s. Every chart float and material description matched
the original full reports.

The portable game export combines the finished layers ahead of time. It
groups opaque charts with compatible channels on fewer texture pages at
the original texel density. On the published four-storey prototype, this
reduced material bindings from 41 to 37, runtime draw calls from 97 to 89,
and resident textures from 88 to 83. Both the orbit and facade views stayed
at the 60 Hz display limit. The encoded textures grew by 0.13% while their
total channel texels fell by 7%. These are single-building measurements.

Horizontal building placement now has a material origin. The finish stays
attached to the building as it moves. World elevation, contact geometry and
external inputs still participate in material identity. In a second scene,
moving one of two different two-storey buildings through 60 drafts averaged
59.0 drawn frames/s and settled in 489 ms. It made one 28.8 MB geometry
request and zero material-layout requests. A separate cross-building test put
two buildings and a pillar in contact. All 882 receiver descriptions matched
a joint three-model solve. Bringing a native AML
floor into contact with Stoa changed 16 of 739 receiver feature descriptions.
The other 723 matched exactly. A second check brought a city pillar against the
pavement. Three building receivers and one pillar receiver read the contact.
With a 100 m slab touching the base, the GPU rebaked 16 receivers and reused
723. Removing the slab recovered all 739 maps from cache. The contact edits
still transferred approximately 100 MB each and took 11–13 s to settle.
At 651 × 616, a comparison against a fresh render of the moved building found
14 pixels whose largest RGB channel difference exceeded eight levels out of
255. The equivalent contact comparison found six pixels.

The 256 px washroom test measured a first drawn tile roughness edit in 15 ms
and a concrete chip edit in 17 ms. Their final maps drew at 15 and 206 ms.
The two warm 60-input gestures each drew all 60 distinct revisions at 60.1
frames/s. Median input-to-draw was 16.6 ms, p95 was 17.7–17.9 ms, and maps
finished 200–215 ms after the last input. A small material whose per-pixel
shader is still compiling can redraw its resident atlas immediately. Larger
bakes and those waiting for new CPU fields keep their settling delay.

Run `npm run bench:world-edits` to generate the seeded shop house in memory
and replay its move, width, taper and selected-building rename. To measure a
different AML, pass the initial scene and successive edited files.
`--drag` interpolates one existing `position`,
`width`, `depth` or `footprint` value to the next file in up to 60 drafts. It
also accepts a directory of sorted AML snapshots for captured gestures or
edits that introduce new attributes. `BENCH_SELECT=city/prototype` selects
that owner before measuring its rename.

```sh
npm run bench:world-edits -- initial.aml --drag moved.aml --drag wider.aml renamed.aml
```

The report distinguishes the first WebGL draw submission, revision
acceptance, the settled frame and the longest animation-frame gap during
background work. Input-to-draw excludes display scanout. For the separate
washroom benchmark, set `STUDIO_URL` to the native Studio, `QUERY=gpuprobe`,
`BENCH_DRAG_FRAMES=60` and `RESOLUTION=256` and run `npm run test:washroom:browser`.

### Resizing into a baked wall texture

Run `npm run bench:world-edits -- --millimeter` for the four-storey Stoa building
at widths 9.800 and 9.801 m. It starts with the initial maps resident. The report
counts stock texels newly drawn and served from cache during the edit. Each use
of a patch contributes its area, so the reuse count can exceed the number of
distinct texels stored.

The anchored concrete base is sampled in metres from 512 × 512 patches. On
eligible flat faces at least 64 texels along an axis, the atlas packs that
dimension in 16-texel blocks when the existing texture allocation has room.
Each chart may add up to 5% more samples for this reserve.
The visible UVs crop the block to the actual wall. Extending the outline inside
it reveals samples at the original density. Bevels, UV seams and faces with
varying normals retain their ordinary sampling where necessary. Under cache
pressure the graph favors stock patches within the existing memory allowance.
Constant properties use one texel per distinct value and format.

Weathering that reads a sill, an opening, a nearby surface or another material
plane keeps those inputs in its cache key. A footprint edit can move those
features throughout a procedural building, so the time to settle all receivers
also includes work after the concrete base has been reused.

On an RTX 5060 Ti (ANGLE D3D11, 794 × 656), all three warm 1 mm trials drew
zero stock texels. Of the 842 final flat charts, 203 changed their visible UV
crop while keeping the same chart rectangle and physical sample grid. Median
input-to-draw was 14.3 ms, regenerated geometry appeared at 925 ms, and all
materials settled at 7.63 s (range 7.58–7.96 s). The previous reference run
settled at 8.17 s and redrew 177 stock patch channels. Of 3,200 final maps
read back from the GPU, 3,114 were byte-identical to a fresh build. The
remaining values differed by at most one storage step, also observed between
two independent fresh builds.

#### Tracing weathering through the editor

Use `npm run bench:world-edits -- --millimeter --trace-maps generated/stoa-weathering.json`
to compare both widths on the GPU. The diagnostic records the receiver name,
chart and grid identities, graph passes, texture sizes and final mip-zero pixel
differences. Run the same command without `--trace-maps` for latency. The timed
report includes a GPU fence after the fully shaded frame and the start and end
of each native request, measured from input.

Before the weathering changes, the resize reused all concrete stock patches
and still issued 5,817 atlas passes. On 739 receivers, 227 kept every final map
identical and 512 changed at least one map or its dimensions. The roof top and
underside kept their physical sample grid and atlas dimensions. Only 1.15% and
1.55% of their base-color texels changed. On the four ground-wall fronts, 13.31%,
23.85%, 39.36% and 6.11% changed in side order. Each map had been evaluated over
the whole surface. The diagnostic includes riser and plinth receivers that were
redrawn even though their final maps were identical.

The editor now indexes the feature segments eligible for each shader query.
For an empty set of weathering zones, a layer can reuse the underlying planes
when the compiler proves that every output returns its corresponding input.
For an isolated footprint edit, WorldSpec geometry and material layout also
start together. The regenerated scene must confirm the full material project
and contact key before accepting that layout.

Three warm 9.800 → 9.801 m edits on the RTX 5060 Ti at 794 × 656 gave
15.8 ms median first feedback, 1.07 s to regenerated geometry, 2.37 s to
accept the final material layout, and 5.06 s to display the fully shaded frame.
The GPU fence signaled at 5.20 s (4.97–5.92 s across runs). A repeat of the
earlier build on the same machine took 8.68 s at the fence. All three edits
drew zero new concrete stock texels. Empty-zone layers avoided 95 passes and
24,980,992 weathering texels. The warm layout payload fell from 42.20 MB to
15.34 MB with lossless deltas. Geometry and material requests started within
8 ms of one another. The initial open of each trial took 24.2–26.1 s.
Comparing the edited building with an independent fresh build at 9.801 m,
3,199 of 3,200 final GPU maps matched byte for byte. The remaining map differed
in two component values by one stored step. At the fixed acceptance camera,
10 of the 806 × 836 viewport pixels changed between the previous release
capture and the optimized edit. The largest change was 5/255 in one channel.
An independent comparison of the preserved release with the current release
reread the 110 edited maps flagged by the cross-build hash check. Of
116,166,656 stored components, 1,717 differed by one 8-bit step in painted
concrete color. All other components in that reread matched.

### Where geometry LOD enters the pipeline

A building footprint edit has three visible stages. The editor first deforms
the resident geometry, then displays the regenerated geometry with available
materials, then applies the finished receiver maps. Ordinary WorldSpec buildings
and ItemSpec meshes each compile at one geometry resolution per edit. Lower
texture mip levels are produced from the completed image on the GPU.

On a 12-logical-processor Windows machine, three consecutive warm 1 mm edits to
the four-storey Stoa building spent 853–883 ms in the native geometry request
and used 734 ms of Studio process CPU time each. One separate end-to-end edit
used 2.22 CPU seconds in Studio, 4.03 in the Chrome renderer process and 8.53
in Chrome's GPU process over an 8.09-second sampling interval. The GPU process
counter measures its CPU threads. GPU device occupancy was not measured.
In a final release check with another Rust build running, first feedback took
26 ms, regenerated geometry 1.21 s, and all receiver maps 9.36 s. Repeating
after the release builds, with a stable copy of the built viewer, took 13.2 ms,
1.16 s and 8.49 s respectively. Both runs drew zero new stock concrete texels.

The Stoa game publisher builds its distance meshes after it has the detailed
primitives. It can simplify several primitives concurrently and keeps the
levels for each primitive together so they share their input buffers. Game
LOD generation is outside both the editor drag and the material-settling path.
On the same Stoa city input (3.03 million eligible triangles), one worker took
21.13 s and four workers took 6.83 s. Peak process memory was 537 MiB and
1,142 MiB respectively. The scene entry and the 23,147,040-byte LOD sidecar
matched the published files byte for byte.

TreeSpec also has authored LODs. Kowloon caches its street tree and shrub
batches by their local definition and appearance, so moving a placement can
reuse all its levels. Avenue groundcover contributes only leaf sites and twigs
to a painted mass. Its mounds skip the two unused distance levels and keep the
near level for the existing vertex-budget check.

Standalone Item Studio can display the exact regenerated shape while an
expensive procedural finish bakes. For compatible resized meshes it carries
the accepted texture onto the new geometry. GLB export uses the completed
revision. In three native 512 px pipe-length edits on D3D11, the
median first draw moved from 327 ms to 7 ms and the finished maps appeared
at 162 ms. These timings include UI scheduling: submitting the shape control
now bypasses the typing debounce. A separate Wasm browser check on the 256 px
washroom rusted-steel finish drew the new pipe at a median 6.5 ms and finished
at 57.6 ms. Its final base color, normal and packed roughness/metalness maps
matched the native maps pixel for pixel.
