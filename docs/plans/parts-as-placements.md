# Parts as placements plan

Detailed design for step 7 of the [runtime efficiency plan](runtime-efficiency.md), written on 2026-11-22 from a read of the source. Paths are relative to the repository root. `K/` means `crates/engine/src/worldspec/themes/kowloon/` and `D/` means `crates/engine/src/itemspec/definition/`.

**Status.** Stages 0 and 1 are implemented and were run (same day). Everything about them in this document was checked against the code and is marked **Corrected**, **Added** or **Verified** where it differs from, or confirms, the first draft. The Rust side of stage 2 is also implemented and was run: the `prototype` part generator, the metadata export (workstream C), one tint per instance for the condenser and the window unit, and prototypes for sixteen more generators (2.3). The part bake itself, and stages 3 onward, are still a design from a source read: nothing in them was built or run, and section 7 lists what remains unverified. The one exception is stage 3's adapter side (2.10), implemented and tested the same day against a synthetic pack. Section 8 holds the frozen contracts, including what a pack must write for a tinted instanced batch (8.4). Line numbers in `D/` and `K/` cited by the first draft have moved with the stage 1 edits; name the function, not the line.

## 1. Findings that shape the design

1. **The native part key is placement- and seed-keyed.** `D/parts.rs:886-898` hashes `(id, shape, placement, slots)`, and a native shape serializes `name, identity, parameters` (`D/compose.rs:292-299`), which includes `seed`. Identity is `name@VERSION` (`K/items.rs:3331-3335`, `VERSION = "14"` at `:53`). A new content key is needed; `geometry_key` must stay as it is.
2. **Flattening happens at every nesting level.** A sub-item part builds its child and calls `item.flattened()` (`D/parts.rs:1162-1163`). A building's `ItemGeometry` therefore has nodes only for its 20 top-level parts, and leaf identity survives only in mesh-id paths (`D/output.rs:647`, `D/parts.rs:1152`; the survey recovers it at `crates/engine/tests/kowloon_variants.rs:160-172, 302-309`).
3. **Instance colour is multiplied into vertex colours.** `K/items/meshes.rs:73-75` copies it; `unbaked_paint` multiplies receiver paint in (`K/items.rs:256-290`). A window unit's palette is `UnitPaint { body, steel, cover }`, drawn from the instance stream after the catalogue pick (`K/aircon.rs:646-652, 722-744`) and used as `shade(body, f)`. A part has several palette entries, and many vertices carry none.
4. **The bake is world-keyed in three places.**
 - `receiverKey` hashes `objectToWorld` and world bounds (`tools/headless/building.mjs:26-30`).
 - The city project maps materials in `world` space (`editor/city-project.ts:135`), with `render-paint` overridden to `anchored` and `render-paint-round` to `uv` (`K/materials/street-materials.json`, `mappings`).
 - The export writes geometry in building space with page-remapped UVs (`tools/kowloon-export/bake-buildings.mjs:232, 244-250`).
 
 The GPU atlas node key is already content-based and names no placement (`browser/renderer/scene-bake.ts:20-30`).
5. **Context enters a receiver through two inputs.** Contacts come from the touching scene (`crates/engine/src/authoring/project.rs:1298-1300`), and reaching features come from other parts (`crates/engine/src/authoring/item.rs:1013-1018`).
6. **The published path does not use Rust batches for buildings.** `compile.py:75` exports each prototype with `superspec glb`. The bake regenerates the building through a project (`building.mjs:33-44`). `pack.mjs:316-336` places whole-prototype meshes and `instance({ min: 2 })` instances them (`:351`).
7. **Parts ride free in building page draws today.** They are joined by page material. Instancing by prototype adds one draw per visible prototype per LOD band, so geometry instancing alone can raise draw count.
8. **Texel sharing does not need geometry instancing.** A flattened building primitive can sample a shared part atlas. `graphicsProfiles.json:30-31` already has the two axes: `assets.weathering` and `assets.partInstancing` (both `consumed: false`).
9. **The model and Rust exporter already carry per-instance tint.** `InstanceBatch.tints`, `vectors` (`crates/model/src/batches.rs:63-66`); the exporter writes `_TINT` (`crates/export/src/gltf.rs:683-684`); the preview maps tints to `instanceColor` (`browser/renderer/batch.ts:105`).
10. **The adapter excludes tinted instances at five sites.**
  - `cityVisibility.ts:280` (`canSubset`, which also gates LOD at `:282-283`)
  - `cityVisibility.ts:1240` (`mergeCityInstances`)
  - `cityClusters.ts:22`
  - `distantGlazing.ts:46`
  - `glazingRange.ts:26`
  
  `cityVisibility.ts:267-268` drops any mesh whose geometry has an `InstancedBufferAttribute` from visibility entirely. Warm-up already keys on `instanceColor` (`worldWarmup.ts:153, 243`).

  **Corrected in stage 3's adapter work:** there was a sixth site, `CityFarHardware.finish`, which compacts instances for a distant draw. The five exclusions are lifted for `instanceColor` (2.10); the drop of meshes with instanced geometry attributes stays. GLTFLoader reads only `_COLOR_0` into `instanceColor`; the exporter's `_TINT` of finding 9 lands on the geometry as an instanced attribute (8.4).
11. **Building walls are generic panels.** `kowloon-floor.item.json` has 8 panel parts; the `wall` slot maps to `render-paint`. Their features are seeded per floor (`kowloon-floor.item.json:288`).
12. **Step 4a is not implemented in `tools/kowloon-export`.** A grep for its terms found nothing.

Found while implementing stages 0 and 1:

13. **The wall stain under an air conditioner is the wall's own feature.** `kowloon-floor.item.json` authors two `rust` features with zones on the `wall` part per fitted bay, under the same random draws that place the unit (the last four `rust` entries of its `features`). The `drip` feature of `kowloon-aircon.item.json` has part reach, so after adoption it reaches only the floor's air-conditioner part. A unique wall therefore keeps its stains whatever happens to the unit, and a shared wall loses them because they are part of the wall's `featured` key (decision 3).
14. **Vertex colours are kept to a thousandth.** `crates/engine/src/itemspec/mesh.rs:37` rounds every render vertex colour to 1e-3. An instance's colour is `round(shade × palette)`, so a shade read back from a probe times the palette matches it to about 1.5e-3, not exactly. This is inside 1/255 (3.9e-3) and the tests hold it to half that.
15. **A condenser has more than one instance colour.** `CondenserPaint { body, steel, duct, cover }` is drawn per instance. See decision 2.
16. **Findings 1 to 3 are verified** by the implementation and its tests. Findings 4 to 12 concern the tools and the adapter and were not re-checked.

Found while implementing the Rust side of stage 2:

17. **No bake reads vertex colour.** A MaterialSpec program's inputs are UV, surface, object and world position, normal, gravity, footprint, bounds, features, zones, edges and planes (`materialspec/program/compiled.rs`, `enum Input`). `vertexAttribute` and `instanceAttribute` are render-only: the evaluator refuses them without render inputs (`program/eval.rs`, `B::InstanceAttribute | B::VertexAttribute`), and no shipped Kowloon material uses either. Vertex colour reaches a baked look only afterwards: the preview copies it onto the display geometry on request (`browser/renderer/scene-bake.ts`, `carryTints`), and the export multiplies it with the material colour into `COLOR_0` (`tools/kowloon-export/bake-buildings.mjs`). Baked texels are therefore independent of instance colour, and the palette split holds for every material.
18. **The wall's paint is a layer, not a multiplier.** The `render-paint` material (`kowloon-concrete-anchored.material.ts`) takes no colour. A building's `paint` is the `color` parameter of a `kowloon-paint-coat.material.ts` layer the items state on the `wall` slot (`kowloon-floor.item.json`, `layers`), which is composed by coverage: `baseColor: mix(coat × patchTone, bare, peeled)` under `coverage`, with repairs (also given `paint`), edge ink, grime, soot and rust runs above it. The baked colour is the paint times a shade where the coat shows and something else where it is peeled, worn or covered. A neutral bake times a tint is exact only on the coat.
19. **A flattened mesh is named through every nesting level.** A building's condenser mesh is `part/side-1/side-1/part/floor-0/floor-0/part/aircon-2/aircon-2/part/condenser/condenser/aircon`: each level adds `part/<id>/<id>`. The shading project prefixes the model (`p0/`), and the delivered city scene the building's owner (`city/<id>/`), which is how `building.mjs` maps one to the other (`toWorld`). Verified by a test (section 7).
20. **A project composes at most 16 models** (`authoring/project.rs`, `MAX_MODELS`), and an item at most 512 parts. A library of prototypes is several projects, or items of several prototype parts each.
21. **A binding's assignment may be the building's own.** A slot the item weathers is bound to a derived assignment (`aged-metal.p0.metal`: material, model and slot) that carries the item's layers, some of which follow a feature of the building (`kowloon-foot-wear` follows `foot`). The layers are in `binding`; the derived identity is kept out of it, in `assignments`.

