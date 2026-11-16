# ItemSpec MVP

> **Assumed pending review (2026-11-16).** Steps 1 to 3 are prototyped in
> `crates/engine/src/itemspec/definition` (tests: `crates/engine/tests/item_parts.rs`;
> reference: `definition/README.md`). Choices made where this plan was open or silent:
>
> - Expressions are strings (open decision 1). Names in them use letters, digits and
>   underscores.
> - The panel is built as a polygon with holes swept through the thickness, not with
>   `Solid::boolean` and `Solid::owned`. It is the same solid, with exact face groups,
>   f64 positions and metric UVs; the boolean kernel is used only as a test oracle.
> - A part has an optional `placement` (position, rotation in degrees), and an
>   omitted part `slots` map gives every face group a slot of its own name.
> - Opening rules place the opening's near edge in the length left after its size;
>   fitting rules place the fitting's centre by arc length. Openings keep a 0.02 m
>   margin. An arch top is 16 chords.
> - A fitting must lie on one straight run, so `{"point": i}` is usable only at a
>   point without a bend. `every` copies are named `<id>-1`, `<id>-2`, ...
> - A flange takes `radius` (round) or `overhang` (any section). Brackets take
>   optional `width` and `thickness`; sills an optional `thickness`. Routes take
>   `bendSegments`, `up` and `capped`.
> - Fittings and tees overlap the body as closed solids; nothing is cut.
> - Features (`joint`, `end`, `bracket-contact`, `opening-edge`, `sill`, `base`,
>   `top`) and one pattern anchor per part are emitted on `ItemGeometry` as polylines
>   and frames. They are not wired into MaterialSpec.
> - `glb` is not accepted as a part, `lathe` and `decals` are not implemented, and
>   glazing is an opaque slab with its own slot (open decision 2 stays open).
> - Per-part reuse is `PreparedItem::rebuild`; `reuse` is unchanged.

> **Assumed pending review (2026-11-16, steps 4 to 7).** Prototyped in
> `authoring/item.rs` and `authoring/project.rs` (`place_item`, `resolve_items`),
> `itemspec/definition/decal.rs`, the Studio (`editor/studio/items.ts`,
> `browser/renderer/item-overlays.ts`) and `superspec-studio inspect`. Tests:
> `crates/engine/tests/item_scene.rs`, `tests/item-washroom-browser.mjs`,
> `tests/item-scene-parity.mjs`; scene: `tests/studio/washroom-items`. Choices made:
>
> - A scene model is `{"kind": "item", "entry", "parameters", "transform", "slots"}`.
>   `transform` is four affine columns. `slots` maps an item slot to a project material
>   (a `materials[].id`); the item's own `slots` entries are not read in a scene, and a
>   slot left unbound keeps a plain surface. Mesh identities are
>   `<model>/part/<part>/<part>/<group>`.
> - A receiver is given the features and the pattern anchor of its own part (not of the
>   whole item), in world meters, through `Project.features` and `Project.anchors`.
> - The session keeps the last generated item per model identity and calls
>   `PreparedItem::rebuild`; mesh resources are named by part key. `metadata.items`
>   reports `generation`, `rebuilt`, part keys, controls, derived values, features,
>   attachments, decals and handles.
> - Decals live on the item (`decals`), outside every geometry key. They sit on a panel
>   face (`front`, `back`), a box's `front` (its y = 0 side) or a route (`along`,
>   `around`; `size` is around by along). A route decal must also clear the fittings. A
>   decal covers at most half a round section. Delivery is a visible, non-collidable
>   mesh listed in `Manifest.decals`, with a blended material that names the image by
>   its bytes. Recipe decals are not implemented.
> - The dimension-to-control link is `handles: [{"id", "parameter", "at", "axis",
>   "scale"}]`: moving the handle `d` meters along `axis` changes the control by
>   `d * scale`. `at` may read controls and derived values.
> - While a slider or handle moves, the Studio shows the per-pixel look and asks for no
>   layouts or bakes; the value it is let go on is baked. The bake graph keeps up to
>   4096 idle nodes (256 MiB) so the maps of untouched parts survive the drag.
> - In `tests/studio/washroom-items` the grime and rust that follow features are layers
>   that the build script adds to copies of the shipped concrete and steel recipes. The
>   shipped recipes read no named feature yet.
> - Acceptance check 1 holds for geometry, tile and decals. The shipped concrete still
>   changes on the old part of a widened wall (it reads receiver bounds); this waits for
>   the roadmap's concrete split.

**Ordering and reconciliation: see [roadmap.md](roadmap.md).** Steps 1 to 3 run in
roadmap stage 1, steps 4 to 7 in stage 3. Changes the roadmap makes here:

