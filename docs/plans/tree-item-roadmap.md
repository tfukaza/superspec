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

### 1C and 1D: prototype (roadmap stage 5, 2026-11-16)

Status: prototype, implemented and checked; unreviewed. The language surface is in the
[program README](../../crates/engine/src/materialspec/program/README.md) ("Render-only inputs" and
"Painted shading, shade bands and delivered programs").

What exists:

- **1C.1** `defineMaterial({ shading: "painted", paint })`. The renderer (`browser/renderer/shade.ts`)
  replaces the light loop's result with the paint (`MS_PAINTED`); the Studio never swaps a painted mesh
  to GPU-baked maps. `Material::shading` and `Material::program` (`superspec-types`) carry it in the
  packet; a GLB keeps the baked PBR maps and records `shading` and `program` in `extras.materialSpec`.
- **1C.2** The delivered program: GLSL, painted expression, mapping, uniform values, required inputs and
  the render inputs to bind (`authoring::material::live_program`). The viewer draws it for a packet it
  has no shading for (`RustScene.deliveredLook`). `superspec_export::live_report`, printed by
  `superspec-studio glb` as `export:` lines, names the skipped effects.
- **1C.3** `sunDirection` reads the lighting record's sun: `SceneLighting.set` keeps the shared uniform
  `programSun`, so a sun edit redraws painted materials with no rebuild and no bake. A bake that reads
  it fails with `MS_INPUT` at the read.
- **1C.4** `shadeBand` and the `edgeBreak` group; the cast shadow is folded in by the minimum through
  `CAST_SHADOW` in `shadow-style.ts` (the styled term, or three.js's plain one).
- **1D.1-4** `instanceRoot`, `instanceTint`, `instanceRandom`, `placedWorld`, named attributes with a
  fallback where a mesh has none, `sampleField` with `ground.color` and `ground.normal` made from the
  ground receiver (`browser/renderer/scene-fields.ts`, `GroundFields`), and instance-aware positions
  in `shade.ts`.

Checks (2026-11-16, RTX 5060 Ti through ANGLE D3D11 unless said):

- `PAINTED=1 node tests/material-washroom-browser.mjs` (256 px, `?ao=off`): the slab is painted, not
  tone mapped, and its pixels in the exact lit paint fall from 70,581 to 15,345 to 8,872 as the sun goes
  from 70 to 33 to 15 degrees while those in the shade paint rise from 20,633 to 28,733 to 85,854 (a
  boundary crosses the floor at 33); three sun moves with full read-backs took 54 ms, and the input and
  draft revisions and the scene bake were unchanged (no rebuild, no bake).
- `tests/live-program-browser.mjs`: the generated GLSL of the painted recipe and of a program reading
  every other render input against the CPU on 48 samples, largest difference 8.3e-8 and 1.6e-6 (the
  same on SwiftShader); the viewer's program material binds root, tint, random, placed position, a
  vertex attribute, its fallback, the field and the sun; the ground fields of a plane read back its
  color and normal.
- `crates/engine/tests/material_live.rs` (8 tests) and `crates/export/tests/live.rs` (2).

Assumptions made without review:

- **Declared live inputs means `shading: "painted"`.** Only painted materials are delivered with their
  program (open decision 1 taken as "declared"). A lit material whose baked channel reads a render-only
  input still fails its bake.
- **Paint is a declared field, not a new output.** The bake's channels stay the PBR fallback and must
  not read the sun; `paint` is evaluated only where the material is drawn. Without `paint` a painted
  material shows its base color unlit. TypeScript recipes only; `.ms` has no painted declaration yet.
- **Painted output is display-referred by the lighting record's `painted.toneMapping`** (open decision
  3): by default exposure and the tone curve are skipped, so the paint reaches the screen as authored;
  the record can turn them on for the whole scene. Ambient occlusion applies to painted pixels (the GTAO
  pass covers the finished image); `painted.ambientOcclusion: false` is reported as not honored.
