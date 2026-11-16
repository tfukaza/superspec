# MaterialSpec: GPU preview baking and CPU bake speed

Scope is **MaterialSpec programs** (the `.material.ts` / `.ms` language and its
authoring session). Kowloon world-surface `Recipe` rasters, large procedural
environment builds and the courtyard cold/warm work are deliberately out of scope
for now; their baselines are kept below only as context.

Two tracks with one rule: **the CPU compiler stays the authority for production,
export and cached artifacts; the GPU is a fast, approximate preview.** This is the
Blender split (viewport vs. final render). Native and Wasm CPU semantics stay shared
and byte-identical ([AGENTS.md](../../AGENTS.md)); the GPU path is a separate,
explicitly non-authoritative profile. The GPU track comes first; the CPU track
(goal: as fast as possible, with unchanged output) runs alongside it.

Read first: [material language](../material-language.md),
[material-typescript.md](../material-typescript.md),
[program README](../../crates/engine/src/materialspec/program/README.md).

## Starting point (verified in the tree)

- `program/glsl.rs` already compiles a checked program to GLSL ES 3.00
  (`Program::shader`), and `browser/renderer/shade.ts` injects it into a three.js
  material: a **per-fragment preview exists**. Parameters and seeds are uniforms.
  Known gaps stated in `glsl.rs`: `runoff` reads no edges, receiver features,
  footprint from screen derivatives, both `select` branches evaluated.
- There is **no GPU bake-to-texture**. Each edit rebakes on the CPU
  (`authoring/material.rs`, `bake_rows` → `bake_block` → `Evaluator`): a 64 px draft
  (~0.45–0.55 s) then full size (~3–4 s at 256 px; bake alone ~0.28 s at 32 px,
  0.47 s at 64 px, 2.9 s at 256 px, headless software rendering, order-of-magnitude).
  material-typescript.md records "GPU evaluation and skipping maps a parameter does
  not affect" as not done.
- [AUTHORING.md](../AUTHORING.md) and [authoring-implementation.md](authoring-implementation.md)
  still say the backend does not compile to GPU shaders; they predate or ignore
  `glsl.rs`.
- The CPU evaluator is a per-texel tree walk of "a few thousand nodes per texel". It
  is already tuned (compiled indices, slot writes, reused field table and mip
  pyramids, height reuse between adjacent texels, texel dedup, row sharing across
  Wasm helper workers). `bake_block` and `bake_rows` handle blocks of rows
  (`BLOCK_ROWS`), cancellation, and `threads`.
- Profiles: `preview` (≤512 px, calling thread) and `production` (≤2048 px, up to the
  4 MiB-texel atlas allowance, threaded, harder PNG compression). Both write the same
  pixels at a given resolution; their cache keys differ.

## Track A: GPU baking (preview only)

Browser/Studio only. No native, Wasm-compiler or export changes. No parity gate, by
decision: GPU output is a preview. It must still be impossible to mistake for
production output.

0. **Reconcile and scope.** Update AUTHORING.md and authoring-implementation.md to
   describe the GLSL preview and the new GPU preview bake. List what the shader cannot
   express (`runoff`, `features`, whatever `shader()` rejects); such programs use the
   CPU path.
1. **Bake-to-texture harness** (`browser/renderer/`). Draw a full-screen quad into
   render targets for a `BakeRequest` domain (origin + u/v axes, resolution). Derive
   `ms_world`, `ms_object`, `ms_surface`, `ms_uv`, `ms_normal`, `ms_footprint` and
   `ms_uv_footprint` per texel from the domain; reuse `shade.ts` uniform binding.
   Keep the README conventions: texel centers, rows from the upper edge, sRGB base
   color/emissive, R=1/G=roughness/B=metalness, +Y tangent normals from height
   derivatives, `emissiveStrength` normalization.
2. **Passes.** Pass 1 writes base color, ORM, emissive and height. Pass 2 derives the
   tangent-space normal from the height target by neighbor fetches (cheaper than
   evaluating the height graph several times).
3. **Edit pipeline tier.** Add a GPU tier ahead of the CPU draft in the authoring
   worker flow. GPU textures follow the existing draft rules: never ready, saved or
   exportable; superseded by the CPU result; their own cache key and `profile` value.
   Fall back to the CPU draft on missing WebGL2/float targets, context loss, shader
   compile failure or an unsupported program.
4. **Parameter vs. structural edits.** Parameter, seed and sampling edits only update
   uniforms and re-render (no recompile). Structural edits recompile the shader
   asynchronously (`KHR_parallel_shader_compile` where present) while the previous
   texture stays displayed.
5. **Progressive refinement.** Low resolution first, then the requested size, in tiles
   or bands to stay under GPU watchdog limits. A newer edit cancels refinement.
6. **Measure and label.** Report edit-to-visible latency per stage (shader compile,
   uniform update, bake pass, mip generation, upload) against the 0.45–0.55 s draft
   and 3–4 s full baselines. Studio labels GPU textures as "preview".

