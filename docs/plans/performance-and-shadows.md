# Performance and shadow plan

**Ordering: see [roadmap.md](roadmap.md).** Phase 0 here is roadmap stage 0; Phases 1
and 2 are stage 1; Phase 3 and lighting export are stage 4; shadows as material
inputs are stage 5. Sharing intrinsic appearance across receivers is now delivered
through weathering layers (roadmap stage 2).

This plan orders the remaining performance work for material compile, preview and
production rendering together with the shadow features, because they share
groundwork. It follows two independent reviews of the work in
[gpu-bake-and-cpu-build-performance.md](gpu-bake-and-cpu-build-performance.md) and
replaces the "Suggested order" of [next-improvements.md](next-improvements.md), which
stays as the backlog. Scene-level evidence uses the washroom scene
(`tests/studio/washroom`). The CPU compiler remains the authority for production and
export; a CPU optimization must not change baked bytes or native/Wasm parity.

## Measured state (2026-11-16)

All on the washroom at 256 px, RTX 5060 Ti through ANGLE/Direct3D 11
(`BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist"`), headless Chromium,
`tests/material-washroom-browser.mjs`. Ranges are over the night's runs on a machine
that was also building; a busy run is up to twice as slow.

- **First picture: 1.4-1.7 s** after the project is opened, and it is the finished
  one: GPU-baked maps on all 11 meshes (64 textures, 18 MB). Of that the bake
  programs' compile is 0.64-0.72 s, the CPU fields of the hybrid programs 0.23-0.35 s
  and drawing every map about 45 ms. A frame of the baked scene is 0.86-0.87 ms of GPU
  time with ambient occlusion and styled shadows.
