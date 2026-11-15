# Selective material-bake dependencies

This slice narrows raster invalidation to the materials used by each job, while
preserving recipes, density, receiver-specific inputs and delivered bytes. It
continues the [extraction reuse work](extraction-reuse.md) and the
[repository decisions](../plans/extraction-decisions.md).

## Decisions recorded before implementation

- Build each job's dependency closure from its declared structural layers and
  coatings, including zero-coverage coatings, and its output source material.
  Keep the complete material definitions: baked delivery retains metadata such
  as emissive values even when it does not affect the sampled color texels.
- Include every companion channel (color, normal, metallic/roughness and height)
  using the session's exact-byte identity. Read required sources again on each
  compile revision so missing, changed and corrupt inputs cannot return stale work.
- Prepare cache hits, unique misses and duplicate slots once. Decode the union
  of missing jobs' companion paths, then retain deterministic output order and
  publish new bakes only after successful receiver binding.
- Keep the existing complete host-material prefix for geometry binding. This
  conservatively retains material/height dependencies, raw receiver geometry and
  external edges. An unrelated material edit may still repeat binding, even when
  no material raster is needed.
- Preserve the total required-host encoded-byte and decoded-pixel allowances.
  Store only the admitted pixel count alongside exact source identities. A source
  without that metadata must be decoded for admission even if its raster is
  retained. No decoded RGBA cache or public `Inputs` ownership change is included.
- Count actual procedural source decodes separately from raster completions and
  the session's final-delivery image validation. Successful decodes can survive a
  later cancelled batch as admission metadata; partial rasters cannot be published.

## Existing repeated work

Before this change, `architecture/visual.rs` hashed all host material definitions
and all required source identities into one raster prefix. A single miss then
decoded all required host sources. A fully warm host already skipped decoding. Ground hosts use
the entire authored material library; building and wall hosts use smaller local
appearance maps. The frozen courtyard's unrelated `blue` definition edit therefore
repeats 43 ground bakes in the pre-change measurement, while the other hosts remain
warm. This is a concrete invalidation problem, not evidence that every expensive
receiver-specific bake can be replaced by a shared intrinsic texture.

The field audit covers metal, paint, frosted glass, facade, deposited facade, tile,
tiled skin, signs, stains and pavement. Their structural layers/coatings have
stable material identities; rasterization already checks that invariant during
sampling. Pavement includes all registered finish sources regardless of current
weights. Signs include paint/shadow coatings regardless of coverage at the origin.

## Verification contract

Focused tests exercise the production admission/key/decoder path with real
receivers, full fresh-output equivalence, selective definition and source edits,
shared source propagation, all companion channels, zero-coverage coatings,
failure/cancellation recovery and source-pixel accounting. Whole-session packet
comparisons and native/Wasm checks cover integration. Measurements and final
verification results follow; the decisions above do not themselves establish a
performance gain.

Six production-path tests pass. Their fixtures enter the same `VisualBaking::bake`
method as host compilation, rather than supplying a synthetic cache prefix. They
compare complete material definitions, image bytes, layouts, bound vertices,
indices and surface slots with an uncached execution. A real 4096-square source
primed through seven separate jobs proves that a combined warm host still counts
each path: six sources fit the 96 Mi-pixel allowance, seven fail without publishing
a binding, and six subsequently recover. Source-identity eviction also regenerates
the exact output. The four existing spatial/reordering/cancellation cache tests
continue to pass. Independent source and test reviews found no blocker.

`sourceDecodes` and `sourceDecodedPixels` count successful procedural decoder
calls and their decoded pixels, including admission work before a later failure.
They do not count the separate final-delivery image validation in `Session`.
`surface-source-decode` now exposes decode/admission time separately; the
`surface-raster` stage still includes worker orchestration and receiver binding
for batches with misses. No decoded image pixels persist in the cache.

Local test evidence:
`generated/verification/selective-raster-tests.log` and
`generated/verification/selective-legacy-cache-tests.log`.

## Frozen courtyard measurements