Risks: float render-target support and driver precision differences (accepted for
preview); mip generation from sRGB targets; context loss in long bakes; `select`
evaluating both branches can be slow or produce NaN in guarded math.

## Lane H: hybrid CPU/GPU placement (prototype)

The CPU is not only a failure fallback: placement is a compile-time decision. `Program::shader_plan`
(`program/glsl.rs`) tags fields that use CPU-only operations (today `runoff`) and partitions the graph:

- everything upstream of a CPU-only node runs on the CPU (data flows CPU to GPU, never back), and is absent
  from the shader unless something else needs it; everything downstream stays in the shader;
- the shader reads each CPU field from a `sampler2D` (`ms_cpuN`, RGBA32F, `texelFetch` at the bake texel,
  so alignment is exact and nothing is filtered or quantized); the plan lists each field with the
  parameters, seed, images and edges it reads (from `program/deps.rs`) and which outputs rest on it;
- `Session::bake_fields` (Wasm `bake_fields`) evaluates the fields over the same atlas a full bake uses,
  with `evaluate_outputs(OutputSet::NONE, ...)`, so no output is computed for them.

Studio (`editor/studio/main.ts`, `browser/renderer/bake.ts`): the chipped-concrete starters, which fell back
with "runoff needs finite source edges", now preview on the GPU. A GPU-side edit reuses the cached field;
an edit to the source edges, gravity or a parameter the field reads shows the previous field at once and
replaces it when the new one arrives (stale while revalidating). The lane's requests (shader plan, field
bake) run on a worker of their own (`AuthoringCompiler` with `keepWorker`), in parallel with the CPU build:
neither terminates the other's worker, and a newer edit abandons a stale lane request (its reply is ignored)
instead of restarting the Wasm module. The quarter-size progressive stage box-filters the field. The caption and `data-gpu-preview` name the CPU-supplied channels (`cpuChannels`) and the GPU-only
ones. Not supported, refused with a recorded reason rather than approximated: a CPU-only operation inside a
helper function the shader needs or directly in an output (no named field to bake), and a CPU field consumed
at coordinates other than the plain bake texel (warped coordinates). Receiver features stay on the GPU for a
swatch (its rim edges), so they are not CPU-only here.

Checks: `glsl.rs` tests (chipped concrete places only `runoff_mask` on the CPU; the upstream closure is
respected; unplaceable programs are refused), `tests/material_fields.rs` (a field bake covers the atlas and
follows the edges), and `tests/material-gpu-preview-browser.mjs` (hybrid starter previews, is not ready, is
replaced by the CPU result; a GPU-side edit bakes no field; an edge edit bakes one and refreshes the preview).
Live renderer operations: the program's live arguments (fields the shader already computes) are evaluated on
the GPU into half-float textures (four arguments per texel), and the viewer's lighting effects read them, so
a preview no longer shows the last CPU build's arguments. Arguments packed in one texture share one sampler
(a 30-argument program had exceeded the 32 texture units). The rusted-steel preview had looked much darker than
its CPU result (mean center RGB about 31/36/37 against 105/110/114); the cause was in the viewer, not the GPU
preview. `live.ts` decoded each texture argument as `dot(texel, channel)` with a `THREE.Vector4` whose default `w`
is 1, so every argument also added the texel's alpha, which holds another packed argument. In the steel study the
`lightFloor` mask (zero at the center, from the 8 mm rim) picked up the gloss spread packed beside it (about 0.95),
saturated, and lit the whole swatch. Arguments are now read as one channel (a swizzle). With that, the baked maps
already agreed (GPU and CPU mean base color 86.2/89.9/89.8, roughness 101.8, metalness 184.9 of 255, flat normals),
the GPU live arguments matched the CPU fields' values (a native bake of `polishedRim` has mean 0.03 and center 0),
and both previews show the same center color: steel 31.2/36.4/37.3 (GPU) against 31.2/36.3/37.3 (CPU), concrete
178.3/173.9/167.5 against 178.1/173.8/167.4. The CPU result of steel is therefore darker than it used to be in Studio;
the recipe's masks are now applied as written. Screenshots: `generated/verification/steel-*-before-fix.png` and
`*-after-fix.png`, `concrete-*-after-fix.png`.

Software-rendering numbers (headless Chromium, 256 px swatch, seed edit so every map rebakes, 180 ms
debounce included, three runs each). CPU only, after the CPU-side speedups: chipped concrete draft about 0.20 s and
full build 0.27-0.28 s; Kowloon concrete 0.30-0.31 s and 0.65 s; rusted steel 0.21-0.23 s and 0.39-0.40 s
(the courtyard figures of 0.45-0.55 s and 3-4 s above predate them). With the GPU lane: the first GPU preview
appeared after about 0.06 s (chipped concrete, hybrid), 0.09-0.12 s (concrete) and 0.06 s (steel); the field bake
took about 12-13 ms; shader compile on first use 22 ms. The GPU lane no longer delays the CPU draft, and the CPU
build is not slowed by it beyond noise (draft 0.20-0.33 s, full 0.27-0.71 s with the lane on).

