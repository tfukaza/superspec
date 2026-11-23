# Architecture Markup Language

AML is the XML interchange format for architectural scenes. LevelSpec can
publish an AML draft as XML, and SuperSpec reads the same format from authoring
tools and handwritten files. Use `.aml`; the same document can
use `.xml`. SVG is a drawing/export format. The package root and normal CLI inputs
accept `<architecture version="1">`, not SVGX.

The [XML schema](../schema/aml.xsd) defines structural vocabulary. `superspec_aml::architecture::parse`
provides source-linked vocabulary diagnostics; the compiler resolves references,
construction relationships, physical geometry and collision. Native validation adds sampled navigation.

## Start a project

```sh
superspec init projects/my-level
superspec validate projects/my-level/level.aml --json
superspec preview projects/my-level/level.aml
superspec build projects/my-level/level.aml --out generated/my-level --json
```

The [starter](../crates/cli/assets/starter.aml) contains a slab, perimeter wall, native
circular opening, and spawn. Dimensions are meters and coordinates are
right-handed Z-up. Exporters own axis conversion. Authored linear dimensions are
normalized to millimeters before resolution; transforms and intersections are
not snapped again. Curve approximation, physical clearance, and cleanup are
separate concerns.

```xml
<architecture version="1" id="room" validation="premap">
  <profile id="footprint" points="0 0; 6 0; 6 4; 0 4"/>
  <path id="front" points="0 0 0; 6 0 0"/>
  <assembly id="shell">
    <wall id="wall" path="front" thickness="0.2" height="3"/>
    <slab id="floor" profile="footprint" thickness="0.2"/>
  </assembly>
  <opening id="window" host="wall" shape="circle" center="3 1.5" radius="0.6"/>
</architecture>
```

## Source objects and composition

- Paths use semicolon-separated XYZ `points`, analytic segment children, or
  ordered shared `paths`. Prefix a path reference with `-` to reverse it.
  `line`, `quadratic`, `cubic`, `bezier`, and elliptical `arc` segments are supported.
  Arc angles are degrees. A path's optional `elevation="start end"` varies height
  by plan distance independently of its curve controls.
- Profiles use XY points by default or `dimensions="3"` for constrained height
  surfaces. They support shared `paths`, `<hole>`, XYZ `interior` points, and
  XYZ `<breakline points="…"/>` constraints. Broken loops and crossing constraints
  are errors, rather than automatically repaired connections.
- `<coordinate-frame>` uses a named parent `frame`, XYZ `position`, extrinsic XYZ
  `rotation` in degrees, and dimensionless `scale`. `<datum>` uses an elevation
  and optional parent datum. Slabs can use `datum` plus `offset` instead of an
  absolute `elevation`.
- Definitions contain native elements; instances reference definitions with
  `ref` and a placement. Expanded members retain stable instance-qualified IDs
  and source ancestry. Cycles and expansion-budget violations fail explicitly.
- Groups organize objects. Assemblies compose physical members. Static and
  dynamic collision states remain separate. Explicit booleans require closed
  native operands; visualization meshes and open patches cannot be operands.
- Walls reference paths and support center/left/right alignment, longitudinal
  `base`/`top` profiles, and explicit floor/soffit anchors. Longitudinal profiles
  use `(distance, height, 0)` points and can include vertical steps.
- Openings name a complete host. `center` is longitudinal distance and height
  for a wall, or local X/Z for a collection. Shapes include rectangles, circles,
  ellipses, semicircular arches, and shared profiles. Through-cuts, recesses, and
  notches remain distinct from separate panels, glazing, infill, and frames.
- Slab elevation offsets its profile. Thickness extends below its top. `flat`
  projects a footprint to an absolute elevation, useful for a ceiling over a
  nonplanar floor. Roof/canopy walking coverage is opt-in through `playable`.
- Native boxes/wedges use corner anchoring by default; `anchor="bottom-center"`
  preserves bottom-centered construction. Cylinders and cones use a bottom
  center; spheres use their center. Dimensions, extrusions, and sweeps remain
  editable native objects.
