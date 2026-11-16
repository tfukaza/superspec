# SuperSpec roadmap: performance, lighting, materials, items and foliage

This is the single ordering for three plans that were written separately and overlap:

| Plan | Detail lives in | Covers |
| --- | --- | --- |
| Performance and shadows | [performance-and-shadows.md](performance-and-shadows.md) | Correctness fixes, scene-view and bake speed, shadow and AO rendering |
| Lighting settings | [lighting-settings.md](lighting-settings.md) (draft spec) | One shared record for global and per-light settings |
| Materials, items, foliage | [tree-item-roadmap.md](tree-item-roadmap.md), [item-spec-mvp.md](item-spec-mvp.md) | MaterialSpec extensions, the ItemSpec MVP, TreeSpec |

Those documents keep their detail and their own checks. This one fixes the order,
resolves where they disagree, and adds weathering layers. Where this document and a
detailed plan differ, this one wins and the detailed plan is updated when its stage
starts. [next-improvements.md](next-improvements.md) remains the unordered backlog.
Scene-level evidence uses the washroom (`tests/studio/washroom`).

## Standing decisions

- The CPU compiler is the authority for production and export. Its optimizations do
  not change baked pixels or native/Wasm parity. A one-time encoded-byte change for
  faster PNG/KTX2 is authorized, with a version bump.
- GPU output is a preview. The scene view defaults to GPU-baked maps; the per-pixel
  shader is the first look and the path used while a dimension or parameter is
  dragged.
- Shadow style and ambient occlusion are global or per-light rendering settings,
  never part of a material. They follow the shared lighting record.
- A material holds no geometry assumptions. Where an effect goes is supplied by the
  geometry's owner as named features and context; what it looks like is a
  MaterialSpec recipe.
- **Grime and other weathering are their own appearances, applied like decals**
  (decided 2026-11-16): a weathering layer is a MaterialSpec recipe, placed and
  parameterized by ItemSpec or WorldSpec, composed over a base material.

## How the plans fit together

1. **One lighting record.** The roadmap's "scene lighting input" (its step 1C.3) is
   the lighting spec's adoption steps 2 and 3. Programs read the sun from that record;
   the hard-coded sun is removed once.
2. **Painted shading and global rendering settings.** `shading: painted` bypasses the
   light loop. Its shade factor (`shadeBand`) reads the same light and the same shadow
   style parameters as lit materials, through `shadow-style.ts`, so cast shadows match
   across both models. Whether ambient occlusion and tone mapping apply to painted
   materials is an open decision of the lighting spec.
3. **Baked maps and live programs.** Games ship baked KTX2 maps by default. A live
   per-pixel program is delivered only for a material that declares live inputs
   (painted, sun-reactive). Those materials make cold shader compile a shipped cost,
   so the compile work in stage 1 is a prerequisite for the roadmap's step 1C.
4. **Three evaluation paths, one rule.** Per-pixel shader while dragging and as the
   first look; GPU bake in UV space when the edit settles; CPU bake for export and
   as the reference. ItemSpec's "drag without a bake, bake on release" uses exactly
   this.
5. **Named features are receiver features.** The features ItemSpec emits (`joint`,
   `sill`, `base`, `top`, `opening-edge`) use the existing receiver-feature machinery.
   The row-wise evaluator currently turns itself off on feature-heavy receivers, so
   that fix (stage 1) precedes feature-driven materials.
6. **Shared intermediate textures.** One mechanism, the pass graph, serves shadow
   and weathering blur, splitting heavy shaders for compile time, live-argument
   passes, scene fields (`ground.color`) and composing weathering layers.
7. **Instancing.** The roadmap's instanced batches and the performance plan's
   material sharing and batching are the same delivery work.

## Weathering layers

A new MaterialSpec recipe kind beside the decal appearance of the roadmap's step 1B.
A decal is a bounded image placed at a position. A weathering layer covers a surface
and is driven by fields.

