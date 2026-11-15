# Existing runtime module ownership

This records the initial extraction. The later
[consolidation](superspec-consolidation.md) moves shipped AML decoding into the
profiles crate, and the [theme runtime boundary](theme-runtime-boundary.md) removes
the temporary forwarding APIs and concrete theme dependencies in generic orchestration.
The retained decisions below describe that earlier cutover, not current import paths.

This reorganization follows the [product boundaries](spec-modularization.md)
using the supported Rust implementation. It changes ownership and dependencies,
without adding an authoring language or editor, changing generated output, or
undertaking performance work.

## Decisions recorded before the moves

| Owner | Existing implementation | Dependency rule |
| --- | --- | --- |
| `materialspec` | Surface inputs, layer composition, sampling, raster layouts, rasterization, deterministic noise, color helpers and encoded material artifacts | Generic material code uses shared values and geometry/image kernels. It does not import a theme, drawing collector or compiler scene. |
| `itemspec` | Existing geometry-only GLB ingestion, bounded attributed mesh storage, polygon emission, planar mapping and smooth-grid triangulation | Item operations consume shared geometry and material contracts. Theme appearance state does not belong in the generic emitter. |
| `worldspec::themes::kowloon` | The existing procedural theme, supplied recipes, palette presets, role bindings, placement, receiver planning, spatial effects and embedded item catalogs | The theme consumes MaterialSpec and ItemSpec and owns recipe keys, raster evaluation and receiver binding. Storage, coalescing and dispatch use the [shared runtime](shared-runtime.md). |
| Compiler assembly adapters | AML/physical-source adaptation, final scene publication and compatibility entry points | Existing public paths forward to the new owners; implementation is not duplicated. |

The old `dressing` directory mixed reusable material operations with authored theme
content. The reusable sampler and artifact code move physically to MaterialSpec.
The palette's color arithmetic moves with those operations; household colors,
shop choices and construction presets remain with Kowloon. The shared exact sRGB
conversion table and typed values retain their separate shared owner.

The theme `Mesh` wraps ItemSpec's bounded mesh and retains its receiver collection
and drawing identity. Theme `Pen` delegates polygon and smooth-grid emission to
ItemSpec, while its composite shapes keep finish selection and member exposure
scopes. Those composites retain their current face, member, exposure and
tint-callback ordering. Moving the complete themed pen into ItemSpec would
introduce a reverse dependency. Receiver binding stays outside the generic
encoded material artifact.
The existing `BakedMaterial::map_uvs` compatibility method is implemented by the
ItemSpec binding adapter over its generic mesh, not by the MaterialSpec artifact
module. Theme mesh wrappers coerce to that generic input.

Kowloon's JSON item catalogs, lettering definitions and business vocabulary move
with their runtime consumers. Relative embedded paths remain local to that tree.
Live native and harness regeneration tools must target the new catalog locations.
Authored material IDs, asset IDs and the externally supplied `assets/materials`
bundle remain unchanged.

## Compatibility boundaries at the initial cutover

`superspec_engine::dressing` and `superspec_engine::imports` initially preserved
caller paths through reexports. These forwarding modules have since been removed;
maintained callers now name their owning module.
The compiler's architecture adapters translate resolved AML and physical geometry
into theme inputs. The [WorldSpec build coordinator](world-build-boundary.md)
owns phase ordering and theme state through typed internal physical stages;
it does not expose the physical compiler's mutable implementation maps.

The Kowloon source decoder initially remained in the core extension registry.
It now belongs to `superspec-profiles`, which composes the shipped profile without
a reverse dependency from the generic parser to the engine.
The common material catalog's AML registration adapter
also continues to own its `Node`/`Document` conversion; the generic material kernel
does not acquire that responsibility.

## Verification

The moves require native and Wasm checks, existing material/import/theme behavior
tests, stable embedded content, and consumer checks for relocated regeneration
paths. Generic modules must have no imports from `worldspec`, legacy `dressing`,
or compiler scene assembly. Existing tests remain behavior checks; no timing or
performance measurements are part of this cleanup.

All thirteen moved JSON catalogs and lettering files retain their original SHA-256
hashes. The importer and noise implementation are unchanged apart from their file
ownership; the surface kernel changes only imports and crate-internal visibility.
Material raster tests use the new canonical owner path. Existing pen golden
fixtures now use canonical owners, while standalone ItemSpec tests
exercise the generic emitter and the patch survey/render admission distinction.

Focused correctness checks passed: `surface_raster` (6), `metal_surfaces` (5),
`pen_emission` (7), `item_geometry` (3), and `timber_mapping` (6). These retain
frozen channel/geometry expectations, reflection and tint ordering, cancellation,
admission limits and failure recovery. The local run log is
`generated/verification/runtime-material-item-tests.log`.
