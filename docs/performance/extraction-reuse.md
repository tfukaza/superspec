# Raster reuse during SuperSpec extraction

The extraction removes redundant placement identity from material-bake keys and
coalesces equal cold requests before worker dispatch. Recipes, density, source
identities, spatial inputs and output bindings remain unchanged. This implements
the first reuse slice in the [extraction decisions](../plans/extraction-decisions.md);
see the [repository README](../../README.md) for build and consumer boundaries.

## Measured boundary and frozen inputs

One baseline process and one extracted process each compiled the same courtyard
cold, then compiled it again with the same session. The boundary runs from the
source request through complete preview-packet construction; packet hashing occurs
after the timer stops. No builds, tests or browser workloads ran concurrently.
These are single observations, not an established overall speedup or memory gain.

- Baseline: LevelSpec `fb5df8cbf1a3c8b5a8a75a66bf976678cf2106ff`, archived before
  removal. Its compiler, core and geometry source are unchanged. A separate driver
  uses the same benchmark function as the extracted CLI and resolves the same
  copied catalog assets through an explicit host adapter.
- Runtime: Rust 1.100.0, WSL2 Ubuntu Linux x86_64, AMD Ryzen 5 9600X with 12 logical
  CPUs; release builds with thin LTO, one codegen unit and four requested raster
  workers. The process cache starts empty; the second request uses its warm session.
- Scene: unchanged LevelSpec `editor/courtyard.aml`, now also in
  `demos/courtyard/scene.aml`, with building texture density 16 and its three local
  companion images. The benchmark freezes that folder under
  `generated/verification/courtyard-input/`. No scene or quality settings were changed.
- Source SHA-256:
  `c069f310bb130d55562c3741280aa69aef01c4ce2d26510b0322bff974a79e66`.
  All 47 loaded asset hashes match across both engines and both requests.

## Results

Times are seconds. Selected phase rows are coarse compiler observations; total
rows include those phases. The cold `surface-raster` block includes decoding,
cached-job orchestration and binding, so it is not a pure raster-kernel timer.

| Boundary or phase | Baseline cold | SuperSpec cold | Baseline warm | SuperSpec warm |
| --- | ---: | ---: | ---: | ---: |
| Complete preview packet | 27.424 | 27.157 | 1.796 | 1.847 |
| Complete compiler output | 27.327 | 27.062 | 1.705 | 1.753 |
| Building drawings | 0.403 | 0.415 | 0.281 | 0.282 |
| Surface asset admission | 0.290 | 0.295 | 0.266 | 0.281 |
| Surface raster block | 24.989 | 24.719 | Not entered | Not entered |
| Warm surface binding | Included above | Included above | 0.147 | 0.151 |
| Asset publication | 0.825 | 0.846 | 0.271 | 0.294 |
| Packet assembly | 0.097 | 0.095 | 0.092 | 0.094 |

Process peak RSS across cold and warm requests was **1,180,008 KiB baseline** and
**1,196,276 KiB SuperSpec**. No memory improvement is demonstrated.

All four complete packets have SHA-256
`823b6d159d6e06b70ab239c11c8e4e24525b343a2dc9675225a378a050247462`:
223,180,332 bytes, 1,589,384 vertices and 697,084 triangles. This verifies the
transported geometry, materials, assets, provenance and diagnostics against the
baseline for this input.

SuperSpec's cold request completed **3,821 unique bakes** covering **55,821,624
channel pixels**, coalesced **64 pending duplicate requests**, and recorded **70
completed-cache hits**. The warm request added no bakes and reused all 20 geometry
bindings. Bake/cache counters measure different events: `hits` and `misses` count
lookups, while `bakedJobs`, `bakedChannelPixels` and `coalescedJobs` describe
successfully bound batches. Failed batches do not increment the latter counters.

The cache shares completed artifacts across hosts and pending work within a batch.
It does not yet coalesce independently executing cross-host jobs. Scene delivery
still retains per-placement material/image identities and copies their bytes.

## Verification and evidence

Four focused reuse tests pass with real receivers, clean-output comparisons,
mixed hits/misses, reordered placements, spatial/source changes, cancellation and
recovery. Three image-session tests and three parallel-baking tests pass. The
building suite passes its complete serial/parallel and incremental packet checks;
five historical sign-plan golden failures reproduce in the untouched baseline
with identical assertion payloads. Their expectations were not weakened.
Independent code and test reviews found no blockers in this reuse slice.

Local evidence is retained under `generated/verification/`:

- `courtyard-baseline.json`, `courtyard-superspec.json`, `courtyard-comparison.json`
  and `courtyard-environment.json` record phases, hashes, counters and build context.
- `courtyard-baseline-time.log` and `courtyard-superspec-time.log` record process
  timing and peak RSS. The archived baseline and matching driver remain alongside them.
- `raster-reuse-tests.log`, `raster-cache-integration.log`,
  `raster-reuse-integration.log` and `building-aml-baseline.log` record verification.

## Next measured changes

1. Derive input identities per material job instead of hashing the whole host's
   material map. An unrelated finish edit should retain other bakes. Preserve
   every sampled layer, coating, source byte and output-affecting parameter.
2. Cache immutable decoded images separately from image-admission metadata.
   Partial misses currently decode all required host images again. Bound decoded
   storage and decode only sources needed by missing work.
3. Represent shared local item meshes plus placement transforms, tints and source
   identity in the model. Offline props already share source catalogs and atlases,
   but compilation expands their vertices for each placement.
4. Separate intrinsic appearance from receiver mapping. Share concrete/grime
   textures where compatible; retain contacts, folds, gravity, runoff, bounds and
   other effective receiver dependencies in mappings or receiver-specific bakes.

Those changes need their own clean-build comparisons and measurements. The small
cold-time difference here does not establish a city-wide performance improvement.
