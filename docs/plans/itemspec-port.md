# Porting the procedural systems to ItemSpec

Status: phase 1 done, pending review (2026-11-18).

ItemSpec is the soft module that makes procedural props from parameters: ducts and pipes of
any length and shape, windows, air conditioners, a choice among pottery, and so on. WorldSpec
stitches items into a scene. An item can hold sub-items, so a wall section adds its own
windows and fixtures, and a wall spanning several stories adds the duct that connects them.

## Phase 1: ready ItemSpec, port the duct system and one building side

### What ItemSpec gained

See `crates/engine/src/itemspec/definition/README.md` (Composition) for the contract.

- **Sub-items.** A part `{"kind":"item","entry":...}` places another item definition, with
  arguments (`{"parameter"}`, `{"derived"}`, `{"expr"}`, `{"choose","by"}` or literals), a
  choice of entries by `pick`, slot renaming, and its attachments exposed as `<part>/<name>`.
  Materials of unassigned slots, imported materials and images come along. Sub-items nest up to
  8 deep; cycles fail with `ITEM_SUB_ITEM`. A changed sub-item rebuilds only its part.
- **Repeats and conditions.** Parts repeat over up to three named axes and are kept by a
  `when` expression; panel openings repeat along one axis and have a `when` too. An item
  expands to at most 512 parts.
- **Expressions everywhere.** Any scalar may be inline expression text. The grammar gained
  comparisons, `%`, `clamp`, `select` and `random(...)`, a bit-exact hash in [0, 1), so a seed
  chooses fixtures per bay the same way on native and Wasm. Boolean controls read as 1 or 0.
