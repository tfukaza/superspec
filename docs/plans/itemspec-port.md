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
| Street and facade details | `shop-room`, `street-stock`, `pavement`, `pedestrian-avenue`, `street-lamp`, `street-cart`, `cornice`, `conduit`, `balcony-door`, `rail`, `ladder` |
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
| `kowloon-sign` | a complete sign, usable alone: a lettered or panelled board on outriggers with fixing plates, braces and a tie rod (`board`), the seeded projecting signs (`blade`, `hung`) or a framed wall panel (`flat`); size, words, orientation, colour scheme, angle and reach are controls; reports its `top` edge and wall `fixing`s | `kowloon/board-sign`, `kowloon/blade-sign`, `kowloon/hung-sign`, `kowloon/sign` |
| `kowloon-shopfront` | a ground bay (shop, shutter, gate, door, service, or an `open` shop room with its trade's stock inside and on the pavement) with a fascia sign, an awning (bracketed, a tarp on poles, or a striped canvas with a valance), a lamp | `kowloon/ground-bay`, `kowloon/shop-room`, `kowloon/street-stock`, `kowloon-sign`, `kowloon/awning`, `kowloon/tarp`, `kowloon/wall-lamp` |
| `kowloon-floor` | one storey: a wall panel with a row of openings (each kept by `windowChance`), windows, AC units, vents, racks, plants, a band; grime under its top, sills and drips | panel, `kowloon-window`, `kowloon-aircon`, natives |
| `kowloon-ground-floor` | the street storey: openings and shopfronts, a fascia; ground stain and moss | panel, `kowloon-shopfront` |
| `kowloon-parapet` | parapet, coping, roof outlet; top grime | panel, box, `kowloon/roof-outlet` |
| `kowloon-duct-riser`, `kowloon-duct-run`, `kowloon-duct-bracket` | a riser over the floors with collared branches and an elbow, 6 cm off the wall on a strap bracket about every 1.3 m; a run under the roof's edge on hangers with a collared drop about every bay | routes, boxes, `kowloon-duct-bracket` |
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

Four differences from the picture were then closed, and the sun turned to the other side:

- **Railing:** `kowloon/balcony` takes `rail: steel`, the reference's three green rails on posts over a
  low upstand, instead of the solid parapet (the side's default).
- **Signs:** `kowloon/board-sign` is a lettered sheet sign on two outriggers with its own text, red or
  ivory, vertical or horizontal, square to the wall or at an angle. A building's `boardPlans` list
  them per edge (`[u, z, width, height, red, horizontal, angle, reach, anchor, end, text]`) and
  `boardTexts` holds the copy. Placed items now keep their vertex colours, and a sign's `lettering` and
  `board` slots are the shipped catalog materials (the glyph atlas and the board), so the studio
  passes the catalog images a project supplies on to the engine.
- **Windows:** `spansPlans` (edge, storey, bank, `[from, to]`) place windows in banks of any width, as
  in the reference model; without it the bays are even.
- **Paint:** the coat's patches are about a third the size (`Patch frequency` 2.4 per metre).
- **Camera and sun:** the `reference` view of `tests/item-facade-browser.mjs` stands at the cut corner, 22
  degrees off the front, 8 above the ground, 62 m away through a 26 degree lens, with the trees hidden;
  the sun is high (55 degrees) from the front-left (azimuth -25), so the wide face is lit and the narrow
  one turned from it. Render: `itemspec/shop-house-reference-view.png`.
- **Sun and shade:** the same script softens the sun, in the scene's global lighting record rather than in any material: intensity 3 to 2,
  shadow `softness` 0.05, `spread` 0.09 and `maxSoftness` 0.3 (the edge widens with the distance to the
  caster), `darkness` 0.2, a bluer `tint` at `tintStrength` 0.5, and interior `noise` of 0.5 over 0.6 m
  patches (`SUN=<azimuth>,<elevation>` sets the direction, `LIGHT_SUN` is JSON merged over the sun,
  `LIGHT=keep` leaves the scene's own lighting).

Renders: `itemspec/shop-house-corner.png` (the lane's plants hidden, before these fixes),
`itemspec/shop-house-lane.png` and `itemspec/shop-house-planned.png` (after them).

### Props and details (2026-11-18)

The reference picture's content that the first port left out, as generators and controls of the same
items. Nothing is particular to the corner shop: the plan file names what goes where.

- **Open shops.** A shopfront of kind `open` is a room behind the opening (`kowloon/shop-room`: floor,
  ceiling, walls, the raised shutter's head, and its trade's furniture and goods: `provisions`, `grocer`,
  `greengrocer`, `hardware` or `herbalist`) with stock on the pavement (`kowloon/street-stock`: stacked
  plastic crates and cartons, wire racks, a sack truck, buckets and brooms). The provisions shop is fitted
  to any room and packed (`shop::provisions::furnish_fitted`): shelves to the ceiling on three walls, a
  display rack inside the opening, goods hung across it. Its goods and the street stock are low-detail
  (a few faces each, no bottoms or backs): a 6 m shop is 21 000 vertices where the baked goods were
  164 000, its street stock 4 000 for 22 000, so about one seeded bay in four is an open shop. A ground
  floor's `shopSpans` place shops of any width, `tradePlan` names their trades and `shopDepth` their depth.
  At a corner of two streets the side that arrives keeps its rooms and the side that leaves yields: its
  bays within `shopDepth` of the corner are only as deep as `cornerClear` (how far the first side's shops
  stay from the corner, 0.5 m) leaves free, and stay shut where that is under 2.4 m. `clutterPlan` names
  what stands by each bay (`none`, `auto`, `crates`, `cartons`, `mixed`, with `-left` or `-right` for the
  jamb) and `gateColor` paints the gates' lattice. The goods' meshes are in a `stock` slot of their own,
  coloured per vertex and faded (`fade`), the room's floor in `shop-floor` and the provisions shop's lit
  tubes in `tube-light`: a project leaves these unbound (plain) rather than baking a material over
  thousands of small pieces, or binds an emissive material to the tubes.
- **Awnings.** `awningPlan` chooses `awning` (the bracketed household canvas), `tarp` (a sagging canvas on
  two poles) or `striped` (the fitted canvas with stripes and a valance). `kowloon/awning` and
  `kowloon/tarp` take `poles` and `stand`.
- **Signs.** `kowloon-sign` (above) is the only sign a building places: its boards (`boardPlans`, whose
  fifth column is a scheme index: ivory, red, green, blue, yellow, black, white, and whose twelfth is a
  number of coloured squares), its corner and street signs and each shopfront's fascia sign. The seventh
  column sets the board's angle from the wall. Use 90° for a board that projects squarely into the street.
  The sixteenth column selects a design (0 plain, 1 hardware, 2 grocery, 3 electrical, 4 wholesale,
  5 pharmacy). The optional seventeenth column indexes a subtitle in `boardTexts`. A design supplies
  the casing, trade emblem and default lettering face. Column 15 or the item's `face` control can
  override that face. Both the City ItemSpec and AML reference corner read the same shipping preset.
  The shaped casings, trade emblems and bilingual lettering draw on Hong Kong's shop-sign traditions.
  The boards use original procedural artwork and the shipped font atlas.
  [M+ Nam Wah Neon archive](https://www.mplus.org.hk/en/collection/archives/nam-wah-neon-company-archive-ca13/),
  [M+ China Neonlight archive](https://www.mplus.org.hk/en/collection/archives/china-neonlight-advertising-company-archive-ca14/),
  [PolyU study of vernacular Hong Kong neon signs](https://ira.lib.polyu.edu.hk/bitstream/10397/92486/1/Kwok_Vernacular_Design_History.pdf).
- **Facade.** Floors are divided by a moulded string course (`kowloon/cornice`, mitred at the corners by
  `miterStart` and `miterEnd`) in place of a box; the parapet has one at the roof line (`ledge`). Street
  walls stand on a dark `plinth` (a slot of its own, 1.02 m). `pipePlan` stands drain pipes where a plan
  says and `conduitPlan` clips conduits with junction boxes to the wall (`kowloon/conduit`; without a plan
  `conduits` are seeded). `canopyPlan` (or `canopyChance`) puts corrugated hoods over window banks. Split
  units have their refrigerant lines and every unit its drain hose.
- **Balcony.** `balconyPlans` (per edge: from, to, depth, fan) sizes it; it has triangular brackets
  beneath (`brackets`), the fan-shaped divider (`kowloon/anti-climb`), potted plants, and a door
  (`doorsPlans`: `kowloon/balcony-door` in an opening of that storey's wall).
- **Windows.** `kowloon/window` takes `bars` (glazing bars across each lower light) and `glazing` (false
  leaves the panes out, so the sashes, curtains and the room show in depth).
- **Roof.** `roofKit` (the default; `rooftop` remains for the seeded deck) is the reference's roof: the
  stair hut with a cat ladder, a round tank on its stand, `aerials` (2) and a rail on the street parapets.
- **Ground.** `pavement` lays a raised pavement along every street edge (`kowloon/pavement`: paving
  stones breaking joint on a bed, kerb stones, an iron drain cover) round the cut corners, over a podium
  under the footprint. The item's z = 0 is the pavement's top and the road is `kerbHeight` (0.15 m) below,
  so a scene stands the building that much above its road.

- **Bake allowance.** A storey's wall no longer has an inner face (its `back` group is not emitted: a
  building's rooms draw their own walls, and windows show a dark pane), which took the reference building's
  baked maps from 6.2 GiB, over the preview's 6 GiB allowance, to 5.0 GiB. A native generator leaves out
  triangles without area, which a scene refuses as receivers (a striped canvas had some).
- A back side's riser stands at the end away from a street corner.

**A second plan.** `tests/studio/washroom-items/small-warehouse.json` builds the small three-storey
warehouse of the second reference picture from the same building item: a 6 by 6 m box with square corners
(`cut` 0), a roller shutter and a door over a tiled `dado`, a number plate (`plaquePlans`: flat signs),
full window cages (`fullCages`) with a corrugated hood, four lettered blade signs, a drain with a
`hopper`, a tiled roof deck (`roofTiles`:
`kowloon/tile-deck`, slot `roof-tile`), and large paving slabs on all four sides (`pavementAround`,
`pavingStone`, `manholes`). `services: false` leaves the back sides without their riser and ducts. Further
controls from the same round: `spandrelPlans` (which window banks have tiled spandrels), `awningLower`,
`roofRailShare` (the rail covers part of each street parapet), `shutterColor` and `doorColor`, and the
balcony's quarter fan (`kowloon/fan-divider`). String courses are thin (0.11 m proud).

A later round planned the rest of that picture, again as controls of the same items (each absent control
leaves the seeded result as it was):

- **Entrances.** `door` (`auto`, `single`, `double`, `glazed`, `steel`) and `transom` (`auto`, `glazed`,
  `mesh`, `none`) name a door bay's entrance (`doors::Design`, read by `kowloon/entrance` and
  `kowloon/ground-bay`); a named door is painted `doorColor`.
- **Fitted bays.** `fittedBays` cuts each planned door's wall opening to the door (2.5 m) and has the
  bay's fixture fill its opening (`kowloon/ground-bay` `opening`): a shutter stands closed between guides
  at the span's edges, in `shutterColor`, under a head box of `shutterHead` metres rusted by
  `shutterHeadRust`; a door's frame takes the span and its transom the head. `meters: false` leaves the
  meter banks between bays out.
- **Cages and storerooms.** `cagePlans` (per edge, storey and bay: `auto`, `plants`, `washing`, `storage`,
  `mixed`, `empty`) names a cage's contents (`kowloon/cage` `contents`; planned storage is stacked
  cartons), and on a building of `fullCages` the `canopiesPlans` cell roofs the cage itself (`roof`)
  instead of hooding the window. `storagePlans` (per edge, a share per storey) stacks cartons behind a
  storey's glass (`kowloon/window` `storage`); `windowTransom` places the windows' transom bar.
- **Walls.** `bandsAround` runs the string courses round every side, mitred at the square corners, and
  replaces the shops' box fascia by the same course. `vents: false` and `ventPlans` (per edge: x, z, and
  optionally width and height) place wall vents by plan; a sixth `conduitPlans` column is the width of a
  junction box at the conduit's end (`kowloon/conduit` `endBox`). `backPipes` (3) and `downpipes` (true)
  are the back sides' seeded pipes and every side's roof downpipe; with `windowsPlans` and
  `fixturesPlans` rows for the back edges a plan leaves a side wall bare.
- **Camera.** A plan's `"/view"` key (`azimuth`, `elevation`, `fov` in degrees, `distance` and target
  `height` in metres) replaces the `reference` view of `tests/item-facade-browser.mjs`.
- **Shop goods.** The fitted shop's wares are two-tone at the same low detail: tins with a band, jars
  with a label under what they hold, bottles with a label band and a cap, boxes and bags with a printed
  panel. Goods hung across an open shop's front (`kowloon/shop-room` `hangClear`, `hangDrop`) are strings
  of packets, bunches of carrier bags, tied sacks, cloths, brushes and cards of small wares on cords of
  varied length; a shopfront with an awning passes the awning's reach as `hangClear`, so they hang
  beneath its edge, and the way in at the middle keeps 2 m of headroom. A shop's street stock has one
  front row of plastic crates, and buckets, a stool and brooms stand by the way in.
- **Gates.** `gateBars` widens a gate's lattice bars (0.5 to 3 times) and `gateBacking` closes sheet
  steel leaves behind it (the lattice's paint times this factor; 0 shows the timber doors), so a
  galvanised gate reads grey from across the street (`ground::Plan::gate_bars`, `gate_backing`).
- **Doors.** `doorPanels` (1 to 4; 0 leaves the door's three) is the number of pressed panels of each
  named leaf (`doors::Design::panels`). `backDoorColor` paints a back side's service door apart from
  `doorColor`, and `backDoorLouvres: false` leaves its leaf plain. An unbaked seeded leaf keeps its
  paint (its surface receiver's paint tints it, as a metal receiver's does). Cartons behind a storeroom
  window (`storagePlans`) stand under half its height, in the room's shade.
- **Cloth.** `kowloon/sagging-tarp` (`items/cloth.rs`) is a sheet hung between wall eyelets and pole tops:
  every edge hangs between its fixings by `sag`, the field bellies, sharp creases run out from every
  fixing (shaded in their troughs by the cloth's tints), a slack sheet gathers in a few seeded
  round-bottomed folds that deepen toward its hem, ropes tie it and a stay ties each post back to the
  wall. With `frame` it is a taut awning on a light frame, with `stripes` bands and a `valance` whose hem
  is cut in `scallops` and bound. `kowloon-shopfront` uses it for its `tarp` and `striped` awnings
  (`awningSag`, `awningStripes`; a stripe is 0.18 m wide unless counted), which a building plans per
  edge and bay (`awningSagPlans`, `awningStripePlans`; 0 keeps the default). `kowloon/sheet-canopy` is a window hood of lapped corrugated sheets, each with its own seeded
  fall, rusted from the drip edge; `kowloon-floor`'s `canopy` projects it 0.5 m.
- **Wires and hardware.** `kowloon/cable` (`items/hardware.rs`) strings a bundle of `count` cables along
  a wall, each hanging by `sag` between clips; `out` takes its far end off the wall to a sign's pole or
  an air conditioner, and `corner` with `cornerRadius` clips it round the end of the wall and on along
  the next. A side places `cablePlan` rows `[x0, z0, x1, z1, sag, out, count, corner radius, boxes]`
  (`kowloon-building` `cablePlans`, one list per edge), or `cables` seeded ones above its string courses
  where no plan is given, the first rounding a street corner. `kowloon/junction-box` stands alone; a
  cable's `boxes` adds them at its fixings. `kowloon/downpipe` `clips` (a side's `pipeClips`) bolts
  two-eared clips round a drain; `meterPlans` (`[x, z, meters]` per edge) places meters where the seed
  would not. A horizontal `kowloon/board-sign` hangs by two straps from one pole with two stays, and
  every sign fixing is a plate with bolts.
- **Neighbours.** `partyPlans` (one per edge: `none`, `wall`, `stub`; street edges ignore it) stands
  `kowloon/party` (`items/party.rs`) against a party edge. `wall` replaces that side by a neighbour's
  plain stained wall 8 cm proud of it and a little past its ends, seeded somewhat taller or shorter than
  the parapet, under a coping (its roof rail goes too). `stub` keeps the side behind a slice of the
  adjoining building 1.1 to 1.9 m deep: storeys of its own (`partyStoreyPlans`, 0 for a seeded few about
  the building's), rows of windows (some with units), string courses, a drain and a parapet; an end
that faces a street (`gable`) has columns of windows across its depth, a soil stack and vents. A party
  edge has no seeded back door or cables. Either is a few hundred faces.
- **Details.** `conduitColor` paints every conduit; `plaquePlans`' seventh column names a plate's
  lettering face (the small warehouse's number is `heavy`: thin strokes vanish from across the street);
  a rusted shutter head box (`shutterHeadRust` > 0) is streaked on its face, rust running down from its
  top over patches of grey.
- **Roof.** `roofRailPlan` gives each edge's parapet its share of rail (0 for none) in place of the
  seeded street rule. `roofClutter` (0 to 1) scales `kowloon/roof-clutter`: vent stubs with cowls or
  goosenecks along `roofVentSide`, pipes on sleepers along `roofPipeSide`, pots along `roofPotSide`, and
  a cable from the first aerial to the stair hut.
- **Drawn weight at street level.** Nothing linear on the street is thinner than about 1.8 cm (cables
  and hems about 1.5 cm), and counts fall where thicker pieces would clog. Paving and kerb stones have
  1.4 cm joints and paving stones a dark 8 mm chamfer into them (`street.rs` `paving_stone`), so a
  joint is a drawn line; drain and manhole covers are four heavy bars and the avenue's slot drain 6 cm
  bars every 0.25 m; benches have two 6 cm slats; bollards are 16.4 cm with a broad band; lamp arms,
  braces and scrolls, bicycles (fat tyres, six spokes, 4 cm tubes) and hand carts are heavier. A
  roller shutter's slats are 0.15 m apart and 2.9 cm deep (`slat`; `shutters::slatted_roller_shutter`)
  and a gate's lattice 2.2 cm uprights 0.22 m apart (`gateSpacing`; `ground::Plan::gate_lattice`):
  only the item generators ask for these, so the older Kowloon models draw as before. A sheet
  canopy's corrugations are 0.14 m apart; a hung cloth's hems and ropes are 1.5 to 1.6 cm thick, its
  posts 6.8 cm and stays 2.6 cm; hung goods' cords are 1.8 cm strips; rack posts, brooms and awning
  poles are stouter. Cables are 1.5 cm and conduits 2.5 cm across.
- **Drawn weight on the facade.** The facade items draw their fine members a little heavier than life,
  so each reads as a hand-drawn line, but no heavier: linear members about 1.8 to 2 cm at least,
  wires 1.4 cm, cage bars 1.8 cm, glazing bars 2.5 cm and window frame faces 4 cm, with fewer pieces
  where heavier ones would clog. A window's frame, mullions, glazing bars and handles are 4, 3.6, 2.5
  and 1.8 cm (`frameWidth`, `mullion`, `glazingBar`, `handle`); security bars 2 cm in a 3 cm border,
  1.6 times the facades' spacing; cages 1.8 cm bars on 2.8 cm rails about 0.22 m apart (`cageBar`,
  `cageRail`, `cageSpacing`); a balcony's steel rails 4 cm on 4.6 cm posts; the roof rail 3.6 cm on
  4 cm posts; a fan divider nine 2 cm bars under a 4 cm rim; the hut ladder 4 cm stiles and 2.4 cm
  rungs; an aerial a 4 cm mast, 2.6 cm boom, four 2 cm elements and 1.4 cm guy wires; a wall lamp's
  guard 1.8 cm wire; a board sign 4.4 cm outriggers, 3 cm braces and 1.8 cm tie rods, and a
  projecting sign's rods and flat bars 1.6 times their own. Signs letter in `heavy` where an `auto`
  face's glyphs are 0.35 to 0.5 m tall, and in `kai` otherwise (`heavy` fills in on small boards and
  plates). A `boardPlans` row's fifteenth column names a face (0 `auto`, 1 `kai`, …, 5 `heavy`); the
  reference plan letters its small 電器修理 board in `kai`. The shared Kowloon drawing code takes these as options
  (`windows::Weight`, `windows::Gauge`, `CagePlan::bold`, `rooftop::Aerial`, `panel::Art`,
  `Proposal::gauge`) whose defaults draw the older models exactly as before.

ItemSpec itself gained two things for this: a choice control reads as its index among its choices in
expressions (so a `when` can test a kind), and an item may have 128 controls and a placed item a million
triangles (the reference building has about 250 000).

### Runoff streaks

The reference's grime is the Kowloon wall condition's runoff (`wall.rs`, `runoff_at` and `deposit_detail`): under
every ledge, sill and roofline each column of wall gets a streak of its own strength and length (up to 2.8 m), fading
as (1 - drop / reach)^1.4, with a dark lip under the source and a fine, wandering vertical grain. The tiles take the
same runoff plus their own streaky deposits and joint dirt (`tile.rs`). `kowloon-runoff` ports that runoff to a
MaterialSpec layer that follows a named feature (`source`), and the items use it in place of the band-shaped top
grime: floors under their top edge, sills and air conditioners' drips, on both the wall and the tiled spandrels;
ground floors under their top and awnings; parapets under their coping. Identical layers from a side's storeys merge,
so a side stays within the 8 layers a slot composes.

**A generic, cached grime.** `kowloon-grime-streaks` is the layer the items use now: grime streaking from whatever
edge it follows (`source`: an authored or generated polyline, so the edge's endpoints are the feature's), with a
colour, strength, longest streak, repeat and shift. The streaks are one cached tile, `grime-streaks.png`
(`tools/grime/streak-pattern.mjs` bakes the wall condition's runoff into it once: 4 m along the edge, wrapping, by
2.8 m down; preview `itemspec/grime-streaks-tile.png`). Per texel the layer only maps into edge space: the distance
to the edge becomes a drop along the surface's downhill direction (gravity projected onto it) and the position along
the edge is moved back up that line, so the same tile is skewed on a sloping edge, stretched for a shorter run and
repeated along any edge, at the cost of two feature queries and one image read. `kowloon-runoff` (the procedural
port) stays as the reference it was baked from.

**Corners.** Each side's walls (`u0`) are measured from the side's distance along the cut footprint
(`<edge>_perimeter`, `_cperimeter`), and a sub-item part takes its sub-item's first part's pattern anchor, so the
concrete and paint run on round every corner and up every storey instead of restarting on each side.

The earlier grey blobs were not grime at all. At production resolution most of the building's 518 receivers did
not fit the preview's 512 MiB of baked maps and stayed on the per-pixel look, which shows a layered slot's base alone:
no paint, no grime. `?bakememory=<MiB>` (64 to 8192) raises the allowance; the building takes about 5.1 GiB at
1024 px and 160 texels a meter, so `tests/item-facade-browser.mjs` asks for 6 GiB.

### Surfaces, dirt and light (2026-11-18, pending review)

The building's look is recipes and layers that any item can use; none is tied to this building.

- **Layers** (`materialspec/program/examples`): `kowloon-wall-dirt` (rain-washed dirt, damp clouds, soot round
  `opening-edge`s, and dirt `settled` on ledges and under them), `kowloon-soot-band` (the plinth's soot from a ground
  line, with a ragged top and a haze above), `kowloon-rust-run` (a stain along a polyline the owner draws from a
  fixing downward), `kowloon-notices` (paper notices from the poster sheet in frames the owner gives as `left`,
  `right` and `bottom` edges: a texel is inside where its two side distances add up to the frame's width), and
  repainted repairs in `kowloon-paint-coat` (`patches`).
- **Which side of an edge.** A distance cannot say whether a texel is under an edge or above it, so streaks ringed the
  ends of short edges and climbed the wall above a floor line. `kowloon-grime-streaks` now also follows a `guard`: the
  same edge drawn 5 cm uphill. The two distances differ by the cosine of the angle off straight below, which keeps
  streaks under their edge. The items draw `eave`/`eave-above`, `sill-above`, `drip-above` and `ledge-above`.
- **Zones instead of searching.** The items now state where runoff hangs (ItemSpec README, weathering zones): the
  storeys' walls `runoff-top` below their tops and `runoff-sill` below their sills (stopping at the openings below),
  the ground storey `runoff-top`, the parapet and the round corner's wall `runoff-coping`, the corner's string courses
  and ledge `runoff-top` (u by their chords, continuing from the sides), and zones on the `drip`s of air conditioners and
  the roof outlet, the balcony's `ledge`, a board sign's `top` and its `fixing-drip` (reach item, onto the wall). A
  material reads `zoneCoord`/`zoneExtent` and never climbs above an edge. Not yet zoned: window heads (the window is
  below them), canopies and awnings (no edge features), notices.
- **Recipes**: `kowloon-wall-tile`, `kowloon-window-glass`, `kowloon-aircon` and `kowloon-poster-print` (catalog sheets
  by the mesh's UVs, aged), `kowloon-canvas`, `kowloon-sign-board`, `kowloon-sign-lettering`, `kowloon-paving`,
  `kowloon-asphalt`.
- **Tints.** A baked look is drawn with the atlas's own geometry, which has no vertex colours, so a bound fixture lost
  the paint its generator gave it. `?bakedtints=on` (`browser/renderer/scene-bake.ts`, off by default) carries the
  tints over, triangle by triangle; the fixtures' materials are then pale and add only age.
- **Limits.** The wall slot is at its eight layers. `kowloon/tarp`'s mesh has a triangle without a direction, which a
  bound receiver refuses, so awning cloth stays unbound (plain tints). Lettering stays the shipped glyph sheet: baked
  at 160 texels a meter its cutout breaks up. Notices are baked at the wall's density, so their print is soft.
- **Cutouts and lettering (later round).** A program's cutout (`alphaCutout` as its opacity) is now drawn by both
  looks: the shader record states `alphaCutoff`, the per-pixel look tests it, and a bake keeps the opacity in its base
  colour map's alpha (`AtlasProgramLayout.cutout`). Sign lettering is therefore bound to `kowloon-sign-lettering` and
  baked (800 texels a meter). Its earlier smears were the recipe's: it read the glyph sheet upside down, and its wear
  was measured on world x + y, which is constant across a sign turned 45 degrees; the sign, poster, air conditioner,
  ink and dirt recipes read `surface` instead.
- **Cutouts at a distance; layered opacity.** Box-filtered mips averaged a thin stroke's alpha below the cutoff, so
  lettering broke up with distance (scaling each level's alpha to keep the full level's share above the cutoff did not
  help: the thick strokes kept the share while the thin ones vanished). A cutout's GPU-baked base colour map now makes
  its own mips (`TargetSpec.coverage`, `GpuBaker.preserveCoverage`): a texel any of whose children passes the cutoff
  passes too (three quarters of the way from the cutoff to its most opaque child, `KEEP_STROKE`), so strokes stay, at
  most a texel of that level bolder, and colours are alpha-weighted so nothing transparent bleeds in. (The child's own
  alpha made dense characters run together; raising the alpha by mip level in the shader filled the board's worn
  ground in.) A layered slot keeps its stack's opacity (the base's, or a layer's that gives one) in
  its composed base colour map's alpha and is drawn as the base says, a cutout or blended (MaterialSpec program README,
  layered slots), so glass and lettering can take weathering layers.
- **Repairs.** `kowloon-repairs` draws hard-edged repair panels and crisp peel patches only inside frames the owner
  draws (`repair`/`repair-inner`, `peel`/`peel-inner`: an outline and the same outline 5 cm in, which together say
  which side a texel is on). Floors and ground floors seed a few per storey; the paint coat's own peeling and lattice
  patches are off. The walls' streaks are one layer following a list (drip, eave, ledge, sill) and a fringe under the
  parapet's `coping`.
- **Edges.** `kowloon-edge-ink` draws every exposed edge of a receiver (convex creases and open rims) as a worn pale
  arris with a narrow, broken line of grime beside it, heavier on upright edges and in inside corners, and rounds
  convex creases over in shading. It took the place of the all-over dirt on walls: plaster is calm, and its marks are
  placed (streaks, soot, rust runs, repairs, edges).
- **Light** (`tests/item-facade-browser.mjs`, the scene's global record only): a bright overcast day: a warm soft key
  from just off the camera (azimuth 6, elevation 52, intensity 1.8), ambient 1.15, exposure 1.06, light shadows
  (darkness 0.1), ambient occlusion on in the views without plants (radius 0.6, intensity 1.6; the occlusion pass does
  not stand foliage cards up, so it is off where trees are drawn), a warm pale backdrop; the `reference` view fills
  the frame (19.5 degree lens) and five detail views were added.

### The scene

`tests/studio/washroom-items/build.mjs` places the side (`FACADE=1`) or the building
(`FACADE=building`) across the lane beyond the washroom's low wall. With `LANE=1` (written by
`treespec_batches::write_lane_plants`) it paves the front street's road and lays a pedestrian
avenue along the building's right side (edge 1): `kowloon/pedestrian-avenue`, a narrow street
8.2 m from the wall line to the far shopfronts and 32 m long, large square slabs laid in a grid
(`stagger: false`) with dark joints, kerbs where it meets the road, black bollards across its
ends (`bollardColor`), and a raised median planter 1.7 m wide (kerb wall 0.35 m) with slot
drains and backless benches beside it, a 3.25 m walkway either side. The median is planted with
groundcover, trees stand in a row down it, 7 m apart, with a shrub between each two.
`tests/studio/washroom-items/avenue.json` is the one source of the avenue's line, median and
stations, read by both `build.mjs` and `write_lane_plants`. The avenue's top is 4.5 cm below the
building's pavement, which stands over it with its kerb stones as an edging. Litter bins stand
past the benches' ends, and two rows of `kowloon/street-lamp` posts (short arms toward the
median, banners on the row by the building; `lamps` in `avenue.json`) stand just past the
building's kerb and clear of the far shopfronts' awnings. Without the washroom the front street
is a whole street: a 7.3 m carriageway running out of the pictures both ways, a far kerb and a
4 m `kowloon/pavement` across it. Past the building's back a three-storey `kowloon/party` stub
closes the avenue's near side as a neighbour's frontage; the far side is closed by a row of six
neighbours (`frontage` in `avenue.json`): `kowloon-ground-floor` shopfronts (shutters, gates,
doors, awnings) under `kowloon/party` stubs of three to five storeys (`depth` fixed, `shops`
leaving their front's ground storey blank) set 0.35 m behind them; the first one's end faces the
front street (`gable: "end"`: windows across it, barred ones in its ground storey, a soil stack,
vents, and a blade sign by its corner). Blade and hung
`kowloon-sign`s project over both walkways between the trees, and the walkways carry pots by
the shop piers, crates and cartons, parked bicycles and loaded hand carts (`kowloon/street-cart`).
Pavements and the avenue are tinted a near-neutral mid grey (#8f8e8a), so the `paving` material
decides their colour; the benches' timber (a silver grey) and the bins are near neutral too. The
avenue and the lamps report a `foot` ring on the paving plane round what stands on it (bollards,
bins, bench supports, the planter, a lamp's base plate) for wear and grime layers to follow. The scene is near the preview's bake allowance (4 GiB at `bakememory`
4096): the neighbours and the street furniture bake coarser than the building. `tests/item-facade-browser.mjs` (`MODEL=building`,
`BUILDING_PARAMETERS`, `RESOLUTION`, default 1024, and `TEXELS`, default 256 per meter) pictures
it in the built studio. The earlier low concrete detail came from baking at 256 pixels per
receiver, about 21 per meter on a 12 m wall.

### Known limits

- Native fixtures are unweathered: Kowloon's weathering bakes are not run for an item. A scene
  project places no vertex colors, so their look comes from the bound project materials.
  Atlas-textured slots (lettering, posters, shop timber) need atlas materials bound to show
  their art.
- A feature reaches the receivers of its own top-level part unless it is authored with a wider
  `reach` (`item` or `scene`, bounded by `within` meters; see the ItemSpec definition README):
  generated features (`top`, `sill`) cannot be flagged, so a feature is authored along them.
- A layer measures to the nearest feature of the names it follows among all its receiver was
  given: on a building side every storey's `top` reaches every wall, so under a parapet's
  coping the streaks from its `top` end where the storey's `top` at its foot becomes nearer
  (halfway down). A layer for one edge should follow a name only that edge has (`eave`).
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
