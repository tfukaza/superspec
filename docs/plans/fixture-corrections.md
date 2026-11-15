# Historical fixture corrections

This correctness pass follows the reviewed
`7fb777e33287efc4c942b8368b05e60bd585c56b404a40cf8d3dd449b37be3d0`
distribution. Its five building integration failures and one window-color failure
were preserved during modularization. The user has now authorized fixing them.
Performance measurement and optimization remain deferred.

The complete baseline subsequently exposed 32 failing cases and 412 passing cases
across 135 native test executables. The earlier six were only the failures visible
in the selected modularization checks. Baseline results and per-case diagnostics
are retained in `generated/verification/failing-tests-baseline/`.

## Rotation correctness

Shop rotation, mirrored room layout and hanging-packet placement created a fresh
`Pen` and lost the room and shelf shading that had just been attached to it.
In the TypeScript source those ambient effects belong
to mesh wrappers and survive a shape rotation, while scoped timber, brushed-metal
and weathering adapters intentionally reset. The new private `shape_frame` helper
preserves exactly that distinction. General reframing continues to derive its
scoped adapters as before.

Regression tests compare a rotated shaded shape with an independently equivalent
facing frame, require actual darkening, preserve survey positions, and retain the
original material-reset and host-handedness behavior. Two additional tests execute
the actual mirrored grocer and posed packet producers, requiring bounded ambient
attenuation with unchanged geometry, UVs, random continuation and surveys.
All four focused regressions pass. Independent review rejected
an initially broader inheritance change; the final implementation preserves only
the ambient effects missing from the original port.

## Independent integration coverage

The building corpus's one native grocer is isolated by its independently captured
room plan. Every one of the original 28,078 shop triangles is reproduced across
all attributes before changing the expected structure. The revised shell and
frontage use independent historical producers with explicit supported parameters;
the changed native interior uses component and composition contracts. All 2,462
receiver charts remain checked. The other seven building cases retain their full
geometry oracles, with separately guarded window, canvas and sign deltas.

The native interior filter names only the grocer body, ceiling services and
checkout register. Unknown descendants fail. Every selected point must stay in
the planned room; floor support, stock height, ceiling service extents and the
register's authored display height are required. The complete building render and
survey must agree to the delivery grid, including the independently checked outer
structure. Maintenance drivers are under `tests/fixtures/rust/reference/building-room*.mjs`.

## Oracle policy

Reproduce the original fixtures with their independent TypeScript reference before
changing expected values. Apply only independently reviewed, intentional appearance
changes that the supported Rust implementation already adopted. Preserve geometry,
material, RNG, receiver, collision, provenance, cancellation and recovery assertions.
Do not derive expected values by copying current Rust output or loosen tolerances.

The building snapshots precede TypeScript commit `3c64de86`, which reduced blade,
hung and plate sign densities. Rust later adopted those values. A withheld sign
with unchanged placement and text can therefore differ legitimately from an older
built-sign snapshot; it is not evidence that collision or hidden-host filtering
should change.

The window snapshot also predates intentionally brighter interior backs, brighter
curtains and folded interior fabric. Rust's `9ed9537` port retained earlier bead and
glass styling and used a separate appearance seed. The corrected reference must
preserve that narrower contract instead of importing every later TypeScript change.

## Reproducing aggregate references

The optional maintenance drivers live under `tests/fixtures/rust/reference/` and
require explicitly staged historical TypeScript and its dependencies. Ordinary
builds use the frozen JSON and have no LevelSpec dependency. Original fixture data
comes from `af7f5e22d78f1f45ff7e5ba21c56c3097f12d13d`; the fixture-era compact
`weathered.ts` has SHA-256
`d37617a867183b868bfd305d31df22c222db53f701fa09303e1dc227871b6825`.

- `windows-interior.mjs STAGED_TS_ROOT ORIGINAL_WINDOWS_JSON OUTPUT_JSON`
  verifies the archived fixture and source hashes before applying the supported
  interior change.
- `facade-capture.mjs STAGED_TS_ROOT BASELINE_FIXTURE OUTPUT_DIRECTORY verify`
  verifies the original facade capture. Prepare a separate after tree with
  `shop-shading.mjs`, `stock-profile.mjs` and
  `facade-profile.mjs STAGED_TS_ROOT TS_3C64_ROOT`, then capture it without `verify`.
- `open-facade.mjs BASELINE_FIXTURE WINDOW_CORRECTED_FIXTURE INTERIOR_EVIDENCE
  BEFORE_CAPTURES AFTER_CAPTURES STAGED_TS_ROOT OUTPUT_JSON` preserves strict
  surrounding geometry and receiver ownership. Its `WINDOW_CORRECTED_FIXTURE`
  argument is the full original fixture after both guarded window and canvas
  corrections; `INTERIOR_EVIDENCE` is the window insertion-range report.
