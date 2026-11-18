# ItemSpec versions 1 and 2

Version 1 definitions load unchanged and keep their geometry keys and meshes.
Version 2 is described [below](#version-2-parts-derived-values-routes-and-panels).

This module owns persistent `.item.json` definitions, procedural local geometry,
and static GLB scene ingestion. It uses caller-supplied bytes and never reads files
or the network. Native and Wasm callers use the same implementation.

`parse(source_name, text)` reads the definition. `prepare` resolves controls,
material requests and asset identities. `PreparedItem::build` creates immutable
local geometry; `PreparedItem::reuse` accepts an existing artifact with the same
geometry key and renews material bindings, attachments, collision and provenance.
Geometry identity includes effective shape inputs, the item transform, and exact
GLB dependency bytes. It excludes finish controls, placement identity and bindings.
Material requests use MaterialSpec module entries and parameter values; a value
`{"parameter":"roughness"}` binds an item control. Geometry cannot read finish controls.

```json
{
  "format": "itemspec",
  "version": 1,
  "id": "pipe",
  "parameters": {
    "length": {"type":"number", "role":"geometry", "default":2, "min":0.01, "max":100, "unit":"m"},
    "roughness": {"type":"number", "role":"finish", "default":0.45, "min":0, "max":1}
  },
  "generator": {"kind":"pipe", "length":{"parameter":"length"}, "radius":0.1, "wallThickness":0.01, "segments":16, "capped":true},
  "slots": {"surface":{"entry":"materials/metal.ms", "parameters":{"roughness":{"parameter":"roughness"}}, "seed":0}},
  "collision": "mesh"
}
```

Controls support number, integer, boolean, color and choice. Color controls accept
six-digit sRGB hex or linear RGB arrays; hex values become linear RGB when bound
to a material parameter. Pipe and duct length
is local Z; duct width is X and height is Y. Wall thickness zero gives a solid
section. Capping is optional. Generated start/end attachments face away from the
item. Authored attachments have unique names, positions (numeric values or geometry
control references) and direction vectors. The optional transform is four affine
columns: three basis vectors and translation, in right-handed Z-up meters.

Collision is `"none"`, `"bounds"`, `"mesh"`, or a box/sphere object with `kind`,
`center` and `size`/`radius`. Explicit shapes use definition coordinates and the
item transform. Bounds, local node transforms and attachments reflect that transform.

Use `{"kind":"glb","asset":"models/item.glb"}` with optional scene index for
imports. Start with empty slots to retain imported PBR materials. Returned slot
names are `material/0`, `material/1`, etc.; missing primitive material uses
`material/default`. A named assignment replaces that slot only. Local primitives
are shared across hierarchy nodes. The importer converts glTF Y-up positions,
normals and node bases to Z-up, retaining UV values, color alpha, tangent handedness,
additional UV sets, exact image bytes and PBR factors. Flattening applies inverse
transpose normals and corrects winding under reflection. Missing normals produce
flat corners and discard authored tangents, as required by the [glTF specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html).
`delivery_tangents` retains authored normal-map bases or derives them from original
UVs before the delivery adapter flips texture coordinates.

The bounded static importer supports triangles, strips, fans, interleaved and sparse
accessors, embedded or supplied relative resources, PNG/JPEG images and core PBR
textures. Animation, skins, morphs, compression and unrepresented vertex/node
extensions produce explicit errors. Material extension and sampler state remain
on the imported records; `ensure_scene_compatible` checks the narrower current
scene renderer contract (UV0, identity texture transform, repeat/linear mipmapped
sampling, supported PBR channels). An unsupported renderer feature must fail at
that boundary rather than disappear from the imported artifact.

Limits include 1 MiB definition text, 64 controls, 128 slots, 256 attachments,
32 MiB main GLB, 8 MiB GLB JSON, 64 MiB aggregate source/image bytes, 16 MiB per
encoded image, 96 Mi pixels, 4096 nodes/primitives, 500000 local vertices and
250000 source/placed triangles and 1000000 placed vertices. Images are validated before publication. Cancellation
and resolver failures return errors; callers publish completed artifacts only.
`retained_bytes` charges all retained attribute capacities, hierarchy and raw PBR
metadata, images, bindings and provenance. Shared allocations are charged fully
per owner so cache admission remains conservative on native and Wasm targets.

## Version 2: parts, derived values, routes and panels

A version 2 definition has no root `generator`. It lists `parts`, each with an `id`,
a `generator`, an optional `placement` (`position`, and `rotation` as extrinsic XYZ
degrees) and an optional `slots` map from face group to item slot. Without a `slots`
map every face group gets a slot of its own name; with one, unmapped face groups are
not emitted. Each part outputs one mesh per emitted face group (`<part>/<group>`)
under a node `part/<id>` that carries its placement. Fixtures for the washroom's
wall, low wall, pipe and duct are in `fixtures/`.

`derived` maps a name to an expression string: numbers, parameter and derived names,
`+ - * /`, parentheses and `min`, `max`, `floor`, `ceil`, `round`, `abs`. Names in
expressions use letters, digits and underscores. A scalar field accepts a number,
`{"parameter": name}` or `{"derived": name}`. Derived values read numeric geometry
controls only. Cycles, unknown names, division by zero and non-finite results are
`ITEM_EXPRESSION` errors at `/derived/<name>`, with the character position in the
message.

| Generator | Fields | Face groups |
| --- | --- | --- |
| `box` | `size` (spans from the part origin) | `side`, `top`, `bottom` |
| `panel` | `width`, `height`, `thickness`, `openings[]`, `patternAnchor` | `front`, `back`, `edge`, `reveal`, `frame`, `glazing`, `sill` |
| `route` | `section`, `wallThickness`, `points[]`, `bendRadius`, `segments`, `bendSegments`, `up`, `capped`, `fittings[]`, `from` | `body`, `fitting`, `bracket`, `bolt`, `end` |
| `pipe`, `duct` | as version 1 | `surface` |
| `item` | `entry` (one or a list), `pick`, `parameters` | the sub-item's slots |
| `native` | `name`, `parameters` | the generator's slots |

Placement rules position openings and fittings: `{"from": "start"|"end"|"center",
"offset"}`, `{"fraction"}`, and for fittings `{"every", "inset"}` (even spacing no
larger than `every`; copies are named `<id>-1`, `<id>-2`, ...) and `{"point": i}`.
For an opening the rule places its near edge inside the length left over after its
own size; for a fitting it places the fitting's centre by arc length.

**Panel.** Width is local X, height Z, thickness Y; the front face is y = 0 and
faces -Y, and the origin is its bottom-left corner. Openings are `rectangle` or
`arch` (a half-round top of 16 chords), `through` or `recess` with a `depth`. They
stay 0.02 m inside the panel and apart from each other, or fail with `ITEM_OPENING`.
A through opening may have a `frame` (`border`, `depth`) and `glazing`
(`thickness`), both centred in the thickness; any opening may have a `sill`
(`depth`, `overhang`, optional `thickness`). The slab is built as a polygon with
holes swept through the thickness, which is the same solid as the panel minus its
extruded openings but keeps exact face groups and f64 positions.

**Route.** `section` is `{"kind":"round","radius"}` or
`{"kind":"rect","width","height"}`. Corners are filleted with `bendRadius`, which
must exceed the section's half extent and fit between neighbouring points
(`ITEM_ROUTE`). Texture coordinates are perimeter by arc length, in meters. A
route with `"from": {"attachment": "<part>/<fitting>/branch"}` takes its points
relative to that attachment of an earlier part. Fittings are `coupling` (`length`,
`clearance`), `flange` (`radius` or `overhang`, `thickness`, `bolts`), `valve`
(`length`, `handle`), `bracket` (`reach`, `direction`, optional `width`,
`thickness`) and `tee` (`direction`, `length`). A fitting must lie on one straight
run (`ITEM_FITTING`). Fittings are closed solids that overlap the body; nothing is
subtracted. Attachments are `<part>/start`, `<part>/end`, `<part>/<fitting>` and
`<part>/<tee>/branch`.

`ItemGeometry` also carries `parts` (ID, geometry key, node and meshes), `features`
and `anchors`, in the same space as `bounds`. Features are named polylines: panels
report `base`, `top`, `opening-edge` and `sill`; routes report `joint` (a ring at
every fitting, with the fitting's length as `width`), `end` and `bracket-contact`.
Materials cannot read them yet. Each part's geometry key hashes the values it
resolved, its placement and its slot map; the item key hashes the part keys and the
transform. `PreparedItem::rebuild` regenerates only parts whose key changed.

**Decals.** `decals` lists images placed on a part: `{"id", "image", "size": [w, h],
"on": {"part", "face"}, "u", "v"}` on a panel face (`front` or `back`) or a box's
`front` (its y = 0 side), and `{"along", "around"}` on a route, where `size` is around
by along and `around` is in degrees from the section's first axis. A decal stays 0.02 m
inside its face and clear of openings, or on one straight run and clear of fittings;
otherwise `prepare` fails with `ITEM_DECAL_FIT` at `/decals/<i>`. `GeneratedItem.decals`
holds each sheet 2 mm off its surface (0.6 mm more per `layer`), flat or wrapped, in
the space of `bounds`. Decals are outside every geometry key and are placed again from
their rules for each set of control values.

**Handles.** `handles` lists `{"id", "parameter", "at", "axis", "scale"}`: an editor may
drag the point `at` along `axis`, and `d` meters change the numeric geometry control
`parameter` by `d * scale` (default 1). `GeneratedItem.handles` gives them resolved,
with the control's value and range.

## Composition: sub-items, repeats, conditions and native generators

**Expressions.** Any scalar field may be an expression string in the derived-value grammar,
which also has `%`, comparisons (`< <= > >= == !=`, giving 1 or 0), `clamp(x, low, high)`,
`select(condition, a, b)` and `random(...)`: a value in [0, 1) hashed from the exact bits of
its arguments (SplitMix64), the same on every platform. A boolean geometry control reads as 1
or 0. `{name}` in a sub-item entry or a route's `from` attachment is replaced by the integer
value of a copy index or derived value.

**Repeats and conditions.** A part may `repeat` over up to three named axes
(`[{"name":"j","count":"floors"},{"name":"i","count":{"derived":"bays"}}]`); copies are
`<id>-<j>-<i>`, and inside the part `j`, `i`, `j_count` and `i_count` read as numbers. `when`
keeps a part (or each copy) only where its expression is positive. A panel opening may
`repeat` along one axis and have a `when` of its own; the 32-opening limit counts copies. An
item expands to at most 512 parts.

**Sub-items.** `{"kind":"item","entry":"window.item.json","parameters":{...}}` places another
item definition as a part. `entry` may be a list, from which `pick` (floored, wrapped into
range) chooses. Arguments are literal JSON values, `{"parameter": name}` (passed unchanged),
`{"derived": name}`, `{"expr": text}` or `{"choose": [values], "by": text}`. The sub-item's
meshes are flattened under the part's node and named `<part>/<mesh>`; its slots keep their names
unless the part's `slots` maps them (an unmapped slot is then dropped). A sub-item slot the
parent does not assign takes the sub-item's own material request, and its imported materials and
images come along. Its attachments become `<part>/<name>`, so a later route can start from them.
Entries are read through the same asset reader as GLB files (a scene project's modules first);
sub-items nest at most 8 deep and a definition that contains itself fails with `ITEM_SUB_ITEM`.
A part's geometry key includes its sub-item's key, so a change rebuilds only that part.

**Native generators.** `{"kind":"native","name":"kowloon/aircon","parameters":{...}}` calls a
generator its host supplies through `Environment::with_natives` (`NativeGenerators`: an identity
per name, which joins the part's geometry key, and a `generate` that returns meshes with slots).
The shipped host (`worldspec::shipped::item_generators`) offers the Kowloon fixtures
(`worldspec/themes/kowloon/items.rs`); without a host, or for an unknown name, preparing fails
with `ITEM_NATIVE`. `prepare` keeps its old signature; `prepare_in` takes the environment.

Composed examples are in `worldspec/themes/kowloon/items/`: `kowloon-duct-riser` (a riser that
spans `floors` with a branch into each, an elbow into the wall at the roof and a hood at its
foot) and `kowloon-facade-side` (one side of a building: a wall panel per floor with a row of
windows, string courses, a parapet, air conditioners and condensers chosen per bay by `seed`, and
the riser as a sub-item when there are two floors or more).

In a scene project an item is a model of kind `item` (see `authoring/project.rs`):
its slots are bound to the project's materials, every part and face group is a receiver
named `<model>/part/<part>/<part>/<group>`, and each receiver is given its part's
features and pattern anchor.

Limits: 64 decals, 32 handles, 64 part definitions (512 parts counting copies), 64 derived values, 128 route points, 256 fittings per route, 32
openings per panel, 250000 triangles per item.

Diagnostics carry source identity and JSON pointers. `parse` locates its errors;
call `locate_diagnostic` on later definition errors with the matching source text
to obtain UTF-16 offsets and one-based line/column. GLB errors name the GLB asset
and JSON pointer instead of falsely pointing into the item definition.
