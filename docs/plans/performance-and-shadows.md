# Performance and shadow plan

This plan orders the remaining performance work for material compile, preview and
production rendering together with the shadow features, because they share
groundwork. It follows two independent reviews of the work in
[gpu-bake-and-cpu-build-performance.md](gpu-bake-and-cpu-build-performance.md) and
replaces the "Suggested order" of [next-improvements.md](next-improvements.md), which
stays as the backlog. Scene-level evidence uses the washroom scene
(`tests/studio/washroom`). The CPU compiler remains the authority for production and
export; a CPU optimization must not change baked bytes or native/Wasm parity.

## What the reviews and measurements established

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
- **GPU timings need re-measuring.** `settle()` reads the wrong framebuffer, so the
  "~3 ms per edit" figure may exclude GPU execution. Cold compile is the real cost:
  the driver finishes at first draw (0.7-0.9 s idle, 4-7 s beside a CPU build for the
  heaviest program).
- **Relief normals in the shader preview are half resolution** (screen derivatives
  per 2x2 pixels). A per-pixel fix is in progress; baked maps do not have the issue.
- **Illustration shadows cost little at runtime**: 2.1 ms to 2.3 ms per frame on the
  washroom. The look is being reworked (darker, bluer, wider gradient, visible noise).

## Phase 0: correctness before more speed

1. Rust (in progress): the lane-evaluator uniform-field bug with regression tests and
   a lanes-vs-scalar check of every example and the washroom; bounded share rows;
   field-plane budget global to a run and sized from the real atlas; no retention of
   empty planes; SBR2 length and finiteness validation; allowance parity.
2. Browser (next for the GPU agent): detect link failures at first use, not only at
   `compileAsync`; evict compiled programs; no field bake from a stale revision and no
   cancelling a newer revision's request; no GPU preview over a failed CPU build;
   redraw the shadow map and clear `drawn` on context restore; count viewer texture
   units correctly (7-8, not 6); a negative cache for sources that fail to compile.
3. Measurement: bind the bake target (or use a fence / timer query) in `settle()` and
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
  implemented by the same module in Studio and the game runtime.
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
