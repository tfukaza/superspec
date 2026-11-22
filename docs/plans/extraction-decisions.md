# SuperSpec extraction decisions

This is a milestone record. See [current closeout status](reorganization-status.md#verification)
for the later fixture repairs, combined review and current distribution pin.

The extraction and existing-code reorganization below are complete. The subsequent
[version 1 authoring milestone](authoring-implementation.md) implements the material
language, MaterialSpec/ItemSpec editors, reusable local instance model, dependency
runtime and saved WorldSpec inputs. The subsequent
[SuperSpec consolidation](superspec-consolidation.md) records current naming,
session/model/profile ownership and source-pin evidence; earlier sections retain
their original cutover scope. The later
[fixture correctness pass](fixture-corrections.md) addresses the test failures
preserved during those cutovers; the historical results below remain unchanged.

Baseline: LevelSpec `fb5df8cbf1a3c8b5a8a75a66bf976678cf2106ff`.
The maintained Rust native/Wasm PoC is the extraction source. Its deferred legacy
parity and performance work remains deferred; extraction does not establish it.
Earlier slices established repository ownership, static AML source registration
and selective raster dependencies. The current reorganization separates shared
types/model/catalog, material and item operations, WorldSpec coordination, physical
stages, asset/runtime code, offline authoring and browser infrastructure. The
[ownership status](reorganization-status.md) distinguishes these completed moves
from new product capabilities such as a material language, dedicated editors and
an instance model.

## Decisions recorded before moving implementation

| Decision | Problems resolved | Verification |
| --- | --- | --- |
| SuperSpec owns the existing Rust workspace, schema, implementation tests and browser runtime. Preserve supported entry points during extraction. | Native, Wasm and viewer code otherwise acquire different owners and can drift. | Native checks, Wasm build, UI typecheck and packet/update tests. |
| Colocate theme catalogs with their Rust owner; colocate embedded Blender adapters with the CLI. | `include_str!` and `include_bytes!` reach into obsolete TypeScript source and harness tools. | Build without the LevelSpec checkout on the dependency path. |
| Ship common catalog images as an explicit asset bundle. | Native lookup and browser imports depend on the old source tree. | Native asset loading, browser bundle and relocatable packaging. |
| Keep sample maps, project art and acceptance workflows in LevelSpec. Pin the extracted implementation and consume its native/browser artifacts. | Compiler tests embed Daiban/Dust II; studies import a checkout-local Wasm build. | Sample-map tests run through the LevelSpec harness using SuperSpec. |
| Preserve the latest Rust material, mesh-catalog and shader generators. Retain Blender ingestion and distinct study effects as host/reference code. | Similar-looking historical studies can be mistaken for supported duplicate implementations. | Current callers and baseline history identify replacements; no blanket deletion of study assets. |
| Key finished rasters by effective content, independently of placement names; coalesce equal cold jobs before execution. | Host ID prevents cross-host reuse; equal misses in a single batch rasterize repeatedly. | Cross-host and cold duplicate requests match clean output and preserve current binding IDs. |
| Keep receiver-specific fields and generator seeds in identity. | Treating all weathering as intrinsic would incorrectly share spatially different pixels. | Changed seed, density, source and spatial inputs remain distinct. |
| Retain behavioral tests and remove redundancy only with surviving coverage. Preserve meaningful contract comments. | Cleanup can otherwise erase regression protection or external constraints. | Independent test and implementation reviews. |

## Remaining architectural findings

- Raster identity now includes each job's complete material/source dependencies.
  Geometry binding still uses a conservative host-wide prefix; unrelated material
  edits can repeat binding without repeating rasterization.
- Existing stock catalogs and atlases are loaded once, but placements expand
  vertices into the shared scene buffers. A reusable mesh/instance contract needs
  coordinated preview and exporter changes; a raster-cache change cannot provide it.
- Building seeds include host identity intentionally. Equal placement-independent
  requests can share rasters after the fix; independently seeded buildings still
  produce distinct weathering.
- Source decoding, baked pixels, geometry binding and packet/GPU publication have
  separate costs and cache boundaries. Report cold and warm results separately.

## Compiler and delivery follow-up

| Boundary | Decision | Failure covered |
| --- | --- | --- |
| Dependency planning | Inspect borrowed graph nodes, record the longest input path with each computed key, and retain just the execution order, keys and reverse edges in the finished plan. | A 129-node chain exceeded the limit in one traversal order and passed in another. Both input orderings now produce source-linked errors, including when the nodes have no requested output. |
| Artifact lifetime | Scope cache access to lookup and publication. A separate producer gate preserves the existing allocation limit, and resetting the store invalidates results from the previous generation. | Production and cancellation callbacks can inspect the cache, and resetting during production cannot repopulate the cleared store. |
| Material bake distribution | Preserve the configured worker mode on reset and clear pending jobs, returned rows and retained image mips. Keep content keys stable across resets and scope share response keys to the reporting generation. | Delayed rows for a cleared bake cannot attach to a new request with the same content. |
| Architectural dressing | Give theme planning read-only physical inputs and a diagnostics sink. After review, give architectural dressing only material binding, render geometry and visual provenance operations. Compose ItemSpec models separately, where authored item collision can be admitted by the model. | Dressing retains canonical colliders, review findings and their source data while appending visible geometry. |
| Composition | Check native instance batches in the same model admission step as ordinary instances, including template ownership, material dependencies, placements and buffer costs. | Duplicate identities or excessive geometry fail before delivery. Replacing visual receivers keeps resources still used by another batch. |
| Browser compiler | Keep cancellation tied to HTTP body consumption and deliver source rejection packets with ranges in both native and Wasm preview. | An old response cannot replace a newer edit after the headers arrived. An invalid draft leaves the accepted scene available and exposes the findings for that draft. |
| Distribution and export | Include adapter and vendored compiler source in the source manifest. Publish immutable companions and update the entry after they are available. | Reinstalling tooling does not change source identity. An existing generation remains readable while a newer build is published. |

In 32 paired native Windows runs, median graph planning fell from 0.66 to
0.48 ms for 256 nodes and from 8.57 to 6.40 ms for 1,536 nodes. Keys, execution
order and checked dependency invalidation sets matched in both workloads. The
test used warmed synthetic graphs and timed planning alone. [Measurements](../../generated/runtime-performance/dependency-plan-comparison/result.json) · [Harness](../../generated/runtime-performance/dependency-plan-comparison/src/main.rs)

## Follow-up slice decisions

These decisions were recorded before implementing the source-extension seam and
selective raster reuse. Existing crate and scene-packet interfaces remain stable.

| Decision | Reason | Required evidence |
| --- | --- | --- |
| Register AML extension vocabulary and source checks explicitly, retaining a default Kowloon profile. | The generic parser currently dispatches dressing directly and owns its single-section rule. | A real test-only extension, isolated profiles, ownership conflicts, source-linked errors, roundtrips and unchanged default native/Wasm output. |
| Name source preparation according to the checks it actually performs. | Typed source admission and ID collection do not establish all geometric reference, collision or navigation checks. | Extension source checks run before asset loading; existing physical compilation still diagnoses its own invalid inputs. |
| Derive each raster's material dependencies from every structural layer and coating. | Ground plans currently include every authored material in one cache prefix. Editing the blue structural finish needlessly rebakes 43 ground jobs in the frozen courtyard. | Unrelated edits retain bakes; dependent definition and companion edits regenerate only affected jobs and match fresh compilation. |
| Decode only the source set required by misses, while retaining total-host admission limits. | A partial miss currently decodes all host sources. Cache hits must not hide missing/corrupt dependencies or bypass resource limits. | Actual decode counters, shared-source cases, all companion channels, cancellation and failure recovery. |
| Keep the existing conservative geometry-binding identity and defer retained decoded RGBA storage. | Geometry binding also depends on height sources; narrowing it is a separate contract. Retained RGBA needs an independently bounded lifetime. | Preserve bound geometry, image bytes and complete packet equality; report remaining binding and decode costs explicitly. |

## Completed extraction slice

SuperSpec now owns the six Rust crates, schema, focused native/Wasm/browser tests,
shared preview/editor, catalog textures and embedded theme assets. Native host
helpers resolve owned assets explicitly; live offline regeneration reads current
catalogs between stages. LevelSpec retains its authoring/evaluation workflows,
sample maps, project art and the unchanged courtyard demo folder.

The harness uses a content-pinned source distribution through `tools/superspec.mjs`.
It checks both the recorded manifest and the actual source before dispatch. Exact
byte preservation in `.gitattributes` prevents Git line-ending conversion from
changing source pins or fixture offsets across platforms. The old implementation
copies were removed only after native, Wasm and browser consumption worked.
CI requires a published SuperSpec commit matching that pin; remote publication and
CI execution are not part of this local verification.

Open AML accepts a source with companions selected together, or a containing
folder. It resolves relative paths and publishes source/assets only after a valid
scene is ready. Failed and obsolete requests retain the previous valid scene.
The editor no longer embeds a harness demo or imports a test fixture as its starter.

## Verification and limits

- Native all-target checking, formatting, regular Wasm and Wasm acceptance builds
  pass. The 44 selected core, geometry, exporter and CLI tests pass.
- Four real-receiver bake-reuse tests, three session-cache tests and three parallel
  baking tests pass. Eight building integration checks pass, including complete
  serial/parallel and incremental/fresh packet equality. Five historical sign-plan
  golden failures reproduce identically in the untouched baseline and remain visible.
- LevelSpec's three Daiban/Dust II tests and five packet/update tests pass through
  the pinned consumer after the implementation copies are removed. UI typechecking
  also passes through the consumer wrapper.
- Real Chromium editor and native live-preview acceptance pass. These cover
  relocated companion folders, failure retention/recovery, cancellation, restart,
  source watching and native/Wasm/GLB parity. Desktop layout was inspected at
  1440×1000 and 1280×900 after correcting file-control header crowding.
- A native bundle assembled outside both repositories loads concrete color/normal
  catalog images with an empty executable search path. Native tests and measurements
  ran on WSL2 Linux; Windows native execution and threaded-browser acceptance were
  not run in this extraction.
- The pinned launcher rejects a changed pin before dispatch, recovers after
  correction, preserves caller cwd and exact arguments, and forwards child exit codes.
- Browser automation now has a shared `tools/browser.mjs` entry point. Engine
  runners and LevelSpec capture/planning scripts import that interface instead of
  reaching into deleted test support. A real LevelSpec capture passed after cutover,
  preserving the accepted source hash and camera state.
- Independent code, test and dependency reviews found no remaining blockers for
  this slice. The full renderer scope runner's migrated paths were syntax checked;
  its exhaustive scene matrix was not rerun.

[Frozen courtyard measurements](../performance/extraction-reuse.md) confirm exact
full-packet output across old/new cold/warm builds. Sixty-four cold requests are
coalesced; the warm build performs no new bakes. The single paired measurement
does not establish a whole-scene speedup or memory improvement.

The initial repository boundary is followed by static AML source registration and
selective raster dependencies. The neutral instance model, physical-scene assembly
interfaces, MaterialSpec source compiler/editors, ItemSpec editor and broader job
dependency graph remain implementation milestones in the product plan.

## Completed source and selective-reuse slice

The [compiled AML registry](aml-registry.md) now owns Kowloon source admission and
its single-section rule. Hosts can compose isolated profiles and register source
checks without changing the core vocabulary. A test-only extension proves typed
decoding, forward declarations, roundtrips, exact Unicode diagnostic ranges,
registration conflicts and recovery. `PreparedDocument` accurately limits its
guarantee to source preparation; physical and navigation checks retain their
existing compiler stages. The registry document records the one intentional
unsupported-child diagnostic refinement.

[Selective raster dependencies](../performance/selective-bakes.md) now include
each job's source material, all structural layers and coatings, and every companion
channel's exact bytes. The compiler prepares hits and misses once, decodes sources
needed by misses, and retains bounded pixel-admission metadata. Warm hosts still
obey the complete source allowance. No decoded RGBA storage is retained, and the
conservative geometry-binding key remains separate from raster identity.

Native all-target checking, formatting, release and serial Wasm builds pass.
The 35 source/architecture/catalog checks, six production selective-cache cases,
four existing cache cases, and nine parallel/ground/session integration cases
pass. Building integration retains eight passing cases and the five historical
sign-plan failures; all ten assertion payloads match the untouched baseline.

The production browser suite passes with native/Wasm packet parity, existing
open/edit/export recovery, and new incremental-versus-fresh reuse checks. In the
small browser scene, cumulative bakes are 2/2/2/4 and source decodes 1/1/1/2 across
cold, warm, unrelated-finish and dependent-texture-scale requests. One initial run
under concurrent native checks timed out in the existing repeated-edit sequence;
the diagnostic and final isolated runs passed that sequence. Its cause was not
established; readiness failures now retain UI state and console evidence.

LevelSpec adopts the reviewed source pin. Its three sample-map checks and five
transport checks pass through the consumer launcher. Windows typechecking and
viewer builds pass; native execution is WSL2 Linux. Windows native execution,
threaded-browser acceptance and remote CI remain unverified in this slice.

## Completed existing-code reorganization

The [ownership inventory](reorganization-status.md) now covers all five product
areas and their shared infrastructure. The existing implementation has separate
types, model and catalog crates; generic material/item modules; WorldSpec build
coordination, layouts and Kowloon content; shared asset/runtime mechanisms; and
model-only export adapters. The physical compiler remains private behind typed
stages, with theme planning state owned by WorldSpec. Compatibility caller paths
remain available without duplicate implementation.

Offline generator algorithms now live with MaterialSpec, ItemSpec and Kowloon.
Their optional authoring dependencies stay out of the runtime Wasm build; native
arguments, filesystem publication and font acquisition remain host adapters.
Browser model transport, rendering, compiler workers and navigation have distinct
owners shared by the editor and native preview. Current documentation and live
regeneration paths follow those owners.

LevelSpec has canonical courtyard, sign-weathering and Dust II demo folders.
Distinct artwork and historical study effects are retained with accurate workflow
status. One superseded TypeScript build wrapper was removed after its unchanged
AML successfully built through the native replacement. The demo migration and
bounded study disposition are recorded in LevelSpec's review documents.

Independent source, test and dependency reviews found no remaining substantive
ownership gaps in the existing-code reorganization. Comments explaining behavior
and limits remain; tests retain independent expectations and failure/recovery
coverage. The new direct consumers exercise the model/export boundary and a
second theme using the common catalog, MaterialSpec and ItemSpec without Kowloon.
No performance measurement or optimization was undertaken in this pass.

### Final verification

- Formatting, workspace all-target checking, native release and serial Wasm builds
  pass. The optional threaded compiler branch also typechecks for Wasm.
- The final foundational run passes 56 tests across types, core, catalog, geometry,
  export and CLI, including five offline-authoring cases. Eight generated offline
  outputs match hashes from the frozen original executable.
- Focused material/item checks pass 27 tests; asset/runtime/scene/source checks pass
  36, and the independent theme test passes. The isolated model/export/delta
  consumer builds and passes without the compiler or AML parser.
- All 28 frozen complete scene/export artifacts match byte-for-byte. The detailed
  courtyard's complete 223,180,332-byte packet retains SHA-256
  `823b6d159d6e06b70ab239c11c8e4e24525b343a2dc9675225a378a050247462`.
  Seeded layout generation, population and validation also match the frozen output.
- Production editor and native file-preview browser acceptance pass, including
  source/asset rollback, supersession, restart and native/Wasm parity. Browser GLB
  delivery passes the Khronos validator and actual Three.js loading with exact
  native/Wasm bytes. The relocated demos also preserve native/browser packets and
  missing-companion recovery.
- LevelSpec's three sample-map tests and five transport tests pass through the
  reviewed source pin. Typechecking passes through the consumer launcher.
- Building integration remains eight passing cases and five existing golden
  failures. All ten assertion payloads match the original baseline by test name;
  only the failure log order changes between serial and parallel test execution.
- The broader procedural fixture run exposes an existing window-color mismatch:
  `identity/casement color[3504]: 0.011 != 0.005`. The untouched and reorganized
  native implementations fail the unchanged fixture identically, and actual
  Chromium/Wasm reports the same assertion. Acceptance-only Wasm diagnostics now
  expose panic messages and check-group names; golden expectations and production
  panic behavior remain unchanged. The fail-fast run does not establish results
  for later fixtures in that group.

Native checks ran in WSL Linux, UI builds/typechecking on Windows, and browser
acceptance in Chromium. Windows-native execution, threaded-browser execution and
remote CI were not run. These results do not claim completion of the new material
language, dedicated editors, instance model or generalized dependency-graph
features described separately in the product roadmap.
