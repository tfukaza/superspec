# SuperSpec naming and architecture consolidation

This is a milestone record. See [current closeout status](reorganization-status.md#verification)
for the later fixture repairs, combined review and current distribution pin.

SuperSpec is the reusable engine and authoring toolkit. LevelSpec is the sibling
harness that uses it to build and evaluate game levels. This follows the completed
extraction and version 1 authoring milestone; their recorded hashes and measurements
remain historical evidence. This slice does not measure or optimize performance.

## Decisions before implementation

| Area | Ownership and cutover |
| --- | --- |
| Rust workspace | Short responsibility directories: `types`, `model`, `catalog`, `aml`, `profiles`, `geometry`, `engine`, `export`, `cli`, `wasm`. Packages use the `superspec-` namespace; Rust imports use `superspec_`. |
| Native/browser hosts | Executables become `superspec`, `superspec-studio`, `superspec-materials`; Wasm artifacts and editor branding use SuperSpec. Update packaging, build scripts, tests and maintained consumers together. |
| Product metadata | Catalog identities use `superspec-material:` and GLB metadata uses `extras.superspec`. Preserve authored source identities, numeric output, deterministic seeds and versioned binary framing. No duplicate legacy binaries or crate aliases. |
| LevelSpec harness | Keep its repository/package identity, maps, projects and historical evidence. Update engine imports, artifact selection and current command documentation. |
| Session ownership | One engine session owns both AML and authoring producer artifacts, with shared lifecycle and bounded retention. Recipe adapters own recipe identities and statistics; the runtime owns artifact retention. |
| Scene composition | World assembly retains shared local resources and placements. Expand into scene buffers at explicit delivery/export boundaries. Physical, navigation, source and render-visibility information remain independent. |
| Export preparation | Lightmap preparation derives an owned export view from an immutable scene. Independent export settings cannot modify the canonical scene or each other. |
| AML profiles | Generic AML has no built-in Kowloon decoder. The profiles crate owns its registration; product hosts explicitly select the default profile. Custom registry entry points remain available. |

## Required verification

Focused session, profile, composition and export behavior tests; native and Wasm
builds; production native/browser packet and GLB parity; editor and catalog loading;
portable native packaging; and the pinned LevelSpec consumer. Compare existing
frozen geometry/material contracts allowing only the deliberate product metadata
rename. Keep original frozen artifacts intact. Review structural and naming changes
independently before writing the distribution manifest or adopting a consumer pin.

Existing historical building goldens and the known window color fixture discrepancy
are separate from this slice and must not be silently rewritten.

The `LSP3`/`LSU1` packet markers, `levelspec-raster/1` observation schema identifier,
established hash-domain strings and default layout
seed are versioned compatibility data, so they retain their existing values. The
current catalog scheme and GLB metadata key are product names and change together
with their maintained consumers. Historical evidence and LevelSpec harness paths
retain the names that identify their original source.

## Completed implementation

All four architectural follow-ups and the naming cutover are complete. Independent
implementation and test reviews approved the session store, profile composition,
retained world model, export preparation and maintained consumer paths. Review
also closed cancellation during physical source loading and aggregate admission
before retained world layers are published.

- `crates/engine/src/session.rs` owns AML and authored artifact lifetimes. The
  theme cache is an adapter over the common runtime store, with its existing
  retention partitions and statistics. Clearing the session preserves live outputs.
- `crates/model/src/composition` owns retained local resources, ordered placements
  and visual replacements. Effective delivery limits are checked without expanding
  each placement; separate retained admission includes superseded layers and counts
  shared resources once. `ProjectOutput::scene` is an explicit delivery boundary.
- `crates/export` exposes immutable lightmap preparation and `world_glb`.
  Derived views preserve source, physical metadata and authored tangent attributes.
- `crates/profiles` owns Kowloon AML decoding and product registration. The generic
  AML crate has no built-in theme decoder; native and Wasm hosts select profiles
  explicitly.
- Native commands, browser artifacts, catalog identities, export metadata,
  packaging, CI, maintained studies and the LevelSpec launcher use SuperSpec names.
  LevelSpec keeps its harness/package identity and authored levels.

## Verification and distribution

The reviewed distribution is
`94fd20c8a9bc009fdb4e2c01c8a18ae0e4b77d629beafb4719b79fc4b0099430`
(873 files). SuperSpec's manifest and LevelSpec's adopted pin both verify.

- Formatting, native all-target checking, all three release commands and 149
  selected native tests pass. These include all foundation/model/export/native
  host tests, engine unit tests and the affected language/session/registry cases.
- Production and acceptance Wasm builds pass. Actual Chromium/native material
  results match in all 15,573 bytes; authored World GLBs match in all 76,040 bytes.
- Typechecking, viewer building, Material/Item editors, AML editor, native preview,
  and Khronos/Three.js export acceptance pass. A maintained LevelSpec study builds
  against the renamed catalog and Wasm exports.
- All 28 frozen packet/export artifacts match after only explicit product-name
  substitutions. The full 223,180,332-byte courtyard packet also matches the frozen
  packet with only 85 catalog-scheme/compiler-label substitutions. Its new hash is
  `5ae3e7f592bdffb8f481532b9ba52a87a04b8214b09043623db6688fb31503bd`.
  Before the naming cutover, the retained World implementation matched the saved
  executable's complete packet and GLB byte for byte.
- A model/export consumer outside both repositories passes without engine or AML
  dependencies. LevelSpec's three Daiban/Dust II cases pass with renamed path
  dependencies. Its pinned launcher preserves cwd, arguments, binary stdout and
  exit codes, chooses the renamed studio, rejects source drift and recovers.
- A relocated bundle runs all three new executable names with an empty search
  path and no executable overrides. Catalog image lookup, missing-image rejection,
  recovery, explicit authored companions and the documented World example pass.

Evidence is under `generated/verification/consolidation-*`, the unchanged frozen
artifacts under `generated/verification/model-contracts/before`, and the earlier
`immutable-world-parity.json` comparison. No performance measurements or tuning
were performed.

This is not a claim that the full legacy suite is green: the previously established
five building goldens and window-color kernel discrepancy remain enabled and were
not rewritten. Native execution was Linux under WSL2; Windows native execution,
threaded-browser acceptance and remote CI remain unverified.
