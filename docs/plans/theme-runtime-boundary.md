# Theme runtime boundary and canonical APIs

This is a milestone record. See [current closeout status](reorganization-status.md#verification)
for the later fixture repairs, combined review and current distribution pin.

This final structural pass follows [SuperSpec consolidation](superspec-consolidation.md).
It preserves the reviewed `94fd20c8a9bc009fdb4e2c01c8a18ae0e4b77d629beafb4719b79fc4b0099430`
distribution's output and public native/browser workflows. Performance measurement
and optimization remain deferred.

## Decisions before implementation

- The session owns generic artifact lifetimes, source admission, revision guards
  and producer bookkeeping. Concrete theme state is created by a supplied factory
  at the composition boundary and released by the session's existing clear lifecycle.
  It does not introduce a second artifact store.
- The world coordinator accepts a compiled theme adapter. Physical lowering exposes
  bounded typed stages; the Kowloon receiving-surface bridge owns its theme planning
  types and calls. Generic orchestration does not inspect concrete theme state.
- `worldspec::shipped` selects the default AML profile and theme. Existing root CLI,
  Wasm and compilation entry points retain their behavior. A real no-theme build and
  an alternate adapter exercise the same neutral coordinator.
- Migrate maintained callers from `dressing`, `imports` and `generation` to their
  actual owners, then remove these forwarding modules. Shared material operations
  go to `materialspec`; Kowloon operations go to `worldspec::themes::kowloon`;
  imports go to `itemspec::imports`; layout producers go to `worldspec::layouts`.
  Compilation callers use root `superspec_engine::compile*` functions rather than
  the former `architecture::compile*` paths.
  Authored AML `<dressing>` vocabulary and source identities remain unchanged.
- The shared obstruction index belongs to `superspec-geometry`, so optical
  visibility and theme placement consume the same neutral implementation. Visual
  publication trait implementations live beside their owning material/mesh types.
- Enforce Rust module and Cargo dependency boundaries with a static architecture
  check that resolves import ownership. Comments and string literals are not
  dependencies. Application adapters remain distinct from generic kernels.
- Update current guidance and clearly label historical decisions. Preserve original
  fixture values and failed legacy expectations; new tests cover real composition,
  lifecycle, admission, cancellation and recovery behavior.

## Acceptance

Require independent implementation/test reviews; native all-target checks and
focused behavioral tests; native and Wasm production builds; browser and LevelSpec
consumer checks; unchanged frozen packets/exports and the complete courtyard packet.
Update source distribution pins only after the final implementation is reviewed.

## Implemented boundaries

`Session` owns one artifact runtime plus generic, type-keyed producer bookkeeping.
Only compiled adapters can supply state types; authored names cannot grow that
registry. Factories and callbacks run without a registry borrow. State is retained
through a scoped `Arc`, and clearing the session invalidates the active build and
drops retained producer ownership. The Kowloon adapter owns its cache bookkeeping
and uses short mutex guards, preserving native `Session: Send` and callback reentry.
Root `BakeStats` exposes the existing neutral counters.

`worldspec::build::ThemeAdapter` is an internal compiled composition boundary.
`worldspec::shipped` selects the default AML profile and Kowloon adapter. The
`architecture::kowloon` bridge can access private receiving geometry; the neutral
coordinator only sees typed physical stages. Recipes, floor finishes, selected
hosts, render edges and surveys remain theme state. The structural phase sequence,
collision-only planning, source errors and native/browser entry points are preserved.

`npm run check:architecture` runs the standalone Rust tool in `tools/architecture`.
It reads Cargo metadata and Rust syntax, resolves module paths, imports, aliases,
reexports and macro paths, and checks owner dependencies. Comments and literals
do not count as dependencies; unsupported include ownership fails explicitly in
checked modules. It checks normal workspace dependency edges, including the
portable model/export layers. It is an architectural linter, not a complete Rust
type checker. CI runs its fixtures and the real-tree scan, followed by normal
compilation checks. Canonical paths and required library coverage prevent a skipped
source tree from producing a false pass.

## Review and native verification

Independent reviews cover the session registry, short cache guards, phase ordering,
physical/theme separation, moved algorithms, canonical caller imports, test behavior
and the architecture checker. A new warm-binding callback test reproduced a held
cache guard before its fix and passes after the guard is released before restoration.
The generic spatial index matches the prior pinned implementation after restoring
only its import owner and cross-crate accessor visibility. Frozen fixture, catalog
and image files retain their exact bytes.

Linux native all-target checking passes under WSL. Focused suites pass: foundation 22, model/export
19, lifecycle/language/composition 51, spatial/assembly 20, engine unit 30, and native
host 35. The checker passes 10 fixtures and scans 312 production Rust modules.
Building integration retains eight passing cases and the five historical failures;
their failing names and complete assertion values match the untouched baseline.
These expectations remain enabled. The larger building suite was executed one case
per process after WSL interrupted aggregate runs; this changes only test execution.

The 30 engine unit cases include a real no-theme build, a second compiled adapter,
physical/source invariants, asset diagnostics and failure/cancellation recovery.
Session coverage preserves actual native thread transfer and permits observer
statistics/clear calls without retaining a registry or cache lock across callbacks.

## Output and host acceptance

Native release commands and both production/acceptance Wasm builds pass. All 28
frozen scene/export artifacts are byte-identical to the preceding SuperSpec
distribution. The complete courtyard packet is also unchanged: 223,180,332 bytes,
SHA-256 `5ae3e7f592bdffb8f481532b9ba52a87a04b8214b09043623db6688fb31503bd`.
No normalization or fixture adjustment is applied in this comparison.

Chromium-Wasm matches native output for material programs (15,573 bytes) and authored
World GLB (76,040 bytes), with the same preceding-milestone digests. The relocated
native bundle passes with an empty executable search path, explicit catalog and
authored companion resolution, missing-input recovery and the documentation World
example. TypeScript checking and the viewer build pass.

Material/Item editors, the AML editor, native preview, packet/update delivery and
Khronos/Three export acceptance pass in Chromium. A standalone model/export consumer
builds outside both repositories without the engine or AML parser in its normal
dependency graph. LevelSpec's three Daiban/Dust II tests pass. Its pinned launcher
preserves working directory, arguments, binary output and exit status, rejects a
mismatched distribution, and recovers with the valid pin; harness typechecking passes.

The reviewed distribution is
`7fb777e33287efc4c942b8368b05e60bd585c56b404a40cf8d3dd449b37be3d0`
(882 files), adopted by both `superspec.manifest.json` and LevelSpec's
`superspec.lock.json`. Source identity remained unchanged throughout final acceptance.
Evidence is under `generated/verification/theme-boundary-*`; checker fixtures and
the real-tree scan are in `architecture-check-tests.log` and `architecture-check.log`.

The preexisting window-color kernel discrepancy remains outside this structural
pass, as do the five preserved building golden failures. A completely green full
workspace suite is not claimed. Native Windows, threaded-browser and remote CI
execution were not exercised locally. Performance measurement and optimization
remain deferred.
