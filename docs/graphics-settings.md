# Graphics settings

**Graphics** selects a preset. **Graphics options** sets render scale, geometry
LOD, material detail, and shadows independently. The link stores the selection
so another viewer or a benchmark can reproduce it.

| Preview | Applying settings |
| --- | --- |
| WorldSpec AML editor and watched file preview | Updates the live viewport and keeps the current source, camera, and selection. |
| MaterialSpec and ItemSpec editors | Updates the live viewport. In ItemSpec, enable **Preview LODs** to view authored alternatives at a distance. The default editing view shows the full-detail item. |
| Native packet and TreeSpec preview | Updates the open scene and selects from the LODs supplied by the packet or tree. |
| Published GLB viewer | Reopens the same scene at the current camera pose. |
| Klali | Updates the running match. The game saves the shared profile alongside its own controls, including the optional SteadyFrame resolution governor. |

## Presets

| Preset | Render scale | Geometry LOD bias | Material LOD bias | Surface anisotropy | Sun shadow | Fixture shadows |
| --- | ---: | ---: | ---: | --- | ---: | ---: |
| Very Low | 100% | 0.55× | 0.5× | 1× | 512 | Off |
| Low | 100% | 0.75× | 0.7× | 2× | 1024 | Off |
| Medium (default) | 100% | 1.0× | 1.0× | Authored | 2048 | 512 |
| High | 100% | 1.2× | 1.25× | Up to 8× | 4096 | 512 |
| Very High | 100% | 1.4× | 1.5× | Up to 16× | 4096 | 1024 |

Medium is the existing runtime baseline. High and Very High are initial desktop
profiles. Every preset renders at the output resolution. A render scale
override, or Klali's SteadyFrame governor, lowers it. Changing render scale changes the drawing buffer and leaves the
viewport and UI at their output size. LOD biases multiply the drawing-buffer
height used to choose levels. Values above 1 retain detail farther away. Material
quality controls authored material LOD, published texture-pair promotion, and
filtering of mipmapped surface textures. Asset packs need to provide higher-detail
alternatives for the viewer to select them. Authoring previews scale from their
existing display pixel ratio, capped at 2× before applying the profile. The
published viewer scales from 1×. Authoring material detail can change the LOD
and sampler for delivered maps. Live bake targets follow the material project's
own resolution and sampler. Shadow map and filtering requests are limited by
the active GPU. The published backend selection and its fallback rules apply
to every preset. The profile affects preview rendering. Saved projects,
authored lighting, and export quality come from their existing source settings.