- Stairs/ramps accept `path` plus `width`, or `left` and `right` rails. Stairs
  accept exact `count` or `maximum-riser`. Filled, sloped, and stepped bodies
  retain actual horizontal treads. Native retaining walls describe explicit
  contacts between source paths.

`superspec_aml::architecture::parse` and `serialize` handle the generic vocabulary.
`superspec_profiles::parse` and `serialize` select the shipped Kowloon profile;
`superspec_engine::compile` compiles that product profile. Compiled output retains the native
document as `architecture`, per-object `provenance`, and separate physical/render
hashes. Shared review/export backends currently also consume a derived runtime
semantic view; that view is not the authoring source or a geometry compiler.

## Architecture and CAD

The [architecture/CAD study](../tests/fixtures/aml/architecture-cad.aml) demonstrates native
profiled walls, a corner notch, circular aperture, constant-width frame, recessed
panel, hollow bent sweep, profile material partition, and solid subtraction.

`<face-material id="border" host="floor" face="top" profile="border-profile"
material="accent"/>` assigns existing exposed faces. Omit `profile` to assign the
whole selected face family. Selectors are `all`, `top`, `bottom`, `front` (-Y),
`back` (+Y), `left` (-X), `right` (+X), `side`, and opening-generated `reveal`.
Selectors use the host frame, including reflection/nonuniform scale. Partition
profiles may have holes and use `projection="xy|xz|yz"` (default XY). They split
render triangles without adding shells or changing collision, navigation, or the
physical hash. Overlapping assignments of different materials are errors.

Physical assemblies and booleans preserve exposed source materials. Conflicting
coincident exposed faces are errors; buried contacts and equal materials compose
normally. `material-owner="member-id"` chooses a descendant's material on its
shared exposed regions. Unique exposed faces retain their own materials. A chosen
collection must itself resolve to one material in each shared region. Nested
boundaries use the nearest applicable explicit ownership decision. An explicit
inner `coincident-material="error"` stops an outer retained-face policy.

Alternatively, `coincident-material="retained-face"` explicitly retains each
composed face's existing source contributor and material. Mechanical migration
uses this policy to preserve historical wall ownership. It follows deterministic
face retention for the ordered inputs and installed kernel; changing source order
or upgrading the kernel can change the choice, as with historical compilation.
Choose this policy or `material-owner` on one boundary. It does not suppress
conflicts between authored face-material partitions. Default native behavior is
`coincident-material="error"`.

Wall base/top profiles cover the entire drawn path, including ordered vertical
steps. A base profile and base-floor anchor are alternative base descriptions;
a top profile and soffit anchor are alternative top descriptions. `contact-floor`
asserts actual contact along the complete authored base, including custom profiles
and footprint holes. `fit-floor` only fits the foot locally where the named floor
meets it, preserving optional partial fitting during migration. It does not assert
support. Anchors use actual transformed soffit surfaces, including slab thickness.

Openings are positioned in their host's local coordinates, even when declared
elsewhere. Wall X is distance along the drawn path and Z is height above that path.
A named collection uses local X/Z. Placements and named frames apply in that host
frame. Through-cuts must intersect the host; notches must additionally meet its
projected boundary. Recess depth starts at the negative-Y face plus `offset` inward
and must leave a back web. Opening details use the same host frame with independent
placement and collision. Frame borders are physical profile offsets, including
ellipses and custom profiles, rather than scaled copies of the opening.

A `window` fits a modeled casement into a named rectangular through-opening.
The opening cuts the wall and supplies the dimensions and orientation of the
frame. The opening must fit on one planar face of a wall or box. Supported
widths are 0.4–4 m and heights are 0.4–3 m. The boolean `open` selects the
authored pose and defaults to `false`.
A fully open sash swings 90 degrees about its jamb, carrying the glass and
handle with it. Keep enough space on the swing side of the wall.

```xml
<path id="east-axis" points="6 4 0; 6 0 0"/>
<wall id="east-wall" path="east-axis" thickness="0.24" height="3"/>
<opening id="east-bay" host="east-wall" shape="rectangle"
         center="2 1.6" width="1.1" height="1.3"/>
<window id="east-casement" opening="east-bay" open="true"/>
```

