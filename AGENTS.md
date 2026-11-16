# SuperSpec implementation guidance

Read `README.md`, `docs/AML.md` and `docs/plans/extraction-decisions.md` first.
The product direction is in `docs/plans/spec-modularization.md` and `material-spec.md`.

Keep native and Wasm compiler semantics shared. Domain coordinates are meters,
right-handed Z-up. Preserve render visibility, canonical collision, navigation and
validation status independently. Sampled navigation is not continuous proof.

Colocate implementation fixtures and assets with their owning modules. SuperSpec
must build without LevelSpec. Authored maps, projects and integrated sample tests
belong in the sibling LevelSpec harness, which pins this implementation.

Portable records belong in `superspec-types` and `superspec-model`; catalog content
and lookup belong in `superspec-catalog`. Exporter consumes the model directly.
Inside the compiler, generic `materialspec` and `itemspec` code must not import
`worldspec` or physical assembly. Kowloon recipes, assets
and theme assembly belong under `worldspec::themes::kowloon`. Keep AML and physical
adaptation explicit; do not move compiler internals merely to rename a directory.
WorldSpec's build coordinator accepts a theme adapter; theme planning state belongs
to the selected theme. Physical lowering stays private behind typed stages. The
`architecture::kowloon` adapter owns concrete receiving-surface adaptation, and
`worldspec::shipped` selects the default profile and theme. Session bookkeeping,
shared `assets` and `runtime` modules must not import theme recipes. Use canonical
owners rather than restoring `dressing`, `imports` or `generation` forwarding APIs.
Run `npm run check:architecture` to check these Rust and Cargo boundaries.
Offline authoring algorithms live behind the compiler's `authoring`
feature; filesystem, font downloads and output publication remain CLI adapters.

Raster keys contain every pixel-affecting input, not placement names. Keep source
identity, artifact identity and placement identity separate. Eliminate duplicate
work before parallelizing. Preserve texture density, seeds, physical output and
visual behavior unless a change explicitly authorizes them. Distinguish cold work,
warm reuse, source decoding, rasterization, binding and GPU upload in measurements.

Retain behavior tests, independently derived expectations, failure recovery and
source-linked errors. Remove tests or comments only when their meaning is obsolete
or already covered. Review structural changes independently and run focused checks
plus native/Wasm and consumer boundary checks before a cutover.

Do not alter source distribution pins until the corresponding changes are reviewed.
Generated output belongs under `generated/` or an explicit external target directory.

The default test, benchmark and screenshot scene is the washroom in
`tests/studio/washroom` (concrete corner, tiled wall, duct and vent, one large
rusted pipe, bench, drain). Reuse it, via `tests/material-washroom-browser.mjs`
(`RESOLUTION`, `SCREENSHOT`) or `build.mjs`, for scene-level checks, timings and
screenshots; extend its model or materials rather than writing a new ad hoc scene.
Swatch and starter tests keep using the shipped starters. To render on a real GPU
instead of headless SwiftShader, set
`BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist"`.
