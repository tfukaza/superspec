# Shop fixture corrections

The shared shop fixtures originally captured the TypeScript producers at
`14ae2704d66f1eaacf0cf24d17ad4e5db8b45839`. Later Rust ports adopted appearance
catalogs, cardboard deformation and analytic room/shelf shading while retaining
some older geometry and explicit native configuration. Replacing their snapshots
with the complete latest TypeScript output would import unrelated changes.

The correction reproduces all original indexed positions, normals, UVs, colors,
indices and material slots before accepting any new values. All 204 authored cases
remain: furniture 36, stock 24, hardware 6, herbalist 6, displays 32 and props 100.
Inputs, support surfaces, fitted sizes and random-stream continuations are retained.
The independent producers assert support and RNG values during capture. Existing
native error, cancellation and recovery assertions remain in place.

## Supported appearance changes

The appearance reference is TypeScript
`3c64de86ebc4390ccfb9ff31a29d0f840716b69a`. Its baked prop and stock-appearance
catalogs match the shipped Rust assets. The correction uses its actual shelf
occlusion, cardboard deformation and molded-plastic wear algorithms, with these
documented native contracts:

- Closed shelving retains its 22 mm boards, uses grime `0.65`, and seeds that
  finish with the independent JavaScript `finishSeed(pen.part)` implementation.
- Variant zero retains original artwork. Positive variants select revised labels.
- The explicit `a6ef414` crate-wear port applies handling wear to molded crates by
  default. Placement identity uses bracketed floating-point vectors.
- The cardboard port emits the same four midpoint triangles in corner A, corner B,
  corner C, center order. Permuting each TypeScript block by `[0, 3, 2, 1]` retains
  every position, normal, UV, color and material; all 44 affected source faces were
  independently checked. Geometry is never inferred from Rust output.

Furniture and herbalist fixtures change only colors. Hardware and display fixtures
also contain independently generated revised cardboard geometry. The prop and stock
fixtures retain every unchanged field. Fixture provenance records the original
fixture hash, source hashes and per-case changes.

The shelf references also exposed a real port bug: rotation discarded the ambient
shading just attached to shelf stock. The repaired shape-frame boundary retains
room/shelf shading and host handedness while continuing to reset scoped material
adapters. See [the correctness pass](fixture-corrections.md).

## Reproducing the references

These optional maintenance tools require staged historical TypeScript and its
dependencies; ordinary SuperSpec builds and tests use the frozen JSON fixtures and
do not depend on LevelSpec or historical source trees.

1. Preserve the original fixture JSON and stage the original TypeScript source in
   separate before/after directories. Capture both component and stock inputs with
   `reference/shop-reference.mjs` and `reference/grocer-props-reference.mjs`.
2. Apply `reference/shop-shading.mjs` and `reference/stock-profile.mjs` to the after
   tree, supplying the TypeScript `3c64de86` tree as their independent source.
3. Capture the after tree with the same inputs. Run `reference/shop-finishes.mjs`
   and `reference/grocer-props-correct.mjs` against the original fixtures and both captures.
   Each tool prints its required path arguments when invoked without them.
4. Review the provenance and deltas, then run the unchanged component tests and
   shared Chromium/Wasm acceptance checks before adopting corrected fixtures.

All tools are under `tests/fixtures/rust/reference/`. Review evidence for this pass
is under `generated/verification/shop-reference/`; focused native results are in
`generated/verification/stock-final-corrections.log`.