Extrusions retain profile holes. Sweeps use local XY cross-sections with holes and
optional triangulation vertices, transported along an open path before the full
instance transform. Their initial X axis is perpendicular to local Z and the path,
falling back to local X for a vertical path. Corners use miter sections. Reversals,
folded sections, closed paths, and self-intersecting sweeps fail explicitly. Geometry
products and intersection work are bounded before or during allocation.

Authored dimensions are normalized once. Kernel input welding permits only exactly
coincident vertices; a nonzero mesh repair requires authored intent and fails.
Material clipping adds coplanar subdivisions without moving the physical surface.
Atlas input identities merge only source double roundoff (16 × machine epsilon ×
coordinate scale), preserving real vertices even when float32 rounds them together.
After packing, native T-junction UV seams can receive tiny shared-vertex adjustments
bounded by recorded guard movement and float32 precision. Aggregate displacement
stays within 0.125 texel and is recorded in runtime statistics. Source/render corner
attributes and collision geometry do not move. Strict overlap, density, bounds,
and GPU float32 audits remain enforced; genuine folds still fail.
The reviewer exposes source profiles, host/assembly/instance ancestry, contributors,
and exposed materials beside the final scene. Runtime JSON includes a separate
collision mesh so render partitions cannot become physical topology.

## Explicit migration

```sh
superspec migrate tests/fixtures/migration/dust2.level.svgx \
  --out generated/dust2-migration --json
```

Migration creates a new `level.aml`, `migration.json`, and content-addressed
companions. It never overwrites the source or an existing destination. Required
companions are snapshotted before publication; failures remove only a newly
created output directory.

This is mechanical source translation. Sliced circular approximations remain
rectangular slices. Existing wall-network composition boundaries, constrained
surfaces, flight parameters, anchors, route limits, and metadata are retained.
Imported meshes become visualization-only objects; old physical behavior becomes
a separate native faceted or bounds proxy. Rotated primitives keep their authored
anchor, avoiding a second quantization of a derived translation.

The native `superspec migrate` command coordinates the explicit
`superspec_aml::migration` decoder and companion publication. Ordinary compilation
never fetches an asset URL; I/O belongs to the host.

## Native circulation

[Circulation constructions](../tests/fixtures/aml/circulation.aml) demonstrate native stairs,
ramps, independent undersides, tapered flights, attachments, and reusable modules.
Flights select a center `path` plus `width` or `width-profile`, or explicit `left`
and `right` rails. Width profiles use local plan distance and width. Paired rails
share normalized plan stations; their mean plan length defines longitudinal
profile distance. Ascending and descending flights retain horizontal stair treads.

Stairs select an exact `count` or an explicit `maximum-riser`, never both. The implicit
maximum is 0.18m when neither is specified; world vertical scale is included.
Placement may translate, yaw, reflect, and scale, but cannot tilt stair treads.
Ramps retain intermediate authored elevations and do not accept stair counts.
Native flights use explicit canopy/roof objects for overhead construction.

Stair `underside` is `filled`, `sloped`, `stepped`, `custom`, or `separate`.
`thickness` controls the soffit offset or individual tread slab thickness.
A stepped solid requires enough thickness to connect successive risers; separate
treads have no implicit wedge. Custom `underside-profile` paths use ordered local
plan distance/elevation samples, including vertical steps, cover the flight, and
remain below every walking top. The reviewer exposes longitudinal walking tops
and custom underside source profiles separately from final geometry.

An `enclosure` names its `host` and opts into `left`, `right`, `start`, or `end`
on a flight, or explicit footprint `loop`/`edge` indexes on a slab, landing, roof,
or canopy. Side direction follows the authored path. `start`/`end` span distances,
signed `offset`, `start-return`, and `end-return` constrain its construction.
Positive offset/returns point toward the selected side. Entrances stay open unless
explicitly selected. Wall/parapet tops follow the continuous guide plus height;
curbs follow treads; optional `top` is an independent absolute local-elevation
profile. Bottoms follow actual tread support. Railings require explicit `rail`
profile children and optional `posts` with positive `maximum-spacing`; gaps remain
physically open. Returns retain the same rail/post construction.

