# Graphics profiles plan

Written 2026-11-22. This plan turns the graphics settings into one set of named
parameters, and defines the presets a player sees (Very Low to Very High) and
the platform profiles (a phone on cellular, a gaming PC) as configurations of
those parameters. It is the design for step 8 of the
[runtime efficiency plan](runtime-efficiency.md).

## Why change it

Today a preset name decides everything, in four separate places.

| Where | What the preset decides | File |
| --- | --- | --- |
| Runtime budgets | Render scale, LOD biases, anisotropy, shadow sizes | `adapters/three/src/graphicsSettings.ts` |
| Runtime features | Bounce, probes, AO, bloom, outline, AA, grain, warm-up, set by "is this a mobile preset" | `adapters/three/src/renderProfile.ts` |
| Which pack is downloaded | `stoacity.low.glb` for Low, `stoacity.very-low.glb` for Very Low, the full pack otherwise | `graphicsPackUrl` in `graphicsSettings.ts` |
| How that pack was built | Texture scale, near-geometry error, building detail level, streaming | `tools/kowloon-export/city-mobile-delivery.mjs`, `compile.py --graphics-detail`, the publish script |

This has three costs.

- **Quality and platform are tied together.** Low means "the phone pack", so a
  desktop player who picks Low downloads phone assets, and a phone cannot ask
  for a small download with sharp nearby textures.
- **The same number lives in several files.** The texture scale for Low is a
  constant in the mobile delivery script; nothing in the runtime knows it.
- **Some values cannot be set at all.** Codec, which maps a pack carries, and
  whether textures stream are fixed by how the publisher happened to be run.

## The model

Three kinds of thing, kept apart.

| Term | Meaning | Chosen by |
| --- | --- | --- |
| **Parameter** | One named, typed value with a range and a default | Defined once, in the schema |
| **Quality preset** | Very Low, Low, Medium, High, Very High: a named set of parameter values a player picks | Player |
| **Platform profile** | What the device and connection can afford: caps, formats and delivery choices | Detected, or stated by the host |

A session's settings are resolved in this order. Later layers win.

1. Schema defaults.
2. The platform profile's values and caps.
3. The quality preset's values, each limited by the platform's cap for that
   parameter.
4. The player's individual overrides, limited by the same caps.
5. Diagnostics, for development only, which are not capped.

So "Medium on a phone" and "Medium on a gaming PC" are the same preset resolved
under different caps. A player who chooses High on a phone gets the highest
values the phone profile allows, and the settings screen can show which
parameters were capped.

### Build-time and run-time parameters

Parameters fall into two classes, and the difference shapes everything else.

- A **run-time** parameter can change while the game runs: resolution, shadow
  size, LOD distance.
- A **build-time** parameter is baked into published files: texel density,
  codec, building detail level. It cannot change without a different download.

A particular combination of build-time parameters is an **asset variant**, and
each one is a full publish with its own bake. Variants are expensive to produce
and to host, so there are only a few of them. The resolved settings pick one
variant, then apply run-time parameters on top.

```text
platform profile ─┐
quality preset   ─┼─► resolved parameters ─┬─► asset variant (which files to fetch)
player overrides ─┘                        └─► run-time settings (how to draw them)
```

## Parameters

Grouped by the resource each one spends. "Now" is where the value lives today.

### Assets (build-time)