- Acceptance check 3 ("rust and grime follow joints, sills and wall bases") is met
  with weathering layers: a slot lists the layers on it, the named feature each one
  follows (`top`, `sill`, `base`, `joint`) and its parameters. Base recipes such as
  concrete no longer contain grime or read receiver bounds.
- "Reusing bakes across a resize" is no longer excluded: the intrinsic base bake is
  reused; only layer mappings and changed geometry re-bake.
- Acceptance check 5: the drag uses the per-pixel shader, the settled view uses a GPU
  bake in UV space, and the CPU bake is for export.
- Open decision 3 is settled: the washroom's hand-built GLB stays as a comparison
  fixture until the acceptance checks pass.
- The hand-built washroom now has one large pipe and no sink (simplified on
  2026-11-16); the routed pipe run with fittings returns through this MVP.

Status: proposal, not implemented. This is the detailed plan for phase 3 of the
[roadmap](tree-item-roadmap.md). It grows ItemSpec from one generator per item
into multi-part parametric items, and proves it by rebuilding the washroom
(`tests/studio/washroom`) from items.

## Goal and acceptance

The washroom is composed from items instead of the hand-written meshes in
`tests/studio/washroom/model.mjs`:

- the tall wall has a framed, glazed window, and the low wall a recess;
- the pipe run has bends, a tee with a branch, a valve, bolted flanges,
  couplings and wall brackets;
- the duct run has a bend, seams, flanges and brackets;
- at least one decal sits on the tiled wall and one on the pipe.

It is done when these hold, natively and in the browser:

1. Widening or heightening a wall keeps the window, tile grout and decal where
   their rules put them. No pattern shifts on the part of the wall that existed
   before.
2. Lengthening the pipe re-spaces its brackets, keeps fittings at their
   stations, and extends the surface pattern without moving the existing part.
3. Rust and grime follow joints, sills and wall bases after every resize.
4. A decal that no longer fits, or lands in an opening, reports a source-linked
   diagnostic and the last valid scene stays visible.
5. A dimension drag in the editor updates through the per-pixel path without a
   bake; the bake runs on release and only for parts whose geometry changed.

## Where ItemSpec is today

- `ItemDefinition` (`crates/engine/src/itemspec/definition/schema.rs`) has one
  `generator`: `pipe`, `duct` (both straight) or `glb`.
- Procedural output is one mesh with one slot, `surface`
  (`definition/procedural.rs`).
- A generator field is a number or `{"parameter": name}`. There are no derived
  values.
- `superspec-geometry::Solid` already provides `cube`, `cylinder`, `extrude`,
  `inset`, `loft`, `boolean`, `union` and owner tracking through booleans
  (`owned`). ItemSpec does not use it yet.
- Scene projects accept `glb` and `aml` models only
  (`crates/engine/src/authoring/project.rs`), so an item cannot be part of the
  washroom scene.

## Design

### Definition version 2

Version 1 files keep loading unchanged; a version 1 `generator` is read as one
part named `item` with the slot `surface`.

```json
{
  "format": "itemspec",
  "version": 2,
  "id": "washroom-wall",
  "parameters": {
    "width":  {"type": "number", "default": 2.7, "min": 1.2, "max": 8, "unit": "m"},
    "height": {"type": "number", "default": 2.6, "min": 1.8, "max": 5, "unit": "m"}
  },
  "derived": {"sillHeight": "min(1.1, height - 1.0)"},
  "parts": [{
    "id": "wall",
    "generator": {
      "kind": "panel",
      "width": {"parameter": "width"}, "height": {"parameter": "height"},
      "thickness": 0.25,
      "openings": [{
        "id": "window", "shape": "rectangle", "width": 0.9, "height": 0.7,
        "u": {"from": "end", "offset": 0.6},
        "v": {"from": "start", "offset": {"derived": "sillHeight"}},
        "frame": {"border": 0.05, "depth": 0.08},
        "glazing": {"thickness": 0.006},
        "sill": {"depth": 0.06, "overhang": 0.03}
      }]
    },
    "slots": {"front": "concrete", "back": "concrete", "edge": "concrete",
              "reveal": "concrete", "frame": "frame", "glazing": "glass",
              "sill": "concrete"}
  }],
  "slots": {
    "concrete": {"entry": "materials/kowloon-concrete.material.ts", "seed": 0},
    "frame":    {"entry": "materials/painted-steel.material.ts", "seed": 1},
    "glass":    {"entry": "materials/glass.material.ts", "seed": 2}
  },
  "decals": [{
    "id": "notice", "image": "decals/notice.png", "size": [0.3, 0.42],
    "on": {"part": "wall", "face": "front"},
    "u": {"from": "start", "offset": 0.5},
    "v": {"from": "start", "offset": 1.4}
  }]
}
```