Three processes per implementation each compiled the unchanged courtyard cold,
compiled it warm, then changed only the authored `blue` material color from
`#63869f` to `#b05d72`. That finish belongs to the stairs and partition; it does
not contribute to the ground rasters. Before/after processes alternated, with no
builds, tests or browser workloads running during these six measurements.
The runtime, hardware, four raster workers, quality settings and preview-ready
boundary are the same as the [extraction measurement](extraction-reuse.md).
No visual or physical quality settings were reduced.

The baseline is the first extraction slice; its source snapshot and release
executable were frozen before these changes. The adopted source distribution is
`397605307b2cacf52dd8c77cd359a9a052a76be4d46da2a4942252105453c9cb`.
Initial exploratory runs are excluded from this table. Values are median seconds,
with the observed range in parentheses.

| Preview-ready boundary | Before | After |
| --- | ---: | ---: |
| Cold scene | 31.686 (26.423–33.220) | 31.490 (28.407–33.683) |
| Warm unchanged scene | 1.991 (1.981–1.993) | 1.944 (1.818–2.145) |
| Unrelated structural-finish edit | 5.968 (5.325–6.088) | 2.357 (1.942–2.403) |

The edit's median is about **60.5% lower**. Cold and warm timings overlap; these
runs do not establish a cold-build or general warm-build speedup. Median process
peak RSS was 1,195,984 KiB before and 1,172,968 KiB after; the observed ranges also
overlap, so no memory improvement is established.

| Completed work | Before | After |
| --- | ---: | ---: |
| Unique cold bakes | 3,821 | 3,768 |
| Cold baked channel pixels | 55,821,624 | 55,727,832 |
| Additional bakes for unchanged warm request | 0 | 0 |
| Additional bakes for blue edit | 43 | 0 |
| Additional baked channel pixels for blue edit | 16,853,814 | 0 |

The new procedural decoder counters record 140 calls / 325,562,316 pixels over
the cold scene, and zero additional calls or pixels for either the warm request
or blue edit. The old implementation did not separately instrument decoding.
Median cold decode/admission time is 2.973 seconds in the new implementation;
remaining cold raster/orchestration work still dominates. Equal source paths
within a host share decoding, but cold hosts can decode the same sources again.

All 18 complete packets match their corresponding expected hashes. The original
and warm scene hash remains
`823b6d159d6e06b70ab239c11c8e4e24525b343a2dc9675225a378a050247462`;
the edited scene hash is
`4af5ffa7d973c5cd82e61c0da6936e6f133437ea84fe04f613a6bb13c874a274`.
All 47 loaded asset hashes match. The unchanged source SHA-256 remains
`c069f310bb130d55562c3741280aa69aef01c4ce2d26510b0322bff974a79e66`;
the edited source is
`ed4fa90ca26451a69f88560a96356e7c3ac669b0ca5c658e37283dfea1498efb`.
A separate fresh process compiles the edited source cold and warm to the same
edited packet hash, confirming complete incremental-versus-clean scene equality.

The browser acceptance also compares cold/warm/unrelated/dependent edits to fresh
browser sessions and native packets. Actual procedural counters are 2/2/2/4 bakes
and 1/1/1/2 decodes in its small fixture. A dependent texture-scale edit changes
baked bytes; an unrelated finish edit preserves them while updating the scene.

Evidence under `generated/verification/` includes `selective-before-{2,3,4}.json`,
`selective-after-{2,3,4}.json`, their `-time.log` files,
`run-selective-measurements.sh`, and `selective-comparison.json`. The comparison
script checks every source, asset and packet hash plus the work deltas.
`selective-fresh-edit.json` records the separate clean edited-scene comparison.
Source preparation, native/Wasm builds and pinned-consumer verification are recorded in
the [extraction decisions](../plans/extraction-decisions.md).

This establishes selective invalidation, not universal texture sharing. Spatial
weathering and intentional seeds remain dependencies. Geometry binding still
rebuilds the affected ground host after the blue edit, and reusable local meshes,
bounded decoded-image storage, and shared intrinsic appearance with separate
receiver mapping remain subsequent architectural work.