| Parameter | Type and range | Now |
| --- | --- | --- |
| `assets.geometryDetail` | 0, 1, 2: authored, reduced, minimal buildings | `compile.py --graphics-detail` |
| `assets.variantBudget` | Variants per part type; 0 for unlimited | `variant-budget` on an AML `<building>`, `compile.py --variant-budget`; unstated: 4, 2 or 1 by `graphics-detail` |
| `assets.nearErrorMeters` | 0 to 0.05: allowed error of the nearest geometry level | `city-mobile-delivery.mjs` tiers |
| `assets.lodLevels` | List of fraction and error per distance level | `city-lod-worker.mjs` constants |
| `assets.farProfile` | `standard` or `aggressive` | `pack.mjs --far-profile` |
| `assets.texelDensity` | 0.125 to 1 of the authored density | Mobile tiers shrink finished pages; to become a bake input |
| `assets.maxPageSize` | 1024, 2048, 4096 | `bake-buildings.mjs` constant |
| `assets.maps.normal` | `full`, `half`, `strong-only`, `none` | Always full |
| `assets.maps.roughness` | `full`, `half`, `constant` | Always full |
| `assets.maps.edge` | `separate`, `packed`, `none` | Separate |
| `assets.maps.emissive` | `page`, `atlas` | Page |
| `assets.maps.detail` | `on`, `off`: finish-detail maps | On |
| `assets.codec` | `webp-lossless`, `jpeg-png`, `uastc`, `etc1s` | WebP in mobile packs, JPEG and PNG otherwise |
| `assets.codecQuality` | Encoder quality and effort | Constants in `ktx2.rs` |
| `assets.weathering` | `unique`, `shared-parts`, `shared-all` | Unique |
| `assets.partInstancing` | `off`, `geometry`, `geometry-and-texels` | Off |
| `assets.materialLayout` | `per-page`, `array-per-building` | Per page |
| `assets.meshCompression` | `none`, `quantized`, `meshopt` | Meshopt in mobile packs |
| `assets.decorationTier` | Lowest preset at which each decoration class is delivered | `graphics-visibility.mjs` table |
| `assets.vegetation` | Tree chain start level, lawn strokes on or off, weed levels | Mobile delivery and `street-trees.mjs` |

### Delivery

| Parameter | Type and range | Now |
| --- | --- | --- |
| `delivery.variant` | Name of the asset variant to fetch | Derived from the preset name |
| `delivery.textureStreaming` | `off`, `pairs`: low resolution first, full on approach | `SUPERSPEC_CITY_TEXTURE_LODS` at publish; refused for mobile packs |
| `delivery.firstFrameBudgetMB` | Download allowed before the first frame | New |
| `delivery.order` | `all`, `spawn-cell-first` | All |
| `delivery.generation` | `prebuilt`, `on-device` | Prebuilt |
| `delivery.transferEncoding` | `none`, `brotli`, `zstd` for the entry, buffers and JSON | Left to the host |

### Memory

| Parameter | Type and range | Now |
| --- | --- | --- |
| `memory.textureBudgetMB` | Resident texture limit; the viewer evicts or holds low mips beyond it | Unlimited in mobile packs |
| `memory.textureMaxEdge` | Largest mip uploaded, whatever the file holds | New |
| `memory.geometryBudgetMB` | Decoded geometry limit, checked at publish | New |
| `memory.textureWarmupMB` | Extra maps uploaded before play | `loading.textureWarmupMiB` |
| `memory.releaseSources` | Free decoded and transcoded copies after upload | New |

### GPU

| Parameter | Type and range | Now |
| --- | --- | --- |
| `gpu.renderScale` | 0.5 to 1.5, the ceiling | `GraphicsSettings.renderScale` |
| `gpu.renderScaleFloor` | Lowest scale a governor may reach | Klali's SteadyFrame ladder |
| `gpu.pixelBudget` | Largest drawing buffer in pixels | 4 MP constant in Klali |
| `gpu.sunShadowSize` | 0, 512 to 4096 | `sunShadowMapSize` |
| `gpu.fixtureShadowSize` | 0, 256 to 1024 | `fixtureShadowMapSize` |
| `gpu.shadowStyle` | `plain`, `illustration` | `look.shadowStyle` |
| `gpu.bounce` | `off`, `smooth`, `painted` | `materials.bounce` |
| `gpu.reflectionProbes` | On or off | `materials.reflectionProbes` |
| `gpu.ambientOcclusion` | `off`, `on` | `lighting.ambientOcclusion` |
| `gpu.contours`, `gpu.outline`, `gpu.bloom`, `gpu.edgeBoost`, `gpu.grain`, `gpu.shopFinish`, `gpu.shopArt` | On or off | `renderProfile.ts` groups |
| `gpu.antialiasing` | `none`, `display`, `msaa` | `effects.displayAA`, and the host's context |
| `gpu.anisotropy` | 1 to 16, or authored | `materialAnisotropy` |
| `gpu.materialLodBias` | 0.5 to 1.5 | `materialLodBias` |

