# MaterialSpec extensions, ItemSpec MVP and TreeSpec

**Ordering and reconciliation: see [roadmap.md](roadmap.md).** Phases 1A and 1B are
roadmap stage 2, phase 3 is stage 3 (its geometry steps start in stage 1), phase 2 is
stage 4, phases 1C and 1D are stage 5, phases 4 and 5 are stages 6 and 7. Where this
plan and the roadmap differ, the roadmap wins. Changes it makes here:

- 1A step 5 is no longer "later": separating intrinsic appearance from receiver
  mapping is delivered in stage 2 as weathering layers (grime, stain, moss and joint
  rust become their own MaterialSpec appearances, applied like decals by ItemSpec
  and WorldSpec over a shared intrinsic base).
- 1B gains the weathering-layer recipe kind beside the decal appearance.
- 1C.3 does not define its own lighting data: programs read the sun from the shared
  record in [lighting-settings.md](lighting-settings.md). Shadow style and ambient
  occlusion are global or per-light rendering settings, never material features;
  `shadeBand` reads the same light and style parameters as lit materials.
- 1C.2 depends on the cold-shader-compile and GPU correctness work in
  [performance-and-shadows.md](performance-and-shadows.md), because a delivered live
  program makes shader compile a shipped cost. Games ship baked maps by default.
- 1A.4 and the item MVP's drag behaviour follow one rule: per-pixel shader while
  dragging, GPU bake in UV space when the edit settles, CPU bake for export.
- Phase 2's instanced batches are the same work as the performance plan's material
  sharing and batching.

Status: proposal, not implemented. This plan sequences three pieces of work that
share groundwork: MaterialSpec features (phase 1), the ItemSpec MVP (phase 3) and
a new TreeSpec module for procedural foliage (phases 4 and 5). It extends the
[product plan](spec-modularization.md) and the [MaterialSpec plan](material-spec.md).

## Goals

- **ItemSpec MVP.** Rebuild the washroom (`tests/studio/washroom`) from items, with
  routed pipes and fittings, walls with windows, and materials and decals that stay
  correct while walls and pipes are resized.
- **TreeSpec.** Procedural foliage, starting with painterly grass and growing to
  bushes and trees. Every plant renders in two styles from one structure:
  illustrative (painted, unlit) and photorealistic (lit PBR).
- **MaterialSpec first.** Both goals are blocked on material features, so those
  land before either module grows.

## References