- **Native generators.** `{"kind":"native","name":...}` calls a generator its host supplies
  (`NativeGenerators`, through `Environment::with_natives`). Generic ItemSpec still imports no
  theme: the shipped host (`worldspec::shipped::item_generators`) registers the Kowloon fixtures.
  The studio session, scene projects (sub-items read the project's modules first) and
  `superspec-studio inspect` all use it.

### The Kowloon fixtures as items

`worldspec/themes/kowloon/items.rs` registers 48 fixtures as `kowloon/<name>`, each drawing the
same code the facades use, unweathered, into ItemSpec meshes (one per material slot) in the
panel convention: X right seen from the front, Z up, front toward -Y, wall plane y = 0. Each
generator documents its anchor and parameters; `tests/kowloon_items.rs` checks every one.

| Group | Generators |
| --- | --- |
| Openings and wall hardware | `window`, `security-bars`, `wall-vent`, `roof-outlet`, `wall-lamp`, `meter-bank`, `roller-shutter` |
| HVAC and services | `aircon`, `condenser`, `riser-duct`, `downpipe`, `pipe-elbow` |
| Balconies and household | `balcony`, `anti-climb`, `plant`, `plants`, `cage`, `drying-rack`, `drying-pole`, `drying-lines`, `awning`, `tarp`, `washing`, `hangers` |
| Ground floor | `entrance`, `ground-bay` (shutter, gate, door, shop, service) |
| Roof | `water-tank`, `stair-hut`, `aerial`, `rooftop-shack`, `rooftop` (a whole roof's equipment) |
| Signs | `sign`, `blade-sign`, `hung-sign` |
| Shop interiors | `grocer-prop`, `shelving`, `counter`, `crate`, `bottle-crate`, `counter-scale`, `price-card`, `pendant-lamp`, `ceiling-fan`, `tube-light`, `wall-clock`, `calendar`, `hanging-scale`, `stool` |

Not ported, because they need whole-facade or room planning state rather than parameters:
facade cells and plans, the facade's drain routing and industrial-duct placement, detail bands,
plinths and envelope, building massing, and whole shop rooms (grocer, restaurant, herbalist,
hardware layouts). A single balcony garment, door hardware pieces and shop rails and hooks are
small omissions.

### Composed items

In `worldspec/themes/kowloon/items/`, checked by `tests/kowloon_facade_item.rs`. A building is not
one definition: each piece is an item of its own that takes parameters and is reused by the
larger ones.

| Item | What it is | Built from |
| --- | --- | --- |
| `kowloon-window` | a window in its opening, optional security bars; reports its `sill` | `kowloon/window`, `kowloon/security-bars` |
| `kowloon-aircon` | a window unit or a split condenser; reports where it `drip`s | `kowloon/aircon`, `kowloon/condenser` |
| `kowloon-shopfront` | a ground bay (shop, shutter, gate, door, service) with its signs, an awning, a lamp | `kowloon/ground-bay`, `kowloon/awning`, `kowloon/wall-lamp` |
| `kowloon-floor` | one storey: a wall panel with a row of openings (each kept by `windowChance`), windows, AC units, vents, racks, plants, a band; grime under its top, sills and drips | panel, `kowloon-window`, `kowloon-aircon`, natives |
| `kowloon-ground-floor` | the street storey: openings and shopfronts, a fascia; ground stain and moss | panel, `kowloon-shopfront` |
| `kowloon-parapet` | parapet, coping, roof outlet; top grime | panel, box, `kowloon/roof-outlet` |
| `kowloon-duct-riser`, `kowloon-duct-run` | a riser over the floors with branches and an elbow; a run under the parapet with a tee per bay | routes, boxes |
| `kowloon-roof` | roof slab and the Kowloon rooftop's equipment | box, `kowloon/rooftop` |
| `kowloon-facade-side` | a side: ground storey (shops, or a floor), upper floors, parapet, downpipe, riser, duct run, industrial ducts, back door, extra pipes | the items above |
| `kowloon-building` | a building from any footprint polygon: a side along every edge, cut corners with signs between streets, a prism roof cut to the footprint, the rooftop | `kowloon-facade-side`, prism, natives |

`kowloon-building` takes `footprint` (counterclockwise plan points) and `streets` (one flag per
edge). Street edges get shopfronts, awnings, ribbon windows and signs. The other edges get a back
door, few small windows (`windowChance` 0.35), barred windows, racks, condensers, three extra
downpipes, a riser and a duct run. A convex corner between two street edges is cut back `cut`
(0.24 m) along each: a 45 degree cut on a square corner, faced by a narrow side with a blade sign.
Concave corners and corners with a back edge stay square. Its defaults are the reference shop
house: 9.8 by 7 m, streets on the front and right, four storeys (3.35 m, then 3 m) and a 0.75 m
parapet.

### Weathering: grime where the items say

Following the 2026-11-16 decision that grime is its own appearance, applied like a decal, the
render is an intrinsic base (`kowloon-concrete-anchored`). The paint and the grime are weathering
layers that the items declare on their `wall` slot. Every wall's list starts with the same
`kowloon-paint-coat` layer (a new shipped recipe: a colour, `paint`, mottled and peeled to the
render beneath in warped drifts, `peeling`). Over it come grime layers, each following a feature
the item reports:

- floors: top grime under the storey's top edge, under every window's `sill` and under every AC
  unit's or condenser's `drip`;
- ground floors: ground stain and moss along the `base`, and grime under awnings;
- parapets: top grime under the coping.

An author changes where grime goes by adding a feature (an authored polyline) and a layer that
follows it, with its own parameters. The Studio composes such a slot's layers in its GPU bake. It
previously took layer values only from the project's own material list. It now also takes the
values of the assignments the engine derives for weathered item slots: the base material's
parameters, and the layers the engine lists for that assignment.

### The reference shop house

The building in the reference picture is the authored `model="reference-corner"` of the AML
dressing (`building/reference_corner.rs`, `reference_details.rs`). `kowloon-building`'s defaults
reproduce it procedurally rather than as a fixed model:

- the 9.8 by 7 m footprint, with streets on the front and right and their corner cut 0.24 m;
- a 3.35 m shop storey and three 3 m storeys under a 0.75 m parapet;
- wide, tall ribbon windows in green frames with transoms, over green tiled spandrels;
- a first-floor balcony on the long street side, window cages, AC units and condensers;
- a provisions shop, a gate and a hardware shop under awnings, with blade and hung signs;
- the cream paint coat peeling to grey render, and the rooftop's hut, tank and aerials.

What goes where is the seed's choice, so a seed and its inputs always give the same building.
Plans override it per bay: `windowsPlans` and `fixturesPlans` (edge, storey from the ground,
bay), and `shopPlans` and `awningPlans` (edge, shop bay). A cell that is missing or `auto` keeps
the seeded choice. `tests/studio/washroom-items/reference-shop-house.json` plans the reference
picture: on the front, a gate and two shops; on the first floor, the balcony and no fixtures;
on the second, two cages and a split unit; on the third, a window unit and a split unit. On the
right side, a gate and the hardware shop, with split units in the bay by the corner. Render:
`itemspec/shop-house-planned.png`.

Renders: `itemspec/shop-house-corner.png` (the lane's plants hidden) and
`itemspec/shop-house-lane.png`. The building's balcony parapet is solid concrete where the
reference has a steel railing. Its sign copy is seeded, not the reference's.

### The scene

`tests/studio/washroom-items/build.mjs` places the side (`FACADE=1`) or the building
(`FACADE=building`) across the lane beyond the washroom's low wall. With `LANE=1` (written by
`treespec_batches::write_lane_plants`) it paves the lane, lays a patch of the washroom lawn's
painted grass toward the lane's open end, and stands three TreeSpec broadleaf trees and two
shrubs in front of the building. `tests/item-facade-browser.mjs` (`MODEL=building`,
`BUILDING_PARAMETERS`, `RESOLUTION`, default 1024, and `TEXELS`, default 256 per meter) pictures
it in the built studio. The earlier low concrete detail came from baking at 256 pixels per
receiver, about 21 per meter on a 12 m wall.

### Known limits

- Native fixtures are unweathered: Kowloon's weathering bakes are not run for an item. A scene
  project places no vertex colors, so their look comes from the bound project materials.
  Atlas-textured slots (lettering, posters, shop timber) need atlas materials bound to show
  their art.
- A feature reaches only the receivers of its own top-level part: an AC unit's drip stains the
  wall of the floor it is in, but a duct run's would not reach the floors below it.
- Back doors stand in front of the ground storey's wall, not in an opening of it.
- Sub-item decals and pattern anchors are not carried into the parent yet.
- Every sub-item entry is resolved against one root (the project, or the top definition's
  directory); entries are not relative to the sub-item that names them.
- The rooftop is a rectangle 0.6 of the footprint's bounds at their middle, which suits convex
  footprints; an L-shaped block wants a deck per wing.
## Phase 2 (next)

1. Move fixture geometry out of the Kowloon theme into generic ItemSpec code where it has no
   theme state (windows, AC units, ducts, downpipes), keeping parity, so other themes reuse it.
2. Weathering for items: run the fixtures' weathering receivers as item material layers, so a
   native fixture looks as it does on a Kowloon facade.
3. Let the Kowloon facade planner place items: its cells choose `kowloon/window` and
   `kowloon/aircon` items rather than drawing them, so WorldSpec and authored items share one path.
4. Balconies and stair cores on the building side; a rooftop deck per wing of a footprint; a
   feature that reaches neighbouring parts (a duct run's drip on the floors below).