- **The cast shadow is folded in, the shadow tint is not.** Painted and lit shadows share their shape
  (softness, spread, edge noise, the same shadow map), but the style's tint and interior noise act only
  on lit materials; a painted material's shade color is its own.
- **`shadeBand` softness is in screen pixels** (as the reference's): it widens the band by
  `softness * fwidth`; the CPU, which has no pixels, reads it as zero. CPU and GLSL agree wherever the
  derivative term is below `range`.
- **Instance inputs are named `instanceRoot`, `instanceTint`, `instanceRandom`** rather than
  `instance.root`: `instance` is already the drawn instance's value vector. `instanceRandom` without a
  `random` attribute is an integer hash of the instance index, so it is stable but not authored.
- **Default bindings are explicit, not what a bake reads.** `RenderBindings::default()` gives the
  defaults (origin, white, 0, the record's default sun, typed attribute values); the authoring session
  binds nothing, so render-only values never reach a bake key.
- **Scene fields:** `ground.color` and `ground.normal` only, rendered on the GPU from straight above
  at 256 texels on the longer side, from the mesh `?ground=` names or the widest flat mesh. The color is
  the ground's base color (or color map), unlit and without a painted ground's shade band, which a blade
  applies itself from `ground.normal` and the sun; the normal is the geometry's, without a normal map's
  relief. They are made when a scene with programs that read them is
  shown, not delivered in the packet or GLB yet (open decision 2: the viewer owns the bake for now).
- **Delivered programs carry no images, features, edges or planes**; a painted program that reads them
  is delivered with its maps only. A delivered program's `object` coordinates are measured from the
  mesh's bounds in the viewer, which may differ from the bake's object frame.

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
Prototype (stage 7): [below](#phase-5-shrubs-and-trees-prototype-roadmap-stage-7-2026-11-16).

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
  should replace it. (All but the stroke layer done since: phase 5 prototype below.)

Needed next from other stages: the portable instanced batch and `InstancedMesh` branch
(phase 2), named vertex and instance inputs and `sampleField` (1D), the painted shading
model and `shadeBand` (1C), and the wind and rejection vertex stages of phase 4's
renderer item. (All done since; phase 4's renderer is the prototype below.)

### Phase 4: painted grass prototype (roadmap stage 6, 2026-11-16)

Status: prototype, implemented and checked; unreviewed.

What exists:

- **Foliage look on a batch.** `superspec_types::batch::Foliage` (`foliage` on a project batch, the model's
  `InstanceBatch` and `Batch`, the packet's `SceneBatch.foliage`, and the instanced node's extras in a GLB): a family
  (`grass`, the only one), the patch it belongs to, its level of detail with the camera `distance` it starts at and the
  share `keep` of instances it holds, the patch's `far`, the profile (`painted` or `lit`), the project material that
  colors it, and the parameters of each family (`bend`, `wind`, `rejection`, `tip`). Unknown families and fields are
  refused (`deny_unknown_fields`), values are range-checked when the batch meets its scene, and a batch without a look
  serializes as before. TreeSpec's `batches::grass` now gives each instance its `tile`, and `batches::grass_levels` emits
  every level with its look.
- **Renderer** (`browser/renderer/foliage.ts`, wired in `scene.ts`): the fixed families. *Bend* is a static lean about
  each blade's root toward a stable heading; *wind* a rigid lean about the root by a gust field travelling over world XY
  with `programTime` (the time-driven render mode) plus a per-blade flutter, after the reference's illustration grass;
  *rejection* drops whole blades in the vertex stage (collapsed outside the clip volume). Painted profile: an unlit
  `MeshBasicMaterial` that follows the scene's painted finishing; lit: `MeshStandardMaterial` with the shadow style.
- **Per-blade color in the vertex stage.** The look's recipe (here `painted-grass.material.ts`) is the same generated
  GLSL as any program, included in the vertex shader with screen derivatives defined as zero, evaluated once per vertex
  with every input at the blade's root (`world` and `placedWorld` the root, `normal` the ground normal there,
  `instanceRoot` the clump origin) and only for blades that survive rejection. The sun's cast shadow at the root is read
  from the shadow map in the vertex stage and folded into `shadeBand`, whose result the families keep (`msFoliageBand`)
  for the per-pixel tip highlight, which is the only per-pixel term. Lit: the recipe's base color as albedo.
- **Optimizations of the list above.** (1) Rejection over the stable candidate set: a blade is kept by its stable
  `random` against a share that falls from 1 at the shade boundary to 0 at `rejection.boundary` meters; moving the sun
  changes no instance data. (2) The boundary-distance field (`FoliageBoundary`): the recipe evaluated at the ground point
  under each texel of the ground fields' rectangle (a world-position target added to `GroundFields`), its band's 0.5
  contour seeded and jump-flooded into meters; remade only when the sun, its shadow map, the ground or the recipe's
  values change. (3) Per-blade color as above. (4) Per-tile culling and levels: tiles are the instances' `tile` runs;
  each frame tiles are frustum-culled and given the level their distance has reached, and the instances of the tiles a
  level draws are packed to the front of its buffers only when that choice changes (a level is one draw call per pass);
  within a level instances thin by `threshold` toward the next level's `keep`, the last toward 0 at `far`. (5) Nothing
  past `far`; levels step 12, 3 and 1 segments. (6) The grazing term keeps a share where the view runs along the ground.
  (7) Clumps with per-vertex roots were already there.
- **Terrain patch.** The washroom had no ground outside its slab, so the test adds one: `buildProject(..., { grass })`
  adds a lawn (an L of rolling ground around the room's open sides) and the meadow grown on it, from one TreeSpec
  document (`write_washroom_lawn`). Without it the washroom GLB and project are byte-identical (checked against `HEAD`).

Checks (2026-11-16, RTX 5060 Ti through ANGLE D3D11, 1440 by 1000 page, 256 px textures, ambient occlusion off unless
said; `tests/grass-washroom-browser.mjs`, report in `generated/verification/grass/washroom-grass.json`):

- The lawn: 5,964 candidates, 3,349 clumps, 24,262 blades at level 0, 16 tiles, 12 batches (4 variants at 3 levels),
  generated natively in 2 ms. The Wasm build delivers every clump at level 0 and each level's share; the native CLI's GLB
  carries the 12 looks in its instancing nodes' extras.
- Steady frame, GPU time (timer queries), median with the grass and without it: framed (the Studio's own framing,
  levels 1 and 2) painted 0.12 / 0.12 ms, lit 0.13 / 0.14 ms; close (levels 0 and 1, 2,023 instances, 14,671 candidate
  blades) painted 0.58 to 0.71 / 0.32 to 0.39 ms with 2,224 blades kept, lit 0.81 to 0.83 / 0.33 to 0.34 ms with 9,514
  (two runs); low (15 of 16 tiles, 18,212
  candidates) painted 0.53 / 0.18 ms with 4,408 kept (2,007 by the grazing term). With wind in the time-driven mode the
  GPU time is the same within noise (0.55 and 0.79 ms close). The grass adds 8 draw calls (21 against 13: 4 variants at
  the 2 levels in view). With ambient occlusion on, close: painted 2.47 / 2.29 ms, lit 2.66 / 2.21 ms (noisier).
- Cold compile against steady: a profile's programs compiled the first time take 90 to 109 ms (painted) and 293 to
  337 ms (lit) in a fresh browser; the page then draws a steady frame in the times above. Page time per frame is 0.3 to
  0.6 ms.
- Moving the sun (54, 62, 47, 33 degrees): 1.2 to 2.9 ms of page time for `setLighting` and two frames, of which the
  boundary field is 0.06 to 0.27 ms of GPU time (made twice per move: once for the new sun, once more after the
  frame that remakes the shadow map); no instance buffer was rewritten (no re-scatter, no re-pack), and the input and
  draft revisions and the scene bake were unchanged. The blades kept follow the boundary (0, 448, 0 and 632 at the
  framed view: a high sun lights every slope of the lawn and a low one shades it).
- Frame-time budget for this scene on this GPU: grass at most 0.5 ms (painted) and 0.6 ms (lit) of GPU time added
  to a frame at the close view, at most 12 added draw calls, a sun move at most 0.3 ms of GPU time and no re-scatter,
  and at most 400 ms to compile a profile cold.
- Pictures: `washroom-grass-{painted,lit}-{framed,close}.png` and `washroom-grass-painted-low.png` (and `-ao` ones).
- `superspec-types` (foliage ranges), `superspec-model` (a look is read with its defaults, delivered beside the batch
  and refused when unknown or out of range), `treespec_batches` (tiles and levels), `tests/browser/rust-batches.test.ts`
  (decoding and refusing looks); `npm run check:architecture`; `bench_fingerprint` identities unchanged
  (`ce6e3a55...`, `b6b04aec...`); `PAINTED=1 node tests/material-washroom-browser.mjs` (the painted slab's pixel counts
  as recorded in stage 5), `tests/live-program-browser.mjs` and `tests/treespec-batches-browser.mjs` pass.

Assumptions made without review:

- **The look travels on the batch, its recipe by project material id.** `Foliage.material` names a project material (one
  assigned to no mesh is allowed and bakes nothing), resolved in the Studio from the project's shading; a viewer of a
  delivered packet alone has no program for it and draws the families with a neutral green. Delivering the recipe's
  program for foliage (like a painted material's `program`) is not built. The GLB carries the look in the instancing
  node's extras only, not on expanded instances.
- **Blades take the ground's paint and band at their roots, the reference's design, by convention.** The grass recipe
  reads `ground.color` and `ground.normal` at `placedWorld` and repeats the lawn's band parameters; nothing checks that
  the two recipes agree. The vertex stage has no screen derivatives, so a band's softness in pixels is dropped there.
- **Cast shadows at the root come from three.js's shadow map with nine plain taps**, not the scene's styled shadow
  (which needs screen derivatives); edges of cast shadows on the ground and on blades differ by the style's softness and
  edge noise. Foliage casts no shadow (its roots would shade themselves) and is left out of ambient occlusion's depth and
  normals (its families do not run there); occlusion still multiplies the finished image.
- **The boundary is the 0.5 contour of the recipe's band** at the ground fields' resolution (256 texels on the longer
  side, 2.7 cm here), made from the ground the scene fields use (`findGround` or `?ground=`); one boundary per scene,
  from the first patch's recipe. A recipe without `shadeBand` has only cast-shadow boundaries.
- **Rejection parameters are the look's, defaults after the Blender study**: 22 cm from the boundary, grazing within
  15 degrees of edge-on keeping half, no fill. The painted profile keeps 10 to 25 percent of candidates in these views.
- **Levels:** a tile draws one level, chosen by the nearest point of its bounds; the meadow's suggested distances were
  shortened for the washroom (7 and 14 m, `far` 22 m). Thinning by `threshold` inside a level makes the change gradual,
  but a tile still switches meshes at once (no cross-fade). Survivors are widened per level (TreeSpec), not continuously.
- **Profiles:** the profile is the batch's, or the page's `?foliage=lit|painted` (`RustScene.setFoliageProfile` in
  tests). The lit profile lights blades by the clump's shade normals; the lawn under it stays painted.
- **Measurement:** blades kept are counted on the CPU by repeating the vertex stage's tests against the boundary field
  read back (`RustScene.foliageState`); the GPU reports nothing about what it rejected.

### Phase 5: shrubs and trees prototype (roadmap stage 7, 2026-11-16)

Status: prototype, implemented and checked; unreviewed. The half-resolution stroke layer is not built.

What exists:

- **Metaball canopy hulls** (`treespec/hull.rs`). A field of Wyvill kernels, one per limb ball plus a core ball at the
  balls' volume-weighted centre, whose level 1/8 is the surface; hull vertices are found by marching inward along the
  directions of a divided octahedron from that centre (a shrink wrap), normals are the field's gradient and `bendWeight`
  follows the limbs nearby. Used three ways: the leaves' canopy shade normals (the hull normal where the ray from the
  centre through a cluster leaves the hull, 75%, tipped by the local gradient, 25%); each level's `hull`
  (`TreeLodDefinition::hull`: `none`, `backing`, a shell at 0.86 of the hull behind the leaves that fills the gaps of
  thinned leaves, or `impostor`, the hull in place of the leaves); and the shadow proxy (`GeneratedTree::shadow`, the hull
  at 0.78). The fixtures step backing, backing, impostor, so the far level is the hull, not thinned cards.
- **Plant families** (`superspec_types::batch::FoliageFamily` `bark`, `leaves`, `hull`, `shadow`, with the optional
  `FoliageCanopy` parameters; `treespec::batches::tree_levels` gives every level of a plant as batches of them;
  `browser/renderer/foliage-plants.ts` draws them). Leaves are camera-facing paint dabs: each cluster turns toward the
  camera about its centre (`facing`), is scaled (`dab`) and rolled by its random value, and is cut out where a
  brush-edged coverage falls below `cutout`; its paint is the recipe evaluated once per cluster in the vertex stage at
  its centre with the canopy shade normal, the cast shadow folded into the band, and a share of the clusters on the lit
  side of the band are highlight leaves. Bark and hull evaluate the recipe per pixel; the hull's silhouette is broken
  into dabs by a world-anchored noise. Wind: the whole plant leans about its base by a gust there, each point by its
  `bendWeight` (so clusters move with their branches and joints do not tear), and cards flutter about their cluster.
- **Levels and shadow proxy.** A plant is one patch with one tile: it draws the level its distance has reached, each
  family thinning toward the next level of the same family (leaves switch to the impostor at once). The shadow proxy is
  drawn only by the sun's shadow camera (layer 7) at every level; bark casts too; leaves and hulls cast nothing. The
  style of shadows stays global (`shadow-style.ts`); plants read the same shadow map.
- **Profiles.** Painted: unlit, the recipe's paint with `shadeBand` on the canopy shade normals as the grass and lawn do
  on theirs. Lit: the light loop with the shade normal tipped toward the card, plus two-sided diffuse (`wrap`), light
  through the leaves from behind (`translucency`) and glossy clusters (`glint`).
- **Washroom.** `buildProject(..., { trees: true })` (`TREES=1`) adds the shrub fixture and the broadleaf fixture at 0.6
  of its size on the lawn (with the lawn and grass), from `washroom-trees.json` (`write_washroom_trees`), colored by
  `painted-bark` and `painted-canopy`. Without it the washroom GLB and project are byte-identical to `HEAD` (checked with
  and without `GRASS` and `PAINTED`), and the GLB with trees is the GLB with grass.

Checks (2026-11-16, RTX 5060 Ti through ANGLE D3D11, 1440 by 1000 page, 256 px textures, ambient occlusion off;
`tests/tree-washroom-browser.mjs`, report and pictures in `generated/verification/trees/`; ranges over the last three
runs):

- Plants: shrub 155 branches, 608 leaf clusters, 30 limb balls, levels 14,622 / 3,292 / 557 triangles, generated in
  3.5 to 3.9 ms; broadleaf at 0.6: 433 branches, 1,698 clusters, 12 balls, 61,746 / 10,798 / 780 triangles, 6.8 to
  7.5 ms. 18 batches, a 13.2 MB document. The Wasm build delivered every plant, level and family with the native
  document's instances.
- Steady frame, GPU time median with the plants / without them: framed (the Studio's framing: the tree at level 1, the
  shrub an impostor) painted 0.11 / 0.10 to 0.11 ms, lit 0.12 to 0.13 / 0.11 to 0.14 ms (within noise), 22 / 17 draw
  calls; close (both at level 0: 2,306 leaf clusters, about 111,000 plant triangles) painted 1.12 to 1.39 / 0.47 to
  0.53 ms, lit 1.24 to 1.36 / 0.46 to 0.50 ms, 27 / 21 draw calls; far (both impostors) painted 0.07 to 0.09 / 0.07 ms,
  lit 0.07 to 0.09 / 0.06 to 0.07 ms, 17 / 13 draw calls. The steady frames do not redraw the shadow map, so the
  proxies and bark add shadow-pass calls only when the light or scene changes. Wind in the time-driven mode: the same
  GPU time within noise.
- Cold compile against steady: switching every foliage batch (grass and plants) to a profile not drawn yet compiled in
  100 to 113 ms (painted) and 150 to 168 ms (lit); loading the project to a baked scene took 3.3 to 3.6 s.
- Moving the sun (62, 47, 33, 20 degrees) at the close view: 1.2 to 2.2 ms of page time for `setLighting`, the frame that
  redraws the shadow map with the proxies, a second frame and `gl.finish`; no instance or attribute buffer of a plant
  was rewritten, the input and draft revisions and the scene bake were unchanged, and each picture differs
  (`washroom-trees-painted-sun-*.png`: the canopy's band follows the sun).
- Frame-time budget for this scene on this GPU: plants at most 1.0 ms (painted and lit) of GPU time added at the close
  view and 0.05 ms at the far view, at most 8 added draw calls in a steady frame (plus 2 per plant in a shadow pass), a
  sun move at most 3 ms of page time with no re-scatter or rebake, and at most 250 ms to compile a profile of all the
  foliage cold.
- `treespec_tree` (11 tests, a new one for hulls: closed and consistently wound, normals outward, positive volume,
  the impostor holding at least 85% of cluster centres by ray crossings, shells and proxy smaller than the impostor,
  shade normals the field's), `treespec_batches` (a plant's batches per part and level, admitted and round-tripped by
  the portable record), `superspec-types` (canopy ranges), `npm run check:architecture`, `bench_fingerprint` identities
  unchanged (`ce6e3a55...`, `b6b04aec...`), `tests/grass-washroom-browser.mjs` (the stage 6 numbers: 2,224 blades kept
  at the close view, 21 draw calls), `tsc --noEmit`.

Assumptions made without review:

- **Hull shape.** One ball per limb (level-1 branch or trunk), its surface at 1.25 times its clusters' spread plus half a
  leaf cluster; a shrink wrap from one centre fills pockets the centre cannot see. Divisions 6 (shell, 288 triangles),
  8 (impostor, 512) and 5 (proxy, 200); shell at 0.86 and proxy at 0.78 of the hull, the proxy inside the shell so the
  shell and the sunny leaves are not shaded by it. All constants, not definition fields.
- **Shade normals changed.** Leaves' `shadeNormal` now come from the hull (they were 70% the canopy ellipsoid's outward
  direction and 30% the limb's), so the plant fixtures' digests changed; the new digests have not been reproduced on a
  wasm32 build (the hull uses only basic operations and `sqrt`). An impostor level has `leaf_instances` 0.
- **Families and parameters.** Plant parts are new `FoliageFamily` values beside `grass`, with one optional
  `canopy` record (refused on grass); defaults facing 0.8, dab 1.25, cutout 0.4, highlights 0.18, translucency 0.45,
  glint 0.2, wrap 0.5. The washroom's look uses wind angle 2.5 degrees and flutter 9 degrees.
- **Cutout is the family's.** A dab's coverage is a fixed brush-edged blob over the card's coordinates compared with
  `canopy.cutout`; the recipe's own `alphaCutout` is not read (that would evaluate the recipe per pixel of every dab).
- **Where paint is evaluated.** Leaves once per cluster (a flat dab); bark and hull per pixel. The cast shadow a plant
  reads is taken 12 cm off its surface along the shade normal, against stripes where the proxy nearly meets the shell
  at a low sun.
- **Wind is rigid per plant.** A lean of the whole plant about its bark's placement by `bendWeight`, the gust sampled at
  the base; no per-branch oscillation. Shadows do not sway (the shadow pass draws bark and proxy without the families).
- **Lit additions outside the light loop.** Wrap and translucency are added to the direct diffuse with the sun's
  irradiance and the families' nine-tap cast shadow; glints lower the roughness of a share of the clusters; lit leaves
  and bark also receive three.js's shadow map.
- **Levels.** A plant's level is chosen by the distance to the bounds of all its parts; leaves switch to the impostor
  without a cross-fade; a plant whose bounds leave the view draws nothing, its proxy included, so its shadow goes too.
- **Washroom plants.** Trees imply the lawn and grass. The broadleaf is scaled to 0.6 in the test (every length and the
  envelope); level distances shortened to 8 and 16 m (shrub) and 10 and 22 m (tree), `far` 60 m; bases 2 cm below the
  lawn. The bark and canopy recipes' colors and band parameters were chosen by eye, not matched to the reference.
- **Measurement.** As stage 6: timer queries per frame, the plants' meshes hidden for the frame without them, ambient
  occlusion off; the GPU time of the sun move's shadow-map frame was not isolated. The native CLI's GLB extras for plant
  looks were not inspected (the exporter writes the `Foliage` record as it writes grass's).

### Exploring: painted masses with a leaf normal map

A second approach to the painted canopy, after the leaf-card mass model (kept at checkpoint `63373e7`): a plant's form
is a rough base mesh shaded through a stroke-painted leaf normal map, with leafy billboards along its silhouette.

- **Leaf normal map** (`browser/renderer/leaf-normals.ts`): a tile painted as a digital painter paints with a leaf-clump
  brush: strokes start on a jittered grid and stamp small clumps of leaves (4 to 7, or 16 smaller) at even spacing; each
  leaf one flat normal near its clump's. Laid in a camera-facing plane anchored at the plant, so leaves are never
  foreshortened; two copies blended by coverage hide the repeat. The base mesh's face normals bulge toward the mean of
  the adjacent faces at each edge (`domeGeometry`), and the result is banded against the sun.
- **Contour fuzz** (`browser/renderer/contour-fuzz.ts`): anchors fixed along every mesh edge show a camera-facing leaf
  card while the edge is on the silhouette, straddling it; the cards read the same leaf map at their world position, so
  the leaves run on from the surface into the outline.
- **Prototype candidate:** the sprig shrub with its 12 mass-model clumps (`fixtures/sprig-shrub-clumps.tree.json`,
  clumps only at 0.92 of their size) as the base mesh, the 16-leaf brush map at a 0.6 m tile, fuzz cards 0.3 m at 14 per
  meter of edge, bark kept; the leaf normals tipped at strength 1.5 and the bands offset by -0.05 so the lit side spreads
  over the upper bands. Rebuilt and pictured by `tests/painted-mass-shrub-browser.mjs`; reference renders in
  `painted-mass/`. Known gaps: ambient occlusion's pass does not draw these shaders (off for now); about 9,600 fuzz
  anchors, most never on the outline (a coarser mesh for the fuzz would cut them); the sprigs' leaves are hidden.
- **Second candidate:** the same shrub with its one canopy hull (the limbs' shrink-wrap, 512 triangles) as the base
  instead of its clumps (`fixtures/sprig-shrub-hull.tree.json`, the hull drawn whole at the near level), all else as the
  first except its leaf normals at strength 1 and bands offset 0.1 (`STRENGTH=1 OFFSET=0.1`: the clump candidate's
  stronger values darken this one); about 1,100 fuzz anchors. Same script; reference renders
  `painted-mass/hull-candidate-*.png`.
- **Performance fallback, if needed:** bake the plant's normals (not colours, so sun and palette stay live) from every
  angle around it, as a 360-degree strip, and slide it to match the view on one camera-facing card (an impostor). Exact
  only for round plants; an octahedral grid of baked views extends it to any shape and viewing height. Best as the far
  level of detail, since a flat card loses depth and parallax up close.

## Open decisions

1. Whether live programs travel in the packet for every scene or only for
   materials that declare live inputs (this plan assumes the latter).
2. Scene-field resolution and who owns the top-down bake.
3. Whether `painted` output is display-referred end to end or converted at the
   output stage.
4. Whether TreeSpec gets its own authoring language and editor in phase 4 or
   starts as a Rust API with fixtures.
