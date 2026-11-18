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

In `worldspec/themes/kowloon/items/`, checked by `tests/kowloon_facade_item.rs`:

- `kowloon-duct-riser`: a galvanized riser spanning `floors`, seams and wall brackets spaced
  by the route's `every` rule, a branch into each floor, an elbow into the wall at the roof and
  a hood at its foot. Pure ItemSpec (routes and boxes).
- `kowloon-facade-side`: one side of a building. A wall panel per floor with a row of framed,
  glazed windows (or plain openings with `kowloon/window`, by `kowloonWindows` and
  `windowKind`), string courses, parapet and coping; ground-floor shop bays; air conditioners,
  condensers, wall vents and security bars chosen per bay by `seed`; a downpipe with its roof
  outlet; and the riser as a sub-item when it has two floors or more.
  With `ductRun` a galvanized duct runs under the parapet from the riser, with a tee into the
  wall at each bay; `ducts` adds up to that many industrial riser ducts (`kowloon/riser-duct`)
  on the piers between bays.
- `kowloon-building`: four sides as sub-items around a footprint, the roof slab and the
  Kowloon rooftop (stair hut, tanks, aerials). With `ducts` (the default) the front, right and
  back sides have risers and duct runs, and the front and back industrial riser ducts.

The scene's lane (`LANE=1`, from `treespec_batches::write_lane_plants`) paves the ground under
the building and the lane, lays a patch of the washroom lawn's painted grass toward the lane's
open end and stands three TreeSpec broadleaf trees and two shrubs in front of the building.

`tests/studio/washroom-items/build.mjs` places the side (`FACADE=1`) or the whole building
(`FACADE=building`) across the lane beyond the washroom's low wall, and
`tests/item-facade-browser.mjs` (`MODEL=building`) pictures it in the built studio. Reference
renders: `itemspec/facade-side-front.png`, `itemspec/building-front.png`,
`itemspec/building-angle.png`.

### Known limits

- Native fixtures are unweathered: Kowloon's weathering bakes are not run for an item. Fixtures
  whose metal always goes through weathering get its recorded paint as vertex colors, but a
  scene project places no vertex colors (its materials shade every receiver), so their look
  comes from the bound project materials. Atlas-textured slots (lettering, posters, shop timber)
  need atlas materials bound to show their art.
- The side's ground floor is one story high and drawn in front of a solid wall; open shop
  fronts need an opening behind them.
- Sub-item decals, features and pattern anchors are not carried into the parent yet.
- Every sub-item entry is resolved against one root (the project, or the top definition's
  directory); entries are not relative to the sub-item that names them.

## Phase 2 (next)

1. Move fixture geometry out of the Kowloon theme into generic ItemSpec code where it has no
   theme state (windows, AC units, ducts, downpipes), keeping parity, so other themes reuse it.
2. Weathering for items: run the fixtures' weathering receivers as item material layers, so a
   native fixture looks as it does on a Kowloon facade.
3. Let the Kowloon facade planner place items: its cells choose `kowloon/window` and
   `kowloon/aircon` items rather than drawing them, so WorldSpec and authored items share one path.
4. Taller ground floors, balconies and stair cores on the building side; corner treatment where
   two sides meet.