## 2. Design

### 2.1 Three identities, three fields

| Identity | Field | Contents |
| --- | --- | --- |
| Source | `path` | Part path, e.g. `side-1/floor-0/aircon-2/condenser` |
| Artifact | `prototype` | sha256 of `"itemspec-prototype-v1"`, generator name, identity (`name@VERSION`), canonical content values, mirror flag |
| Placement | `path` + `transform` + `inputs` | Never hashed into an artifact |

- Content values are the catalogue entry, structural parameters and `graphicsDetail`. They exclude seed, part id, placement and `variantBudget`.
- The entry is named directly, so budgets nest (`K/variants.rs:9-15`) and Medium and High share prototypes.
- The slot-to-material binding is not in the geometry key. It joins the raster key.

### 2.2 Engine model (generic, no theme import)

*Implemented in stage 1. The exact signatures are frozen in section 8; this section keeps the reasoning. Lines marked **Corrected** differ from the first draft.*

**`D/compose.rs`**: two defaulted trait methods, so other hosts are unaffected.

- `prototype(name, parameters) -> Option<NativePrototype>` describes an instance: `content` (canonical, no seed), `inputs` (named colours, linear RGB) and `mount` (`Wall` or `Free`). It draws nothing.
- `prototype_meshes(name, content, cancelled) -> Result<NativeOutput, String>` draws the shape: part-local, neutral colours, roles filled.
- **Corrected:** `prototype_meshes` has a default too (`generate(name, content)`), which serves a generator with no inputs whose content is its parameters. The conduit uses it.
- **Corrected:** a prototype is drawn once per prepared item, not once per part. `Environment` holds a prototype cache that every native part prepared under it shares, sub-items included. Without it a building drew each condenser's prototype again in every `kowloon-aircon` sub-item.
- **Corrected:** roles are not declared separately. A prototype's `roles` are the names of its instances' `inputs` in name order, and instances of one prototype must name the same inputs (checked at build).

**`D/output.rs`**

- `ItemMesh` gains `roles: Vec<u8>` (0 means untinted, k means palette entry k−1), with `skip_serializing_if` empty. It joins the `index_vertices` key.
- `ItemGeometry` gains two fields, both skipped when empty:
- `prototypes: BTreeMap<String, Arc<Prototype>>`. **Corrected:** `Prototype` also carries `identity` and `mirrored`, which are in its key.
- `placements: Vec<PartPlacement>`, where `PartPlacement { prototype, path, node, transform, inputs, slots, meshes: Vec<usize> }`
- `ItemGeometry::placed()` returns placements with `world_transforms()[node] ∘ transform`. This is the "flattening composes the transforms" requirement.
- **Added:** `PartPlacement::down()` gives item-space down in prototype coordinates, rounded to 1e-6: the gravity class of 2.5.
- **Added:** `PreparedItem::part_contents()` lists every leaf part with three content keys (`bare`, `shape`, `featured`). This is the panel content key of 2.7, computed without building a mesh; stage 0 measures with it.

**The expanded meshes stay exactly as they are.** A placement annotates which expanded meshes it covers. That is the whole compatibility argument:

- `flattened()` (`D/output.rs:637-679`), `measure()`, collision (`authoring/item.rs:1213-1286`), features, anchors, zones, contacts, occlusion and field bakes read the same data as today.
- A consumer that ignores `placements` renders as today.
- The cost is holding prototype meshes as well. Deriving expanded meshes from prototypes is a later optimization, gated by the reconstruction test below.

**`D/parts.rs`**

