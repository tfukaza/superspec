# Implemented capabilities and current limits

AML `.aml` or identical `.xml` is the editable source. Rust owns parsing,
resolution, geometry, procedural models and materials, validation/navigation,
exports, the CLI and the native file-preview service. Browser Wasm uses that
compiler. TypeScript owns Three.js presentation, browser input and UI automation.

## Current native workflows

- `init`, `inspect`, `validate`, `scene`, `glb`, `obj`, `plan`, `build` and `preview`.
- `generate` with seven seeded layout families and theme data, including pickups,
  spawn orientation and enemy posts.
- `trace` with PNG/JPEG input, bounded segmentation, AML output and raster evidence.
- `migrate` with historical SVGX translation, companion copying and GLB proxy imports.
- `reconstruct` with independent landmarks, silhouettes, strokes and masks,
  reference rendering and an optional model-assisted revision loop.
- `bake` with native preparation and publication around external Blender/Cycles.
- `game-export` through the native build pipeline and a Y-up game manifest.
- `visibility` with supported observation samples, optical occlusion,
  protected pairs/areas, entry angular spans and explicit omissions.
- `benchmark` with cold, warm-full and supplied edit cases through packet assembly.

The architecture compiler supports profiles with holes, paths, floors, walls,
openings, flights, frames/datums/instances, terrain/grading, water, references,
materials and structural assertions. Procedural courtyard output includes
buildings, shops, signs, roof equipment, balconies, ground and connecting spans.

Validation keeps physical collision separate from visual detail. It reports
source-linked findings, land and boat navigation, declared routes, entrance
capacity, floor coverage and traversal links. Navigation and visibility provide
sampled evidence. They do not establish continuous gameplay reachability.

Portable delivery includes GLB/glTF, OBJ/MTL, SVG plans, runtime JSON and collision data. Optional FBX uses Blender. Quake delivery produces convex brushes, a WAD
and BSP output when the external tools are supplied. Open water and incompatible
Quake materials produce explicit errors.

## Browser and release boundary

The Wasm editor supports live AML edits, local companion assets, validation and
GLB download. Incremental scene packets reuse unchanged buffers and images.
Three.js retains matching meshes and textures, and renders on demand.

Native preview serves prebuilt UI with local-only file access. Both browser
surfaces provide orbit cameras and TypeScript standing, jumping and crouching inspection. Capture
automation records accepted source/asset fingerprints and actual camera state.
A packaged release includes the executable, reviewer and material assets.

Parallelism is bounded. Raster, image-admission and completed-binding caches
have explicit memory limits. Shared-memory Wasm remains experimental and uses
a pinned Rust standard-library rebuild. Plain Wasm supports ordinary hosting.

## PoC limits

The user selected a PoC-first implementation with compact verification.
Comprehensive feature parity, broader architectural warning coverage, tighter
intra-area route bounds and cross-platform/browser hardening remain outstanding.
The former graph-exploration lab and illustrated-renderer extras are outside the current browser PoC. Their source is retained
in this worktree's local recovery snapshot.

GLB visual imports currently carry geometry. Use explicit native geometry for
their collision proxy. Model-assisted reconstruction needs initial AML reference
declarations. Its HTTP transport was checked against a local fixture server.
A paid provider run has not been exercised.

Local Windows native, Chromium and Blender checks are recorded in
[Rust progress](../../levelspec/docs/RUST_PROGRESS.md). Remote CI and destination-engine gameplay
checks have not been run. The one-second incremental preview goal remains open.
