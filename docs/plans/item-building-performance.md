# The item building: where its time and memory go

Status: measured 2026-11-18 on the merged shop house (`MODEL=building LANE=1 RESOLUTION=1024 TEXELS=160`,
540 receivers, 77 M atlas texels, 263 k triangles), RTX 5060 Ti through ANGLE/D3D11.

Measure with `tests/item-building-perf.mjs` (the Studio: cold, reopened, one control changed; `perf.json` and two
pictures) and `crates/engine/tests/item_building_bench.rs` (the engine alone; `tools/perf/item-building-project.mjs`
writes its project). `tools/perf/compare-pictures.mjs` compares two runs' pictures. Compare pictures with ambient
occlusion off (`ao=off`; `AO=off` for `tests/item-facade-browser.mjs`): with it on, two runs of one build differ.

The viewport states the stages of a scene's opening (`data-scene-open`) and of its bake (`data-scene-bake`).

## Switches (page parameters, for comparisons)

| Parameter | Effect |
| --- | --- |
| `bakememory=<MiB>` | Everything the bakes hold of the GPU: maps on screen first, kept textures while they fit. The building needs about 1.9 GiB on screen. |
| `bakekeep=all` | A mesh's maps hold every texture of its pass graph, as before `NodeSpec.transient`. |
| `bakepace=timer` | Bands a zero timer apart at a fixed 131072 texels, as before the GPU-paced bands. |
| `bakeband=<texels>` | A fixed band size. |
| `livebits=8` | Live argument textures as bytes (see below). Not the default. |

## Texel density by material, slot or mesh

A project's `texelDensity` is every receiver's. `texelDensities` states another by material assignment id (every
receiver of the material; an item's weathered slot takes its bound material's) or by mesh id (which comes first); an
item model's `slotDensities` states one for the meshes of one of its slots.

```json
{ "texelDensity": 160,
  "texelDensities": { "poster-print": 640, "sign-board": 640 },
  "models": [{ "id": "building", "kind": "item", "slotDensities": { "lettering": 800 } }] }
```

`resolution` still bounds each receiver's texture: one that cannot reach its density keeps the resolution at the
density that fits. The CPU bake, the layouts the GPU bake draws from and field bakes read the same density
(`Request::density_of`). Signs and posters at 640 cost the building 59 MiB more of 1868.

## Texture formats: what was checked

- **Live argument textures** (RGBA16F, two per receiver of the metal and paint materials; 698 MiB of the 1868 MiB on
  screen). As bytes (`livebits=8`) they take a quarter of that. Read back, the arguments reach 1.02 and 1.45 in two
  channels (18% and 1% of those channels' texels are above 1), which bytes cut off. Pictures: 1.9% and 4.7% of pixels
  differ in the two test views, none by more than 5 of 255 and almost all by 1 or 2 (mean 0.008 and 0.019 of 255).
  It is small but it is not nothing, and the cut-off is a loss by construction, so bytes stay opt-in. A format that
  keeps the range (a per-channel scale stated by the program, which the layout's `scale` and `offset` already carry
  to the material) would make it exact enough to default; the engine would have to state each argument's range.
- **Properties** (RGBA8 for roughness and metalness): the standard material reads green and blue, so a two-channel
  texture needs a patched lookup. 393 MiB would become 196. Not done.
- **Intermediates** (heights, offsets, planes) are no longer held by the maps on screen; only what fits is kept.

## Sharing the bakes of repeated fixtures (not implemented: it changes pixels)

Today every receiver is baked on its own. The key of a bake holds the receiver's atlas with its world positions, and
materials are mapped in world or anchored space, so two windows of one shape get different texels (their noise and
wear continue across the building rather than repeat). 414 of 5430 textures are shared.

The design, if repeats may look alike:

1. A material mapped `object` on an item part is evaluated in the part's own frame. The layout a host bakes from is
   stated in that frame (positions and normals before the placement), with the placement beside it.
2. The bake's key is the program, its values, that object-space layout, the features in the part's frame and gravity
   in the part's frame. Two placements of one part with one orientation then resolve to the same nodes, which the
   pass graph already shares by key; rotated placements share when the program does not read gravity.
3. The renderer draws such placements as one instanced batch (`batch.ts` already bakes a batch once, on its
   template), which also removes their draw calls.

Expected on the building: 68 shapes are repeated across 194 of 560 meshes with one orientation and material, so 126
fewer bakes and draws (22%), 12.5 M of 75.3 M atlas texels (17%, about 300 MiB of maps on screen and the same share
of the bake) and 13 k vertices. What is lost: each repeat carries the same stains and scratches as its twins unless
a per-instance value varies them (the batch already offers `value` and `tint` per instance), and weathering that
follows the wall (runoff under a sill) stays on the wall, which is not shared. A seed per instance read as a live
argument would keep variety without separate bakes.

## Vertices and draw calls

- Native generators emitted every corner anew. Their meshes now name each vertex once (`ItemMesh::share_vertices`):
  611 045 to 435 998 vertices for the building, nothing drawn differently.
- Whole-mesh instancing would save only 13 k more vertices: the shop stock is already merged into 17 meshes.
- 569 draws cost the page about 7 ms a frame and the GPU about 6 ms, inside a 60 Hz frame. Each baked receiver has
  maps of its own, so merging draws needs shared atlas pages or the instancing above. The 20 or so meshes of unbound
  slots (stock, tarps) could be merged by material without that; it would remove about 3% of the draws.

## What is left, by size

1. Opening (about 6.6 s to the first picture after the file is read): the engine's description 1.9 s (contacts and
   the 23 MB answer), the per-pixel looks' compile about 2 s, preparing the looks 0.7 s.
2. The bake (about 8 s): now mostly the GPU's own time on the walls' eight layers; the slowest program draws about
   500 texels a millisecond.
3. An edit re-describes the whole project (1.6 s) and bakes every receiver whose inputs changed.