### CPU

| Parameter | Type and range | Now |
| --- | --- | --- |
| `cpu.geometryLodBias` | 0.5 to 1.5 | `geometryLodBias` |
| `cpu.partLod` | Near level per part type: 0, 1, 2 | New, from per-part LOD |
| `cpu.drawBudget` | Target draws per frame; LOD bands merge to meet it | New |
| `cpu.lodBandMerge` | Extra triangles a view may draw to remove one LOD band draw, 0 to 1,000,000 | `lodBandMergeTriangles` |
| `cpu.glazingDistance` | Metres beyond which window glass is left out, or `all` | `glazingDistance` |
| `cpu.vegetationDistance` | Full-detail weed distance, lawn cull distance, grass draw distance | Constants scaled by LOD bias |
| `cpu.occlusionGrid` | `fine`, `coarse` | `visibility.occlusionGrid` |
| `cpu.clusterDraws` | `wide`, `pairs`, `off` | `visibility.clusterDraws` |
| `cpu.shadowCasters` | `static-cached`, `dynamic` | Host option |
| `cpu.firstFrameShaders` | `off`, `parallel`: link the first view's shaders before its first frame. On at every preset and platform | `loading.firstFrameShaders` |
| `cpu.shaderWarmup` | `off`, `parallel`, `serial`: the fuller preparation after the first frame | `loading.warmup`, `loading.shaderWarmup` |

The existing `diagnostics` groups stay for development switches that are not
settings: culling toggles, GPU probes, the look-editor values.

## Quality presets

A preset states what the player is asking for. It names an asset variant class
and run-time values, and leaves formats and delivery to the platform.

| Parameter | Very Low | Low | Medium | High | Very High |
| --- | --- | --- | --- | --- | --- |
| `assets.geometryDetail` | 2 | 1 | 0 | 0 | 0 |
| `assets.variantBudget` per part type | 1 | 2 | 4 | 8 | 16 |
| `assets.texelDensity` | 0.125 | 0.25 | 1 | 1 | 1 |
| `assets.maps.normal` | none | strong-only | full | full | full |
| `assets.maps.roughness` | constant | full | full | full | full |
| `assets.weathering` | shared-all | shared-all | shared-parts | shared-parts | unique |
| `gpu.renderScale` | 1 | 1 | 1 | 1 | 1 |
| `gpu.sunShadowSize` | 512 | 1024 | 2048 | 4096 | 4096 |
| `gpu.fixtureShadowSize` | 0 | 0 | 512 | 512 | 1024 |
| `gpu.bounce` | painted | painted | painted | painted | painted |
| `gpu.ambientOcclusion` | off | off | on | on | on |
| `gpu.bloom`, outline, contours, grain | off | off | on | on | on |
| `gpu.anisotropy` | 1 | 2 | authored | 8 | 16 |
| `cpu.geometryLodBias` | 0.55 | 0.75 | 1 | 1.2 | 1.4 |
| `cpu.lodBandMerge` | 16000 | 8000 | 1000 | 1000 | 1000 |
| `cpu.glazingDistance` | 32 | 16 | all | all | all |
| `cpu.partLod` near level | 2 | 1 | 0 | 0 | 0 |
| `cpu.drawBudget`, street | 110 | 150 | 180 | 180 | 180 |

Decided on 2026-11-22:

- **Medium keeps the full authored texel density.** It is not halved.
- **Weathering.** Very Low and Low share baked texels between identical parts
  and between identical wall panels. Medium and High share them between
  identical parts only, and keep unique walls. Very High and Ultra keep every
  surface unique.
- **The variant budget grows with the preset**: one variant per part type at
  Very Low, 2 at Low, 4 at Medium, 8 at High, 16 at Very High and 64 at Ultra.
  The sets nest: each preset's
  variants include every lower preset's, so a part library built for the
  highest preset serves all of them.

