# Runtime efficiency plan

Written 2026-11-22. This plan covers triangle count, draw calls, GPU memory and
download size for published WorldSpec cities, measured on the Stoa city that
Klali plays. It records what was measured, what was decided, what has been
built, and the order of the remaining work.

## Target

Klali runs at 100% render resolution and 100 fps on an iPhone 17 Pro, ideally on
the Low preset. Savings that do not change the picture apply at every preset.

The working model for a phone in a browser:

| Resource | Availability | Consequence |
| --- | --- | --- |
| Network | Scarce, and slow to improve | Ship the smallest form and expand it on the device |
| Memory | Tight | The expanded form has to fit; never decode to full uncompressed size |
| CPU and GPU | Plentiful | Spend compute on transcoding, generation and instancing |

A preinstalled game on a gaming PC inverts this: download size is free and the
GPU is the limit. The settings design in [Graphics budgets](#8-graphics-budgets)
keeps those cases separate.

## Decisions

All by the project owner on 2026-11-22 unless noted.

| Decision | Scope |
| --- | --- |
| Every preset renders at 100% resolution. Klali's SteadyFrame governor may go lower during play. | All presets |
| Low and Very Low may be baked at their own, reduced texture density. | Low, Very Low |
| Very Low drops normal and occlusion maps. | Very Low |
| Shop goods may be vertex-coloured, without atlas textures. | Low |
| Timber windows keep their lighter worn-edge tint at Low. Very Low does without it. | Low, Very Low |
| The procedural generators may change authored output so that each part type has about five shared variants. | All levels |
| GPU-compressed textures, after a before-and-after check on the washroom scene. | All presets |
| Identical parts share one set of baked texels, and identical wall panels repeat the same weathering. | Low, Very Low |
| No fixture (shop spot light) shadows. | Low, Very Low |

| Medium keeps the full authored texel density. | Medium |
| Identical parts share baked texels; walls stay unique. | Medium, High |
| Every surface keeps unique weathering. | Very High, Ultra |
| The variant budget per part type grows with the preset: 1 at Very Low, 16 at Very High, 64 at Ultra. | All presets |
| A frame-rate governor may lower shadows and LOD before resolution. | Run time |
| Prototype generating the city on the device. | Delivery |

## Measured baseline

Desktop measurements use the packaged viewer at 1920×1080 on an RTX 5060 Ti.
They show counts reliably. Frame times on that machine say little about a phone.

### Where the cost comes from

One cause explains most of it. The building bake gives every surface instance
its own texels on per-building 2048-pixel atlas pages. Each page is one
material, one draw, and three to five full textures.

| Measurement | Value |
| --- | --- |
| Building materials across the seven prototypes | 340 (184 mixed-surface pages, 156 concrete pages) |
| Share of potential draws that are buildings | 82% (1,816 of 2,213 at Low) |
| Share of baked building texels on the shell (walls, bands, tiles, parapets) | 70% |
| Share on instanced parts | 20% |
| Atlas page fill | 52% |
| Full-detail triangles that are exact duplicates of another part, allowing rotation | 40% (2.51M, of which 1.50M are distinct) |
| Window AC units: instances and distinct shapes | 68 and 64 |
| AC lines and hoses: instances and distinct shapes | 278 and 207 |
| Window frames: instances and distinct shapes | 188 and 110 |
| Condensers: instances and distinct shapes | 105 and 2 |

Parts differ because each instance has its own seed and draws continuous
dimensions from it. The compiler tracks part identity and supports instanced
batches end to end, but nested building parts are flattened into building
coordinates, the bake keys each surface by its position, and the viewer refuses
LOD and culling on tinted instances.

### Per preset, before this work's remaining steps

| | Medium | Low | Very Low |
| --- | ---: | ---: | ---: |
| Download | about 1.26 GB | about 230 MB | about 80 MB |
| Textures resident, with mipmaps | about 21 GB | about 1.26 GB | about 340 MB |
| Draws, street | 621 | 672 | 645 |
| Draws, city overview | 1,312 | 1,366 | 1,218 |
| Triangles, city overview | 2.64M | 1.38M | 1.41M |

Low and Very Low are the interim packs: the Medium bake with smaller textures.
They do not contain the reduced buildings yet. Medium has no texture streaming
in the installed pack, so every image is resident.

All draws at Low and Very Low are the main colour pass. The sun shadow is cached
and costs no city draws per frame. At Low the overview binds 577 materials,
switches shader program 390 times and binds textures about 3,000 times a frame.
Each LOD band of a batch is a separate draw, so LOD adds 587 draws in the
overview while removing 11M triangles.

### One building at each detail level

Measured from generated buildings, four floors.

| Building | Full | Low | Very Low |
| --- | ---: | ---: | ---: |
| Two open shops | 468,272 | 66,980 | 26,468 |
| No open shop | 218,768 | 31,306 | 15,710 |

### Phone

The game did not reach a first frame in reasonable time on the phone over the
dev server. A default join loads the small `stoa.glb` arena (512 KB) and about
52 MB in total, mostly Klali's character animations, hair, outfits and
navigation data. So that failure is not explained by the city packs. The cause
is not yet known. A production build is served beside the dev server for
comparison, and the dev server now reports the phone's errors.

## Done

| Change | Where |
| --- | --- |
| Mobile packs keep distance LODs, bound near-geometry error (Low exact, Very Low 3.5 cm), and scale textures in proportion | `tools/kowloon-export/mobile-geometry.mjs`, `city-mobile-delivery.mjs` |
| Bounce lighting on at every preset; all presets at 100% resolution; no fixture shadows at Low | `adapters/three/src/renderProfile.ts`, `graphicsSettings.ts` |
| Viewer accepts LOD sidecars in folders with dots | `adapters/three/src/cityLods.ts` |
| Weed underpainting reduced level and preset-scaled detail distance; painted lawn beds culled beyond 30 m; no baked grass fallback at Very Low | `mobile-geometry.mjs`, `cityVegetationLod.ts`, `paintedLawn.ts`, `graphics-visibility.mjs` |
| A cheaper rendition can become the near level at a lower preset, with per-preset switch distances | `adapters/three/src/graphicsVisibility.ts` |
| Street tree LOD chain (14,342 → 5,006 → 2,228 → 1,028 triangles). Reaches a city on its next publish. | `themes/kowloon/authoring/street-trees.mjs` |
| `graphicsDetail` level 0–2 for buildings, with reduced hardware, windows, shops, cloth and props | `themes/kowloon/graphics_detail.rs` and the generators; AML `graphics-detail` |
| City compile at one detail level | `tools/kowloon-export/compile.py --graphics-detail` |
| Variant catalogue (step 6) for condensers, window units, AC lines and hoses, windows, cages, plant rows, drying racks and sheet canopies: `variantBudget` on the building items and natives, defaulting to 4, 2 and 1 at detail 0, 1 and 2 | `themes/kowloon/variants.rs`, `aircon.rs`, `items.rs`, `items/kowloon-aircon.item.json`; survey and checks in `crates/engine/tests/kowloon_variants.rs` |
| Reduced roof decks are one face and one chart each (level 1: 10 triangles with the deck's edge, level 2: 2), in place of a face per tile | `themes/kowloon/items/street.rs` `tile_deck` |
| Coincident faces of two materials removed from the roof slab, shop rooms, fascias and duct fittings; survey and check | `crates/engine/tests/kowloon_coincident.rs` |
| Klali: renderer 0.4.2 and the interim Low and Very Low packs installed; dev-only client error reporter | sibling `klali` checkout |

## Blocked and in progress

1. **The building bake does not see the detail level.** Baking a
   `graphics-detail="2"` building fails with "Balcony receiver lost original
   triangles", because the bake's own compile of the building disagrees with the
   exporter's. Until this is fixed no Low or Very Low city can be published.
   Being fixed now, with a test that compiles one building through both paths.
2. **Thirteen `superspec-engine` test targets fail**, from work that predates
   this plan. Being diagnosed.
3. **Smaller open defects**: duct bracket straps are 11 mm at reduced levels,
   under the 20 mm rule; `hardwarebounce=coarse` raised a WebGL error.
4. **Why the phone fails to load** is unknown.

## Work plan

Steps are ordered so that each unblocks the next. "Neutral" means no visible
change.

### 1. Publish the tiers

- Fix the bake path (above), rebuild the Windows-native tools and the headless
  bake bundle, and publish Medium, Low and Very Low with
  `generated/graphics-detail/publish-tier.ps1`.
- Publish Medium with texture streaming on.
- Install into Klali with `generated/graphics-detail/install-klali.mjs` and
  measure in the viewer and on the phone.

### 2. Neutral runtime wins

Adapter only, every preset.

- Sort opaque draws by shader program before material.
- Draw a batch at its finest selected LOD level when the triangle cost is small,
  so LOD bands stop multiplying draws.
- Do not allocate what a preset never samples: no shadow-cache copy when shadows
  are off; release transcoded mip arrays after upload.

Estimated: street draws 672 → about 550–600, overview 1,366 → about 1,000–1,100.

### 3. Per-tier bake density

Low and Very Low.

- Bake Low at a quarter and Very Low at an eighth of the Medium texel density,
  with the 2-texel chart gutter kept at the tier's own resolution. Shrinking
  finished pages erases the gutter today.
- Lower the layout density in the compiler, or area-filter. A bilinear shrink of
  4× or 8× aliases.
- The tier density must be part of the bake key, or a Medium bake would be
  reused for Low.
- Pack charts across pages and prototypes. Raise the page limit to 4096 at
  Medium, which keeps the same texels on fewer pages.

Estimated pages per building: 25–63 today, 2–4 at Low, 1 at Very Low.

Status: built as a re-layout of the Medium raster, in
`tools/kowloon-export/chart-atlas.mjs` and `bake-buildings.mjs --density`. The
GPU evaluates materials at the authored density (17–30 s a building), and each
chart is then area-filtered to the tier's size with its own 2-texel gutter. A
lower layout density in the compiler would save a few seconds a building, point
sample the procedural materials and need an engine-wide density scale. Measured
on the Stoa prototypes: 25–63 pages become 9–17 at Medium, 6–8 at Low and at
Very Low, and building materials fall from 372 to 111, 67 and 64. Medium page
texels fall by 29%. Low keeps about the same texels and Very Low gains 19%,
because a real gutter around a chart of one or two texels costs more than the
chart; step 4a's sharing and merged charts are what reduce them. Pages are
split by what the publisher decides per material (hardware, kept finishes,
maps), so a building has more pages than its texels need. Medium's distance
levels still collapse through chart seams (`SUPERSPEC_CITY_LOD_SEAMS`), and
with fewer, larger primitives they mix more parts: protecting seams halves
that error and costs about 50% more distant triangles.

### 4. Texture format and content

- **GPU-compressed textures for every role**, as KTX2 that transcodes to ASTC on
  iPhone and BC7 on desktop: a quarter of the GPU memory. Extend the existing
  data-texture path in `city-data-textures.mjs`, pad pages to multiples of four,
  and have Klali serve the transcoder. Check the washroom scene before and
  after. A test encode measured 10–15% smaller downloads than lossless WebP at
  Low; at Medium, colour and roughness roughly double against JPEG unless pages
  shrink first.
- **Emissive** at a quarter of the page edge or in its own atlas. 26 of 28
  emissive pages are almost entirely black.
- **Fold the edge mask into the roughness texture.** Nearly all "occlusion"
  pages are an edge and specular mask, not ambient occlusion.
- **Drop normal pages that are exactly flat.** Neutral.
- **Roughness-only pages where metalness is zero.**
- Low: drop weak normals, or halve normal and roughness resolution. Very Low:
  colour and emissive only, with roughness and metalness as material constants.

The finish-detail images look unreferenced to a glTF reader but are bound
through `extras.worldSpecSurface.detailTexture`. Keep them.

Estimated resident textures: Low about 100–130 MB, Very Low about 15–25 MB,
Medium on a phone about 350–500 MB with streaming.

### 4a. Shared wall texels and merged pages at Low and Very Low

Low and Very Low only. Medium keeps its look; these tiers may change theirs.

Walls and other shell surfaces are about 70% of baked texels, and every surface
of every building has its own. Nothing repeats: not between placements of one
prototype, and not between panels of the same plaster. A wall's variety should
come from a few shared weathering textures and a colour, not from unique pixels.

Done as a post-process of the published pack, in the mobile delivery, before
textures are compressed:

- **Measure the duplication**: wall charts that are near-copies within a
  building, across placements of a prototype, and across prototypes; texels left
  when each material and size class keeps 1, 2 or 4 representative charts.
- **Share texels**: charts of the same material map onto a small set of
  representatives, chosen by a hash of the placement so neighbours differ. No
  stretching; crop or tile.
- **Vary by colour**: a building's wall colour is a tint (vertex colour or
  instance tint) over a neutral shared weathering texture, taken from the mean
  colour of its original chart.
- **Keep unique** what is position-specific: signage, shop art, text, lit window
  patterns.
- **Merge pages**: what remains of a building, or of the city per material
  class, goes onto one or a few atlas pages so its surfaces share a material.
  Gutters of at least 2 texels at the final resolution, dilated, and holding at
  the coarsest mip and streaming companion in use.
- Materials that differ only in values that can be unified are merged. Opaque,
  masked and blended surfaces stay separate.

Why: at Low, after the runtime work, the street view is 423 draws with 329
materials bound, against a budget near 150. Opaque building pages are 63–70% of
street draws, many under 200 triangles each. This acts on about 270 of the 423.

The same sharing belongs in the bake itself (step 7), where a wall panel type is
baked once and placed many times. That version serves every tier; at Medium and
High walls stay unique and identical parts share, as decided.

Status: built as `tools/kowloon-export/mobile-atlas.mjs`, run by the mobile
delivery with `--atlas` (off by default). Tests are in `mobile-atlas.test.mjs`.

| What | How it is done |
| --- | --- |
| Pages | A building's charts are found from its geometry, filtered from the full-resolution pages to the tier's density, and packed onto one page per material class. Charts 16 texels or more across get a 4-texel gutter, smaller ones 2. |
| Primitives | The viewer draws each glTF primitive on its own, so the primitives of a class are joined, with their distance levels. A building is about five primitives. |
| Unified | Hardware loses its edge mask. Surfaces without a finish fold together. Shop fittings take the hardware finish. Lit and unlit glass share a material, and glass is drawn from outside only, in one pass. |
| Arena | The arena's 37 concrete primitives are joined across their meshes, with each placement applied, onto one page, then split into a 3 by 3 ground grid. One primitive for the whole arena lost its occluding walls and raised the street view from 145 draws to about 200. |
| Sharing | Charts of the same shape and layout share texels, two representatives per group at both tiers. A chart is tinted through its vertex colour only when its colour is more than 30% from its representative's. Sharing across chart sizes is not built. |

Measured on the published Stoa city at 1920×1080 with KTX2 textures. "Before"
is the same publisher without `--atlas`.

| | Low before | Low | Very Low before | Very Low |
| --- | ---: | ---: | ---: | ---: |
| Draws, street | 422 | 147 | 409 | 132 |
| Draws, market | 322 | 80 | 307 | 71 |
| Draws, terrace | 523 | 136 | 502 | 127 |
| Draws, city overview | 663 | 235 | 638 | 226 |
| Draws, spawn lane | 270 | 60 | 266 | 57 |
| Materials bound, street | 329 | 77 | 334 | 73 |
| Download | 70.0 MB | 54.5 MB | 17.4 MB | 15.8 MB |
| Textures resident, all near | 160 MB | 118 MB | 28.5 MB | 28.6 MB |
| Textures resident, all far | 38 MB | 12 MB | 6.3 MB | 5.6 MB |
| Streaming pairs | 641 | 71 | 285 | 27 |

Low meets its draw budget (150 street, 250 overview). Very Low does not (110
and 180). Its street view is now fixtures and trim (25), trees (20), building
hardware (14), glass (13), arena floors (12), shop stock (12), arena concrete
(12) and grass (10). None of those are building pages. Trees and grass are
separate placements with their own distance levels, and the stock and floors
tile their textures, so none of them can go onto an atlas page.

A merged hardware primitive is over the viewer's 36,000-index limit, so the
viewer cuts it into cells and simplifies loose fittings by cell. Distant
laundry and condensers go sooner than before. `primitiveIndexLimit: 36000`
keeps them, at about 13 more draws a view.

### 5. One material per building

Every preset, neutral.

- Put a building's pages into layers of one array texture, with a per-vertex
  layer index. Layers do not bleed into each other.
- Move what splits materials today into data: emissive presence and strength,
  edge data, shop fitting, recipe and reflection class as a per-vertex finish id
  into a small table; balcony concrete as a flag. Opaque, blended and masked
  surfaces stay separate.
- Publish single-placement buildings as instanced so they share the program.

Estimated: opaque building draws 457 → about 38 at street level and 966 → about
94 in the overview.

The shader work here is the same as step 7 needs. Build it once.

### 6. Variant catalogue for the generators

Every detail level. This changes authored output, as decided.

- Each part type draws from about five fixed variants, chosen by seed, in place
  of continuous per-instance dimensions: AC unit sizes, line rises and runs,
  window sash angles and curtain shares, cage layouts.
- Variety comes from which variant is picked, where it is placed, and its
  colour.
- Colour becomes a per-instance input. The same variant in two colours is one
  geometry.
- Start with the condenser (largest triangle share, with tint, texture variants
  and a wall drip) and duct straps (a plain control). Measure distinct shapes,
  triangles, texels and draws before and after, with matched screenshots.
- Then AC units, lines, windows and cages, then the rest.
- The pinned full-detail digests in `kowloon_graphics_detail.rs` change here.
  Re-pin them with the reason recorded.

Collision, navigation, features, zones and validation stay as they are. Surveys
read level 0.

Status: the catalogue exists for the part types in the Done table. A budget of
`n` uses the first `n` of a part type's 64 entries, so budgets nest; 0 draws
every instance from its own stream, as before. The pinned digests were replaced
by behaviour checks. Survey with
`cargo test --release -p superspec-engine --test kowloon_variants -- --ignored --nocapture survey`.
A budget other than the detail level's own is stated with `variant-budget` on an
AML `<building>` or `compile.py --variant-budget`, and reaches the bake's
project with the building. Washing on racks and in cages is a catalogue entry's
garments in the fixture's own dyes. Still open: the part types not yet converted
(cables and wiring, tarps and awnings, shop rooms and stock, rooftop kit).

### 7. Parts as placements with a shared atlas

- Natives return a prototype: a content key plus instance inputs. Generic
  ItemSpec carries it and never imports the theme.
- `ItemGeometry` gains placements: prototype key, part path, transform, inputs.
  Flattening composes the transforms.
- Placements become instanced batches keyed by content, merged across buildings.
- Bake each prototype once in object space into a city-wide part atlas. The
  raster key is the prototype content, material program and parameters, local
  features and zones, density, and gravity in part space. It contains no world
  transform and no names.
- Keep source identity, artifact identity and placement identity as separate
  fields.
- Wall panels are prototypes too: a panel of one material and size class is
  baked once and placed many times, with the building's colour as an instance
  input. This replaces the pack post-process of step 4a.
- At Low and Very Low, identical parts and identical wall panels share texels.
  At Medium and High, identical parts share texels and walls stay unique. Very
  High and Ultra keep unique weathering and can still share geometry by carrying
  each instance's chart rectangle and palette as instance attributes.
- The viewer must accept tinted instances in LOD selection, instance subsets,
  clusters and distant glazing. It excludes them today.
- Wall occlusion and field bakes must keep reading the expanded geometry.

Estimated: about a quarter of building texels move to one shared atlas; with
shell sharing the Low pack is about 250 MB before compression. Full-detail
distinct triangles 2.51M → about 1.2–1.3M.

### 8. Graphics budgets

The full design is in the [graphics profiles plan](graphics-profiles.md): every
graphics value becomes a named parameter, the presets a player sees are
configurations of those parameters, and a platform profile caps them for the
device and connection. In outline, split the single preset into independent
budgets, and define profiles as combinations.

| Budget | Controls | Bound by |
| --- | --- | --- |
| Delivery | Download size, codec, streaming, prebuilt or generated on the device | Network |
| Memory | Resident texture and geometry size, texture resolution cap | RAM |
| GPU | Resolution, shadows, lighting model, post effects | GPU |
| CPU | Draw budget, LOD distances, part LOD | CPU |

| Profile | Delivery | Memory | GPU | CPU |
| --- | --- | --- | --- | --- |
| Phone, web | Smallest, streamed | Tight | Moderate | Tight |
| Desktop, web | Small, streamed | Loose | High | Loose |
| Native PC, preinstalled | Largest, prebuilt | Loose | Maximum | Loose |

The presets a player sees move the GPU and CPU budgets. Delivery and memory
follow the device and connection. Today the preset name also selects the pack,
so a phone cannot combine a small download with sharp nearby textures.

Proposed starting budgets, to be corrected by phone measurements:

| | Very Low | Low | Medium and above |
| --- | ---: | ---: | ---: |
| Draws, street | 110 | 150 | 180 |
| Draws, overview | 180 | 250 | 280 |
| Shader programs per frame | 20 | 20 | 30 |
| Resident textures | 40 MB | 150 MB | 400 MB on a phone, 1.5 GB on desktop |

The draw budget assumes Safari costs two to three times the desktop per draw,
with an uncertainty of about ±2×. The memory budget assumes a page limit near
600 MB. Apple publishes no figure.

### 9. Later

- **Per-part LOD**, chosen per instance, including inside 12 m where the city
  LOD always draws level 0 today. The generator-authored levels 0–2 become a
  declared LOD group per prototype.
- **The Very Low building as an instanced far level** of each prototype, in
  place of per-cell block proxies for a city of repeated prototypes.
- **Arena, fixtures, props and trees** onto shared arrays and instancing: about
  60–130 draws.
- **Generate on the device.** The city source is a small scene file and seeds,
  and the compiler builds for Wasm. Prototype one building on a phone and
  measure generation time and what to do about baked weathering before
  committing.
- **Progressive loading**: the spawn cell at low texture resolution first, the
  rest streamed behind it.
- **Out of scope: Klali's game assets.** Character animations, hair, outfits and
  game mechanics belong to the game's own development. They are most of a
  default join's download (about 40 of 52 MB), so first-frame time on the small
  arenas depends on them. This plan covers the world's graphics: maps, their
  textures and the renderer.
- **Unfinished from the reduced renditions**: vines, lawn stroke thinning at
  Low, gate lattices and doors, weed omission beyond 45 m at Very Low, tree
  switch distances scaled by placement size.

## Expected result at Low

Derived from the measurements above, not measured after the changes.

| | Today | After steps 1–5 | After steps 6–7 and the small-draw work |
| --- | ---: | ---: | ---: |
| Resident textures | about 1.26 GB | 100–130 MB | lower |
| Draws, street | 672 | about 210 | 120–150 |
| Draws, overview | 1,366 | about 430 | 190–230 |

## How to measure

- Viewer: `npm run bench:published-city -- <pack.glb> --graphics <preset> --views market,close-shop,city,city-pan,spawn-walk`,
  with `BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist"`. Add `--passes`
  for pass attribution. Compare counts and matched screenshots; repeat timings.
- Triangles by part: `cargo test -p superspec-engine --test kowloon_graphics_detail -- --ignored --nocapture report_triangles_by_part`.
- Phone: Klali's settings menu has **Show FPS** and **Graphics details** (draw
  calls, triangles, preset, resolution). Record them at spawn, the market and
  the city overview, at Low and Very Low, before and after each step. Two such
  points give the phone's real cost per draw and correct the budgets above.
- For any change that should be neutral, compare per-pose work and images
  against a repeated control. For a quality trade, state what was simplified.

## Risks

- Phone numbers are assumptions until measured. The budgets above may be off by
  a factor of two either way.
- Savings in steps 3–7 are scaled from full-detail measurements. No tier pack
  with the reduced buildings has been built yet.
- Compressed textures change pixels slightly, and download can grow at Medium.
- Repeated weathering is visible where identical panels sit side by side.
- The variant catalogue changes full-detail silhouettes.
- The fixture-shadow rebuild redraws the street in one frame at Medium.
- Klali runs sun and fixture shadow passes every frame for moving characters.
  The viewer benchmark does not, so game numbers will be higher.