- **Looped and inline noise.** The engine emits each program in two forms (`glsl.rs`):
  lattice noises evaluated by loops over arrays, which Direct3D compiles about three
  times faster, and each noise where it is used (`#define MS_INLINE_NOISE`), which
  draws faster: a frame while a concrete parameter is dragged (two wall meshes on
  their per-pixel look, the rest on maps) is 3.1-3.6 ms of GPU time looped and
  1.00-1.07 ms inline (timer query, the washroom test's `Drag frame time` with
  `QUERY=gpuprobe`; the heavier a program's noise, the wider the gap). A program
  drawn once should be looped and one drawn every frame inline, so:
  - bakes use the looped form (each texture is drawn once);
  - `?preview=shader` shows the looped form first and swaps to inline behind it
    (first picture 1.8 s, per-pixel relief 4.4-4.6 s, inline everywhere 11.7-12.5 s);
  - the default view keeps a per-pixel look per mesh for edits. It starts looped and
    is replaced by the inline form once the scene is baked and at rest, a program at
    a time, 5.5-6.2 s after the scene was complete (`settleEditLooks` in
    `scene.ts`). Nothing waits for it: an edit made meanwhile draws the looped look
    and its latency is unchanged, and a scene rebuilt by a source edit starts with
    the inline programs the scene before it still holds (baked, with its per-pixel
    looks ready, 0.27 s after the edit). `data-noise` says `inline` when done;
    `?noise=looped|inline` forces a form.
- **Pass graph.** Every GPU bake is a list of keyed nodes (`bake-graph.ts`), and a
  node is drawn only when no texture holds its key. Since tonight each texture of an
  atlas bake is keyed by the uniforms it reads, not by every value
  (`attachmentReads` in `atlas.ts`): a tile's glaze roughness draws 1 texture per
  mesh instead of 5, a concrete chip amount 4 of 5 (roughness and metalness do not
  read it), and the normal is drawn only when the height or the normal offset is. A source edit that leaves a
  program as it was draws nothing (64 of 64 textures reused); one that changes the
  tile's program draws its 10 textures. `?bakekeys=whole` restores one key per bake
  for comparison (the pictures are identical). `tests/material-bake-keys-browser.mjs`
  (`npm run test:bake-keys:browser`) changes each of the washroom's 120 parameters
  and compares every texture with a bake of everything: none differs, and 467 of
  the 969 textures were not drawn again.
- **The shader record** now carries `id`, `uniforms`, `samplers`, `live[].classes`
  and `entries`; the host uses the first four in place of hashing and reading the
  GLSL (`shaderIdentity`, `programLayout`). It still reads the source for one thing:
  two live arguments that are one field (`ms_f20 = ms_f7;`) share a texture channel,
  and `classes` names each argument's own field. Naming the repeated field in
  `varying` would remove that. Per-output dependencies are not in the record:
  `attachmentReads` derives them from the uniforms each entry point's text names,
  which is sound while entry points expand every helper, and reads `shader.reads`
  (output name to uniform names, `live` for the live fields) first when the engine
  adds it.
- **1024 px.** A preview project holds at most 512 texels a side; the washroom at
  `RESOLUTION=1024` was refused by the Studio and the test waited for a revision
  that could not come (the run that "sat idle"). The test now gives such a project
  the production profile and stops at once when the Studio reports an error; with
  `?preview=bake` at 1024 px the first preview is 1.5 s and the scene is loaded in
  5.1 s.

- **A texture leak in the viewer, fixed.** three.js's shadow pass shares one depth
  material between meshes and leaves the last `map` it was given in the uniform when
  the next mesh has none, so the first shadow map drawn after a textured scene was
  replaced uploaded that scene's last image again, into a GPU texture nothing freed
  (one per such replacement). The texture pool now retires a texture so that it
  cannot be uploaded again (`textures.ts`). `tests/rust-scene-browser.mjs` found it:
  its supersession, late-failure and close-during-decode cases had never run,
  because they held a decode that the pool made unnecessary.

What remains, in order of what an author would notice:

1. The 0.64-0.72 s of bake-program compile is most of the first picture. Whether the
   browser's program cache answers a second load in the same profile is measurable
   (`npm run bench:shader-compile` with `PROFILE`) and was not measured tonight.
2. The first edit after a load draws the looped look for up to 6 s. Compiling the
   inline form of the selected material first would shorten that for the material
   being edited.
3. Per-output dependencies and the repeated-field rule from the engine (above), then
   the same per-texture keys for swatch bakes, which still key every texture by
   every value.
4. An intrinsic domain and layer composition from the engine, so the intrinsic and
   mapping nodes (built and tested, unused) can share one bake between receivers.
5. Phase 2 below is untouched by tonight's work.

## What the reviews and measurements established

The first two points predate the GPU-baked scene view and the looped noise form; the
section above has the current figures.

- **The default scene view does not bake.** Scene projects render each material as a
  per-pixel shader (`compileLive`, "shader preview"). The washroom's ~6 s load on an
  RTX 5060 Ti is scene composition plus driver shader compiles, not CPU baking; with
  `?preview=bake` the first preview is ~2.6 s and loaded ~5.8 s (single runs, busy
  machine). CPU bake speedups therefore reach swatch editing, the baked scene mode
  and export, not the default scene load.
- **CPU bakes are faster where comparable.** Against the pre-work commit, on the
  materials it can build: concrete 1.6-1.9x, a 10-material washroom subset 2.3-3.5x
  (includes threading and the feature-grid index). Output differs from that commit
  only through texel-density sizing, which predates the speedups (medium-high
  confidence; confirm by rebuilding without `texelDensity`).
- **A confirmed correctness bug exists in the row-wise evaluator** (a field cached as
  uniform from a partial mask), plus memory-bound and share-parsing defects. Being
  fixed.
- **GPU timings, re-measured.** `settle()` used to read the canvas instead of the bake
  target; it now reads a texel of the target each pass drew into. Re-measured on the
  RTX 5060 Ti (D3D11), a warm 256 px bake is 3-4 ms in all (pass 1 about 1-3 ms, mips
  0.5-2 ms, normals 0.4-0.9 ms) and a 512 px one about 3.6 ms in two bands, so the
  "~3 ms per edit" figure stands. From the edit to the preview on screen: 19 ms for
  the quick stage and 36 ms for the full 256 px stage of a light program, 80-85 ms at
  512 px or after a source edit, about 120 ms when a CPU field is baked again, and
  140-210 ms for the heaviest program (Kowloon concrete) with the default shadow style
  and ambient occlusion in the viewer's frame. The washroom frame is 2.1 ms of GPU
  time (timer query) with styled shadows and 2.7 ms with ambient occlusion as well.
  Cold compile is the real cost: the driver finishes at first draw (0.7-0.9 s idle,
  4-7 s beside a CPU build for the heaviest program).
- **Relief normals in the shader preview were half resolution** (screen derivatives
  per 2x2 pixels). They are per pixel now; because that about doubles the programs'
  compile, the first look still uses the derivative relief and each program is swapped
  in the background (washroom, real GPU: first look about 5 s, per-pixel after about
  14 s). Baked maps do not have the issue.
- **Illustration shadows cost little at runtime**: 2.1 ms to 2.3 ms per frame on the
  washroom. The look is being reworked (darker, bluer, wider gradient, visible noise).

## Phase 0: correctness before more speed

1. Rust (in progress): the lane-evaluator uniform-field bug with regression tests and
   a lanes-vs-scalar check of every example and the washroom; bounded share rows;
   field-plane budget global to a run and sized from the real atlas; no retention of
   empty planes; SBR2 length and finiteness validation; allowance parity.
2. Browser (done, see `tests/material-gpu-preview-browser.mjs`): detect link failures at first use, not only at
   `compileAsync`; evict compiled programs; no field bake from a stale revision and no
   cancelling a newer revision's request; no GPU preview over a failed CPU build;
   redraw the shadow map and clear `drawn` on context restore; count viewer texture
   units correctly (7-8, not 6); a negative cache for sources that fail to compile.
3. Measurement (done; figures above): bind the bake target (or use a fence / timer query) in `settle()` and
   re-measure every GPU figure on the real GPU. Add timer-query frame time to the
   washroom test. No performance claim below is accepted without these.

## Phase 1: the scene view (preview latency and frame cost)

The scene view is what authors look at, and it is the slowest path.

4. **GPU bake in UV space for scene projects.** Rasterize each mesh in atlas-UV space
   with world position and normal varyings (plus dilation), generalizing `GpuBaker`
   beyond the swatch quad. Take the atlas layout from Wasm instead of duplicating it
   in TypeScript. Result: baked-map materials in the scene view (viewer cost per frame
   instead of the whole program per pixel), texel-resolution filtered normals, the
   same textures a game would use, and the hybrid lane per receiver. Keep the
   per-pixel shader as the instant first look while the bake completes.
5. **Cold compile.** In order of expected payoff, each measured on the washroom and
   the heaviest starter: prewarm after first ready and serialize the GPU compile
   after the CPU build (do not compete for cores); one compile in flight, latest
   wins; specialize the GLSL runtime so the HLSL backend inlines less (separate 2D/3D
   and gradient/cellular paths, uniform-bounded loops, only the functions a program
   uses, common-subexpression hoisting in the emitter); test whether the first-draw
   stall is a second pixel-shader variant for the five-buffer layout and, if so,
   choose attachments per program.
6. **Live-argument passes.** Stop recompiling and re-evaluating the whole program per
   16 arguments: write them into spare draw buffers of pass 1 or emit a restricted
   entry point; pass position-independent arguments as uniforms.
7. **Viewer jank.** Keep one material per mesh and swap maps in place; decode PNG
   results off the main thread; remove synchronous read-backs from the edit path.

## Phase 2: CPU compile and production output

8. **Keep the authoring worker alive.** A superseded build terminates the worker, so
   per-map and per-field retention is lost and the next build is cold. Use cooperative
   cancellation (the Rust baker already polls) for the main and lane workers.
9. **Make retention work with distribution.** Field planes are used only by the local
   serial evaluator, and browser bakes of 8,192 texels or more are distributed, so
   retention is rarely used there. Bake locally when planes cover most dirty work, or
   let helpers keep planes.
10. **Row-wise evaluator on feature-heavy receivers.** Its decline bound assumes every
    feature is visited, so receivers with many features run fully scalar. Sub-batch
    rows or charge actual visits.
11. **Hybrid field bakes.** `bake_fields` re-decodes images, rebuilds the atlas and
    evaluates with the scalar evaluator on every call. Use cached artifacts, the row
    evaluator and a key like field planes.
12. **Per-build overhead.** Cache image digests, receiver digests and the dependency
    table; one evaluator and one contiguous run of rows per thread; fewer per-texel
    clones; integer wrap instead of float remainder in image sampling on Wasm.
13. **Production encode.** Encode maps on helper workers in the browser (serial
    today); measure KTX2; reuse encoder workers; pre-bake production output in idle
    time once cancellation is cooperative. Faster deflate or row-split deflate changes
    bytes and needs an explicit decision.
14. **Persistent bake cache** keyed by the complete bake key, for reopen and CI.

## Phase 3: runtime rendering in a game

15. Ship baked KTX2 maps, not per-pixel program materials. Share materials by
    signature and batch static meshes. Give live textures mipmaps; trim the
    environment-filter taps; use one anisotropy setting for preview and final.
16. Static scenes keep an on-demand shadow map; dynamic casters are handled separately.

## Shadows, in step with the phases

- **With Phase 0-1 (now): runtime illustration style.** Finish the darker, bluer
  style with a distance-dependent penumbra and visible noise. Fix the texture-unit
  count first, since every shadowed material spends a unit on the shadow map. The
  style is a global rendering setting of the scene, not part of any material: it is
  not a `LiveOperation` or a MaterialSpec construct, and no material turns it on or
  parameterizes it. Then define the portable contract: a scene-level rendering record
  (`shadowStyle`) stored with the project or scene and delivered in the model packet,
  implemented by the same module in Studio and the game runtime. The Studio viewer
  now holds these settings in one `RenderingSettings` record
  (`browser/renderer/rendering.ts`), on by default, filled from the page URL.
- **Screen-space ambient occlusion (done in the Studio viewer).** Also a global
  rendering setting of the scene (`ambientOcclusion` in the same record, on by default,
  `?ao=off`): three.js's GTAO pass with its denoise, a radius in scene meters, drawn
  through a multisampled linear target with the renderer's tone mapping applied
  afterwards. It is distinct from the planned baked `occlusion()` material field
  below, which is authored per material and baked from the occluder service; the
  screen-space pass knows only what the camera sees. It costs about 0.6 ms of GPU time
  per washroom frame on the RTX 5060 Ti and about a third more frame time under
  SwiftShader. Remaining: carry it in the same scene-level record as the shadow
  style, and apply it to ambient light only (it multiplies the finished image).
- **With Phase 1: instant preview of baked shadow fields.** The UV-space bake has each
  texel's world position, and the viewer already renders a sun shadow map. Sampling
  that map in the bake pass previews `shadow(...)` fields on the GPU at no extra
  scene cost, before any CPU ray casting exists.
- **With Phase 2: the occluder service and CPU shadow fields.** A content-hashed BVH
  with deterministic ray casts gives the authoritative `shadow`, `occlusion` and
  `sunExposure` fields, baked at low resolution through the hybrid lane, with a
  selective dependency on occluders in the bake key.
- **Pass graph (shared mechanism).** Materialized intermediate textures serve three
  needs at once: blur and edge-distance of shadow masks, splitting heavy programs
  into smaller shaders for cold compile, and the live-argument passes.
  The mechanism exists (`browser/renderer/bake-graph.ts`): every GPU preview bake,
  atlas and swatch, is a list of named nodes with keys; a node is drawn only when
  no texture holds its key, and nodes are shared and kept by the graph. A separable
  blur node and the intrinsic-plus-mapping node shape are built and tested
  (`tests/browser/bake-graph.test.ts`, `tests/bake-graph-browser.mjs`) but not used
  by any material yet: that waits for the engine to emit an intrinsic domain,
  layer composition inputs and per-output dependencies.
- **With Phase 3: baked stylized shadows for static scenes.** Bake the styled shadow
  (blur, noise) into a per-receiver shadow mask through the pass graph, applied in the
  material by one texture fetch. A static level then needs no shadow-map pass and no
  filter taps at runtime; dynamic objects keep the runtime style.
- **Placement by shadow** stays last.

## Lighting and rendering settings spec (required before the settings spread)

Shadow style and ambient occlusion are being prototyped as Studio viewer query
parameters, and the viewer's lights are hard-coded in `browser/renderer/scene.ts`
(sun direction and intensity, hemisphere light, environment intensity, tone mapping)
while `tests/studio/washroom/_view.mjs` hard-codes different ones. Before these
settings are used anywhere else, define one clear specification for lighting and
rendering settings, shared by MaterialSpec, ItemSpec, WorldSpec, the Studio renderer
and the three.js game runtime. This is a design task with its own document
(`docs/plans/lighting-settings.md`) and review; nothing below is decided yet except
the scoping rule.

The spec must define:

- **Scope.** Which settings are global to a scene (ambient occlusion, exposure and
  tone mapping, environment light, ambient light) and which belong to a light source
  (direction or position, color, intensity, whether it casts shadows, and its shadow
  style: softness and penumbra growth, edge and interior noise, tint, darkness, seed).
  Shadow style and AO are never part of a material. A material may only read a
  light's shadow or exposure as an input field.
- **Records and ownership.** Portable records in `superspec-types` and
  `superspec-model`, delivered in the model packet and stored with a project or
  scene; how WorldSpec themes, ItemSpec previews and Material Studio supply or
  override them; one shipped default (the washroom look) and named presets.
- **Units and conventions.** Meters, right-handed Z-up, linear color, radians;
  physically meaningful parameters (penumbra and AO radius in meters) rather than
  renderer-specific ones (shadow-map texels, kernel taps).
- **Renderer contract.** What every consumer must implement, what it may approximate,
  and how an unsupported setting is reported rather than silently dropped (as live
  operations do). The Studio viewer and the game runtime use the same module.
- **Agreement between runtime and baked results.** Baked `shadow`, `occlusion` and
  `sunExposure` fields, GPU previews of them and placement surveys read the same
  light definitions, so baked and live shadows describe the same sun.
- **Export.** What a GLB can carry (`KHR_lights_punctual`) and what needs a sidecar
  or extension for style and AO.
- **Determinism and versioning.** Seeds for noise, a version on the record, validation
  with source-linked errors, and which settings affect cache keys (none for runtime
  style; light definitions do for baked shadow fields).

Until the spec exists, new lighting or shadow settings stay in the Studio viewer
behind query parameters and one scene-level settings object, and are not added to
AML, material programs or export.

## Decisions (made by the user, 2026-11-16)

- Shadow styling and ambient occlusion are global or per-light rendering settings,
  never part of a material, and need the shared lighting settings spec above.

- The scene view defaults to baked maps from the GPU (item 4); the per-pixel shader
  is only the first look while the bake completes.
- A deliberate byte change for faster PNG/KTX2 encode is acceptable (item 13). It
  must come with a cache and identity version bump, be made once, and be recorded
  with before/after sizes and timings; pixels must not change.
- The illustration shadow style becomes the default once its look is approved.