- `PartShape::Native` build: after `generate`, call `prototype()`. If it returns `Some`, record the placement (identity transform in part space, all adopted meshes) and insert the prototype once by key. A part that draws nothing records none. The build fails if the prototype's slots and triangle counts are not the instance's.
- `PartShape::Item` build: lift the child's placements. Prefix `path` with `self.id`, take the child's `placed()` transform (the child's item space is this part's space), remap mesh indices through the flattening order and `adopt()`, and compose slot renames. `adopt()` drops unmapped slots, so the placement carries the surviving slot map and only the surviving meshes; a placement left with no mesh is dropped.
- The kept-part branch carries over `previous.placements` for that part's node and the prototypes they name.
- Mirrored placements (negative determinant through the part's node) name the reflected prototype: the mirror flag is in the key, the prototype is reflected in its local X, and the stored transform is composed with that reflection. Stored transforms are therefore proper motions at every level, and a second reflection higher up returns to the unreflected prototype bit for bit.
- **Corrected:** a rotation never changes the prototype key. The first draft's test list said a non-Z rotation gives a distinct key; that belongs to the raster key (2.5), through `down()`.

**Native and Wasm parity.** Everything is in shared `crates/engine` with no `cfg`. New fields serialize only when non-empty. `ItemPart.geometry_key`, `PreparedItem.geometry_key` and mesh artifacts do not change. No code outside tests serializes `ItemGeometry` or `GeneratedItem` as a whole (checked by search), so no packet can carry the new fields.

### 2.3 Theme natives (`K/items.rs`, `K/aircon.rs`, later the other catalogued generators)

*Implemented in `K/items/prototypes.rs`, with two delegating methods in `K/items.rs`, entry points in `K/aircon.rs` and the stated entry in `K/variants.rs`.*

- `Natives::prototype` returns `Some` only for converted generators. `prototypes::sharing` sorts them into three kinds:
  - **Stated.** The generator reads no seed and its colours are parameters: its parameters less `variantBudget` are its content, and `generate` draws the prototype. `kowloon/conduit`, `security-bars`, `wall-vent`, `downpipe`, `riser-duct`, `cornice`, `roof-outlet`, `wall-lamp`, `balcony`, `balcony-door`, `fan-divider` (wall), `rail`, `ladder` (free). Given a seed, such a part is not described.
  - **Entry.** The seed picks a catalogue entry and nothing else, and colours are parameters: content is the parameters with `entry` in place of `seed`. `kowloon/window`, `plants`, `sheet-canopy`, `sagging-tarp`. No input.
  - **Tinted.** As Entry, and the instance's stream also draws one colour. `kowloon/aircon` (the window unit), role `body`. The condenser is the same with a content of its own: `{entry, width, height, plates, weight, graphicsDetail}`, role `body`.
- **Added:** an entry is drawn as a shared shape under `variants::with_entry(entry, tint, …)`: every `Budget::pick` on the thread gives that entry whatever the seed, and `variants::instance_colour(role, own)` gives the stand-in colour for the one the instance owns. A generator needs no second code path; it reads its one colour through `instance_colour`.
- A fixture with no catalogue (`variantBudget` 0) returns `None` and stays unshared.
- **Decision 2 applied (changes output; `VERSION` 14 → 15).** The condenser's bracket steel, line-set duct and cover, and the window unit's bracket steel and cover, are now the catalogue entry's: `planned_condenser` and `unit` take them from `condenser_entry` and `unit_entry`, and the instance stream draws the case paint alone. Every condenser and unit prototype reports exactly one role. Geometry per entry is unchanged (the survey's condenser and unit rows are the same triangles in the same number of shapes). Pictures of three back and front walls before and after are in `generated/graphics-detail/placements/condenser-{before,after,compare}-*.png`: case paints are as they were, and brackets and covers of one entry are now alike.
- **Not shared, and why.** `kowloon/cage` draws four kinds of colour from its own streams: the rust in its paint, its roof sheet, a dye for each garment of its washing, and a caged unit's paint. `kowloon/drying-rack` draws a dye for each garment. Neither is one shape and one tint. Sharing them means giving all but one of those colours to the entry, which removes the variety `kowloon_variants.rs` (`washing_is_dyed_by_its_fixture…`) holds; it is an owner decision. They are 0.49M and 0.23M triangles at detail 0.
- Parts drawn by generic generators (the duct brackets' `strap`, `hanger` and `plate`, 0.26M triangles in 2 shapes each) have no native to describe them. They need the panel route of 2.7: a generic content key.
- `prototype_meshes` draws the entry with a neutral palette and fills `roles`. Two ways to get roles:
- **Probe passes (implemented).** Draw once with every role mid-grey and once with one sentinel colour per role. A vertex both passes colour alike is the fixture's own. A vertex that changed by exactly one role's ratio is a shade of that role, and its grey-pass colour over the grey is its shade. Anything else fails the build with the two colours in the message. No drawing code changes.
- **Corrected:** the first draft probed with white. `shade()` clips at 1 (`materialspec/color.rs:5-11`), so a shade above 1 would read wrong against white. Grey at 0.5 leaves room up to a shade of 2.
- **Explicit roles** threaded through `Tint` (`K/pen.rs`, `enum Tint { Color, Field }`). Cleaner, but touches every tinted call.
- A generator whose colours do not factor fails loudly at build, so it must not be registered in `prototypes::describe`. That is the fallback: leave it out.
- `kowloon/conduit` needs no palette: it takes no seed and a constant colour (`K/items/kowloon-aircon.item.json:41-50`). Its content is its parameters less `variantBudget`.
- Bump `VERSION` only if a generator's ordinary output changes. Stage 1 did not; decision 2 did (15).

### 2.4 Export: a sidecar, not a model change (first)

`Session::place_item` already returns per-part metadata (`authoring/item.rs:1028-1032, 1137-1145`), and the bake already reads it (`building.mjs:46-48`).

- **Done:** `placements` and `prototypes` are in that metadata (`Session::place_item`; the bindings are filled in `Session::resolve_items`, where a model's slots, layers, mappings and densities are resolved). They reach a host in `metadata.items[<model>]` of every shading report (`shade-layouts`, `project-fields`' sibling actions), with no change to `crates/cli/src/bin/studio`: `shading.rs` already passes the session's metadata through. `PreparedProject::items()` reads the same record without shading.
- `bake-buildings.mjs` writes `building-N.parts.json` from it. The schema is in section 8.2.
- **Corrected:** the slot map belongs to the placement, not the prototype. Two parents may map one prototype's slots to different item slots, and a parent may drop one (`PartPlacement.slots`).

`superspec glb`, `crates/model` and scene packets are untouched in this step. Moving prototypes into the shared model as batches (`spec-modularization.md:114`, `city.rs`) is a later stage, because `same_design` requires `batches.is_empty()` (`worldspec/city.rs:295-296`) and would stop recognising repeated buildings.

### 2.5 Part bake: each prototype once, in object space

New files: `tools/headless/parts.mjs`, `tools/kowloon-export/bake-parts.mjs`, and a `partProject()` beside `cityProject()` in `editor/city-project.ts`.

**Input.** The union of `prototypes` across every `building-N.parts.json` of the city.

**Library project.** One item model per prototype, with an inline synthetic definition (`ModelSource.source` is accepted, `authoring/item.rs:754-756`):

```json
{"format":"itemspec","version":2,"id":"part-<key>","parts":[{"id":"p","generator":{"kind":"native","name":"...","parameters":{}}}]}
```

- **Corrected, then built:** that definition only works where content is the generator's parameters. The library uses the `prototype` part generator instead (`D/schema.rs`, `PrototypeReference`; documented in `D/README.md`):

```json
{"format":"itemspec","version":2,"id":"part-<key>","parts":[{"id":"p","generator":{"kind":"prototype","name":"kowloon/condenser","content":{"entry":3,"width":0.95,"height":0.6,"plates":true,"weight":2,"graphicsDetail":0},"mirrored":false}}]}
```

- Its meshes are the engine's prototype meshes value for value (tests in `item_placements.rs` and `kowloon_prototypes.rs`), with neutral colours and `roles`. The item metadata lists the roles per vertex under `meshRoles`, by scene mesh name (`<model>/part/p/p/<slot mesh>`), and the scene delivers the vertices in the same order.
- The model is unplaced, so part space is the bake space. Model ids derive from the content key, never from a path.
- A project holds at most 16 models (finding 20): the library is several projects, or one item per group of prototypes with each `prototype` part placed apart.
- A binding's `layers` are the building's slot layers (finding 21). The library assignment for a raster is built from the binding: entry, parameters, seed and layers. A layer that follows a building feature finds none on a prototype; that is the third class of 2.6.
- Layouts come from `compile("shade-layouts")` and baking from `SceneBaker`, as in `building.mjs:42-106`.

**Raster key.** sha256 of:

- producer identity (same construction as `bake-buildings.mjs:297-311`)
- prototype content key
- per slot: material assignment (entry, parameters, seed, layers) and mapping mode
- local features and zones (the existing `receiverKey` fields, minus `objectToWorld`)
- texel density: slot density × tier factor (step 3 says density must be in the key)
- gravity in part space
- context proxy (2.6)

It contains no world transform, no path and no building name.

**Gravity.** City and facade placements are turns about Z (`worldspec/city.rs:333-369`, `D/parts.rs:543-561`), so part-space gravity is `[0,0,-1]` for nearly all placements. A placement with another rotation gets its own gravity class, which means a second raster of the same geometry.

**Atlas.** City-wide pages through `chartPages` (`bake-buildings.mjs:189`).

- Each prototype's charts are packed as one rectangular block per alpha class. Per-instance chart rectangles (2.8) need that later.
- Emitters and optional-tier charts stay on their own pages, as now (`:184-189`).

**Output.** `parts-pbr.glb` and `parts.atlas.json` (key → mesh, block rectangle, raster key).

- One mesh per prototype in prototype-local coordinates.
- `COLOR_0` holds shade factors; `_MS_ROLE` holds roles.
- Material extras include `worldSpecPartAtlas: true`.

**Building bake.** Placement-covered meshes become a third class beside `nativeMeshes`: present in the scene, not targets (`building.mjs:54-57`), and not exported (`bake-buildings.mjs:65-68`). They still occlude, make contacts and supply features for walls, because the building project is unchanged. The building bake key (`bake-buildings.mjs:330`) gains the shared set.

### 2.6 Weathering that depends on world context

| Class | Example | Handling |
| --- | --- | --- |
| Part-space only | Rain streaks on the casing, its own zones and creases | In the prototype bake; exact for a gravity class |
| Part onto wall | Drip below a unit | Receiver is the wall. Where walls stay unique (Medium, High) nothing changes. Where walls are shared (Low, Very Low) the drip is dropped by default; an instanced blended stain quad is the alternative (owner decision 3) |
| Wall or neighbour onto part | Contact darkening on the back, reached features | Not in shared texels. For `mount: Wall`, the library scene adds an undelivered wall slab at y = 0 (the natives' wall plane, `K/items.rs:5-7`) so contact and occlusion fields see a wall. The proxy kind is in the raster key |

### 2.7 Wall panels as prototypes

Panels need no theme hook. A panel's generic content key is the hash of its resolved shape, slots and zones: today's key minus `id` and `placement` (`D/parts.rs:889`).

1. **Measure first.** Count distinct panel content keys per size class across the seven prototypes, with and without openings and features. Features are seeded per floor (`kowloon-floor.item.json:288`), so whether any wall repeats today is unknown. **Measured in stage 0** (section 3): no wall repeats inside a building; 131 walls are 26 size classes.
2. **Share by material and size class.** A panel maps onto a representative baked once at neutral colour; the building's colour is an instance or vertex input. Crop or tile, do not stretch. This is the bake-side form of step 4a's rule and replaces its post-process.
3. **Keep unique** signage, shop art, posters and lit window patterns.

This needs the render-paint program to accept a multiplicative colour over neutral weathering, and it changes the look. It is its own stage and an owner decision.

### 2.8 What is shared at each preset

| Preset | `assets.weathering` | Parts | Walls |
| --- | --- | --- | --- |
| Very Low, Low | `shared-all` | Part atlas at tier density | Shared panel prototypes, colour as input |
| Medium, High | `shared-parts` | Part atlas at full density | Unique, per building, as today |
| Very High, Ultra | `unique` | Shared geometry; each instance keeps its own texels | Unique |

For `unique`, each instance's charts are the prototype's block translated into a building page, so the instance carries `_CHART` (offset, scale) and a page or layer index. This is only worth building on step 5's array textures; otherwise instances on different pages cannot share a draw. No `full` variant pack exists yet (`graphics-profiles.md:372-373`), so this stage is last.

### 2.9 Pack

New `tools/kowloon-export/part-batches.mjs`, called from `pack.mjs` after the placement loop (`:316-336`).

- For each building placement and each part placement, compute the world transform (building ∘ part) and convert Z-up to glTF as `pack.mjs:331-333` does.
- **Choose per prototype**, because of finding 7:
- *Instanced* when the triangles saved, (instances − 1) × triangles, pass a threshold. One `EXT_mesh_gpu_instancing` node per prototype and alpha class, merged across all buildings, written directly as `street-batches.mjs` does. gltf-transform's `instance()` carries only TRS.
- *Flattened with shared texels* otherwise. The triangles stay in the building mesh, UVs point into the part atlas, and the tint is folded into `COLOR_0`. Texels are shared at no draw cost.
- Per-instance tint is `_COLOR_0` for one role, and `_PALETTE_1..3` for more. The exact form the adapter draws is in 8.4; `_PALETTE_n` is not drawable yet (2.10).
- Glass and other blended primitives of a prototype never carry a tint. That keeps `distantGlazing` and `glazingRange` working unchanged.
- Tools that read instancing attributes must accept the extra attributes and apply tint where they bake colour: `city-lod.mjs:213`, `block-proxy.mjs:18, 46`, `bounced-light.mjs:27`, `declared-lod-pack.mjs:46, 208`, `mobile-resources.mjs:33, 82`.

### 2.10 Adapter

*Implemented on 2026-11-22 (workstream F) against a synthetic tinted-instance GLB; no published pack carries tinted instances yet. The pack-side contract is frozen in 8.4. Name the function, not the line: `cityVisibility.ts` moved with the same day's LOD band merging and glazing range edits.*

A batch's tint is Three's `InstancedMesh.instanceColor`. GLTFLoader fills it from `_COLOR_0` of `EXT_mesh_gpu_instancing` and leaves the prototype's geometry shared (8.4). The shared copy and comparison helpers are in `adapters/three/src/instanceTint.ts` (new); they copy whole items, so any accessor layout survives.

| Site | State |
| --- | --- |
| `CityVisibility` constructor, `canSubset` | **Done.** `instanceColor` no longer bars subsets, which also admits tinted batches to LOD selection and to LOD band merging. A mesh whose geometry holds an `InstancedBufferAttribute` is still left out of visibility altogether |
| `CityVisibility.selectPlacements`, `shadowFrame` | **Done.** Both subset fills copy each kept instance's tint with its matrix and upload the rewritten range. A light's caster subset is a clone, so it has the tinted depth program of its source |
| `mergeCityInstances` | **Done.** The tint layout (presence, array type, item size, normalization) is in the grouping key, so tinted copies merge with tinted and untinted with untinted, and tints are concatenated with matrices |
| `clusterCityGeometry` (`cityClusters.ts`) | **Done.** The exclusion is gone; each spatial piece is a clone with the batch's tints. Part meshes are far below the 36,000-index threshold, so this is rarely reached |
| `CityClusterDraws` | **Done.** A pair forms only with one tint layout and joins a level only where both halves' tints are equal; a wide group needs equal tints throughout. The joined draw borrows `instanceColor` with `instanceMatrix`, and releases neither on disposal |
| `CityFarHardware.finish` | **Done (was not listed).** A distant hardware draw compacts instances like a subset, so it copies tints too |
| `distantGlazing.ts`, `glazingRange.ts` | **No change**, given untinted glass (2.9). A tinted pane keeps its own draw. Tested: an instanced, untinted pane is batched and range-culled beside tinted batches |
| `worldSpecMaterials.ts` | **Done.** `applyInstanceRoles` multiplies the tint into vertices of role 1 only, and extends `customProgramCacheKey`. `bindScene.ts` applies it to a material when a mesh drawn with it has `_MS_ROLE`. It is its own composable patch beside the chart patch, so a role material need not be a chart material, and a pack without the attribute gets no change at all, not even to a cache key. The addition is compiled only into the tinted instanced variant of the vertex stage |
| Shadow passes | **Verified.** Canonical casters and per-light subsets are drawn with the tinted depth program. Tint does not affect depth |
| First-frame preparation | **Verified, no change.** `shadowDepthStage` keys on `instanceColor`, `warmupMesh` copies it and `renderer.compile` covers the camera's subsets. One more colour program and one more depth program per material that draws both tinted and untinted batches; none links after the first frame |
| `CityTextureResidency`, `DeclaredLods` | **Read, no change.** Residency measures every instance's bounds from its matrix and never reads tints. Declared LODs choose per node or per mesh material, not per instance, and their compile-only holders already take the receiver's `instanceColor` |
| Palettes over one colour | **Deferred** (owner decision 2). GLTFLoader puts any other instancing attribute (`_PALETTE_n`, or the Rust exporter's `_TINT`) on a per-node clone of the geometry as an `InstancedBufferAttribute`. Such a mesh is left out of visibility, LOD and merging today; subsets would need that attribute reordered on a geometry clone per subset |

Limits to know:

- A tinted batch drawn with a role material must carry `_MS_ROLE`. A missing vertex attribute reads as 0, so a tinted batch without it on such a material would be drawn untinted. Without the attribute anywhere on a material, Three's stock behaviour holds: the tint multiplies every vertex.
- Roles above 1 are treated as untinted.
- The role patch is a WebGL `onBeforeCompile` edit, like every other material patch in the adapter.
- Found while testing, and independent of tint: Three's second and later shadow-map refreshes link new depth programs for instanced casters, because a shadow pass reads the light counts of the previous frame. First-frame preparation covers the first refresh only.

### 2.11 Interactions

- **Step 4a.** Wall prototypes replace it. If 4a ships first, it must skip `worldSpecPartAtlas` materials and may merge part pages with building pages at Low.
- **Step 5.** The part atlas becomes one city-wide array texture, so all opaque part batches share one material and program. Flattened parts need the building material to bind the shared part array beside its own; copying part layers into each building would undo the sharing. The shader variant (instanced, tinted, layer index, optional chart rectangle) is the same for steps 5 and 7: one owner for `worldSpecMaterials.ts`.
- **KTX2.** Part pages are ordinary pages: pad to multiples of four, 2-texel gutters at final resolution. Bake the atlas per tier density rather than shrinking it.
- **Texture streaming.** A city-wide page is near something from most poses, so it will tend to stay resident at full size. Order pages by part class, and measure.
- **Step 9, per-part LOD.** The content key names the entry and the detail level, so entry e at details 0, 1, 2 is three prototypes of one LOD group (the key minus `graphicsDetail`). Feed them to the adapter as authored levels in the existing geometry-to-levels map (`cityLods.ts`, consumed at `cityVisibility.ts:282`), in place of simplified levels. Record the group in `parts.json` now; build it later.

## 3. Stages

Rebuild shorthand:

- **R** = WSL `cargo test` (targets named per stage), plus the Wasm build and parity check.
- **W** = Windows-native `superspec.exe` and `superspec-studio.exe`.
- **H** = `npm run build:headless`, then `node --test tests/headless-building-levels.test.mjs`.
- **A** = `npm run check`, `npm test`, `npm run build:viewer`.

All figures below are estimates.

**Stage 0. Survey.** New `crates/engine/tests/kowloon_prototypes.rs`. Rebuild: R only.

- Report, per part kind over the seven prototypes at details 0 to 2: instances, distinct shapes, triangles, and distinct panel keys.
- This supplies the ratio every later estimate needs. The survey file `generated/graphics-detail/variants/survey.txt` was absent.

**Done.** Run with `cargo test --release -p superspec-engine --test kowloon_prototypes -- --ignored --nocapture survey`; it writes `generated/graphics-detail/placements/survey.txt`. Measured on 2026-11-22, counting all 29 buildings placed from the seven prototypes:

| | Detail 0 | Detail 1 | Detail 2 |
| --- | --- | --- | --- |
| Triangles in the city | 12,246,291 | 1,768,722 | 727,161 |
| Leaf parts (instances) | 10,241 | 10,241 | 8,485 |
| Distinct shapes, all 77 kinds (70 at detail 2) | 428 | 400 | 315 |
| Triangles of one of each distinct shape | 1,077,517 | 165,330 | 62,977 |
| Condensers: instances, prototypes | 502, 4 | 502, 2 | 502, 1 |
| Condensers: triangles, of which distinct | 2,479,356, 19,752 | 111,444, 444 | 8,032, 16 |
| Conduits: instances, prototypes | 1,393, 20 | 1,393, 16 | none drawn |
| Conduits: triangles, of which distinct | 748,152, 38,464 | 83,928, 2,754 | 0 |
| Triangles the two shipped prototypes cover | 3,227,508 (26%) | 195,372 (11%) | 8,032 (1%) |

After the Rust side of stage 2 (same survey, same day; the city's triangles and shapes per kind are unchanged):

| | Detail 0 | Detail 1 | Detail 2 |
| --- | --- | --- | --- |
| Triangles the shipped prototypes cover | 6,498,214 (53%) | 665,045 (38%) | 155,503 (21%) |
| Condensers: prototypes, colourings | 4, 81 | 2, 75 | 1, 72 |
| Window units: placements, prototypes, triangles | 335, 4, 618,758 | 335, 2, 36,740 | 335, 1, 3,350 |
| Windows: placements, prototypes, triangles | 923, 74, 883,164 | 923, 48, 128,192 | 923, 33, 38,530 |
| Sagging tarps | 61, 8, 772,644 | 61, 8, 102,116 | 61, 7, 36,086 |
| Downpipes | 151, 14, 462,288 | 151, 14, 37,708 | 151, 14, 1,208 |
| Sheet canopies | 122, 6, 195,448 | 122, 4, 34,072 | 122, 2, 12,664 |
| Plant rows | 129, 10, 147,496 | 129, 6, 56,613 | 129, 3, 26,713 |

- Windows are 74 prototypes where the survey counts 17 + 5 shapes, because a window's frame, trim and curtain colours are parameters and so content: one shape in two buildings' colours is two prototypes. They share within a building and between buildings of one family.
- The largest kinds still outside are `shop/room` (2.22M), `shop/stock` (1.12M), `floor/cage` (0.49M), the roof tank (0.24M), `floor/rack` (0.23M), `shop/clutter` (0.23M) and the duct brackets' straps (0.22M).

Before it:

- The condenser is the largest part kind at detail 0: a fifth of the city's triangles in 4 shapes. The 502 instances are 116 in the seven prototypes, in 92 colourings.
- **Corrected:** the first draft cited 105 condensers in 2 shapes (`runtime-efficiency.md:70`). Stage 0 finds 116 authored (502 placed) in 4 shapes at detail 0, 2 at detail 1 and 1 at detail 2, which are the budgets 4, 2 and 1.
- Every placement is a turn about Z: one gravity class, no reflected prototype.
- After the condenser, the triangles are in `shop/room` (2.22M in 10 shapes, 52 instances), `shop/stock` (1.12M, 9 shapes), `floor/window` (0.83M, 17 shapes, 867 instances), `shop/tarp` (0.77M), `aircon/unit` (0.62M, 4 shapes, 335 instances) and `floor/cage` (0.49M, 10 shapes, 283 instances). Windows, window units, cages, racks, plants and canopies are the stage 4 types with many instances per shape; the shop rooms are few instances of unique shapes and gain little from placement.
- Of the owner's three figures (section 7), this survey reproduces two: 12.25M is the city's triangles at detail 0 and 1.08M the triangles of one of each distinct shape. It does not produce 2.06M, nor the runtime plan's 2.51M; neither was reconciled.

Wall panels (2.7 step 1). The same at every detail, since a panel reads no graphics detail:

| Panel part | Slot | Instances | In the seven prototypes | Size classes | Without openings | With openings | With features |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `wall` | `wall` | 580 | 131 | 26 | 26 (54) | 107 (131) | 115 (131) |
| `tiles` | `tile` | 535 | 122 | 2 | 2 (7) | 2 (7) | 2 (7) |
| `parapet` | `wall` | 120 | 29 | 13 | 13 (27) | 25 (29) | 25 (29) |
| `plinth` | `plinth` | 107 | 23 | 3 | 5 (14) | 5 (14) | 5 (14) |
| `dado` | `dado` | 77 | 20 | 12 | 15 (15) | 15 (15) | 15 (15) |
| `tileStart`, `tileEnd` | `tile` | 77 each | 16 each | 3 | 3 (5) | 3 (5) | 3 (5) |
| `dadoEnd` | `dado` | 24 | 7 | 6 | 6 (6) | 6 (6) | 6 (6) |

The last three columns count distinct content keys across the city; in brackets, the sum of the distinct keys inside each prototype.

- **No wall panel repeats inside a building today.** The 131 walls of the seven prototypes are 131 distinct shapes once their openings count. Across prototypes 24 coincide (107 distinct), and 8 of those differ again by their features.
- Without openings the 131 walls are 26 size classes (54 summed per prototype). Sharing walls at Low therefore cannot be by identical content; it must be by size class with crop or tile, as 2.7 step 2 says, and it loses each wall's openings-aware zones and its `peel`, `repair` and `rust` features.
- The small panels already repeat: 122 sill tile panels are 2 keys, and they carry zone rules without changing that.
- The building item states no material for `wall`, `tile`, `plinth` or `dado`. Their requests are bound by the host (the city project's mappings), so the building's colour does not reach the wall through the item's `materials`.

**Stage 1. Engine model plus condenser and conduit.** Rebuild: R, and `npm run check:architecture`.

**Done.** Tests are behavioural: invariants and relationships, no digests, golden files or triangle pins. Generic ones are in `crates/engine/tests/item_placements.rs` against a test host; Kowloon ones in `crates/engine/tests/kowloon_prototypes.rs`; the shared rebuild check is `tests/support/placements.rs`.

- Seeds with one entry and one span give one prototype key; a different entry, span, detail or generator identity gives another. A larger budget's entry folds onto the smaller's.
- Placing prototype meshes through `placed()` reproduces the expanded meshes corner for corner, over 40 seeds, two rooms, three details and budgets up to 64, and for a whole building. Positions and normals to 1e-9 m. **Corrected:** colours to 0.5/255 rather than exactly, because vertex colours are rounded to 1e-3 (finding 14).
- A building's meshes, nodes, parts, features, anchors, bounds, collision, attachments, materials and `geometry_key` are equal with and without placements. **Corrected:** this is a new test against a host wrapper that describes no prototype, not an extension of `kowloon_variants.rs`, which another change was editing.
- A kept part keeps its placements on `rebuild`, and the rebuilt item's placements and prototypes equal a fresh build's.
- **Corrected:** a mirror gives a distinct key and a proper transform; a non-Z rotation keeps the key and gives a distinct `down()`.
- A prototype is drawn once for all the parts of an item that place it.
- **Corrected:** "Native and Wasm packets are byte-equal to before" is not a test, since it would be a golden comparison. It follows from the equal expanded meshes and from nothing serializing the new fields (2.2); the Wasm crate was checked to compile.

**Stage 2. Sidecar and part bake for the two types.** Rebuild: R, W, H.

**Rust side done** (R run; W and H are needed before a tool sees it). Tests, all behavioural, in `crates/engine/tests/item_placements.rs` and `kowloon_prototypes.rs`:

- A `prototype` part's meshes are the prototype a building holds, value for value, at every detail; an unknown generator fails at `/parts/0/generator/name` with `ITEM_NATIVE` and a source offset, and refused content at `/parts/0/generator`.
- A real building's metadata, through JSON text: every placement's meshes exist in the scene and in the city's own compilation under the owner prefix, no mesh is covered twice, every transform is affine with a positive determinant and is the same wherever the building stands, and the prototypes drawn from the record alone and placed by it are the scene's meshes, corner for corner, in position and colour.
- A library project delivers each prototype with its vertices in the prototype's order and the roles listed per vertex.
- Every condenser prototype has the one role `body`; instances of an entry are bit-equal in shape and in every colour that is not the case paint.
- Fixtures of one entry and the same parameters are one prototype with at most one role, placing it rebuilds the fixture, and a cage or a drying rack is not described.

Still to do, tool side:
- In the washroom scene (`tests/studio/washroom`, via `tests/material-washroom-browser.mjs` and `tests/material-bake-keys-browser.mjs`), place its vent twice with a Z turn and assert:
- one raster for two placements;
- moving a placement bakes nothing;
- changing density, gravity or a material parameter rebakes;
- an X-tilted placement bakes separately.
- The building bake's wall receivers have the same receiver keys as before.
- The wall under a condenser keeps its drip at Medium.

**Stage 3. Pack and adapter for the slice.** Rebuild: A; none for Rust. The adapter work can start on day one against a synthetic tinted-instance GLB (`tests/batch-scene-browser.mjs` is the precedent).

Tests (**adapter side done**, against the synthetic pack of 8.4; the pack side is not built):
- A tinted batch selects LOD bands per instance, and subset colours follow subset matrices.
- Merged batches keep per-instance colour.
- No shader program is first linked after the first frame.
- A shadow pass draws the tinted batch.
- **Added:** role-0 vertices are not tinted (GPU pixels); clusters, joined cluster draws and distant hardware draws keep tints; an instanced, untinted pane is still batched; a pack without roles renders identically.

Measure with `npm run bench:published-city` on `market,close-shop,city` against a repeated control.

Expected for the slice:
- Condenser rasters fall from one per instance to one per prototype. **Corrected by stage 0:** 116 condensers in the seven prototypes (502 placed) in 4 prototypes at detail 0, so at most 4 rasters per material binding in place of 116. Conduits: 321 authored in 20 prototypes.
- Street draws change by about −5 to +8.
- Building pages change little.
- The slice proves the path; it does not move the totals.

**Why the condenser.** It has few shapes, the plan already names it as step 6's first part (`runtime-efficiency.md:276-277`), and it exercises palette, a drip feature (`kowloon-aircon.item.json:56`) and wall mounting. Conduit is the control with no palette.

**Stage 4. Remaining catalogued types.** Window units, windows, cages, plant rows, drying racks, sheet canopies and tarps, in that order. Rebuild: R, W, H per batch of generators.

Expected at Medium (the 20% is measured, the rest are estimates):
- Parts are 20% of baked building texels (`runtime-efficiency.md:64`). That share shrinks by distinct prototypes ÷ part instances, to be taken from stage 0.
- About 50 to 60 of the 340 building materials go, replaced by 6 to 10 part pages.
- Street draws about −35 to −55 of 621.
- Pack triangles fall by roughly 1.2M, using the plan's own figure (`runtime-efficiency.md:322`).
- Download about −150 to −220 MB of 1.26 GB.

**Stage 5. Tier atlases and profile wiring.** Bake the part atlas at 0.25 and 0.125 density, mark `assets.weathering` and `assets.partInstancing` consumed, and record them in `worldSpecDelivery`. Rebuild: H, A. Test: `tests/graphics-profiles.test.mjs` resolves each preset to its sharing mode.

**Stage 6. Wall panels** (2.7), after its measurement. Rebuild: R, W, H. Expected at Low: the 70% shell share (`runtime-efficiency.md:63`) falls to a few shared pages per material class, toward the plan's 120 to 150 street draws together with step 5.

**Stage 7. Unique weathering with shared geometry** (2.8), on step 5's array textures.

**Stage 8. Per-part LOD groups** (step 9), and prototypes as batches in the shared model for the live preview.

## 4. Parallel workstreams (disjoint files)

Freeze two contracts first: the trait and struct signatures in 2.2, and the `parts.json` and `parts.atlas.json` schemas in 2.4 and 2.5. D, E and F can then work from hand-written fixtures. **Done:** both are in section 8, and workstreams A and G and the first slice of B are complete.

| | Workstream | Files | Needs |
| --- | --- | --- | --- |
| A | Engine model | `D/compose.rs`, `D/parts.rs`, `D/output.rs`, `D/mod.rs`, new `tests/item_placements.rs` | Nothing |
| B | Theme natives | `K/items/prototypes.rs` (new), `K/items.rs`, `K/aircon.rs`, later `K/items/*.rs`; `tests/kowloon_prototypes.rs` | A's signatures |
| C | Metadata export | `crates/engine/src/authoring/item.rs`, `crates/cli/src/bin/studio/*` | A |
| D | Part bake | new `tools/headless/parts.mjs`, new `bake-parts.mjs`, `tools/headless/building.mjs`, `bake-buildings.mjs`, `editor/city-project.ts` | Schema |
| E | Pack and delivery | new `part-batches.mjs`, `pack.mjs`, `city-lod.mjs`, `block-proxy.mjs`, `bounced-light.mjs`, `declared-lod-pack.mjs`, `mobile-*.mjs` | Schema |
| F | Adapter (**done**, 2.10) | `cityVisibility.ts`, `cityClusters.ts`, `cityFarHardware.ts`, `worldSpecMaterials.ts`, `bindScene.ts`, new `instanceTint.ts`; `worldWarmup.ts` needed no change | Nothing |
| G | Survey and wall measurement | new `tests/kowloon_prototypes.rs` | Nothing |
| H | Profiles | `graphicsProfiles.json`, `graphicsSettings.ts`, `publish-tier.ps1` | Stage 4 |

Constraints:
- `cityVisibility.ts` (64 KB) and `worldSpecMaterials.ts` each need a single owner.
- `worldSpecMaterials.ts` is also step 5's file, so sequence the two.
- `bake-buildings.mjs` is touched by both C's consumer and D: give it to D.

## 5. Risks

- **Draw count can rise** if every prototype is instanced (finding 7). The per-prototype choice in 2.9 is the control. Stage 0 gives the counts for the slice: 4, 2 and 1 condenser prototypes and 20, 16 and 0 conduit prototypes at details 0, 1 and 2.
- **Look changes on shared parts.** They lose position-specific pattern (materials are world-mapped today) and neighbour context. Identical units side by side will repeat.
- **Colour factorization may fail** for generators that mix a palette colour with a constant at draw time, or use `Tint::Field`. The reconstruction test catches it; the generator then stays unshared or is edited.
- **A shared page stays resident**, so memory at Low may not fall as far as texel counts suggest.
- **Block proxies and bounce light** must expand tinted instances, or far views change colour.
- **Bake compile disagreement.** The synthetic library definitions add a second regeneration path beside the building's. The plan's blocked item 1 is this kind of failure. Test: the library's prototype meshes equal the engine's `prototype_meshes` byte for byte.
- **Memory during compile** grows by the distinct prototype meshes.

## 6. Decisions (taken 2026-11-22)

1. **Instancing threshold.** Small or rare prototypes stay flattened with shared texels. A prototype is instanced only past a measured threshold of triangles saved (2.9); the threshold is set from stage 3's draw-count measurement, not from this plan.
2. **Palette size.** One tint per instance. Secondary colours (bracket steel, line-set duct, cover) are owned by the catalogue entry.
   - *Consequence found in stage 1, done in stage 2.* The condenser drew steel, duct and cover from the instance stream, so its prototype reported three to five roles. They are now the entry's, as are the window unit's steel and cover (2.3); `VERSION` is 15. Every shipped prototype has one role or none. The engine model still carries any number.
   - *Open under this decision:* the cage and the drying rack (2.3).
3. **Drips under parts on shared walls** are dropped at Low and Very Low. No stain quads. (These are the wall's own `rust` features: finding 13.)
4. **Wall proxy for mounted parts** (2.6) is accepted: a standard wall slab stands in for per-site contact weathering. `Mount::Wall` is in the model and both shipped prototypes report it.
5. **Wall sharing at Low and Very Low** is approved. The representatives per material and size class are chosen after stage 6's measurement; stage 0's panel table (section 3) is the first input.
6. **Roles by probe passes first.** Implemented in `K/items/prototypes.rs` (`probed`). Explicit roles through `Tint` stay the fallback for a generator whose colours do not factor.
7. **Shared model.** The sidecar is used for publishing. Prototypes move into `crates/model` batches for the live preview in stage 8.

## 7. Not verified

Settled by stages 0 and 1:

- **Verified:** the wall stain under an air conditioner is the wall's own authored `rust` feature (finding 13).
- **Verified:** placements are the same across graphics detail levels. Every level places the same condensers with the same case, duct and steel colours, from fewer prototypes (`kowloon_prototypes.rs`, `every_graphics_detail_places_the_same_condensers_in_the_same_colours`). The conduit draws nothing at level 2, so it has no placement there.
- **Verified:** nothing outside tests serializes `ItemGeometry` or `GeneratedItem` as a whole (search of `crates/`). `authoring/item.rs` and `session.rs` read fields.
- **Verified:** the Stoa buildings place every condenser and conduit by a turn about Z and mirror none (section 3, stage 0), so one gravity class and no reflected prototype exist today.
- **Answered:** how a building's paint reaches the wall material. It is the `color` of a paint-coat layer over `render-paint`, composed by coverage, not a multiplier (finding 18). For stage 6 a shared wall therefore needs either two colour maps (the coat's shade, to be multiplied by the paint, and everything else) or the acceptance that peeled render, grime, soot and rust take the paint's tint. Inside the coat the colour is a factor, as far as the layer's own source shows; the shared `illustrate` helper it passes through was not read.

Settled by the Rust side of stage 2:

- **Answered: no material program reads vertex colour during the bake** (finding 17). The palette split is right for every material.
- **Verified:** the names the building bake's scene gives item meshes (finding 19). `meshes` in the metadata are those names; the test `a_buildings_metadata_places_its_prototypes_onto_the_scenes_meshes` finds each in the project's scene and, under `city/<id>`, in the compiled city scene.
- **Verified:** a project of item models with inline sources and no AML packet prepares and yields a scene (`Session::prepare_project`; the library test). What the `shade-layouts` action of the studio binary returns for one was not run.

Still open:

- What `includeScenePacket: true` returns for a library project through the studio binary.
- **Settled by the adapter work (2.10, 8.4):** `gltfLoader.ts` adds no instancing code of its own; Three's GLTFLoader turns `_COLOR_0` into `instanceColor` and puts every other instancing attribute, `_TINT` included, on a per-node geometry clone. `CityTextureResidency` measures each instance by its matrix and ignores tints. `declaredLods.ts` selects per node or mesh, never per instance, and already compiles its variants with the receiver's `instanceColor`.
- `block-proxy.mjs` beyond the cited lines.
- The reconciliation of the triangle totals: the plan's 2.51M (`runtime-efficiency.md:66`) and the owner's 2.06M against stage 0, which reproduces only 12.25M and 1.08M.
- Findings 4 to 12, and every number in section 3 that stage 0 did not measure.

## 8. Frozen contracts

Frozen on 2026-11-22 after stage 1. Workstreams C to F work from these. Change one only by editing this section and telling its consumers.

### 8.1 Rust (implemented; `superspec_engine::itemspec::definition`)

Host side, `D/compose.rs`. Both methods have defaults, so an existing host compiles unchanged.

```rust
pub trait NativeGenerators: Send + Sync {
    // ... existing methods unchanged ...

    /// Cheap, draws nothing. Instances that return equal `content` differ only by `inputs`.
    fn prototype(&self, _name: &str, _parameters: &Values) -> Option<NativePrototype> { None }

    /// Part-local, the slots `generate` uses, `roles` filled, neutral colours:
    /// a vertex of role k carries its shade of the (k-1)th input in name order.
    /// Called once per prototype of one prepared item.
    fn prototype_meshes(&self, name: &str, content: &Values, cancelled: &dyn Fn() -> bool)
        -> Result<NativeOutput, String> { self.generate(name, content, cancelled) }
}

pub struct NativePrototype {
    pub content: Values,                     // canonical: no seed, part or placement
    pub inputs: BTreeMap<String, [f64; 3]>,  // role name -> linear RGB
    pub mount: Mount,
}
```

Output side, `D/output.rs`.

```rust
pub struct ItemMesh {
    // ... existing fields unchanged ...
    pub roles: Vec<u8>,   // prototype meshes only; 0 = own colour, k = role k-1. Not serialized when empty.
}

pub enum Mount { Free, Wall }   // serialized "free" | "wall"; Wall: wall plane y = 0, standing toward -Y

pub struct Prototype {
    pub kind: String,            // "native"
    pub name: String,            // generator name
    pub identity: String,        // name@VERSION
    pub content: Values,
    pub mirrored: bool,          // reflected in local X; not serialized when false
    pub meshes: Vec<ItemMesh>,   // prototype coordinates, generator slots, neutral colours, roles
    pub features: Vec<Feature>,  // the generator's features for `content`
    pub roles: Vec<String>,      // input names, in name order
    pub mount: Mount,
}
impl Prototype {
    /// sha256("itemspec-prototype-v1\0" + JSON [kind, name, identity, content, mirrored]), lower hex.
    pub fn key(kind: &str, name: &str, identity: &str, content: &Values, mirrored: bool)
        -> Result<String, String>;
    pub fn own_key(&self) -> Result<String, String>;
    pub fn reflected(&self) -> Self;   // x negated, triangles turned, `mirrored` flipped; an involution
}

pub struct PartPlacement {
    pub prototype: String,                   // key into ItemGeometry::prototypes
    pub path: String,                        // part IDs outermost first: "side-1/floor-0/aircon-2/condenser"
    pub node: usize,                         // the node `transform` leads to
    pub transform: Transform,                // [[f64; 3]; 4]: images of X, Y, Z, then translation. Determinant > 0.
    pub inputs: BTreeMap<String, [f64; 3]>,  // one per prototype role
    pub slots: BTreeMap<String, String>,     // prototype slot -> item slot; a dropped slot is absent
    pub meshes: Vec<usize>,                  // expanded meshes covered: one per prototype mesh whose slot is in `slots`, in prototype mesh order
}
impl PartPlacement {
    /// Item-space down in prototype coordinates, unit, rounded to 1e-6. [0, 0, -1] for every turn about Z.
    /// Meaningful for a placement from `ItemGeometry::placed`.
    pub fn down(&self) -> Result<[f64; 3], String>;
}

pub struct ItemGeometry {
    // ... existing fields unchanged ...
    pub prototypes: BTreeMap<String, Arc<Prototype>>,  // not serialized when empty
    pub placements: Vec<PartPlacement>,                // not serialized when empty
}
impl ItemGeometry {
    /// Placements with `transform` composed through `node`: prototype coordinates to item coordinates.
    pub fn placed(&self) -> Result<Vec<PartPlacement>, String>;
}
```

Panel content keys, `D/parts.rs` and `D/mod.rs`. Read-only; nothing is built.

```rust
pub struct PartContent {
    pub path: String,             // part IDs outermost first
    pub kind: String,             // "panel", "box", "route", "arc", "prism", "pipe", "duct", "native:<name>", "item:<entry>"
    pub slots: Vec<String>,       // slots of the outermost item its face groups land in
    pub size: Option<[f64; 3]>,   // panel: width, height, thickness
    pub openings: usize,          // panel: openings and seats
    pub features: usize,          // zone rules plus authored features on the part
    pub bare: String,             // sha256 of (kind, size, slots): a panel without its openings
    pub shape: String,            // sha256 of (resolved shape, slots): the geometry key less ID and placement
    pub featured: String,         // `shape` plus zone rules and authored features in part coordinates
}
impl PreparedItem {
    pub fn part_contents(&self) -> Result<Vec<PartContent>, ItemDiagnostic>;
}
```

Guarantees the tests hold (`crates/engine/tests/item_placements.rs`, `kowloon_prototypes.rs`):

- The expanded meshes, nodes, parts, features, anchors, bounds, collision, attachments, materials and every geometry key are the same with and without placements.
- Placing each prototype's meshes by `placed()` gives the flattened meshes it covers, corner for corner: positions and normals to 1e-9, texture coordinates exactly, colours as shade times input to 0.5/255 (finding 14).
- A prototype key never depends on seed, part ID, placement, rotation or `variantBudget`. It does depend on generator, identity, content and reflection.
- A kept part keeps its placements on `rebuild`; a rebuilt item's placements equal a fresh build's.
- An item with no prototype serializes with no `placements`, `prototypes` or `roles` field.

Kowloon prototypes shipped (`K/items/prototypes.rs`):

| Generator | Content | Roles | Mount |
| --- | --- | --- | --- |
| `kowloon/condenser` | `entry`, `width`, `height`, `plates`, `weight`, `graphicsDetail` | `body` | wall |
| `kowloon/aircon` | its parameters less `seed` and `variantBudget`, with `entry` and `graphicsDetail` | `body` | wall |
| `kowloon/window`, `plants`, `sheet-canopy`, `sagging-tarp` | the same | none | wall |
| `kowloon/conduit`, `security-bars`, `wall-vent`, `downpipe`, `riser-duct`, `cornice`, `roof-outlet`, `wall-lamp`, `balcony`, `balcony-door`, `fan-divider` | its parameters less `variantBudget`, with `graphicsDetail`; only where it is given no `seed` | none | wall |
| `kowloon/rail`, `ladder` | the same | none | free |

Changed after the freeze (2026-11-22, Rust side of stage 2), all additive but the roles:

- Every shipped prototype has one role, `body`, or none (decision 2). A consumer may still meet more: the model allows it.
- Generator identities are `@15`.
- A new part generator draws a prototype as it is: `{"kind":"prototype","name":…,"content":{…},"mirrored":false}` (`Generator::Prototype(PrototypeReference)`, 2.5 and `D/README.md`). Its meshes keep `roles`, so `ItemMesh::roles` is no longer empty on every expanded mesh: it is filled on the meshes of a `prototype` part, and nowhere else.
- `PreparedProject::items()` gives a prepared project's item metadata.
- The trait is unchanged.

### 8.2 `building-N.parts.json` (the metadata is implemented; the file is written in stage 2)

One per baked building, beside its GLB. Everything in it comes from `ItemGeometry::placed()` and `prototypes` of the building item, plus the building's material assignments. Coordinates are the building item's: metres, right-handed, Z up.

`prototypes` and `placements` below are what the engine now reports for each item model, verbatim, in `metadata.items[<model id>]` of a shading report (`<model id>` is `p0` in the building bake). The tool adds `version` and `building`. A `prototype` part's item also has `meshRoles`: `{ "<mesh name>": [0, 1, 1, …] }`, one role per vertex in the scene's vertex order.

```json
{
  "version": 1,
  "building": "building-3",
  "prototypes": {
    "<prototype key>": {
      "kind": "native",
      "name": "kowloon/condenser",
      "identity": "kowloon/condenser@15",
      "content": { "entry": 3, "width": 0.95, "height": 0.6, "plates": true, "weight": 2.0, "graphicsDetail": 0 },
      "mirrored": false,
      "roles": ["body"],
      "mount": "wall",
      "slots": ["aircon", "paint", "metal"],
      "triangles": 4922,
      "lodGroup": "<sha256>"
    }
  },
  "placements": [
    {
      "prototype": "<prototype key>",
      "path": "side-1/floor-0/aircon-2/condenser",
      "transform": [1, 0, 0, 0,  0, 1, 0, 0,  0, 0, 1, 0,  4.2, 0, 7.4, 1],
      "down": [0, 0, -1],
      "inputs": { "body": [0.61, 0.6, 0.55] },
      "slots": { "aircon": "aircon", "metal": "metal", "paint": "paint" },
      "binding": {
        "aircon": { "entry": "kowloon-aircon.material.ts", "parameters": { "yellowing": 0.25 }, "seed": 7, "layers": [], "mapping": "world", "density": 256 },
        "metal": { "entry": "kowloon-rusted-steel.material.ts", "parameters": { "…": 0 }, "seed": 5,
                   "layers": [ { "entry": "kowloon-foot-wear.material.ts", "parameters": { "…": 0 }, "seed": 31, "follow": { "foot": "foot" } } ],
                   "mapping": "world", "density": 160 }
      },
      "assignments": { "aircon": "aircon-casing", "metal": "aged-metal.p0.metal", "paint": "aged-paint.p0.paint" },
      "meshes": [
        "p0/part/side-1/side-1/part/floor-0/floor-0/part/aircon-2/aircon-2/part/condenser/condenser/aircon",
        "p0/part/side-1/side-1/part/floor-0/floor-0/part/aircon-2/aircon-2/part/condenser/condenser/paint",
        "p0/part/side-1/side-1/part/floor-0/floor-0/part/aircon-2/aircon-2/part/condenser/condenser/metal"
      ]
    }
  ]
}
```

Rules:

- `prototypes[key]` is `Prototype` without `meshes` and `features`. `slots` lists its meshes' slots in mesh order and `triangles` sums them. `mirrored` is always written.
- `lodGroup` is `Prototype::key` computed with `graphicsDetail` removed from `content`: the same entry at details 0, 1 and 2 shares it (2.11). Optional; a reader must not require it.
- `transform` is 16 numbers, column-major: the Rust rows with 0, 0, 0, 1 appended. Its determinant is positive. It leads from prototype coordinates to building coordinates and contains no world placement of the building.
- `down` is `PartPlacement::down()`. It is building-space down, which is world down as long as buildings are placed by turns about Z (`worldspec/city.rs`). A tool that tilts a building must recompute it.
- `inputs` has exactly the prototype's `roles` as keys. Linear RGB.
- `slots` maps prototype slots to building slots. A slot missing here was dropped by an enclosing part; its triangles are not in the building.
- `binding` gives, per prototype slot in `slots`, the material assignment its building slot resolved to: entry, parameters, seed, layers (the material's own, then the item's for that slot, lowest first), mapping mode (`uv`, `object`, `world` or `anchored`) and texel density (the mesh's, else the assignment's, else the project's; `null` where the project states none). It is everything of the raster key (2.5) that the prototype does not already fix. Its hash, used in 8.3, is sha256 of its JSON with object keys sorted and no whitespace. A slot the project binds to no material (a plain surface, or a catalogue material the item states itself) is absent from `binding`: it is not a receiver.
- `assignments` gives, per slot of `binding`, the identity of that assignment in the building's project (the `material` of the mesh's entry in `metadata.shading`). It may name the model and the slot (`aged-metal.p0.metal`, a slot the item weathers), so it is no part of the binding or of any key.
- `meshes` names the meshes covered as the bake's scene names them: `<model id>/` then the flattened name, which repeats `part/<id>/<id>` at every nesting level (finding 19). They are the keys of `metadata.shading`, `metadata.bindings` and the item's `meshSlots`; the delivered city scene has the same names with the building's owner (`city/<id>`) in place of the model id. One per prototype mesh whose slot is in `slots`, in the prototype's mesh order, which is the order of `prototypes[key].slots`.
- `path` is a source identity. It appears in no key.
- A file with no placements has empty `prototypes` and `placements`.

### 8.3 `parts.atlas.json` (specified; written in stage 2)

One per city and density tier, beside `parts-pbr.glb`.

```json
{
  "version": 1,
  "producer": "<producer identity>",
  "densityFactor": 1,
  "pages": [
    { "index": 0, "alpha": "opaque", "width": 2048, "height": 2048,
      "textures": { "baseColor": "parts-0.basecolor.png", "normal": "parts-0.normal.png", "orm": "parts-0.orm.png" } }
  ],
  "rasters": [
    {
      "raster": "<raster key>",
      "prototype": "<prototype key>",
      "down": [0, 0, -1],
      "binding": "<sha256 of the placement binding>",
      "proxy": "wall",
      "mesh": "prototype/<prototype key>/<raster key, first 16 hex>",
      "blocks": [ { "alpha": "opaque", "page": 0, "rect": [0, 0, 256, 128] } ]
    }
  ]
}
```

Rules:

- A placement finds its raster by three values: its `prototype`, its `down` and the hash of its `binding`. Exactly one raster matches. Two placements with equal triples share texels.
- `raster` is the raster key of 2.5: producer, prototype key, binding, local features and zones, density, `down` and `proxy`. It contains no transform, path or building name.
- `proxy` is `"wall"` for `mount: "wall"` (decision 4) and `"none"` for `"free"`.
- `mesh` names a mesh in `parts-pbr.glb`: prototype coordinates converted to glTF Y-up as the pack converts buildings, `TEXCOORD_0` into the atlas page, `COLOR_0` the shades, and `_MS_ROLE` (unsigned byte, one per vertex) the roles. One primitive per block.
- `blocks` gives each alpha class one rectangle, in texels of its page, origin top left: `[x, y, width, height]`. A prototype's charts for that class lie inside it, so a later stage can translate the block per instance (2.8).
- `pages[].textures` are relative file names. Padding and gutters follow the KTX2 rule of 2.11.
- `densityFactor` is the tier factor (1, 0.25, 0.125); each tier has its own file and pages.

### 8.4 Instanced part batches in the pack GLB (implemented in the adapter; written by the pack in stage 3)

What `@superspec/three` draws as a tinted instanced batch, checked against Three 0.186's GLTFLoader with a synthetic pack (`tests/browser/tinted-instances.fixture.ts`). The pack writes this; the adapter needs nothing else.

```json
{
  "extensionsUsed": ["EXT_mesh_gpu_instancing"],
  "nodes": [
    { "name": "part/<prototype key>/opaque", "mesh": 7,
      "extensions": { "EXT_mesh_gpu_instancing": { "attributes": { "TRANSLATION": 40, "ROTATION": 41, "SCALE": 42, "_COLOR_0": 43 } } } }
  ],
  "meshes": [
    { "primitives": [ { "material": 3, "indices": 30,
        "attributes": { "POSITION": 31, "NORMAL": 32, "TEXCOORD_0": 33, "COLOR_0": 34, "_MS_ROLE": 35 } } ] }
  ]
}
```

Per instance, on the node's `EXT_mesh_gpu_instancing.attributes`:

- `TRANSLATION`, `ROTATION`, `SCALE` as the extension defines them. The node's own transform must be the same for every node that should merge into one draw (identity, as `street-batches.mjs` writes it).
- `_COLOR_0`: the instance's one tint. `VEC3`, `FLOAT`, linear RGB, one per instance, in instance order. It becomes `InstancedMesh.instanceColor` and multiplies the vertex colour in linear space. Values above 1 are allowed. (By a read of the loader, `VEC3` or `VEC4` of normalized unsigned bytes or shorts is passed through with its layout and the shader reads the first three components. Only float `VEC3` was run. Float is the contract because a dark linear tint loses precision in bytes.)
- **No other attribute.** GLTFLoader gives the node its own clone of the geometry for any other name (`_TINT`, `_INSTANCE`, `_PALETTE_n`) and stores it there as an instanced attribute. That mesh is then left out of visibility, LOD, merging and clusters. The Rust exporter's `_TINT` (`crates/export/src/gltf.rs`) is therefore not read as a tint by the adapter: a pack must write `_COLOR_0`.
- A node with no `_COLOR_0` is an untinted batch and draws exactly as today.

Per vertex, on the prototype's primitive:

- `_MS_ROLE`: `SCALAR`, `UNSIGNED_BYTE`, not normalized, one per vertex. 0 keeps the vertex's own colour; 1 multiplies it by the instance's `_COLOR_0`. Any other value is treated as 0 for now (one tint per instance, decision 2). It is the same attribute `parts-pbr.glb` carries (8.3); Three names it `_ms_role`.
- `COLOR_0`: the vertex's shade factor for a role-1 vertex, and its own baked colour for a role-0 vertex, linear, as 8.3 has it. A role-1 vertex is drawn as `COLOR_0 × _COLOR_0 ×` texel.
- The role lives on the vertex and the tint on the instance. Neither is in the material, so one part-atlas material serves every prototype, tinted and untinted.

Rules:

- **Every tinted batch of a material whose meshes carry `_MS_ROLE` must carry `_MS_ROLE` itself.** A missing vertex attribute reads as role 0, so such a batch would be drawn untinted. A material none of whose meshes has `_MS_ROLE` keeps Three's stock behaviour, where the tint multiplies every vertex: correct for a prototype that is all one role, without the attribute.
- Tinted and untinted nodes of one prototype may share its mesh and material. They become two draws (two shader variants), never one.
- All tinted nodes that should merge must use one `_COLOR_0` accessor layout.
- Blended primitives (glass) carry no `_COLOR_0`. A tinted pane would keep its own draw and lose distant batching and range culling.
- Copies merge across buildings when they share the primitive (same glTF mesh and primitive), material, node transform, layers and shadow flags. Publish one mesh per prototype and alpha class and reference it from every node.
- LOD levels come from the city LOD sidecar by primitive, as for any batch. Levels share the prototype's vertex attributes, so roles and shades carry over.
- Flattened parts (2.9) fold the tint into `COLOR_0` and carry no role and no instance attribute. The adapter draws them as ordinary building geometry.
- Not supported yet: more than one tint per instance, and per-instance chart rectangles (2.8).

Guarantees the tests hold (`tests/browser/tinted-instances.test.ts` in `npm run test:city-runtime`; `tests/tinted-instances-browser.mjs` on a GPU, `npm run test:tinted-instances:browser`):

- Each instance in view is drawn exactly once, at its own LOD level, with its own tint, through merging, subsets, band merging, clusters, joined cluster draws and distant hardware draws.
- A light's caster subset keeps each kept instance's tint, and the shadow pass draws the tinted batch.
- On the GPU, a role-1 vertex shows shade × tint and a role-0 vertex its own colour, for every placement; without `_MS_ROLE` every vertex is tinted.
- After first-frame preparation the shadow pass and the colour pass link no program for tinted batches.
- A pack without `_MS_ROLE` has no material changed. The installed Medium and Low packs render bit-identically before and after, in the `market`, `close-shop` and `city` views at 1920×1080.
