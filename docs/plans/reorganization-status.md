# Existing implementation reorganization

The existing-code ownership migration is complete. It follows the modularization
plan while preserving the supported native, Wasm, CLI and browser contracts.
Performance measurement and optimization are deferred by the user's instruction.
The subsequent [SuperSpec consolidation](superspec-consolidation.md) records the
crate/product rename, shared session lifecycle, retained world instances,
immutable export preparation and explicit source-profile ownership.
The [theme runtime boundary](theme-runtime-boundary.md) completes generic session
bookkeeping, supplied theme composition, canonical caller imports and automated
dependency checks.

| Responsibility | Implementation owner | Boundary |
| --- | --- | --- |
| AML, source admission and extension profiles | `superspec-aml`, `superspec-profiles`, engine `source` | Generic AML and shipped Kowloon profiles have separate owners; source checks precede generation without claiming physical certification. |
| Physical lowering, collision and structural/navigation checks | Compiler `architecture`, `validation` | Private physical state is accessed through typed build stages; visual theme state is separate. |
| Shared values and deterministic helpers | `superspec-types` | No parser, theme, renderer or host dependency. |
| Scene and delivery records, complete packets and scene deltas | `superspec-model` | Export and transport consume the same concrete types without the compiler or parser. |
| Common catalog definitions and image identities | `superspec-catalog` | AML adaptation belongs to `superspec-aml`; hosts supply image bytes. |
| Material sampling, composition, raster layout and encoded artifacts | Compiler `materialspec` | Generic operations do not import themes or compiler assembly. |
| Local meshes, face/grid emission, GLB ingestion and chart binding | Compiler `itemspec` | Theme finish state stays outside the generic mesh. |
| Scene ordering and seeded layouts | Compiler `worldspec` | The neutral coordinator consumes a theme adapter; `shipped` selects the default profile and theme. |
| Kowloon recipes, assemblies, placement and content | `worldspec::themes::kowloon` | Shared material/item operations are inputs; theme recipes stay local. |
| Source admission, resolved bytes and content identity | Compiler `assets` | Host loading policy is supplied; no native filesystem dependency. |
| Completed artifact storage and indexed job execution | Compiler `runtime` | Producers supply payloads and work; runtime does not import recipes. |
| Offline material/item generators | MaterialSpec, ItemSpec and Kowloon `authoring` modules | The optional authoring feature contains portable algorithms; CLI adapters retain filesystem, font acquisition and publication. |
| Target preparation and serialization | `superspec-export` | Normal dependencies exclude the compiler and AML parser. |
| Browser model, rendering, compiler workers and navigation | `browser/model`, `renderer`, `runtime`, `navigation` | Editors and preview use shared infrastructure. |
| Authored maps, demos, studies and integrated acceptance | Sibling LevelSpec | Its launcher verifies the SuperSpec source distribution before dispatch. |

Root compilation and shared scene type APIs remain available. The temporary
`dressing`, `imports` and `generation` forwarding modules have been removed;
maintained callers use their owning module. Asset content and catalog behavior
were preserved apart from the documented namespace cutover. Reference fixture
corrections have independently reviewed provenance;
tests retain expected geometry, channel values, errors and recovery behavior.

The canonical demo inventory includes courtyard, sign weathering and Dust II.
The material study owns its distinct companion images and uses registered IDs for
byte-identical shared images. Maintained callers follow the new locations;
historical evidence remains with its study.

The product plan also describes new capabilities: material source-language
compilation and dedicated MaterialSpec/ItemSpec editors, a reusable instance
representation, richer registered checks and a generalized dependency graph.
Those are feature-development milestones, not existing code moved by this
reorganization. The subsequent [authoring milestone](authoring-implementation.md)
implements the material language, two editors, instance model and shared job graph.
The later consolidation replaces mutable lightmap preparation with immutable
derived views and unifies session artifact ownership. Source preparation still
has its explicit source-only guarantee.

## Verification

Independent reviews cover the dependency direction, moved algorithms, serialized
field order, source identities, phase order, cancellation, resource admission and
failure recovery. A caller-created shared model exports without the compiler or
parser; a second test-only theme uses the common catalog and generic material/item
interfaces without importing Kowloon.

The original extraction cutover and its 806-file source pin are recorded in the
[extraction decisions](extraction-decisions.md#final-verification). Its inherited
building and window failures were retained during the move. The subsequent full
baseline exposed 32 failing cases; the [fixture correction record](fixture-corrections.md)
documents their repair, independent reference provenance and preserved behavior
coverage. Those historical failures are resolved.

The combined closeout also reviews the later MaterialSpec language, receiver
features and editor changes. It fixes module visibility, source-edit boundaries,
CLI checker snapshots, asynchronous editor races, metadata delivery and resource
accounting. These corrections are described in the
[combined review](fixture-corrections.md#combined-closeout-review).

The final reviewed distribution contains 943 files with SHA-256
`4187c218dbed5b1c7273e4974f268c17d78c7e0e7ebb76087731a44132f1c073`.
SuperSpec's `superspec.manifest.json` and LevelSpec's `superspec.lock.json` adopt
that same source; both distribution checks pass.

The complete native run passes 528 cases across 139 targets, with no failures or
ignored cases. All 444 original baseline cases remain present and pass, including
all 32 originally failing cases. Source and executable hashes stay unchanged
throughout execution. Workspace doctest targets, formatting and the architecture
check of 319 Rust modules and all normal Cargo dependency edges pass.

The full shared kernel suite passes in Chromium-Wasm, including the receiver-index
regression. MaterialSpec and WorldSpec native/Wasm parity, recipe editing and race
regressions, material/item and AML editors, preview recovery, and browser export
acceptance pass. Application and generated SDK types pass; the real TypeScript CLI
accepts the three shipped recipes and reports source-linked invalid-import errors.
All 28 frozen packet/export artifacts remain byte-identical. Relocated portable
commands pass asset-resolution and recovery checks, and a standalone model/export
consumer builds without the engine or AML parser in its normal dependencies.

The pinned LevelSpec consumer passes all three Daiban/Dust II cases, five
packet/update cases and TypeScript checking. Its launcher preserves the caller's
working directory, arguments, binary output and exit status, rejects a mismatched
distribution before dispatch, and recovers with the valid pin. Final evidence is
recorded in `generated/verification/modularization-closeout-summary.json`, with
native case details in `generated/verification/failing-tests-closeout/results.json`
and host checks under `generated/verification/modularization-closeout-*`.

The reorganization, independent review, fixture repair and source-pin adoption are
complete. Further language/editor capabilities remain feature-development work
within their own specifications; performance work is a separate deferred task.

Native execution is Linux under WSL2, with Chromium browser acceptance and Windows
TypeScript checking. Native Windows, threaded-browser acceptance and remote CI were
not run locally. Performance measurement and optimization remain deferred.