**Bounce stays `painted` at every preset.** A review suggested `smooth` for the
lower presets to save shader work. An earlier measurement showed `smooth` at
about half the scene brightness of `painted`, so it stays until that difference
is understood.

Ultra, the capture profile for stills and movies, remains future work. Its
asset values are a variant budget of 64 and unique weathering.

## Platform profiles

A platform profile states what the device and connection can afford. It sets
formats and delivery, and caps on what a preset may ask for.

| Parameter | Phone, cellular | Phone, Wi-Fi | Laptop, web | Desktop, web | Native PC |
| --- | --- | --- | --- | --- | --- |
| Limit that binds | Network, then memory | Memory | GPU, memory | GPU | GPU |
| `assets.codec` | etc1s | uastc | uastc | uastc | uastc |
| `assets.meshCompression` | meshopt | meshopt | meshopt | meshopt | quantized |
| `delivery.textureStreaming` | pairs | pairs | pairs | pairs | off |
| `delivery.order` | spawn-cell-first | spawn-cell-first | spawn-cell-first | all | all |
| `delivery.firstFrameBudgetMB` | 15 | 40 | 80 | 150 | none |
| `delivery.generation` | on-device, once proven | prebuilt | prebuilt | prebuilt | prebuilt |
| `memory.textureBudgetMB` | 150 | 150 | 500 | 1,500 | 4,000 |
| `memory.geometryBudgetMB` | 100 | 150 | 300 | 600 | none |
| Cap on `assets.geometryDetail` | 1 at best | 1 at best | 0 | 0 | 0 |
| Cap on `assets.texelDensity` | 0.25 | 0.25 | 0.5 | 1 | 1 |
| Cap on `gpu.sunShadowSize` | 1024 | 1024 | 2048 | 4096 | 4096 |
| Cap on `gpu.fixtureShadowSize` | 0 | 0 | 512 | 1024 | 1024 |
| `gpu.pixelBudget` | 1.5 MP | 1.5 MP | 4 MP | 8 MP | none |
| `gpu.renderScaleFloor` | 0.6 | 0.6 | 0.6 | 0.7 | 1 |
| `cpu.shaderWarmup` | off | off | parallel | parallel | parallel |
| Default preset | Very Low | Low | Medium | Medium | High |

Every number in this table is a starting proposal. The phone rows in particular
wait on device measurements: the memory figures assume a page limit near 600 MB,
and no phone has yet reported frame time, draws or a successful city load.

### Detecting the platform

- The host states it when it knows: a native shell sets `native-pc`.
- Otherwise the adapter infers it. A coarse pointer with touch points means
  phone; `navigator.deviceMemory` and the GPU's maximum texture size separate
  laptop from desktop where they are available.
- Cellular against Wi-Fi uses `navigator.connection` and the `Save-Data` hint
  where the browser provides them. Safari provides neither, so an iPhone starts
  as "phone, Wi-Fi" unless the player chooses a data-saving option. The settings
  screen offers that choice.
- The player can always override the detected profile.

## Asset variants

Each variant is one publish. The resolved `assets.*` values select the nearest
published variant, and the settings screen shows any parameter the variant could
not honour.

| Variant | Geometry detail | Part variants | Texel density | Maps | Codec | Weathering | Serves |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `minimal` | 2 | 1 | 0.125 | Colour, emissive | etc1s | Shared, parts and walls | Very Low; phone on cellular |
| `reduced` | 1 | 2 | 0.25 | Colour, strong normals, roughness, emissive | uastc | Shared, parts and walls | Low; a phone's ceiling |
| `standard` | 0 | 4 to 8 | 1 | All, edge packed | uastc, streamed for web | Shared parts, unique walls | Medium and High |
| `full` | 0 | 16 | 1 | All | uastc, streamed for web | Unique | Very High |

Because the variant budget and the weathering policy now differ between Medium,
High and Very High, those presets differ in build-time parameters and cannot all
be served by one bake of today's pipeline. Two things keep the count at four.

- **Medium and High share `standard`.** With nested variant sets and parts
  delivered as placements, a pack is a shell plus a list of placements into a
  part library. Medium and High then differ only in which library entries their
  placements name, so they are two placement lists over one set of baked files,
  not two bakes.
