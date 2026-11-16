# Next improvements after the GPU preview and bake-speed work

Follows [gpu-bake-and-cpu-build-performance.md](gpu-bake-and-cpu-build-performance.md).
That plan is done through the GPU preview tier, the hybrid CPU/GPU lane (`runoff`),
GPU live-operation arguments, the dedicated lane worker, per-map and per-field
retention, and row-wise evaluation. This file lists what remains, ordered by value
against risk. Items marked **measure first** need a profile or real-hardware
numbers before a design is chosen. The standing rule applies: the CPU compiler is
the authority for production and export, and a CPU optimization must not change
baked bytes, identities or native/Wasm parity.

## Where things stand (from measurements)

- CPU cold bakes are unchanged by edit-side work (about 0.5 s concrete at 256 px,
  1.7 s at 512 px, 5.7 s at 2048 px production). Edits are far faster (1.3–8.7x), but
  a first bake, an opened project and a structural edit still pay the full evaluator
  cost, which is dominated by tree-walk dispatch and value copies (57–58% of time).
- Serial PNG encode is 0.65–1.1 s at 2048 px, up to 53% of a production build
  (courtyard), because the largest single map bounds the parallel encode.
- GPU preview timings are software-rendered only. Real-GPU behavior, async shader
  compile, context loss and the sRGB attachment path are unverified.
- Rusted-steel's GPU preview is much darker than its CPU result (root cause under
  investigation).
- Kowloon world-surface rasters (cold 27–31 s courtyard) are untouched; see the
  out-of-scope section of the first plan.

## A. Validate and harden the GPU preview

1. **Real-GPU validation (measure first).** Run the GPU suite with
   `BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist"` (flag already in
   `tools/browser.mjs`) on this machine and, if possible, an integrated and a mobile
   GPU. Check the sRGB attachment write, half-float emissive, `KHR_parallel_shader_compile`
   benefit, band size against the watchdog, and publish real latency numbers.
2. **Context-loss and failure tests.** Force `WEBGL_lose_context`, compile errors and
   missing float targets in tests; assert the CPU path takes over with a recorded
   reason. Add the dedicated render-target eviction test.
3. **Lane worker cost.** The lane duplicates the Wasm instance; stale requests are
   abandoned rather than terminated. Share the compiled module, cap memory, and
   terminate a stuck lane request after a timeout.
4. **Scene projects and non-swatch receivers.** The preview only covers swatches
   today. Extend the domain computation to project receivers and multi-chart atlases
   (reusing the CPU atlas layout through a Wasm call rather than duplicating
   `swatchDomain()` in TypeScript, which currently skips silently if packing changes).
5. **Explicit final render control.** Add a "Render final" action (as in Blender's
   viewport vs. render) next to the preview label, showing which lane produced the
   current view and offering the authoritative CPU result on demand.
6. **Capability badges in the editor.** Show per node whether it runs on GPU, hybrid
   (CPU-baked field) or CPU only, and why a program cannot preview, so authors know
   what blocks instant feedback.

## B. Generalize the hybrid lane

7. **Capability tags in the catalog.** Replace the ad hoc `runoff` detection in
   `glsl.rs` with per-operation capability metadata (`gpu`, `cpu-field`, `cpu-only`)
   used by both the shader planner and the editor badges.
8. **More CPU-field operations.** Receiver-feature queries beyond swatch rims,
   image-derived fields, and future filters. Each must declare which parameters,
   seeds, images and edges it reads so `deps.rs` drives its cache key.
9. **Texture-space filters as passes (design first).** Blur, erosion, distance
   transforms and flow need a materialized intermediate texture, so the graph is cut
   into passes: fuse pointwise runs into one shader and cut at neighborhood
   operations. A cut texture is produced by a GPU multi-pass or by the CPU. This is
   the substrate for the "unusual rendering techniques" planned later.
10. **Warped-coordinate CPU fields.** A CPU field read at non-grid coordinates is
    refused today. Define bilinear sampling of cached field maps for preview, clearly
    marked approximate, plus the rule for when it is allowed.
11. **View-dependent and screen-space operations.** Keep them renderer-only and put
    them behind the portable `LiveOperation` contract so the three.js game runtime and
    Studio implement the same set.

## C. Cold-bake speed on the CPU

12. **Evaluator dispatch (measure first).** Dispatch and value copies are 57–58% of
    cold time. Candidates, each gated by identical output: compile the checked program
    to a flat register bytecode with preallocated slots; specialize the node graph to
    closures; widen the row evaluator's lane loops with f64 SIMD kernels that keep
    per-sample operation order. Native lane kernels for builtins already measured no
    gain, so start from a fresh profile of the lane evaluator itself.
13. **Coarse-to-fine reuse.** A 64 px draft and the full bake sample different points,
    so nothing is reused. Evaluate whether a hierarchical sampling order (draft texels
    are a subset of the full-resolution grid) can make the draft a prefix of the full
    bake, so refinement completes rather than restarts. Must remain byte-identical for
    the final size.
14. **Viewport-driven baking.** Prioritize rows or tiles that the camera sees; leave
    the rest to the full bake. Needs a tile-granular cache key.