### Derived values

A `derived` map of name to expression. The grammar is numbers, parameter and
derived names, `+ - * /`, parentheses and `min`, `max`, `floor`, `ceil`,
`round`, `abs`. It is parsed in Rust, shared by native and Wasm, with
diagnostics pointing at the expression. A scalar field accepts a number,
`{"parameter": name}` or `{"derived": name}`. Derived values inherit the
`geometry` role rule: they cannot read finish controls. Cycles and non-finite
results are errors.

### Placement rules

One rule type positions openings, fittings and decals, and is what makes
resizing predictable:

| Rule | Meaning |
| --- | --- |
| `{"from": "start" \| "end" \| "center", "offset": m}` | A fixed distance from one end, or from the centre. |
| `{"fraction": f}` | A fraction of the available length. |
| `{"every": m, "inset": m}` | Repeats at an even spacing no larger than `every`, inside the insets. Fittings only. |
| `{"point": i}` | At route point `i`. Fittings only. |

`start` and `end` follow the part's own axes: `u` and `v` on a panel face,
arc length on a route.

### Generators

| Kind | Fields | Face groups |
| --- | --- | --- |
| `box` | `size` | `side`, `top`, `bottom` |
| `panel` | `width`, `height`, `thickness`, `openings[]` | `front`, `back`, `edge`, `reveal`, `frame`, `glazing`, `sill` |
| `route` | `section`, `wallThickness`, `points[]`, `bendRadius`, `segments`, `fittings[]` | `body`, `fitting`, `bracket`, `bolt`, `end` |
| `lathe` | `profile[]`, `segments`, `sweep` | `outer`, `inner`, `rim` |
| `pipe`, `duct`, `glb` | unchanged | `surface`, or imported slots |

**Panel.** Openings are `rectangle` or `arch`, with mode `through` or `recess`
(with `depth`). Each opening is extruded through the thickness and subtracted
with `Solid::boolean`; `Solid::owned` tags the cut faces so they become the
`reveal` group. A frame is the opening ring minus `Solid::inset`. Openings must
stay inside the panel with a margin and must not overlap.

**Route.** `section` is `{"kind": "round", "radius"}` or
`{"kind": "rect", "width", "height"}`. Points are scalars, so a parameter can
move a point. Corners are filleted with `bendRadius`; a radius that does not fit
between neighbouring points is an error. The body is lofted along the
centreline with metric UVs (arc length by perimeter). Fittings:

| Fitting | Fields |
| --- | --- |
| `coupling` | `at`, `length`, `clearance` |
| `flange` | `at`, `radius`, `thickness`, `bolts` |
| `valve` | `at`, `length`, `handle` (`wheel` or `lever`) |
| `bracket` | `at`, `reach`, `direction` |
| `tee` | `at`, `direction`, `length` |

A `tee` emits an attachment named `<part>/<fitting>/branch`. Another route can
start there with `"from": {"attachment": "riser/tee-1/branch"}`, which is how a
branch stays connected when the riser is resized. Every fitting and both ends
also emit attachments, replacing today's generated `start` and `end`.

**Lathe** is a stretch item for the sink basin and is not required for
acceptance.

Each part outputs one mesh per face group that has a slot. Parts keep separate
meshes so a change to the pipe does not disturb the wall's receiver.

### Features and pattern anchors

Each part reports named features and one anchor frame with its meshes:

- panel: `opening-edge`, `sill`, `base`, `top`; anchor at the bottom-left corner
  of the front face, or an authored `patternAnchor`;
- route: `joint` (at every fitting), `end`, `bracket-contact`; anchor at the
  route start.