- **Until parts are placements**, Medium and High use one variant budget
  between them, and High's extra variants wait.

Ultra would be a fifth variant, built only for capture.

A variant records the parameters it was built with in its entry, under
`scene.extras.worldSpecDelivery`, which today holds only the preset name. The
viewer reads them, so it never has to guess what a pack contains.

## Budgets are checked

A resolved profile carries budgets, and both ends check them.

- **At publish.** After packing a variant, the publisher measures download size,
  resident texture size with mipmaps, decoded geometry size, materials, and
  draws at fixed review poses. It fails the publish when a variant exceeds the
  budgets of a profile it claims to serve. The measuring scripts written during
  the efficiency work become this check.
- **At run time.** The viewer holds textures to `memory.textureBudgetMB` and
  merges LOD bands toward `cpu.drawBudget`. The host's governor moves
  `gpu.renderScale` between its floor and ceiling.

## Schema

Version 2 of the render profile. Version 1 files keep working: a version 1
`preset` reads as a version 2 `quality` with the platform detected, and version
1 `overrides` and `diagnostics` map onto the new names.

```json
{
  "version": 2,
  "platform": "phone-wifi",
  "quality": "low",
  "overrides": {
    "gpu": { "sunShadowSize": 512 },
    "cpu": { "geometryLodBias": 0.9 }
  },
  "diagnostics": { "visibility": { "lod": false } }
}
```

`resolveRenderProfile` returns the resolved parameters, the chosen asset
variant, and a list of parameters that a cap or a missing variant changed.

Links stay short: `graphics=low` and `platform=phone-cellular` select named
entries, and a `profile` parameter carries a structured one.

## Where things live

| Piece | Owner |
| --- | --- |
| The parameter schema, presets, platform profiles and resolution | `@superspec/three`: `graphicsSettings.ts`, `renderProfile.ts` |
| One data file of parameters, presets, profiles and variants, read by both the adapter and the publisher | New, beside the adapter source |
| Building asset variants from that file | `tools/kowloon-export`: compile, bake, pack and delivery read variant parameters in place of their own constants |
| Platform detection defaults | The adapter, overridable by the host |
| The settings screen, the governor, and the native shell's platform statement | The host, Klali |
| Theme-specific meaning, such as which decorations a detail level drops | The Kowloon theme |

Profiles are run-time and publishing configuration. They stay outside AML,
except for `graphics-detail` and `variant-budget` on a building, which already
exist.

## Order of work

1. **Schema and resolution**, with today's values. Introduce the parameter
   names, the two-layer resolution and version 2 reading, with presets set to
   exactly what they are now and one platform profile per current behaviour. No
   visible change. Tests: every existing preset resolves to its current values.

   *Done 2026-11-22.* The data file is
   `adapters/three/src/graphicsProfiles.json`, resolved by
   `resolveGraphicsParameters` in `graphicsSettings.ts`. The tests are in
   `tests/graphics-profiles.test.mjs`. Where today's values differ from the
   tables above, the data file holds today's:
   - The `baseline` platform profile has no values and no caps, and is the
     default. The five profiles in the table exist with their proposed values,
     and nothing selects them until step 5.
   - Medium leaves the sun and fixture shadow sizes to the scene (`authored`),
     and High leaves the fixture size to it. The table's 2048 and 512 are what
     a cap compares `authored` with.
   - Ambient occlusion, contours, outline, bloom and edge boost are `auto` at
     Medium and above, which follows the scene's lighting record, and `off`
     below. Shadow style is `authored` at every preset.
   - Low and Very Low state `cpu.shaderWarmup: off`, `gpu.antialiasing: none`
     and `memory.textureWarmupMB: 0` themselves, because the baseline profile
     states nothing. The phone profiles state them as well.
   - `assets.lodLevels`, `assets.codecQuality`, `assets.decorationTier`,
     `assets.vegetation` and `cpu.vegetationDistance` are
     reserved names without a value shape, and are not resolved.
   - A parameter marked `consumed: false` in the data file carries its planned
     values and changes nothing.