- `building-signs-setup.mjs ORIGINAL_SOURCE_ROOT OUTPUT_ROOT [FIXTURE_ERA_PEN_TS]`
  stages the two sign-density profiles. Run `building-signs-capture.mjs SOURCE_ROOT
  OUTPUT_JSON ORIGINAL_JSON` for each, then `building-signs-samples.mjs ORIGINAL_JSON
  BEFORE_CAPTURE_JSON`. Finally, `building-signs-correct.mjs ORIGINAL_JSON
  CURRENT_JSON REFERENCE_ROOT OUTPUT_JSON` produces a guarded candidate preserving
  the existing room, window and canvas corrections.

The full sign reference reproduces every original attribute, including receiver
frame UV projection, and all 2,991 original receiver samples. Reduced density also
changes upper-sign placement after lower reservations disappear, so the correction
reruns the complete independent settlement rather than editing counts alone.

## Verification scope

Discover and execute the complete native workspace tests, including failures hidden
behind the earlier stopping point. Use separate processes for individual tests where
needed to avoid the WSL interruptions observed during large aggregate fixture runs.
Run the shared kernel assertions in Chromium-Wasm, then relevant browser/consumer
checks. Record independently reviewed fixture provenance and update source pins only
after the changes and resulting checks have been reviewed.

Another session implemented MaterialSpec catalog/language changes during this
pass. The distribution pins stayed unchanged while the combined implementation
awaited independent review. The subsequent closeout reviewed those additions and
the fixes below; current verification and adoption are recorded in the
[reorganization status](reorganization-status.md#verification).

## Repair milestone validation (historical)

These results identify the earlier 939-file repair snapshot. Final combined
acceptance and the adopted pins are recorded in the
[reorganization status](reorganization-status.md#verification).

The full native run executes 503 cases across 137 test targets: 503 pass, with no
failures or ignored cases. All 444 original cases remain present, including all
32 repaired failures. The complete shared
kernel assertions pass in Chromium-Wasm, including the corrected building and
facade corpora. Packet/update contracts, MaterialSpec and WorldSpec native/Wasm
parity, MaterialSpec/ItemSpec and AML editors, preview recovery, browser export,
SDK type contracts and portable command/asset recovery checks pass.

The 28 frozen packet/export artifacts remain byte-identical. The full courtyard
packet retains identical metadata, positions, normals, UVs, indices and asset bytes;
473,345 color channels across 33 procedural visual meshes change from the repaired
ambient shading. The original packet hash is
`5ae3e7f592bdffb8f481532b9ba52a87a04b8214b09043623db6688fb31503bd`, and the
corrected packet hash is
`9e0b7b44df10c06a9a390241e61bb06a9f2a034284a407ae76452819cae58c63`.
Evidence is in `generated/verification/failing-tests-courtyard-parity.json` and
`generated/verification/failing-tests-final-browser.log`.

Concurrent MaterialSpec changes invalidated two native build snapshots. This
milestone's complete native run used an isolated 939-file source snapshot with SHA-256
`68686cbfbaefe2ca0474d116c2b9e02693820b1f11139c99a1a5e35e43331846`.
Discovery compares every test-enabled Cargo workspace target with the executable
listing and binds source, fixture and executable hashes before allowing reuse.
The runner rejects any source or binary drift and retains the original failing
baseline. This also catches new targets that were absent from older artifact lists.

The completed native run is conclusive, and the working tree at that milestone
matched the snapshot exactly. Workspace doctest targets, formatting and all 319 Rust module
boundaries pass. Repair-milestone results are recorded in
`generated/verification/failing-tests-corrected-results.json` and
`generated/verification/failing-tests-summary.json`. CI now runs the complete native
workspace on Linux and Windows and the full shared kernel checks in Chromium-Wasm.

## Combined closeout review

Independent review of the later MaterialSpec additions found correctness issues
in source editing, module visibility, asynchronous editor state and retained
metadata. The fixes preserve neighboring declarations during layer removal,
resolve helper references within their defining modules, enforce explicit named
exports, and keep existing host parameter IDs stable. Invalid layer entries no
longer shift edit targets, and helper arguments correctly shadow outer bindings.

CLI edits use exclusively created temporary files. Type checking reports process
failures and uses a separate source, SDK and configuration snapshot for each
invocation. Native and TypeScript checks share the same admitted source modules.
Controlled concurrent-check tests verify admitted inputs, cleanup and
preservation of an existing shared configuration.

Editor controls and pending responses are bound to the complete source revision
and project identity. Stale checks cannot remove current overrides; failed edits
restore the last accepted preview and preserve the camera. Authored labels and
units reach session metadata and visible controls while machine IDs stay stable.
Retained resource accounting includes this metadata and live-operation records.

Receiver-feature extraction uses checked index arithmetic, including on 32-bit
Wasm. The same malformed-index regression runs natively and through the browser
kernel acceptance entry point. These changes are correctness and maintainability
work; no performance measurement or optimization was undertaken.