15. **Threaded preview in the browser.** Preview bakes run on the calling thread;
    reuse the Wasm helper pool (`bake-pool.ts`) for preview-size bakes when it beats
    its coordination cost (measure first, including Wasm timings, which were not
    measured).
16. **Persistent bake cache.** Content-addressed on-disk cache keyed by the complete
    bake key plus a format version, for repeat cold runs in CI and the CLI. Needs
    integrity checks, size bounds and concurrent-writer safety.

## D. Production encode and export

17. **PNG encode (measure first).** The largest map bounds wall time at 2048 px. Faster
    deflate (libdeflate or zlib-ng), a faster filter selector, or row-split deflate
    would change bytes; that requires a deliberate cache and identity version bump,
    agreed first, not a silent change.
18. **KTX2 encode.** Never measured; basisu is likely the heaviest stage of a full
    export. Measure per stage, then parallelize or cache per map.
19. **Export-side reuse.** Skip re-encoding maps whose bake key is unchanged between
    exports.

## E. Kowloon world surfaces (previously deferred)

20. Decoded-image cache with bounded storage (140 decodes for 47 assets in the
    courtyard), cross-host job coalescing, and intrinsic/receiver texture sharing to
    cut the 3,821 cold bakes. See [extraction-reuse.md](../performance/extraction-reuse.md)
    and [selective-bakes.md](../performance/selective-bakes.md). Optional GPU baking
    of those recipes via the study shaders is a separate, later decision.

## F. GPU baking in shipped games (decision needed)

21. Using the GPU bake at load time in the three.js runtime is faster but not
    bit-identical, which conflicts with deterministic cache keys and byte-identical
    exports. If wanted: an explicit opt-in profile, never mixed with CPU artifacts,
    with its own cache namespace. Default stays CPU-compiled assets.

## G. Quality and process

22. Replace `tests/tmp-*.mjs` diagnostics with kept probes or remove them.
23. Real-worker coverage for the distributed bake protocol (`bake-pool.ts`, SBR2
    shares) in a browser test; today only Rust tests and a Node replay cover it.
24. A CI or local benchmark guard: `bench_fingerprint` plus the identity matrix, so a
    pure optimization that changes bytes fails immediately.
25. Split the broad commit `15fd24e` only if a clean history matters; it mixes earlier
    uncommitted work with the agents' changes.

## H. Shadows as authoring inputs

Priority set by the user: baked appearance and runtime shadow styling matter;
placement by shadow matters least. "Shadow" is three separate things sharing one
occluder service.

26. **Occluder service (groundwork).** A content-hashed BVH over the lowered scene
    geometry and a deterministic ray cast (`program/numeric.rs` math, identical on
    native and Wasm). It becomes a receiver input next to the edges `runoff` and
    `creaseDistance` read.
27. **Baked appearance fields (important).** `shadow(sunDir, softness)`,
    `occlusion(radius)` and `sunExposure(...)` (visibility averaged over several sun
    directions, usually the better weathering mask). They are CPU-only fields, so the
    hybrid lane serves them: the CPU bakes the field, the GPU preview samples it. Bake
    them at low resolution and filter, since shadows are smooth and many rays per
    texel are slow in Wasm. Rules: the occluder set joins the bake key through a
    selective dependency (only occluders that can reach the receiver), swatches get a
    stand-in occluder, and baked shadow is an appearance mask, never a darkening of
    base color that the renderer's own shadows would double.
28. **Mask helpers.** Pointwise use (mask, threshold, noise-broken edges, remap)
    already works on any field. Blur and edge distance are neighborhood operations and
    need the pass graph (item 9): materialize, filter in meters using each chart's
    texel density, never bleed across chart boundaries.
29. **Runtime shadow styling (important).** Renderer-side live operations behind the
    portable `LiveOperation` contract: shadow softness (shadow-map radius or variance
    shadow maps), noise-jittered shadow edges, shadow tint, and the light's shadow term
    exposed as a mask for other live effects. Dynamic lights only; implemented
    identically in Studio and the three.js game runtime; not representable in a plain
    GLB. Builds on the Studio viewer's default-on shadows.
30. **Placement by shadow (least important, later).** A light survey over ground and
    wall samples exposing `shadowAt(point)` / `sunExposure(point)` as placement
    predicates. CPU-only and deterministic; surveyed on architecture first, with a
    defined second pass for prop-on-prop shadows.

Order: 26, then 27 and 29 in parallel (29 does not need the occluder service), 28
with the pass graph, 30 last.

## Suggested order

1. A1–A2 (real GPU numbers and failure tests): cheap, and they decide how much to trust
   everything else.
2. C12 profile and A4/A5 in parallel: the biggest remaining CPU win and the largest
   user-visible gaps.
3. B7–B8 (catalog tags, more CPU fields), then B9 (passes) before any new filter work.
4. D17–D19 once the encode decision is made.
5. C13–C16 by profile; E and F only on explicit decision.

## Decisions needed

- Is a deliberate byte-change for faster PNG/KTX2 encode acceptable (item 17)?
- Is GPU baking in shipped games wanted at all (item 21)?
- Should Kowloon world builds (item 20) come back into scope next?
