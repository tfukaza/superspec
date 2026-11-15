# Material and item authoring implementation

This is a milestone record. See [current closeout status](reorganization-status.md#verification)
for the later fixture repairs, combined review and current distribution pin.

The existing-code reorganization and version 1 authoring milestone are complete.
This implements the first editor/language workflows from `material-spec.md` and
`spec-modularization.md`, within the capability boundaries recorded below.
Performance measurement and tuning remain deferred by the user's instruction.
The subsequent [SuperSpec consolidation](superspec-consolidation.md) records the
current package names and architecture. The acceptance hashes below identify this
earlier milestone, before the product metadata rename.

## Decisions before implementation

- A bounded, typed MaterialSpec language is compiled and evaluated by the same
  Rust code on native and Wasm. Versioned `.ms` modules have imports, typed public
  parameters, reusable expression functions, intermediate fields and PBR outputs.
  Source modules and presets are authored data; compiled programs and maps are
  derived artifacts. There is no separate JavaScript implementation of the language.
- Material preview and export use the same evaluator and channel conventions.
  Physical coordinates, pixel footprints, source images and finite runoff edges
  are explicit inputs. Baking a selected receiver retains its physical geometry
  and its authored source; preview assignments belong to the editing session.
- Versioned `.item.json` definitions describe procedural pipe/duct items or an
  imported GLB, public geometry/finish controls, material requests, attachments
  and collision intent. Import preserves scene hierarchy, transforms, UVs,
  material slots and supplied image bytes. Unsupported features fail explicitly.
- Shared immutable mesh resources and separate placements form the reusable
  instance model. The existing scene packet remains a compatibility delivery
  adapter. Export preparation builds owned output from this model.
- The generation dependency graph records content and implementation identities,
  declared inputs and source locations. It rejects cycles, coalesces equal work,
  preserves valid immutable results, and publishes only the current successful
  revision. Existing legacy compiler behavior remains compatible.
- Material and item editors share the existing renderer and Rust worker boundary.
  They support source edits, parameter controls, AML folders and GLB input,
  selection, mapping, field inspection, save/reopen and error recovery. Source
  errors retain the accepted model, material, camera and selection. Saved bundles
  include modules, presets and companion bytes, rather than generated maps alone.

## Acceptance inventory

Each capability needs implementation review and behavior evidence before its
status is changed to complete.

| Capability | Required evidence | Status |
| --- | --- | --- |
| Material language and SDK | Imports and source errors; type checks; helper composition; physical units, seeds, footprint and channel conventions; chipped concrete and painted metal | Complete |
| Material editor | Real native/Wasm recipes on AML and transformed GLB; selection and restore; live edits and parameters; intermediate fields; save/reopen; failure and supersession recovery | Complete |
| Runoff and supplied maps | Finite edges, twenty vertical/inclined/horizontal receivers, slope/gravity behavior, missing input recovery and repeated image identity | Complete |
| Item definitions and editor | Procedural and imported item; geometry and finish controls; saved recipe assignment; slots, transforms, attachments and collision; save/reopen | Complete |
| Shared instance model and export | Shared local mesh across placements, per-instance identity/materials/collision, nonmutating independent exports, existing packet interoperability | Complete |
| Shared dependency runtime | Source/import/image/geometry/material dependencies reused by project export, cycles, concurrent coalescing, selective invalidation, cancellation, latest revision and recovery | Complete |
| Integrated world use | Saved material and item used in WorldSpec, overrides and provenance, same result in editor and native build | Complete |
| Existing behavior and consumer handoff | Existing focused checks, native/Wasm and production browser workflows; reviewed source manifest and LevelSpec pin | Complete |

No timing or benchmark result is required for this implementation pass.

## Implemented contracts and version 1 boundaries

The [authoring guide](../AUTHORING.md) documents editor controls, source syntax,
saved projects, native commands and WorldSpec placement. `levelspec-studio` accepts
raw material/item entries and relocatable project bundles. Material and item
editors share one worker, renderer, project format and native generation engine.

The material backend evaluates portable fields into textures. Texture mipmaps
respond to the view; arbitrary GPU material programs and scene-buffer effects are
not implemented. Unsupported operations fail explicitly. Planar finite-edge runoff
is supported; curved flow and pooling remain the later extensions identified by
the product plan. Static GLB import preserves its source data and checks the
narrower renderer/export capability boundary instead of silently losing features.

Shared local geometry and immutable generation artifacts are available to library
consumers. The compatibility packet still expands placements into its existing
buffers. Producers run serially within an authoring session; equal concurrent
consumers share work, and one canceled consumer cannot publish or cancel a job
still needed by another current consumer. Hardware instancing, parallel scheduling,
persistent storage and finer scene/export scheduling remain separate extensions.
No throughput or memory-performance claim follows from the admission-budget tests.

## Verification evidence

- Native coverage includes 15 material-language cases, 12 ItemSpec cases, four
  authoring/asset-dependency cases, two composition-validation cases, five runtime
  graph cases, one program-retention case, three WorldSpec cases and three native
  authoring CLI cases. The 31 shared types/model/core/geometry/export tests pass.
- The same material fixture produces 15,573 identical native and Chromium/Wasm
  bytes, including sampled fields, baked channels and HDR emission. Production
  WorldSpec export produces 76,040 identical GLB bytes, with explicit slot
  overrides, four reachable catalog inputs, supplied image recovery, collision
  metadata and unchanged accepted preview state. Removing the saved finish from
  the active world also removes its image dependency.
- Production studio browser coverage exercises both editors, imports, parameter
  and code edits, source diagnostics, field/channel inspection, assignment versus
  selection, restore, finite runoff, missing inputs, superseded work, save/reopen
  and exact native GLB export. View-only lighting changes preserve authored
  channels and camera. The 1440×1000 and 1280×900 layouts were inspected visually.
  Replacing an AML/GLB companion rebuilds the retained base before publication;
  invalid replacements retain the previous input snapshot and corrected imports
  recover with matching preview, save and export bytes.
- The full frozen courtyard also opens in the new Material editor: all 4,177
  visible mesh IDs and original bindings match native output. Applying chipped
  concrete to a courtyard wall preserves its exact indexed triangle positions,
  every scene bound, camera and selection; restoring returns every original
  binding. Original/applied/restored views were inspected, with no browser errors
  or capability limit hit. Evidence is in `generated/verification/courtyard-studio/`.
- The existing AML editor, native file preview and GLB browser runners pass.
  Exported GLBs also pass Khronos validation and load in Three.js. Five packet and
  update checks pass. All 28 frozen complete scene/export artifacts, the frozen
  layout result and the 223,180,332-byte courtyard packet remain exactly equal.
- Native all-target checking, formatting, production/acceptance Wasm builds,
  TypeScript checking and viewer bundling pass. A release assembled outside both
  repositories loads its packaged images with an empty executable search path,
  rejects a removed packaged image, recovers with exact bytes and builds the
  documented WorldSpec example. The native preview host serves both studio pages
  and their worker/Wasm resources with the required headers.
- Independent implementation, dependency, test and host reviews approved the
  final changes. The reviewed 864-file source distribution is
  `040452d3a8aad8583c0786a772c38f3b0b37c087d53e3a76c93905c65948ff65`;
  SuperSpec's manifest and LevelSpec's pin both verify. The pinned consumer passes
  three Daiban/Dust II cases, five packet/update cases and TypeScript checking.
  Its authoring wrapper preserves caller cwd, relative executable selection,
  arguments, binary stdout and native exit status. A deliberately mismatched
  distribution is rejected before dispatch, and the correct pin recovers.

Detailed local evidence lives in `generated/verification/authoring-*`,
`generated/verification/final-authoring-*`, `generated/verification/material-program-*`
and `generated/studio-acceptance/`. These are generated verification artifacts,
not authored source or distribution identity.

At this milestone, legacy theme generation and mutable lightmap preparation
retained their behavior. The subsequent consolidation replaces the mutable API
and unifies session ownership. Broader legacy Rust parity, threaded-browser acceptance and remote
publication/CI execution are not established by this milestone. Performance work
remains deferred by the user's instruction.

Five historical building golden assertions and the existing window-color kernel
failure were already reproduced in the untouched extraction baseline. Their
expectations remain enabled; this milestone does not claim an entirely green
workspace suite. Native execution here is Linux under WSL2; Windows native
execution and remote CI have not been run.
