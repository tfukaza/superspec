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

An item owner may declare render alternatives with
`"lods": [{"distance": 20, "parameters": {"segments": 8}}, {"distance": 50, "parameters": {"segments": 4}}]`.
Each override names a declared geometry control and satisfies its type and bounds.
There are at most seven alternatives, with positive, strictly increasing distances
in meters. The original definition and effective controls remain level zero.
Collision, attachments and editor handles remain those of that canonical item.

The shared native/Wasm authoring session emits the alternatives for standalone items
and item models in WorldSpec projects as portable geometry LOD groups. Every level
uses the same material binding and baking path. The low-level `prepare`/`build` calls
still produce one effective control variant; `render_alternative` supplies the source
and controls of a declared alternative. A composed item's root owns its level choices:
sub-item generators read the controls passed by that root, rather than independently
switching topology inside the parent's combined geometry.

The Kowloon static tree intentionally keeps TreeSpec tier 1 as its existing near
appearance and selects the authored tier 2 at 45 m. Painted avenue trees and shrubs
already carry their full authored bark levels; the groundcover carrier deliberately
uses one source tier to construct a single painted mass.

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

Limits include 1 MiB definition text, 160 controls, 128 slots, 256 attachments,
512 MiB main GLB, 8 MiB GLB JSON, 1 GiB aggregate source/image bytes, 64 MiB per
encoded image, 512 Mi base-level pixels, and 4096 nodes/primitives. Source geometry
allows 500,000 local vertices and 250,000 triangles. Across all placements, one
item may use 1,500,000 vertices and 1,000,000 triangles. Images are validated before publication. Cancellation
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
| `box` | `size` (spans from the part origin), `bevel`, `bevelSegments`, `bevelAxis`, `miters` | `side`, `top`, `bottom` |
| `panel` | `width`, `height`, `thickness`, `openings[]`, `patternAnchor`, `bevel`, `bevelSegments`, `arris`, `arrisStart`, `arrisEnd`, `miters` | `front`, `back`, `edge`, `reveal`, `frame`, `glazing`, `sill` |
| `prism` | `footprint`, `height`, `inset`, `bevel`, `bevelSegments` (see Footprints) | `side`, `top`, `bottom` |
| `arc` | `radius`, `turn`, `segments`, `section[]`, `closed`, `bevel`, `bevelSegments`, `step`, `patternAnchor`, `rings[]` | `side`, `end` |
| `route` | `section`, `drawn`, `wallThickness`, `points[]`, `bendRadius`, `segments`, `bendSegments`, `up`, `capped`, `fittings[]`, `from` | `body`, `fitting`, `bracket`, `bolt`, `end` |
| `pipe`, `duct` | as version 1 | `surface` |
| `item` | `entry` (one or a list), `pick`, `parameters` | the sub-item's slots |
| `native` | `name`, `parameters` | the generator's slots |
| `prototype` | `name`, `content`, `mirrored` | the generator's slots |

Placement rules position openings and fittings: `{"from": "start"|"end"|"center",
"offset"}`, `{"fraction"}`, and for fittings `{"every", "inset"}` (even spacing no
larger than `every`; copies are named `<id>-1`, `<id>-2`, ...) and `{"point": i}`.
For an opening the rule places its near edge inside the length left over after its
own size; for a fitting it places the fitting's centre by arc length.

A route's `points` may also reference a numeric grid parameter containing XYZ triples.
For mounted runs, `supports` references a second grid with one perpendicular offset
from each segment's centerline to its receiving face. A zero offset leaves that leg
unsupported. The resulting bands, arms and footplates share the `bracket` slot.
Set `omitUnfittableRepeats` to omit repeated fittings that fall on a bend or end.

**Bevels.** `bevel` is a radius in meters that rounds real edges where a silhouette shows them,
in `bevelSegments` chords (1, a chamfer, by default; or 2); 0, the default, leaves the part and
its geometry key as they were. A box rounds its twelve edges, or with `bevelAxis` (`x`, `y` or
`z`) only the four along that axis, so boxes laid end to end (a coping, a fascia, a kerb) keep
one section. A prism rounds the rims of its top and bottom. A panel rounds the rim of its front
face and the front rim of every opening; openings then stay twice the radius plus 2 mm from
the panel's edge and from each other where that exceeds 0.02 m. The radius is less than half of
every dimension it rounds (`ITEM_GEOMETRY` at the field). Bounds, face groups, features and
anchors do not change: a rounded strip joins the wall it leaves (`side`, `edge`, `reveal`),
carries on that wall's texture coordinates by arc length, and its normals turn from the wall's
to the cap's, so one chord already shades as a round. A box costs 60 triangles with one chord
(140 with two, 28 along one axis) against 12. Shading-only rounding belongs to materials.