- **Appearance (MaterialSpec).** A layer outputs contributions to base color,
  roughness, metalness, height and opacity, with a blend rule per channel. It reads
  only abstract inputs: distance to a named feature, a mask, gravity-run coordinates
  (`runoff`), and, later, `occlusion` and `sunExposure`. It never reads receiver
  bounds. Examples: top grime, base stain, moss, rust at joints, runoff streaks.
- **Application (ItemSpec).** A part's slot lists the layers on it, the feature each
  one follows (`top`, `sill`, `base`, `joint`) and its parameters. This is where an
  author turns grime on and tunes it.
- **Context (WorldSpec).** Placement supplies what an item cannot know: ground
  contact, shelter, gravity, neighbours. An elevated item gets no base moss.
- **Composition and reuse.** The base material's intrinsic bake is shared across
  receivers and across a resize. A layer's mapping is per receiver. Composition
  happens in the bake (and its GPU preview). Bake keys: the intrinsic bake excludes
  receiver geometry; the mapping key includes geometry, features and context; a
  layer's appearance key includes neither.
- **Migration.** Split `kowloon-concrete` into an anchored intrinsic base (aggregate,
  pores, panel seams and cutouts on the pattern anchor) plus top-grime, base-stain and
  moss layers. Split rust at joints out of `kowloon-rusted-steel` the same way. The
  washroom must look the same before and after the split at default parameters.

This pulls "separate intrinsic appearance from receiver mapping" forward from the
roadmap's later list, because it is also the largest remaining cut to bake work.

**GPU preview of layered slots (prototype, 2026-11-16).** The scene view composes a
layered slot in the pass graph (`GpuBaker.bakeStack`; the program README's "Layered
slots"): a plane node per channel of the base and of each layer, each layer reading the
planes beneath it, maps composed from the planes that last changed each channel, and the
base's intrinsic outputs read from shared lattice tiles. Measured on the split item
washroom at 512 px on the real GPU (`tests/stage2-gpu-layers-browser.mjs`): see that
test's report under `generated/verification/stage2-gpu-layers/`. Assumptions not yet
reviewed:

- The per-pixel look (first look, edits while they settle, handle drags, `?preview=shader`)
  of a layered slot shows its base alone, and says so (`data-layers`, the status line).
  Showing layers per pixel needs either one linked program per stack or planes the
  per-pixel look can read; neither is built.
- Planes are half floats (heights single precision) and carry no leg points, so a
  layer's relief at a chart's edge and fine colour steps differ from the CPU's exact
  planes. There is no GPU-against-CPU gate; the test reports the share of pixels off by
  more than 16/255.
- Intrinsic tiles are 512 lattice cells square from cell -64, at most eight per
  receiver; a receiver with a chart off its anchor's lattice evaluates its base itself.
- A layer's own CPU fields (the moss's `shelter`) are baked through
  `bake_project_fields` names `stack.<k>/<field>`, with the layer's followed features.
  The occluder service binds planes only for a slot's base program's external inputs
  (`occlude_project`), so the moss's `occlusion` and `sunExposure` read their fallbacks
  in the CPU bake and in the preview alike; whether layers should ask for planes is open.
- Layer parameters reach the GPU bake through uniforms (`setShadingParameters`), as a
  material's own do; the Studio has no control for them yet, so the test edits them by
  opening the project again.

## Stages

Each stage lists what it delivers and its gate. Stages overlap where the work is in
different code; the parallel tracks are noted.

### Stage 0: correctness and measurement (in progress)

Performance plan Phase 0: the row-wise evaluator bug and memory bounds in Rust; the
browser review fixes; corrected GPU timing. Also in flight: per-pixel relief with a
cheap first look, the approved shadow preset as default, ambient occlusion.

Gate: all review findings fixed or explicitly deferred; lanes equal scalar bit for
bit on every example and the washroom; GPU timings re-measured on the real GPU.