- Painterly source: `experiments/painterly-shader` in
  [tfukaza/lab](https://github.com/tfukaza/lab) (Babylon.js). Its
  `docs/shader-pipeline.md` and `src/generator/illustration-material.ts` define the
  look. Ground and blades share one unlit shader; a blade takes the ground paint
  and ground normal at its root, so it only reads where it crosses a shade
  boundary. Placement is thinned to the shade boundary and convex crests.
- Blender study (outside this repository): `painterly_eevee.blend` and
  `painterly_brush_foundation.blend`, which place blades only near colour changes.
- Current state, from a code survey on 2026-11-16:
  - A delivered material is baked PBR maps plus six fixed live operations
    (`crates/types/src/material.rs`, `browser/renderer/live.ts`).
  - Per-pixel program evaluation exists only as a Studio preview
    (`crates/engine/src/materialspec/program/glsl.rs`, `browser/renderer/shade.ts`).
  - Programs have no sun, time, vertex-colour or per-instance input, no colour
    ramp and no cutout output. The renderer is always `MeshStandardMaterial`.
  - Placements are expanded to flat meshes at delivery
    (`crates/model/src/instances.rs`); the packet has no instancing, no lighting
    data and one UV set. The renderer draws on demand with a hard-coded sun.
  - ItemSpec has one generator per item (straight pipe, straight duct, GLB) and
    one `surface` slot. `superspec-geometry::Solid` already has extrude, inset and
    booleans but ItemSpec does not use them.
  - Each receiver bakes its own atlas keyed on its geometry, so any resize
    re-bakes. Concrete is centred on receiver bounds and tile grout on the world
    origin, so both shift when a wall is resized.

## Phase 1: MaterialSpec

Four steps. 1A and 1B unblock ItemSpec; 1C and 1D unblock TreeSpec. Each step
keeps native and Wasm semantics shared and adds its inputs to the per-map
dependency keys in `materialspec/program/deps.rs`.

### 1A. Stable mapping under resize

1. **Pattern anchor.** Add an `anchor` frame to the receiver inputs, supplied by
   the caller (item part, AML object or scene assignment), defaulting to today's
   behaviour. Add an `anchored` coordinate beside `surface`, `object` and `world`.
   Move the concrete and tile recipes to it so bounds changes stop shifting the
   pattern.
2. **Mapping per slot.** Let an item slot choose `uv`, `object`, `world` or
   `anchored` instead of forcing `Mapping::Object`
   (`crates/engine/src/authoring/item.rs`). Swept parts use their metric UVs
   (arc length by circumference), so added length extends the pattern.
3. **Named receiver features.** Extend receiver features
   (`materialspec/program/features.rs`) from creases, boundaries, seams and
   contacts to caller-named sets such as `joint`, `station`, `opening-edge` and
   `sill`, queried with the existing distance and frame operations.
4. **Live evaluation for items.** Extend `Session::shade_project` to `kind:"item"`
   so a dimension drag updates through the per-pixel path and bakes only on
   release.
5. **Later, not in this phase:** separating intrinsic appearance from receiver
   mapping so a resize can reuse bakes
   ([extraction reuse](../performance/extraction-reuse.md)).

Checks: resize `wall-tall` in the washroom and compare the overlapping region
before and after; a lengthened pipe keeps its existing pattern; rust follows a
moved `joint` feature; native and Wasm bakes agree.

### 1B. Cutout, ramps and decal appearance

1. **`alphaCutout(coverage, threshold)`** as specified in
   [material-language.md](../material-language.md). It sets `alpha_mode: MASK`
   and `alpha_cutoff`; add a recipe-level `doubleSided` declaration.
2. **`authoredMask(source, mapping)`** from an image or a mesh attribute.
3. **`ramp(stops, t)`**: a colour ramp with linear or constant interpolation,
   evaluated in linear space, with a declared display-referred variant for
   painted materials.
4. **Decal appearance.** A recipe kind that outputs colour, opacity and a
   per-channel blend rule, sized in meters and baked independently of any
   receiver. Placement stays with ItemSpec and WorldSpec
   ([decal contract](spec-modularization.md#decal-definitions-and-placement)).

Checks: a torn-sticker decal on the tiled wall exports as MASK with the declared
cutoff; ramp output matches an independently computed table; failing cases cover
a missing mask source and an out-of-range cutoff.

### 1C. Delivered live programs and the painted shading model

1. **Shading model.** Add `shading: lit | painted` to `Material`. `painted`
   bypasses the light loop and tone mapping so authored colours reach the screen
   unchanged. GLB keeps the baked PBR fallback and records the model in
   `extras.materialSpec`.
2. **Live program contract.** Promote the generated fragment program from Studio
   preview to a delivered payload for materials that declare live inputs. The
   packet and GLB extras carry the program, its uniforms and its required
   inputs; bakes stay the authority for the fallback.
3. **Scene lighting input.** Carry sun direction, colour and intensity in the
   scene manifest and replace the renderer's hard-coded sun. Programs read
   `sunDirection` at the live stage only; a bake that needs it is a diagnostic.
4. **`shadeBand(normal, sun, threshold, softness, range)`** plus a world-anchored
   edge-break term, matching the reference's shade factor. Cast shadows fold into
   the same factor, building on `browser/renderer/shadow-style.ts`.

Checks: a painted material on the washroom slab moves its shade boundary with the
sun and triggers no bake; CPU and generated-GLSL evaluation agree on a sample
grid; an importer without the payload shows the baked fallback and the export
report names the skipped effects.

### 1D. Instance, vertex and scene-field inputs

1. **Instance inputs.** `instance.root` (world position of the instance origin),
   `instance.tint` and `instance.random`. Render-only values stay out of bake
   keys.
2. **Vertex inputs.** Named scalar attributes such as `heightRatio`, declared by
   the mesh producer.
3. **Scene fields.** `sampleField(name, worldXY)` reads a named top-down field,
   first `ground.color` and `ground.normal`, produced from the ground receiver's
   own material. This is what lets a blade match the ground under its root.
4. **Instance-aware world position** in `browser/renderer/shade.ts`, which
   ignores `instanceMatrix` today.

Vertex deformation and time are deliberately not MaterialSpec features. Wind and
bend belong to TreeSpec's fixed shader families in phase 4.

Checks: with default bindings, a program using instance inputs bakes to the same
result as before; a renderer unit test binds each input; a missing field reports
a source-linked diagnostic.

## Phase 2: Shared delivery

- **Instanced batches** in the model, packet and incremental updates: one mesh,
  transforms and typed per-instance attributes, kept unexpanded. GLB uses
  `EXT_mesh_gpu_instancing`, with expansion as the fallback.
- **Renderer:** `InstancedMesh` branch in `browser/renderer/scene.ts`, a
  time-driven render mode beside on-demand rendering, and draw-call and triangle
  counters next to the existing timings.
- **Scene fields** delivered as textures.

Used by ItemSpec for bolts and brackets and by TreeSpec for blades and cards.

## Phase 3: ItemSpec MVP

Detailed in [item-spec-mvp.md](item-spec-mvp.md). Its geometry steps do not
depend on phase 1 and can run alongside it.

1. **Multi-part items:** a `parts` list, each part with a generator, transform
   and slot, plus derived parameters.
2. **Pipes:** a route generator (points, bend radius, round or rectangular
   section) with fittings at stations: elbow, tee, coupling, bolted flange,
   valve, bracket. Ported, not imported, from the Kowloon theme.
3. **Walls:** panel, opening (rectangle, arch), frame, glazing and reveal on
   `superspec-geometry::Solid`.
4. **Slots per part and face group:** pipe body, fittings, brackets; wall face,
   reveal, frame, glass.
5. **Named features** emitted for phase 1A.
6. **Decal placement:** anchored to a feature with a rule (absolute offset or
   fraction). On resize a decal keeps its size and re-anchors; if it no longer
   fits or lands in an opening it raises a diagnostic. First rendering is offset
   meshes, flat on walls and wrapped on pipes.
7. **Editor and scenes:** sliders and drag handles, and items accepted in scene
   projects.

Done when the washroom is rebuilt from items with a windowed wall and a pipe run
with a tee, valve, flanges and brackets, and: widening the wall keeps grout,
window and decal anchored; lengthening the pipe re-spaces brackets; rust follows
joints; no pattern shifts.

## Phase 4: TreeSpec, painted grass

A generic engine module beside `materialspec` and `itemspec`. It must not import
`worldspec`; add it to the checked modules in `tools/architecture`.

- **Structure:** species recipes produce blade profiles, later branch skeletons,
  canopy hulls and card templates, each with a level-of-detail chain and
  per-vertex wind data.
- **Distribution:** density fields and placement rules produce instance batches
  through a generic surface sampler. WorldSpec supplies terrain and decides
  where plants go.
- **Look:** two profiles per plant over the same geometry and instances.
- **Renderer:** a small fixed set of foliage shader families (bend, wind,
  rejection) with parameters, not arbitrary shader code.

Optimizations to build in from the start:

1. Reject blades far from the shade boundary in the vertex shader, over a stable
   candidate set, so a sun change needs no re-scatter.
2. Precompute a boundary-distance field when sun or terrain changes.
3. Evaluate colour per blade in the vertex stage; only the tip highlight stays
   per pixel.
4. Tile the field, cull per tile, and thin by a stable per-blade threshold while
   widening survivors.
5. Draw no blades in the far field; step segments from 12 to 3 to 1 before that.
6. Add a grazing-angle term to the rejection test for silhouettes.
7. Instance clumps of 5 to 10 blades with per-vertex root offsets.

Done when painted grass on a terrain patch matches the reference views, holds a
recorded frame-time budget, and the same field renders with the lit profile.

## Phase 5: Shrubs and trees

Branch skeletons, metaball canopy hulls, camera-facing paint dabs and highlight
leaves with cutout alpha, level-of-detail steps and a shadow proxy. The lit
profile adds two-sided diffuse, glints and translucency. An optional
half-resolution stroke layer covers the Blender study's blurred blades.

### Phases 4 and 5: geometry prototype (2026-11-16)

Status: prototype, geometry only. `crates/engine/src/treespec` generates grass
patches, shrubs and trees; its [README](../../crates/engine/src/treespec/README.md)
describes the records and every attribute. Nothing here is rendered with painted
materials yet.

Assumptions made without review:

- Open decision 4 is taken as "Rust API with JSON fixtures first". Definitions are
  `*.grass.json` and `*.tree.json`, shaped like item definitions.
- The instanced batch is a plain struct in the module (mesh index, transforms, named
  per-instance scalars and vectors); nothing converts it to the portable record of
  phase 2 yet. That record's four `values` and a tint per instance do not hold the
  per-instance `shadeNormal`, `groundNormal` and `pivot` vectors a plant needs.
- Grass is placed by looking straight down at a ground triangle mesh, so density is per
  square meter of plan and overhangs are not supported, as in the reference tools.
- Colour is not TreeSpec's: an instance carries a `variation` value for a ramp, not a
  tint. `instance.tint` is left to the look profile.
- Wind data is a per-vertex weight and a pivot, not a shader. The fixed shader families
  stay as listed above.
- `base` is the only named feature a plant reports.

What exists, against the lists above:

- Phase 4 structure and distribution: blade clumps of 5 to 10 blades with per-vertex
  root offsets (optimization 7), a stable candidate set with a per-instance thinning
  threshold (1 and 4), square tiles with bounds (4), and segment steps 12, 3 and 1 with
  widened survivors (4 and 5). Not started: the boundary-distance field (2), per-blade
  colour evaluation (3), the grazing-angle term (6), both look profiles and the
  renderer's shader families.
- Phase 5: branch skeletons with a pruning envelope, bark tubes with metric coordinates,
  leaf clusters as one instance batch with canopy shade normals, three fixtures (shrub,
  broadleaf, conifer) and three levels of detail each. Branch junctions overlap; they are
  not welded. Not started: metaball canopy hulls (only the per-limb balls are reported),
  camera-facing dabs, the shadow proxy, the lit profile and the stroke layer. The far
  level of a tree is thinned cards, which reads poorly up close; a hull or impostor
  should replace it.

Needed next from other stages: the portable instanced batch and `InstancedMesh` branch
(phase 2), named vertex and instance inputs and `sampleField` (1D), the painted shading
model and `shadeBand` (1C), and the wind and rejection vertex stages of phase 4's
renderer item.

## Open decisions

1. Whether live programs travel in the packet for every scene or only for
   materials that declare live inputs (this plan assumes the latter).
2. Scene-field resolution and who owns the top-down bake.
3. Whether `painted` output is display-referred end to end or converted at the
   output stage.
4. Whether TreeSpec gets its own authoring language and editor in phase 4 or
   starts as a Rust API with fixtures.