2. **Move the publisher's constants into the data file.** The mobile tiers
   table, the detail level and the streaming flag come from variant definitions.
   Publishing the same variants as today produces the same bytes.
3. **Record build parameters in the pack entry** and have the viewer read them.
4. **Separate pack selection from the preset.** `delivery.variant` comes from
   the resolved profile, not from the preset name. A desktop player on Low no
   longer downloads the phone pack unless the resolved parameters call for it.

   *Done 2026-11-22 in the adapter and the published viewer.* Each preset asks
   for a variant. The resolved one is the highest variant, at or below it, that
   the resolved `assets.geometryDetail` and `assets.texelDensity` allow and
   that has a pack. `graphicsPackUrl` takes that variant. Three packs exist, so
   `full` falls back to `standard` and Very High reports it. Low's own
   parameters still call for the `reduced` pack on every platform, so a desktop
   on Low fetches it until a profile or an override asks for more. Klali still
   picks its pack from the preset name in its own `mapDelivery.ts`.
5. **Platform detection and the override in Klali's settings screen**, including
   the default preset per platform.

   *Adapter side done 2026-11-22; Klali's side is open.*
   `detectGraphicsPlatform` in `adapters/three/src/graphicsPlatform.ts` maps
   stated signals to a profile, and `detectBrowserGraphicsPlatform` gathers
   them. Detection is opt-in: the host passes the result as
   `resolveRenderProfile`'s `platform` option, and `baseline` applies
   otherwise. Choices made:
   - A phone with no connection information, which is every iPhone, is
     `phone-wifi`. Data saving, a cellular connection type, or an effective
     type slower than 4G makes it `phone-cellular`. A 4G effective type does
     not, because Wi-Fi reports the same.
   - A tablet reads as a phone.
   - A device that reports neither memory nor a texture limit is
     `desktop-web`, the profile without caps. It is `laptop-web` only on
     evidence: under 8 GiB, or a texture limit under 16384.
   - The phone profiles now state shader warm-up off, texture warm-up 0 and
     no antialiasing. Low and Very Low still state them too, because removing
     them would change those presets on `baseline`.
6. **Budget checks at publish.**
7. **Add parameters as their features land** from the efficiency plan: codec,
   map selection, weathering sharing, part instancing, material layout, texture
   budget, draw budget. Each arrives with its preset and profile values.
8. **Tune the profiles on real devices.**

Steps 1 to 4 can start before the efficiency work finishes. They change how
values are named and chosen, not what is drawn.

## The governor

Decided on 2026-11-22: a frame-rate governor may lower quality parameters other
than resolution, and does so before it lowers resolution.

- It steps only run-time parameters, and never below the values of the next
  preset down. Build-time parameters stay as resolved.
- Proposed order, cheapest to notice first: shadow sizes, then
  `cpu.geometryLodBias` and `gpu.materialLodBias`, then `cpu.partLod`, then
  `gpu.renderScale` down to its floor. Restoring runs in the reverse order.
- A profile lists which parameters the governor may move, so a capture profile
  can forbid all of them.

Klali's SteadyFrame governs resolution today. It would take this ladder from the
resolved profile in place of its fixed list of scales.

## Generation on the device

Decided on 2026-11-22: prototype it. The city source is a small scene file plus
seeds, and the compiler already builds for Wasm.

The prototype answers three questions on a phone, for one building:

1. How long does the Wasm compiler take to generate it at detail levels 1 and 2?
2. How much memory does generation hold at its peak?
3. What stands in for baked weathering? Candidates are the shared part atlas
   downloaded once, weathering evaluated on the device's GPU, or vertex colour
   only at the lowest presets.

It fits best after parts become placements, because the placement list is what
would be generated, and the part library is what would still be downloaded.

## Open questions

- What does a phone actually hold? The memory and draw figures wait on device
  measurements.
Decided on 2026-11-22: the variant budgets for Low, Medium and High are 2, 4
and 8. Four published variants per map for now (`minimal`,
`reduced`, `standard`, `full`). Ultra would be a fifth, built only for capture.