CPU results that Lane H builds on (byte-identical output; `bench_fingerprint` identities unchanged:
concrete `ce6e3a55...c561`, steel `b6b04aec...dfa1` at 256 px, native release 0.52 s and 0.17 s on this
machine): per-map retention keyed by what each map's outputs read (`program/deps.rs`, `authoring/material.rs`),
and row-wise evaluation of 64 samples per walk (`program/lanes.rs`). Native/Wasm parity fixtures for the
`.ms`, TypeScript and Kowloon programs pass unchanged.

## Track B: CPU MaterialSpec bake speed

Goal: lowest practical bake time for the same bytes. Every change is a pure
optimization: baked pixels, material identities and cache keys do not change
(`bench_fingerprint` prints the identities; native/Wasm parity must hold; new math
uses `program/numeric.rs`, never platform libm). Eliminate duplicate work before
parallelizing; measure cold work, warm reuse, decoding, evaluation, encoding and
upload separately.

0. **Profile before changing anything.** Use the documented `perf` setup and
   `bench_fingerprint`, on the real example programs (Kowloon concrete, rusted steel,
   noise, tiled courtyard) at 64, 256, 512 and 2048 px. Produce: ns/texel and
   nodes/texel per program; time by node class (noise/fbm/cellular, image sampling,
   arithmetic, `select`); share of evaluation vs. normal derivation vs. byte
   encoding vs. PNG/KTX2 encode vs. worker round trip and the 180 ms debounce. Past
   guesses about this evaluator were wrong three times, so numeric targets come only
   after this step.
1. **Skip work a change does not affect.** The program is reusable across parameter
   changes, but an edit rebakes every map. Analyze the compiled DAG (`compiled.rs`) for
   which outputs and fields depend on each parameter, seed, image and sampling
   setting. Then: (a) map-level: rebake only affected channels and keep the others by
   key; (b) field-level: retain per-texel values of expensive unchanged fields under a
   bounded memory budget and recompute only dependents. This is the largest expected win
   for slider dragging and matches the "not done" note.
2. **Compile-time work.** Check what `compiled.rs` folds. Constant-fold and
   common-subexpression-eliminate subgraphs that do not vary per texel, hoist
   per-bake invariants (domain-constant terms, parameter-only terms) out of the texel
   loop, and consider specializing parameters to constants for `production` bakes
   where the program is not reused across parameter sets.
3. **Cheaper evaluation, profile-gated.** If dispatch and value copying dominate,
   evaluate each node over a whole row or block (struct-of-arrays) instead of one texel
   at a time. This amortizes dispatch and enables SIMD while keeping the same `f64`
   operations in the same order per texel, so results stay identical. Also: skip
   `select` branches that are provably unused for a texel and short-circuit zero-weight
   blends where results are bitwise unchanged.
4. **Footprint-aware work.** Reuse the existing texel dedup and "face smaller than the
   minimum chart" rules; extend to skip octaves/detail already below the texel
   footprint only where that matches the language's defined fade-to-mean behavior
   exactly.
5. **Parallelism after dedup.** Tune row-share granularity and thread count per
   profile; balance by estimated row cost; keep Wasm helper-worker sharing
   (`distribute_bakes`, `bake-pool.ts`) byte-identical to a local bake.
6. **Production encode.** Measure PNG (balanced DEFLATE) and KTX2/basisu separately from
   evaluation at 2048 px; they may dominate export time. Candidates: parallel or
   streamed encode, reuse of unchanged channels, avoiding intermediate copies.
7. **Caching.** Check key granularity so unrelated edits retain bakes (as
   selective-bakes.md did for world jobs). A persistent on-disk bake cache keyed by the
   full content key is possible but deferred: it adds state outside the process.

Verification per step: focused tests with identical-output comparison against the
pre-change bake, failure and cancellation recovery, native/Wasm parity
(`material_program`, `material_ts`, `material_kowloon`, browser parity suites), and
`npm run check:architecture`. Timings use alternating processes and report ranges,
noting overlaps. No speedup is claimed without that evidence.

## Order

- A and B touch different code (browser TS + `glsl.rs` vs. Rust evaluator/baker) and can
  run as separate sessions.
- A: steps 0–3 give the first visible win (GPU draft); 4–6 follow.
- B: step 0 first, then 1 (biggest edit-latency win), 2, then 3–6 by profile.

## Out of scope for now

Kowloon `Recipe` raster jobs and large-environment cold/warm builds (courtyard baseline:
cold 27–31 s, 3,821 bakes, warm ~1.8 s; see
[extraction-reuse.md](../performance/extraction-reuse.md)), decoded-image and cross-host
caching there, intrinsic/receiver texture sharing, GPU baking of those recipes, GPU use
for export, and any native GPU path.