Attachments join the host's physical assembly, creating a boundary when needed,
and inherit its visibility, collision, and dynamic state unless overridden.
Explicit conflicting assembly ownership fails. Openings cut the complete attachment,
including returns. Their coordinate contract is host-local X/Z, like named
collections; it is not distance unrolled around an enclosure.

Connections require valid endpoint references and final physical support. A supplied
`path` must resolve to a nonzero world-space mouth span. An omitted optional path
produces `CONNECTION_UNVERIFIED` during navigation analysis. Paths describe entrance
spans, not movement centerlines. A completely obstructed mouth produces
`CONNECTION_BLOCKED` even when an endpoint route can detour elsewhere; an unverified
sampled crossing remains a warning.

Land route counts share one physical-resource analysis across compilation, CLI
validation and producer publication. Duplicate, reversed, overlapping and contiguous
mouth declarations share capacity; separated mouths and enabled directional ability
links can supply independent resources. A reachable endpoint path establishes at
least one route. Additional verified resources form a conservative lower bound.
`maximumDisjointRoutes`, when present, is an optimistic bound over grouped declared
resources. Flat-domain cuts use native solid sections only when support stays within
the represented elevation and footprint; raised, lowered or undeclared bypasses
keep connectivity optimistic. Missing ownership or contradictory evidence removes
the upper bound. A proven shortage is `PHYSICAL_ROUTE_REQUIREMENT`; an inconclusive
shortfall is `PHYSICAL_ROUTE_UNVERIFIED`, a nonblocking warning. Neither result proves
continuous gameplay reachability. Water route counting retains its existing contract.

With sealed analysis requested, explicit connection and route failures also apply
with dynamic barriers closed. Generic closed-state floor coverage remains a separate
report unless a route/connection requires it. `NAV_HEADROOM` and `NAV_WIDTH` report
measured clearance, blocking source objects, and local inspection bounds. Representative
failures survive the detailed sample cap for every region/reason.

Navigation route reports expose
copied Z-up `path` coordinates for inspection. The reviewer moves through existing
navigation with physical capsule support, step limits, and standing clearance on
bounded movement substeps. It does not add support, widen openings, or change the
player body. Final physical-solid IDs remain independent of face contributors so
boolean and opening surfaces preserve interior collision. Recompilation or physical
mesh replacement invalidates the movement cache. These checks are traversal evidence,
not continuous gameplay physics proof. Recast's existing zero-step slope voxel
limitation remains; physical slope tests isolate that limitation with supplied exact
navigation geometry.

## Delivery status and acceptance

Foundation, architecture/CAD, and native circulation are implemented. The native
[Dust II source](../../levelspec/demos/dust2/scene.aml) uses editable footprints, elevation datums,
and consolidated walls. All sixteen original route assertions retain their
endpoints, player dimensions, minimum route count, and local distance limits;
actual paths replay through the physical inspection walker. Its
[spatial contract](../../levelspec/reference/dust2/README.md) remains the authority for estimated
dimensions and reference evidence. Mechanical migration is tested separately.

The bounded [Daiban courtyard](../../levelspec/examples/daiban.aml) contains native apertures,
profiled screens, distinct stair soffits, galleries, planters, rails, recessed
panels, a column, and overhead construction. Four explicitly local routes replay
with unchanged standing dimensions. Its [source packet](../../levelspec/reference/daiban/README.md)
records estimated geometry and reviewed plans/sections. Photographic fidelity
remains on hold: primary camera registration and high/overhead comparisons are
incomplete. Compiler and traversal success do not release that separate judgment.
Historical SVGX studies and frozen reconstruction evidence remain unchanged.

The [phase 3 review record](../../levelspec/docs/reviews/aml-phase3.md) records scope, checks, review
findings, and limits. [Phase 4 terrain and level metadata](TERRAIN.md) adds
heightmaps, constrained patches, grading, targeted exposure checks, bounded playable
zones, clearance assertions, tiles, and caching. Phase 5 cuts maintained CLI, reviewer, generators, tracing, reconstruction, and
exports over to AML. SVGX authoring APIs and JSON/Python design-plan frontends are
retired; the private read-only SVGX decoder is used only by explicit migration.
Historical studies remain in place and require their recorded revision for exact
compiler replay. [Producer interfaces](PRODUCERS.md) describe the supported native
entry points.
Imported visualization geometry stays excluded from architectural and physical
checks; native proxies are independently checked. Runtime JSON version 4 and
native export metadata version 3 remain independent of AML version 1.

