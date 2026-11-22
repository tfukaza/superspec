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

## Presets

| Preset | Render scale | Geometry LOD bias | Material LOD bias | Surface anisotropy | Sun shadow | Fixture shadows |
| --- | ---: | ---: | ---: | --- | ---: | ---: |
| Medium (default) | 100% | 1.0× | 1.0× | Authored | 2048 | 512 |
| High | 100% | 1.2× | 1.25× | Up to 8× | 4096 | 512 |
| Very High | 125% | 1.4× | 1.5× | Up to 16× | 4096 | 1024 |

Medium is the existing runtime baseline. High and Very High are initial desktop
profiles. Changing render scale changes the drawing buffer and leaves the
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

Low and Very Low will be tuned on laptops and mobile hardware. Ultra will be a
capture profile for stills and movies, with independent sampling and output
settings. A later pass can add quality-specific decorative placement where it
preserves gameplay geometry and visibility.

The design follows Unreal's preset groups and independent resolution scaling,
Unity's separate LOD and texture controls, and the way Doom Eternal documents
quality tiers alongside individual texture overrides. Unreal's Movie Render
Queue also gives offline capture its own settings and sampling. [Unreal
Scalability](https://dev.epicgames.com/documentation/en-us/unreal-engine/scalability-reference-for-unreal-engine)
· [Unity Quality](https://docs.unity.com/en-us/engine/6000.6/manual/unity-editor/editor-settings-reference/comp-manager-group/class-quality-settings)
· [Doom Eternal PC settings](https://help.bethesda.net/app/answers/detail/a_id/49836/~/system-requirements---doom-eternal)
· [Unreal Movie Render Queue](https://dev.epicgames.com/documentation/en-us/unreal-engine/cinematic-render-settings-and-formats-in-unreal-engine)

## Profile schema

All previews accept `graphics=medium`, `graphics=high`, or
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

| Diagnostic group | Examples |
| --- | --- |
| `visibility` | `lod`, `cityCulling`, `detail`, `clusterDraws`, `blockProxy` |
| `materials` | `textureLod`, `grain`, `bounce`, `reflectionProbes`, `singlePassGlazing` |
| `lighting` | `shadows`, `fixtures`, `shadowCache`, `ambientOcclusion`, `paintedShadows`, `paintedSpots` |
| `effects` | `displayAA`, `edgeBlur`, `bloom`, `outline`, `shopArt` |
| `loading` | `warmup`, `shaderWarmup`, `textureWarmupMiB` |
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
on both backends. A host supplies its renderer, viewport and settings UI. Scene
lighting, materials and geometry still come from WorldSpec exports. Profiles
are runtime configuration and sit outside AML. The Klali menu can adopt the
same contract when its adapter distribution is updated.

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