### Stage 1: foundations

- **Lighting record** (lighting spec steps 1 to 3): settle the spec, add the records,
  build the viewer's lights, shadow style and AO from it.
- **Scene view** (performance Phase 1): GPU bake in UV space per mesh, cold-compile
  reductions, live-argument passes, viewer jank.
- **Edit loop and CPU** (performance Phase 2): keep the authoring worker alive,
  retention with distribution, lanes on feature-heavy receivers, cached field bakes,
  faster encode, persistent bake cache.
- **ItemSpec geometry** (item MVP steps 1 to 3): parts and derived values, routes
  and fittings, panels and openings. Independent of everything above.

Tracks: browser (scene view), Rust engine (CPU), Rust types/model plus viewer
(lighting), Rust itemspec (geometry).
Gate: washroom opens with baked maps by default with recorded load and frame times;
its picture is unchanged by the lighting record; item geometry checks pass.

### Stage 2: materials that follow geometry

- Roadmap 1A: pattern anchor, mapping per slot, named receiver features, live
  evaluation for items.
- Roadmap 1B: cutout, ramps, decal appearance.
- Weathering layers and the concrete and steel split (above), with the pass graph.

Gate: resizing `wall-tall` leaves the existing region's pattern unchanged; grime
follows a moved sill and base; the intrinsic concrete bake is reused across the
resize; native and Wasm agree.

### Stage 3: ItemSpec MVP

Item MVP steps 4 to 7: items in scenes, features and anchors, decals, editor. The
washroom is rebuilt from items, with weathering layers applied by feature.

Gate: the item MVP's five acceptance checks, with the washroom GLB build kept as a
comparison fixture until they pass.

### Stage 4: shared delivery

Instanced batches in model, packet and renderer; scene fields as textures; a
time-driven render mode; lighting export (`KHR_lights_punctual` plus extras) and the
game runtime module for lights, shadow style and AO.

### Stage 5: live and painted materials, and shadows as inputs

- Roadmap 1C and 1D: painted shading model, delivered live programs, `shadeBand`,
  instance, vertex and scene-field inputs. **Prototype done 2026-11-16, unreviewed**: see
  [tree-item-roadmap.md](tree-item-roadmap.md#1c-and-1d-prototype-roadmap-stage-5-2026-11-16)
  for what exists, the checks and the assumptions (painted finishing is the lighting record's
  `painted` setting; only painted materials are delivered with their program).
- Occluder service and the `shadow`, `occlusion` and `sunExposure` fields, which
  weathering layers can then read (moss in the shade).

### Stage 6 and 7: TreeSpec

Painted grass, then shrubs and trees, as in the roadmap's phases 4 and 5.

- Stage 6, painted grass: **prototype done 2026-11-16, unreviewed**: foliage looks on instanced batches, the fixed
  grass shader families (bend, wind, rejection), per-blade color from a painted recipe in the vertex stage, the
  boundary-distance field, per-tile levels, and a lawn with grass added to the washroom. See
  [tree-item-roadmap.md](tree-item-roadmap.md#phase-4-painted-grass-prototype-roadmap-stage-6-2026-11-16) for what
  exists, the measurements and frame-time budget, and the assumptions.

### Later

Baked stylized shadow masks for static scenes; placement by shadow; Kowloon
world-build performance; an AML surface for lighting.

## Open decisions, consolidated

Lighting: intensity units; exposure and tone mapping per scene or per output; one or
several styled lights; per-light or per-scene shadow style; first authoring surface;
theme locks; whether AO and tone mapping apply to painted materials.

Materials and items: live programs for every scene or only declared ones (assumed:
declared); scene-field resolution and owner; painted output display-referred or
converted; item expressions as strings or JSON; glazing in the MVP; whether a
weathering layer may also be rendered as a separate runtime layer instead of being
composed into the bake.

TreeSpec: its own authoring language in phase 4, or a Rust API with fixtures first.