The [phase 5 cutover record](../../levelspec/docs/reviews/aml-phase5.md) records the native API retirement,
export/lighting checks, independent reviews, and remaining acceptance limits.

Paper wall decals can be authored as `<poster>` children of `<dressing>`, independently of wall finish recipes. See [poster placement and the shared atlas](DRESSING.md#paper-posters-on-walls).

## City layouts

A top-level `<city>` places reusable ItemSpec props with the shipped Kowloon
profile. Open the [corner avenue compiler fixture](../crates/engine/src/worldspec/fixtures/corner-avenue.aml)
in the AML editor or run `superspec preview crates/engine/src/worldspec/fixtures/corner-avenue.aml`.

```xml
<architecture version="1" id="promenade">
  <city>
    <avenue id="paving" position="0 0 0.105" length="32" width="8.2"/>
    <tree id="median" position="4.5 -4.1 0.315" count="4" step="7 0 0"/>
  </city>
</architecture>
```

Each placement requires a unique source `id` and a world-space XYZ `position`.
Optional `heading` rotates the local axes counterclockwise about +Z in degrees.
For a row, `count` includes the first placement and `step` is the world-space XYZ
displacement between neighbors. A layout may expand to 1,024 placements per
document, subject to the scene geometry limits.

| Element | Local anchor and controls |
| --- | --- |
| `building` | Lot origin at pavement height. `floors` selects 2–12 storeys (default 4). `width` and `depth` are 6–14 m (defaults 9.8 × 7). `seed` selects repeatable architecture and dressing (0–31, defaults to floor count). Use `front-trade` to open the first street-facing bay as a `provisions`, `grocer`, `greengrocer`, `hardware`, `herbalist`, or `roast` shop. `graphics-detail` draws the same building with less geometry for a lower viewer preset: `0` is the authored building (default), `1` is for Low and `2` for Very Low. It changes the picture only; placements, colours and physical output are those of level 0. `variant-budget` (0–64) states how many shapes each part type, such as an air conditioner or a window, draws from its catalogue; unstated it follows the detail level (4, 2 and 1 at levels 0, 1 and 2), a value is rounded down to a power of two, and `0` lets every instance draw its own shape. It is render-only too. The same intrinsic attributes share a design across placements. For the authored four-storey demo use `model="reference-corner"` with its original dimensions and seed. |
| `road` | Near starting corner at road surface. `length` extends along +X and `width` toward −Y. |
| `sidewalk` | Starting corner at paving height on the wall side. The strip runs along +X with its kerb toward −Y. Accepts `length` and `width`. |
| `pillar`, `wall` | Closed concrete member with its lower corner at the placement. Width follows +X, depth +Y, and height +Z before heading rotation. Defaults are 0.5 × 0.5 × 3 m for a pillar and 4 × 0.28 × 2.2 m for a wall. Each member has its own metric concrete finish and shares the washroom weathering recipe. |
| `procedural-wall` | Standalone windowless concrete wall. Uses the same local axes as `wall` and dresses the −Y face with service pipes, wires and paper posters. `seed` selects a repeatable arrangement. Set `dressing="false"` for a plain weathered wall. |
| `pipe-run`, `duct-run` | Sweeps a circular pipe or rectangular duct along local XYZ centerline points, for example `points="0 0 0; 0 0 2; 4 0 2"`. Pipe `radius` defaults to 0.055 m. Duct `width` and `height` default to 0.32 and 0.22 m. Elbows follow the polyline, with sleeves or flanges on straight runs. |
| `avenue` | Starting corner at paving height. A pedestrian strip extends along +X, with its median halfway across its `width` toward −Y. Accepts `length` and `width`. |
| `tree` | Trunk foot. The Kowloon theme supplies TreeSpec bark levels, illustrated foliage and a shadow proxy. Trees and shrubs accept a uniform `scale` from 0.05 to 5, default 1. Scaling preserves the root position. |
| `shrub` | Root position for a low illustrated shrub. Supports the same heading and row controls as trees. |
| `weed` | Root position for a small clump of soft undergrowth and grass blades. The default `variant="mound"` grows against masonry. Use `variant="seam"` for a lower broken line of growth. The long axis follows local +X so `heading` can align it with a joint. Supports the tree placement and uniform scale controls. |
| `grass-field` | Places a painted lawn patch at its local origin. Supply its ItemSpec-compatible GLB companion with a relative `src`, e.g. `src="planting/court.glb"`. The asset carries the metric footprint, rolling ground relief, tuft atlas and grass pigment. Heading and row controls apply to the whole patch. |
| `lamp` | Post foot. Optional `banner="true"` hangs a cloth banner. The arm points toward local −Y. |
| `wall-extraction-grille` | Centre of the rear wall flange. Width 0.696 m, height 0.390 m and projection 0.066 m along local −Y. |
| `passage-sign-lamp` | Backplate centre on the wall. The folded hood projects 0.19 m along local −Y. Allow 0.45 m in width and 0.22 m in height. |
| `ceiling-bulkhead` | Centre of the ceiling mounting plate. The oval cast housing measures about 0.33 × 0.24 m. A wire cage protects the warm prismatic lens. Allow 0.16 m below the soffit and 0.37 m from the mount along local +X for the surface conduit. |
| `ceiling-twin-strip` | Centre of the ceiling mounting face. Two cool opal tubes sit beneath folded aluminum reflectors along local X. `length` is 0.4–2.4 m (default 1.2 m). Allow 0.29 m across the housing and 0.12 m below the soffit. With `enclosed="true"`, a continuous ribbed opal cover, gasketed ivory end caps and zinc clips enclose the batten. The enclosed fitting measures 0.272 m across and hangs 0.106 m below the mount. |
| `ceiling-girder` | Start of the upper flange. The beam extends along local +X. `length` is 1.6–20 m (default 4 m). Place intermediate ceiling fixings with `hangerStations="2 0 0; 5 0 0"` and set `soffitGap` to the vertical distance to the soffit in metres. |

These theme props contribute visual geometry. Author AML slabs, walls or collision
proxies independently where the layout requires walking surfaces or obstructions.

For service runs, `supports` accepts one local XYZ offset per centerline segment.
Each offset points perpendicularly from the centerline to its mounting surface.
The generator places clamps and footplates along straight sections. Use
`0 0 0` for an unsupported leg such as a short wall penetration.

### Building footprints

`<city><building>` places the shipped `kowloon-building` ItemSpec. Supply
`footprint` in counterclockwise local XY metres in place of `width` and `depth`.
The first edge takes the family's first facade. Trace a C or H as a single
concave outer loop. An O-shaped building has one or more `<hole>` loops for
open courtyards. The ItemSpec kit generates facades and parapets on the inner
edges and leaves the courtyards open through the roof.

| Shape | Example `footprint` in local metres |
| --- | --- |
| C | `0 0; 8 0; 8 2; 2 2; 2 6; 8 6; 8 8; 0 8` |
| H | `0 0; 2 0; 2 3; 6 3; 6 0; 8 0; 8 8; 6 8; 6 5; 2 5; 2 8; 0 8` |

```xml
<architecture version="1" id="courtyard-building">
  <city>
    <building id="tenement" position="0 0 0" floors="3" seed="6"
              footprint="0 0; 16 0; 16 14; 0 14" ground-shops="false">
      <hole points="4 4; 4 10; 12 10; 12 4"/>
    </building>
  </city>
</architecture>
```

Use `ground-shops="false"` for a residential ground floor and `pavement="false"`
for a building on an existing deck. Boundaries must be simple, with at most 64
corners across the outer loop and all holes. Holes can use either winding and
must clear each other and the outer loop. Allow room for the inset roof and use
courtyard edges at least 0.3 m long. Facade spans can be up to 30 m long. The
authored reference corner uses its own footprint. Use native solids for gameplay
collision.

For standalone ItemSpec projects, use the same `footprint` coordinates and pass
the inner loops as the `courtyards` parameter. Older scenes with building rules
under `<dressing>` can migrate their lots to city placements using this syntax.