**Panel.** Width is local X, height Z, thickness Y; the front face is y = 0 and
faces -Y, and the origin is its bottom-left corner. Openings are `rectangle` or
`arch` (a half-round top of 16 chords), `through` or `recess` with a `depth`. They
stay 0.02 m inside the panel and apart from each other, or fail with `ITEM_OPENING`.
A through opening may have a `frame` (`border`, `depth`) and `glazing`
(`thickness`), both centred in the thickness; any opening may have a `sill`
(`depth`, `overhang`, optional `thickness`). The slab is built as a polygon with
holes swept through the thickness, which is the same solid as the panel minus its
extruded openings but keeps exact face groups and f64 positions.

*Seats.* A part laid flat on a wall (tiles, a dado, a plinth band) stands only millimeters
proud of it, closer than a depth buffer separates surfaces seen from afar, so the wall shows
through it as the camera moves. A recess with `"seat": true` is cut for such a part instead:
the wall has no face behind it. A seat reports no `opening-edge` or `sill` and stops or clips
no zone (the wall's weathering runs on behind it as if it were whole); it may come within 2 mm
of the panel's edge and of other openings (a bevelled panel keeps its own margin), so a band
laid along the whole wall leaves only hairlines of wall behind its ends. It has no sill. Lay
the part on the wall with its front `t` proud and its back face left out (a panel with only
its `front` and `edge` mapped), and cut the seat a few centimeters deep, so that nothing lies
behind the part's front within the recess's depth plus `t`.

*Miters.* A panel or an X-axis box (`bevelAxis: "x"`) takes optional `miters: [start, end]`.
Each value is the signed tangent of half the plan turn. At a square outside corner, both
joining pieces use 1. Each end moves inward by its value times the depth from `y = 0`, so the
pieces share the joint plane. Negative values extend the ends at an inside corner. A panel
miter must agree with any arris turn on that end. Openings need clearance from the miter
through their full depth. A rectangular seat with square jambs can taper at the back to
keep its opening in place. Panel miters require a square front rim.

*Arrises.* Two flat receivers that meet at a corner cannot be rounded by a material, and a
`bevel` rounds a panel's whole rim. `arris` (a radius) rounds only the panel's upright end
edges, where `arrisStart` and `arrisEnd` give the tangent of half the plan turn there (1 at a
square corner, a footprint edge's `_miter_start` and `_miter_end`; 0 leaves the end sharp).
Each of the two panels at a corner draws its own half of the round as one chord of its `front`,
from where the front leaves off (the radius times the tangent before the corner) to the
round's middle on the corner's bisector, with normals that turn to the bisector's, so the two
chords are one round of two segments. The end face starts behind the round, the top and bottom
lose its corner, and openings keep clear of it. An opening's `jambs` (a radius, rectangles
only) rounds its two upright front arrises, the edges of the piers beside it, in two chords of
the `front` that turn into the reveals; the opening may still start 0.02 m above the panel's
foot, and needs the radius more of wall on each side. A panel has a `bevel` or arrises and
jambs, not both (`ITEM_GEOMETRY`, `ITEM_OPENING`).

**Arc.** A section swept round a plan arc: a wall, a band or a moulding that turns a round
corner as one surface. The part's origin is where the arc starts; it leaves along +X, turns to
the left (toward +Y) by `turn` degrees (1 to 180) about an upright axis `radius` away, in
`segments` chords (8), and its outside faces -Y at the start as a panel's front does, so it
stands where a wall along +X ends and a wall along the turned direction starts where it ends.
`section` lists 2 to 32 `[out, up]` points: meters outside the arc of `radius`, and height. A
closed section (the default) runs counterclockwise in (out, up) and has two `end` caps; with
`"closed": false` it is a strip whose visible side is on its right (an outer face is listed
from its foot to its head), without ends. Every swept face is in `side`, one mesh whose
vertices are shared along the arc with normals that turn with it, so the chords shade as one
round and none of them is a crease; the section's own corners stay sharp unless `bevel` rounds
them (set back that far along both edges, in 1 or 2 chords). `step` splits section edges
longer than it into even rows. Texture coordinates are pattern coordinates in meters: `u` is
the distance along the chords of the arc of `radius` (for every section point, so a band keeps
its wall's pattern) and `v` the height of the section's first point run on along the section,
both measured from `patternAnchor`. A wall before the arc and one after it that measure their
patterns round the same footprint (`_perimeter`, `_cperimeter`) meet it without a seam when the
arc's slot is mapped by its texture coordinates; mapped by its anchor (the frame of its start)
the pattern is right where the arc starts and turns away from it. Features: `base` and `top`
follow the arc at the section's lowest and highest points, and `rings` (`{"name", "at": [out,
up]}`, at most 16) add named ones, each a polyline of `segments + 1` points; `start` and `end`
are the section where the arc begins and ends, where it meets what stands before and after it.
`recesses` (`{"from", "to", "depth"}`, with an optional one-axis `repeat` and a `when`; at most
16 counting copies) set the part of an upright outer face (a section edge of constant `out`
listed upward) between two heights back `depth` into the section: the arc's form of a panel's
seat, for a band laid round a curved wall. A recess lies inside one such face, a millimeter
apart from its ends and from other recesses, or reaches an open section's end, which then starts
or ends at the recess's back. Its floor and head do not advance `v`, so the face above it keeps
its pattern; a bevelled section has none.

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

`"drawn": {"section": {...}, "offset": [across, up]}` draws the body and its end caps
with another section, its centre moved `offset` meters from the centreline (across the
section, then along `up`; zero by default). Only those triangles change: the path, the
fittings and their solids, features, attachments and decals still follow `section`, so
an item can draw a member heavier or lighter without changing what it reports. The
drawn section must leave the wall a positive inner section and, with its offset, fit
inside `bendRadius` (`ITEM_ROUTE`). A drawn section equal to `section` with no offset
is the same route, with the same geometry key.

`ItemGeometry` also carries `parts` (ID, geometry key, node and meshes), `features`
and `anchors`, in the same space as `bounds`. Features are named polylines: panels
report `base`, `top`, `opening-edge` and `sill`; routes report `joint` (a ring at
every fitting, with the fitting's length as `width`), `end` and `bracket-contact`.
Materials cannot read them yet. Each part's geometry key hashes the values it
resolved, its placement and its slot map; the item key hashes the part keys and the
transform. `PreparedItem::rebuild` regenerates only parts whose key changed.

**Placements.** A native generator may describe an instance as a shared shape and
the instance's own colours (`NativeGenerators::prototype`, `prototype_meshes`).
`ItemGeometry` then also carries `prototypes` (by content key: generator, identity,
content and reflection, never a seed, part or placement) and `placements`
(prototype, part path, node, transform, inputs, slots and the expanded meshes
covered), lifted through sub-items and kept on a rebuild. They annotate the expanded
meshes, which stay complete: `ItemGeometry::placed` composes the transforms, and
placing each prototype by it rebuilds what `flattened` gives. A placement that
mirrors its prototype names the reflected prototype, so transforms are proper
motions. Both fields are absent from an item no generator describes, and no geometry
key reads them. `PreparedItem::part_contents` lists each leaf part's content keys
(its geometry key less ID and placement) without building anything.

**Prototype parts.** `{"kind":"prototype","name":"kowloon/condenser","content":{...}}`
draws one shared shape as it is: the meshes `NativeGenerators::prototype_meshes` gives
for `name` and `content`, which is what `ItemGeometry::prototypes` holds for the key of
that generator, its identity and that content, value for value. `content` is passed
literally (at most 128 values; no `{"parameter"}` or `{"expr"}` forms, since a content
is a recorded value), and `mirrored: true` draws the shape reflected in its local X,
as a mirrored placement names it. A tool that has only an item's placement records
(prototype name, content and reflection) can so build a library item per prototype and
bake each once. The part's meshes keep the prototype's neutral colours and their
`roles` (per vertex, 0 or the 1-based index of the role in the prototype's `roles`);
it records no placement, and its geometry key differs from a `native` part's with
the same values. An unknown `name` fails at `/parts/<i>/generator/name` with
`ITEM_NATIVE` when the item is prepared; content the generator does not draw fails
at `/parts/<i>/generator` with `ITEM_GEOMETRY` and the generator's message when the
part is built. A prototype part belongs to `parts`, like every version 2 generator.

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
or 0, and a choice control as its index among its choices. `{name}` in a sub-item entry or a route's `from` attachment is replaced by the integer
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
A generator may also report features (`NativeGenerators::features`, none by default): edges in
the coordinates of its meshes, with surface normals, worked out from its parameters without
drawing, which a part's `zones` rules hang zones below as below a panel's (`kowloon/party`
reports `top`, `base`, `eave`, `sill` and `drip` on each face of a neighbour's wall or stub;
`kowloon/riser-duct` reports `fixture`, `fixing`, `rust` and `drip` on the wall plane, reaching
the item's receivers within 1 m, where its brackets and connections are fixed;
`kowloon/pedestrian-avenue` reports a closed `foot` ring on its paving plane round every
bollard, litter bin and bench support and round its planter, and `kowloon/street-lamp` its base
plate's outline, which reaches the paving of the item under it, within 0.5 m).

**Footprints.** A `points` control holds a plan polygon (3 to 64 `[x, y]` points,
counterclockwise seen from above) and a `flags` control up to 64 booleans. A repeat axis with
`"footprint": {"points": ..., "streets": ..., "cut": ...}` runs over the polygon's edges (edge i
from point i to point i + 1) instead of a count. Each copy reads `<name>_x`, `_y` (the wall's
start), `_dx`, `_dy` (its direction), `_length`, `_street`, `_next_street`, `_previous_street`,
`_cut`, `_previous_cut` and the cut corner at its end, `_cx`, `_cy`, `_cdx`, `_cdy`, `_clength`,
plus the footprint's bounds `_min_x`, `_min_y`, `_max_x`, `_max_y`. A convex corner between two
street edges is cut back `cut` along each (at most a third of either edge). `placement.along`
sets a part's X axis to a plan direction without trigonometry, so its front (-Y) faces out of the
footprint. The `prism` generator sweeps the cut footprint, moved `inset` inward, up by `height`
(face groups `side`, `top`, `bottom`). A clockwise footprint fails with `ITEM_FOOTPRINT`.

`footprint.holes` optionally names a numeric `grid` control containing enclosed plan loops.
Each loop has at least three XY points, with at most 64 corners across the footprint.
Holes accept either winding and must stay strictly inside the outer loop and apart from one
another. Their edges follow the outer edges in a repeat, with `_courtyard` set to 1 and each
part's front facing into the hole. The prism's top and bottom preserve the holes, and its
inner side faces look into them. A prism with holes uses a zero bevel.

For roof-mounted parts, a repeat axis can use `"footprintMode": "rectangles"` for several
nonoverlapping rectangles, or `"largestRectangle"` for one. On a concave or holed profile,
the rectangles fit inside the footprint inset by 0.2 m. `_min_x`, `_min_y`, `_max_x` and
`_max_y` give the chosen rectangle's bounds. `_shaped` is 1 for these profiles, and convex
footprints give one rectangle with the original plan bounds. On shaped street edges,
`_inward_depth` measures clearance to the nearest footprint boundary across that wall's
width, which can gate the placement of a deep interior kit.

A footprint with `"round": n` (2 to 16) cuts each such corner into `n` facets instead of one:
the chords of the arc that touches both edges where the cut starts and ends (on a square corner
its radius is the cut). `_facets` is their number at the edge's end (0 where it is not cut, 1
for a chamfer), and `"facet": k` chooses the one that `_cx`, `_cy`, `_cdx`, `_cdy` and
`_clength` describe, so a part repeated over a count and then over the footprint
(`[{"name":"f","count":"n"},{"name":"c","footprint":{..., "round":"n","facet":"f"}}]` with
`"when":"f < c_facets"`) stands on every facet. `_perimeter` and `_cperimeter` are the distance
round the cut outline to the wall's and to that facet's start, for patterns that run on.
`_miter_start`, `_miter_end`, `_cmiter_start` and `_cmiter_end` are the tangent of half the plan
turn at the wall's and the facet's ends (1 at a square corner), which a moulding mitres by. A
prism and a pavement given the same footprint follow the same outline. `_turn` is the plan turn
at the edge's end in degrees and `_cradius` the radius of the arc its cut touches both edges
with (0 where it is not cut): what an `arc` part placed at `_cx`, `_cy` (facet 0) along `_dx`,
`_dy` needs to turn the corner as one surface on the same chords.

**Plans.** A `grid` control holds nested lists (at most four deep, 4096 entries) of numbers
or, with `choices`, of texts among them: an explicit plan of what goes where. Expressions read it
with `cell(grid, i, j, k, l)`: a text's index among the choices, a number's value, and 0 where the
grid has no such element; `count(grid, i, ...)` is the length of the list there (0 where there is
none), so a plan can also say how many windows a storey has and where. `text` and `texts` controls
hold free text (up to 256 bytes, 64 of them), handed to a part by `{"element": texts, "at": expression}`. So a definition whose first choice is `auto` falls back to its seeded
rule wherever a plan is silent: `select(cell(plan, i) == 0, random(seed, i) < chance, cell(plan, i)
== 2)`. `{"element": grid, "at": expression}` hands one element (a row of a plan) to a sub-item,
or nothing where the grid has none, so the sub-item keeps its default.

**Features and weathering layers.** `features` places named polylines (`name`, `part`, `points`,
optional `face`, `normal`, `width`, `closed`, one-axis `repeat` and `when`) beside those the
generators report; a feature whose part was not emitted is dropped. `layers` maps a slot to
weathering layers, lowest first: `{"entry", "parameters", "seed", "follow"}`, a MaterialSpec layer
recipe whose named features `follow` maps to the receiver's (`{"top": "sill"}` makes the top
grime measure from sills). Layer parameters may bind controls. A sub-item's features join its
part in the parent, and its layers join the slots they land in, so a window's `sill` or an air
conditioner's `drip` reaches the wall it is on when both are in one sub-item. In a scene project a
slot with layers is bound to an assignment of its own, `<material>.<model>.<slot>`: the bound
material with the item's layers over the material's own, mapped as it is.

*Following several features.* A `follow` entry is one receiver feature or a list of 1 to 16:
`{"source": ["top", "sill", "drip"], "guard": ["eave-above", "sill-above", "drip-above"]}`. The
layer's name then holds the polylines of all of them, so `featureDistance("source")` is the
distance to the nearest and `featureTangent` the direction of the nearest, in one layer where
each source used to cost one of the eight a slot composes. A list is a set: it is kept ascending
without repeats and a list of one name is that name, so equal layers of sub-items still merge and
a layer that follows one feature keeps its identity and cache keys. The CPU bake, the field bake
and the features `shade_project` packs for a GPU host (`layerFeatures`) all read the union.

*Reach.* A receiver is given the features of its own top-level part: every sub-item under that
part, at any depth, so every `top` of a building side reaches every wall of that side, and a
query finds the nearest of them. An authored feature may reach further with `"reach"`:
`"part"` (the default), `"item"` (the receivers of every part of the placed top-level item it
ends up in, through any depth of sub-items) or `"scene"` (those of every item model of the scene
project as well): `{"name": "drip", "part": "run", "points": [...], "reach": "item", "within": 4}`.
The reach is bounded: a receiver outside the feature's part is given it only when its bounding
box comes within `within` meters of the feature's (3 by default, more than 0 and at most 50, in
scene meters; `ITEM_FEATURE` otherwise, and for a `within` without a wider reach), so a layer
should not query further than that. A receiver is given at most 256 such features from the other
parts of its item and 256 from other items, the nearest first; the item's metadata counts those
left out (`reachDropped`, `sceneReachDropped`). Generated features (`top`, `sill`, `joint`, ...)
reach their part; to carry one further, author a feature along it. The default leaves geometry
keys and every receiver's features as they were.

**Weathering zones.** An item says where runoff, rust and soot hang instead of a material
searching for edges: a zone is a feature's polyline (the edge) with coordinates of its own, `u`
meters along the edge and `v` meters below it along downhill, which a material reads with
`zoneCoord`, `zoneExtent` and `zoneSpan` (see the MaterialSpec program README). Nothing above an edge is in
its zone, so streaks always start at their edge and never climb.

- A part's `zones` rules hang zones below the features its generator reports:
  `{"name": "runoff-top", "below": "top", "face": "front", "length": 2.8, "u0": {"parameter": "u0"},
  "until": "openings", "clip": "none", "scale": 1, "seed": 0, "depth": 0.03}` (`below` names the
  generated feature: a panel's `top`, `sill` or `base`, an arc's `top` or a ring, or one a native
  generator reports; `face` keeps one
  face's; `name` defaults to `below`). `u` is `u0` plus the edge's start along the part's X axis,
  then meters along the edge, so storeys and sides placed with a footprint's `u0` (and an arc round
  a corner, by its chords) run on round a building. On a panel `until: "openings"` (the default)
  ends each column at the head of an opening below the edge, and `clip: "openings"` instead leaves
  the opening's rectangle out and lets the zone go on below it. A rule named after its feature puts
  the zone on that feature; otherwise a copy under the zone's name carries it.
- An authored feature carries one with `"zone": {"length", "down" ([0, 0, -1]), "depth" (0.03),
  "u0", "scale", "seed"}`: an air conditioner's `drip`, a balcony's `ledge`, a sign's fixing, a
  window's `runoff-head` down its frame's face and `runoff-glass` down the glass behind it (each
  on its own plane, which the window's `head`, `jamb` and `glass` features also state). With
  `reach` it lands on the receivers of other parts (a sign's fixing on the wall behind it).
- A zone holds the receiver points within `depth` meters of the plane of its edge and `down`, on a
  face that faces across it, below the edge, within `length` and the column's stop. Where zones of
  one name overlap, the one whose edge is nearest above wins. A zone's identity (`id` in [0, 1))
  is hashed from where its edge starts in the world, its name and its seed.
- Zones are features: they reach receivers as features do, a layer follows them by name or list
  (`"follow": {"zone": ["runoff-top", "runoff-sill"]}`), and distance queries still measure to
  their edges. They are part of a part's geometry key and of every bake's layout key. Limits: 16
  rules a part, 256 stops and holes a zone, 16384 zone edge segments a receiver. `u` and `v` are
  meters only under rigid placements.

**Slot densities.** `densities` maps a slot to the texels per meter it asks for (1 to 100000):
`{"lettering": 800}` for a sign, less for stacked goods. It is a default the generated item
reports (`densities`), not part of its geometry key; whatever its host states comes first. A
sub-item's densities join the slots they land in, as its layers do: the parent's own statement
for a slot holds, else the greatest any sub-item asks for it. In a scene project a receiver is
baked at the first density stated of: its mesh id in the project's `texelDensities`, its model's
`slotDensities`, the item's `densities`, its material's id in `texelDensities`, and the project's
`texelDensity`. The density is part of the receiver's layout, so every bake of it is keyed by it.

Composed examples are in `worldspec/themes/kowloon/items/`: small items (`kowloon-window`,
`kowloon-aircon`, `kowloon-shopfront`, `kowloon-parapet`, `kowloon-duct-run`, `kowloon-duct-riser`
and the strap or hanger bracket both stand on, `kowloon-duct-bracket`, `kowloon-roof`), storeys made of them (`kowloon-floor`, `kowloon-ground-floor`), a building side
that stacks storeys and adds its services (`kowloon-facade-side`), and `kowloon-building`, which
places a side along every edge of its footprint, shops on street edges, back doors and pipes on
the rest, and a cut corner with a sign between two streets (`cornerStyle` `chamfer`, or `round`:
one `kowloon-corner`, whose wall, plinth, tile bands, string courses and coping are arcs of
`cornerSegments` chords, with a `kowloon/pavement-corner` round it; `cornerTiles` runs the tile
band through it). Its `arris` rounds every other convex corner and the shop piers' edges.
To build a C or H shape, trace it in the building's `footprint` control. To build a ring,
set `courtyards` to a list of enclosed XY loops. The same facet, window, trim and roof
recipes continue around all edges and roof wings. Set `groundShops` to `false` for a
residential first storey.

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