Low and Very Low skip screen-space ambient occlusion and finishing passes and
use simpler shadow and material defaults. On the next map load, Klali picks
the matching Stoa delivery. WorldSpec looks for the pack of the resolved
[asset variant](#parameters-platforms-and-asset-variants) beside a same-origin
GLB. Other assets use the lighter runtime settings with their
regular files. Ultra remains a future capture profile for stills and movies.

## Parameters, platforms and asset variants

A preset is a set of values for named parameters. The parameters, the presets,
the platform profiles and the asset variants are declared in one data file,
[`graphicsProfiles.json`](../adapters/three/src/graphicsProfiles.json), which
the adapter resolves. The [graphics profiles plan](plans/graphics-profiles.md)
is the design.

| Term | Meaning |
| --- | --- |
| Parameter | One named value in the `assets`, `delivery`, `memory`, `gpu` or `cpu` group, such as `gpu.sunShadowSize`. |
| Quality preset | Very Low to Very High: the parameter values a player asks for. |
| Platform profile | What a device and connection can afford: its own values, and caps on what a preset may ask for. |
| Asset variant | One publish of a map: `minimal`, `reduced`, `standard` or `full`. |

A session resolves in this order, and later layers win: the parameters'
defaults, the platform profile's values, the preset's values, then individual
overrides. The platform's caps then limit the result. Diagnostics are
development switches that no cap limits.

The platform profiles are `baseline`, `phone-cellular`, `phone-wifi`,
`laptop-web`, `desktop-web` and `native-pc`. `baseline` is the default. It has
no values and no caps, so every preset resolves to the table above. The other
profiles hold the plan's starting proposals.

Detection is opt-in. `detectGraphicsPlatform(signals)` infers a profile from
signals the caller states, and `detectBrowserGraphicsPlatform()` gathers them
from the browser first. A host opts in by passing the result as the `platform`
option of `resolveRenderProfile`. A platform in the profile or the link wins
over it. Without the option, `baseline` applies.

| Signals | Platform |
| --- | --- |
| The host states `native` | `native-pc` |
| A mobile user agent or client hint, or a coarse primary pointer with touch points | `phone-wifi` |
| A phone whose connection type is cellular, whose effective type is slower than 4G, or with data saving on | `phone-cellular` |
| Any other device reporting under 8 GiB of memory or a texture limit under 16384 | `laptop-web` |
| Any other device | `desktop-web` |

Loading has two separate shader steps. `cpu.firstFrameShaders` links the first
view's shaders off the main thread before the first frame, and is on at every
preset and on every platform. It needs `KHR_parallel_shader_compile`. Without
the extension the first frame links them itself. `cpu.shaderWarmup` is the
fuller preparation after the first frame, which also covers variants the first
view hides. Low, Very Low and the phone profiles leave that one off. Before
this split, Low and Very Low linked every shader inside the first frame, which
blocked the page for about 20 seconds on the published Stoa packs.

An iPhone reports neither its memory nor its connection, so it is `phone-wifi`
until the host passes a data-saving choice. A tablet reads as a phone. The
phone profiles turn off shader warm-up, texture warm-up and antialiasing at
every preset. Low and Very Low also state those themselves, so they keep them
on `baseline`.

Each preset asks for an asset variant. The viewer fetches the highest variant,
at or below that one, that the resolved `assets.geometryDetail` and
`assets.texelDensity` allow and that has a pack.

| Variant | Pack beside `map.glb` | Asked for by |
| --- | --- | --- |
| `minimal` | `map.very-low.glb` | Very Low |
| `reduced` | `map.low.glb` | Low |
| `standard` | `map.glb` | Medium, High |
| `full` | None yet, so `standard` is fetched | Very High |

On `baseline` this requests the same pack for each preset as before. On a
phone profile, High fetches the `reduced` pack, because the profile caps texel
density at a quarter.

`resolveRenderProfile` returns `parameters`, `platform`, `variant` and `changed`
beside `settings` and `diagnostics`. `changed` lists each parameter whose
resolved value differs from what was asked, with the reason: `cap` for a
platform cap, `variant` for a value that follows the chosen asset variant, and
`unpublished` for a variant with no pack. `graphicsPackUrl(url, variant)` names
the pack.

The data file marks each parameter `consumed`. Parameters whose features do not
exist yet, such as `assets.codec`, `assets.variantBudget`, `assets.weathering`,
`memory.textureBudgetMB` and `cpu.drawBudget`, carry their planned values and
change nothing. The publisher does not read the file yet. Its texture scales
and detail levels remain constants in `tools/kowloon-export`.

Published Stoa mobile packs use smaller textures, compacted vertex data, and
Meshopt compression. Low keeps a quarter of each texture's authored edge length
and Very Low an eighth, so a 2048-pixel atlas becomes 512 and 256 pixels. Images
of 128 pixels or less stay as authored. Building pages baked at the tier's own
density, described below, are delivered as baked. Low draws the original geometry up
close. Very Low draws a rendition within 3.5 cm of it. Both keep the farther
distance levels of the full pack, and the viewer selects them sooner at the
lower presets. Baked bounce lighting stays on at every preset. The server still
uses the same map ID, collision and navigation. Publish both variants beside the original Stoa in Klali:

~~~sh
node tools/kowloon-export/city-mobile-delivery.mjs path/to/stoacity.glb --preset low --out generated/stoa-mobile/maps/stoacity.low.glb
node tools/kowloon-export/city-mobile-delivery.mjs path/to/stoacity.glb --preset very-low --out generated/stoa-mobile/maps/stoacity.very-low.glb
~~~

Add `--atlas` to merge each building's atlas pages. The bake gives a building
dozens of pages, each its own material and draw. With `--atlas` the publisher
repacks the building's charts, at the tier's density and with a gutter around
each, onto one page per material class, and joins the primitives that then share
a material, with their distance levels. A building then draws as under ten
primitives. Glazing, signs, emissive surfaces and each finish keep their own
material. The tier table in `tools/kowloon-export/mobile-atlas.mjs` holds the
page limit, the gutters and what a tier may unify. `--atlas-tier` takes
overrides as JSON, such as `{"share":{"representatives":2}}` to let identical
parts share one chart's texels. The option composes with `--gpu-textures`.

For buildings with their own Low and Very Low renditions, compile the city once
per tier before baking and packing. `compile.py --graphics-detail 1` draws every
building at its Low rendition and `--graphics-detail 2` at Very Low. A building
can also state `graphics-detail="0|1|2"` in AML. Level 0 is the authored
building. The reduced levels omit bolts and thin fittings, mass shop stock, and
coarsen cloth, while placement, weathering inputs, and collision stay the same.
`compile.py --variant-budget N` (or `variant-budget="N"` on a building) states how
many shapes each part type draws from its catalogue; unstated, a level draws 4, 2
or 1. Run the mobile delivery on each tier's pack.

Bake each tier's buildings at the tier's own texel density:

~~~sh
node tools/kowloon-export/bake-buildings.mjs COMPILED_DIR --density low
node tools/kowloon-export/bake-buildings.mjs COMPILED_DIR --density very-low
~~~

`--density` takes `medium` (the authored density, the default), `low` (a
quarter), `very-low` (an eighth) or a fraction. The GPU still evaluates every
material at the authored density. The bake then gives each chart its own
rectangle on the building's pages: the chart's texels are averaged down to the
tier's size, colour in linear light and normals as unit vectors, and the
2-texel gutter is rebuilt at the page's resolution. Shrinking a finished page
by 4 or 8 leaves less than one gutter texel, so neighbouring charts bleed into
each other. `--page-limit` sets the longest page side, 4096 by default. The
density, the page limit and the layout are part of the bake's key, so a bake
at one density is never reused for another.

Pages hold charts that need the same maps and the same treatment. Small parts
and surfaces with metal are the building's hardware, which gets distance
levels and can be culled. They stay apart from the shell's plain walls, and
signs, posters and lamps stay apart from both. A part's charts lie together in
one block, so a distance level or a coarse mip of that part mixes only its own
texels. `--layout receivers` restores the earlier layout of whole receiver
atlases on 2048 pages at the authored density, for comparison.

Signs keep every triangle and their full textures at every distance: their
pages carry `cityDetailKeep`, and the distance levels skip them. Roof tiles
count as the building's mass, not as hardware. Fewer pages mean larger
primitives, in which a distance level that collapses through chart seams mixes
more parts. `SUPERSPEC_CITY_LOD_SEAMS=near` therefore protects chart seams in
the levels within 18 cm of the surface and lets the farther ones collapse
through them. Medium uses it with the standard far profile. Low and Very Low
protect every seam, as before.

`city-mobile-delivery.mjs --atlas` takes charts from pages already at its
tier's density as the bake placed them, with the bake's 2-texel gutter, and
still merges their materials and primitives and shares texels between
identical parts.

Each page's material states its density as `extras.worldSpecTexelDensity`. The
mobile delivery leaves a page already at its tier's density as baked and
scales every other texture by the tier, so the same command serves a pack
baked at the authored density and one baked for the tier. A finish's detail
image keeps a quarter of the authored density at every tier.

Deploy each GLB and its corresponding data directory together. The CLI prints
the bytes for the original and the mobile map, excluding game code and assets
such as character animations.

WorldSpec's [render visibility labels](AUTHORING.md#native-builds-and-worldspec)
select which entities appear at each tier. The shared Three.js loader applies
the same rule in the published viewer and Klali. The mobile publisher also
prunes the omitted nodes' and primitives' exclusive geometry and textures from
the download. Stoa metadata goes into future city builds automatically.
Use `--legacy-kowloon-labels` when repacking a Stoa export that predates the tags.

The design follows Unreal's preset groups and independent resolution scaling,
Unity's separate LOD and texture controls, and the way Doom Eternal documents
quality tiers alongside individual texture overrides. Unreal's Movie Render
Queue also gives offline capture its own settings and sampling. [Unreal
Scalability](https://dev.epicgames.com/documentation/en-us/unreal-engine/scalability-reference-for-unreal-engine)
· [Unity Quality](https://docs.unity.com/en-us/engine/6000.6/manual/unity-editor/editor-settings-reference/comp-manager-group/class-quality-settings)
· [Doom Eternal PC settings](https://help.bethesda.net/app/answers/detail/a_id/49836/~/system-requirements---doom-eternal)
· [Unreal Movie Render Queue](https://dev.epicgames.com/documentation/en-us/unreal-engine/cinematic-render-settings-and-formats-in-unreal-engine)

## Profile schema

All previews accept `graphics=very-low`, `graphics=low`, `graphics=medium`, `graphics=high`, or
`graphics=very-high`. A `profile` parameter points to a JSON file served beside
the published map, or contains a URL-encoded profile object. The viewer writes
the inline form when you apply custom settings.

```json
{
  "version": 1,
  "preset": "high",
  "overrides": {
    "renderScale": 1.0,
    "geometry": "medium",
    "materials": "very-high",
    "shadows": "high"
  },
  "diagnostics": {
    "visibility": { "clusterDraws": "pairs" },
    "lighting": { "paintedShadows": true, "paintedSpots": true },
    "loading": { "warmup": false }
  }
}
```

`version` is required. `preset`, `overrides`, and `diagnostics` are optional.
Render scale takes a factor from 0.5 to 1.5. Each quality group inherits the
base preset unless overridden. Diagnostics are optional test controls, and the
schema rejects unknown fields and invalid values with the field name.

Version 2 names the platform and the preset, and overrides individual
parameters. `platform=phone-wifi` is the short link form, beside `graphics=low`.

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

`platform`, `quality`, `overrides` and `diagnostics` are optional. Without a
platform, the host's stated platform applies, and `baseline` otherwise. Without
a preset, the platform's default preset applies. Version 1 profiles keep
working and resolve to the same values as before. A version 1 profile is read
as its version 2 equivalent, in which each override group becomes the parameter
values of the preset it names. `upgradeRenderProfile` returns that equivalent.
The viewer's graphics controls keep a profile's version. On a version 2
profile, a new preset keeps the platform and the overrides, and each quality
group is laid over the overrides as the parameter values it stands for.

| Diagnostic group | Examples |
| --- | --- |
| `visibility` | `lod`, `cityCulling`, `detail`, `clusterDraws`, `blockProxy` |
| `materials` | `textureLod`, `grain`, `bounce`, `reflectionProbes`, `singlePassGlazing` |
| `lighting` | `shadows`, `fixtures`, `shadowCache`, `ambientOcclusion`, `paintedShadows`, `paintedSpots` |
| `effects` | `displayAA`, `edgeBlur`, `bloom`, `outline`, `shopArt` |
| `loading` | `firstFrameShaders`, `warmup`, `shaderWarmup`, `textureWarmupMiB` |
| `instrumentation` | `gpuProbe` |
| `look` | `shadowStyle`, `shadowBlur`, `aoRadius`, `contourWidth` |

The [validator and legacy alias table](../adapters/three/src/renderProfile.ts)
define all diagnostic values and defaults. Legacy flags continue to work and
take precedence when present. The viewer writes them back as structured
diagnostics when applying graphics settings. Map selection, camera, and renderer
remain separate link parameters. A `graphics` shortcut can override the base
preset in a profile file. Published-city streaming, fixture shadow maps, and
finishing diagnostics apply when the published scene and backend support them.

SuperSpec exports `resolveGraphicsSettings`, `resolveRenderProfile` and their
types from `@superspec/three`. The published viewer consumes the resolved profile
on both backends. Klali consumes the same presets and overrides from its menu,
an inline profile, or a `graphics` shortcut. The host supplies its renderer,
viewport, and settings UI. Scene lighting, materials, and geometry come from
WorldSpec exports. Profiles are runtime configuration and sit outside AML.

## Benchmarks

```sh
npm run bench:published-city -- path/to/stoacity.glb --graphics high --resolution 1920x1080
npm run bench:published-city -- path/to/stoacity.glb --profile quality.json --resolution 1920x1080
```

Reports record the resolved preset, output resolution and drawing-buffer
resolution. A 1920×1080 output at the Very High default renders at 2400×1350.
Set `overrides.renderScale` to 1 when isolating the effect of other settings at
1080p. Use the same pack and camera routes when comparing tiers, and inspect
both street-level and overhead views. Record uncapped throughput separately
from display-paced frame delivery.