These feed the receiver features and the `anchored` coordinate from
[roadmap phase 1A](tree-item-roadmap.md#1a-stable-mapping-under-resize). Until
1A lands, features are emitted and inspected but materials cannot read them.

### Decals

A decal has an appearance (an `image` asset first; a decal recipe once
[roadmap phase 1B](tree-item-roadmap.md#1b-cutout-ramps-and-decal-appearance)
lands), a `size` in meters, a target face and a placement rule per axis. On a
route it uses `along` (a rule) and `around` (degrees).

- Rendering: a render-only mesh, offset 2 mm from the surface plus 0.6 mm per
  `layer`, flat on panels and wrapped on round sections. It is not collidable.
  This follows the Kowloon poster precedent without importing it.
- Fit: the rectangle must lie inside the face, clear of openings, with a 2 cm
  margin. On a route it must lie on a straight run. Otherwise `ITEM_DECAL_FIT`.
- Resize: size is fixed; position is re-evaluated from the rule.
- Identity: the decal image or bake is one artifact; each placement is a
  separate identity and never enters a raster key.

### Identity and reuse

Each part gets its own geometry key from the values it actually reads. The item
key is the hash of its part keys, transform and decal placements.
`PreparedItem::reuse` then works per part: a finish edit rebuilds nothing, and a
pipe edit leaves wall meshes and their bakes untouched.

### Items in scene projects

`compose_models` accepts `{"kind": "item", "entry", "parameters", "transform"}`
beside `glb` and `aml`, and `Session::shade_project` covers the resulting
meshes. This is what lets the washroom mix items with its remaining GLB parts
and use the per-pixel path during a drag.

### Limits and diagnostics

64 parts, 128 route points, 32 openings per panel, 256 fittings per route,
64 decals, 64 derived values. Every error carries the item source and a JSON
pointer, in the existing `ITEM_*` style: `ITEM_PART`, `ITEM_EXPRESSION`,
`ITEM_ROUTE`, `ITEM_OPENING`, `ITEM_FITTING`, `ITEM_DECAL_FIT`.

### Boundaries

`itemspec` must not import `worldspec` or `architecture`. Route, fitting and
opening code is written against `superspec-geometry`; the Kowloon `PipeSystem`
and the AML opening cutter are references, not dependencies. If AML later shares
a kernel (opening shapes, sweeps), it moves down into `superspec-geometry`.
Run `npm run check:architecture` at every step.

## Steps

Steps 1 to 4 need nothing from MaterialSpec and can run alongside roadmap
phase 1. Step 5 needs 1A; recipe decals in step 6 need 1B.

| Step | Work | Main files | Checks |
| --- | --- | --- | --- |
| 1. Parts and derived values | Version 2 schema, expression parser, per-part keys, multi-mesh output, `box` | `itemspec/definition/schema.rs`, `mod.rs`, `procedural.rs`, new `expression.rs` | Version 1 fixtures produce identical geometry keys and meshes. Expression results against hand-computed values; cycle and unknown-name errors point at the source. |
| 2. Routes and fittings | `route` generator, fittings, attachments, `from` connections | new `definition/route.rs`, `fittings.rs` | Centreline length equals straights plus arcs. Bracket count and spacing from `every` match the formula. A too-tight bend fails with `ITEM_ROUTE`. A branch follows its tee after a resize. |
| 3. Panels and openings | `panel`, openings, frames, glazing, sills, face groups | new `definition/panel.rs` | Volume equals panel minus openings. Reveal faces carry the `reveal` group. Overlapping or out-of-bounds openings fail with `ITEM_OPENING`. |
| 4. Items in scenes | `kind: "item"` models, live shading for items, washroom walls and pipe moved to items | `authoring/project.rs`, `authoring/item.rs`, `tests/studio/washroom/` | Washroom builds natively and in Wasm with the same packet. `tests/material-washroom-browser.mjs` screenshot shows the window and fittings. |
| 5. Features and anchors | Emit features and anchor frames; per-slot mapping | `itemspec/definition/*`, `authoring/item.rs` | Resize checks 1 to 3 from the acceptance list, measured as pixel difference over the unchanged region. |
| 6. Decals | Definition, placement, fit checks, offset meshes | new `definition/decal.rs`, `browser/renderer/scene.ts` | Acceptance check 4. A decal keeps its offset across three wall widths. No z-fighting in the washroom screenshot. |
| 7. Editor and CLI | Range sliders, overlays for features, anchors and decal bounds, drag handles on declared dimensions; `superspec-studio inspect` for items | `editor/studio/parameters.ts`, `layout.ts`, `main.ts`, `crates/cli/src/bin/studio/` | Acceptance check 5, with edit-to-preview time recorded during a drag and after release. |

Each step keeps behaviour tests with independently derived expectations, checks
native and Wasm agreement, and updates `definition/README.md` and
[AUTHORING.md](../AUTHORING.md).

## Not in the MVP

- A TypeScript SDK for items. Definitions stay JSON.
- Booleans between arbitrary parts, curved walls, doors and hardware.
- Decals across wall corners or pipe bends.
- Hardware instancing of bolts and brackets; they are expanded meshes until
  [roadmap phase 2](tree-item-roadmap.md#phase-2-shared-delivery).
- Reusing bakes across a resize. A resized part re-bakes; unchanged parts do
  not.

## Open decisions

1. Expressions as strings (assumed here) or as structured JSON.
2. Whether a window's glazing needs a transmissive material in the MVP or an
   opaque placeholder is acceptable.
3. Whether the washroom keeps its GLB build as a comparison fixture once items
   replace it.
