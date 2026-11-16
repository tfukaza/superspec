# Lighting and rendering settings: specification, version 1

**Status: adoption steps 1 to 3 are implemented (2026-11-16). The open questions were
settled with the defaults under
[Assumed pending review](#assumed-pending-review-2026-11-16); those are the parts to
review. Steps 4 to 6 are not started.** It answers the requirement in
[performance-and-shadows.md](performance-and-shadows.md): one specification for
global and per-light settings, shared by MaterialSpec, ItemSpec, WorldSpec, the Studio
renderer and the three.js game runtime. The defaults are the Studio viewer's values
before the adoption and the approved shadow look, so adopting the record changed no
picture (see [Verification](#verification-of-steps-1-to-3)).

Where it lives: the record and its limits in `crates/types/src/lighting.rs`; reading
it from JSON and `Manifest::lighting` in `crates/model/src/lighting.rs`; the viewer's
mirror (type, default, validation, page overrides) in `browser/renderer/lighting.ts`;
the view built from it (`SceneLighting`) in `browser/renderer/rendering.ts`. Two
fixtures, `crates/model/src/lighting.default.json` and `lighting.invalid.json`, are
checked by both the Rust tests and `tests/browser/rust-lighting.test.ts`, so the two
implementations cannot drift apart.

Sequencing is in [roadmap.md](roadmap.md): adoption steps 1 to 3 are stage 1, export
and the game runtime are stage 4, material inputs are stage 5. This record is also
the "scene lighting input" of [tree-item-roadmap.md](tree-item-roadmap.md) step 1C.3;
there is one record, not two. An additional open question from that plan: whether
ambient occlusion and tone mapping apply to `shading: painted` materials.

## Decided

- Shadow styling and ambient occlusion are rendering settings: global to a scene or
  attached to a light source. They are never part of a material and are not
  `LiveOperation`s. A material may only read a light's shadow or exposure as an input.
- The approved default look is the tight neutral-gray `illustration` shadow plus
  ambient occlusion (GTAO), as the Studio viewer ships them.
- Such settings live in this record and in the Studio viewer's session overrides
  (query parameters), not in AML, material programs or export, until the later
  adoption steps say otherwise.

## Goals and non-goals

Goals: one portable record that every consumer reads; physical units; the same
picture in Studio and in a game for the settings a renderer supports; explicit
reporting when a renderer cannot honor a setting; baked and live shadows that agree.

Non-goals for version 1: point and spot lights, time of day and animation, volumetric
effects, global illumination, per-object light linking, and any per-material
override of these settings.

## The record

A scene carries one `Lighting` record. JSON shape (camelCase, as other delivered
records); Rust types in `superspec-types`, carried by the scene in `superspec-model`.

```jsonc
{
  "version": 1,
  "exposure": 1.0,                    // linear multiplier before tone mapping
  "toneMapping": "aces",              // "aces" | "neutral" | "none"
  "environment": {                    // image-based light
    "source": "studio",               // the viewer's built-in neutral room; "none" disables
                                      // it; other identities are catalog names (step 4)
    "intensity": 0.35,
    "rotation": 0.0                   // radians about +Z
  },
  "ambient": {                        // hemisphere light: sky from +Z, ground from -Z
    "sky": [1.0, 1.0, 1.0],           // linear RGB
    "ground": [0.184, 0.184, 0.138],  // linear RGB (#777768)
    "intensity": 0.65
  },
  "ambientOcclusion": {
    "enabled": true,
    "radius": 0.35,                   // meters: how far around a point occluders are sought
    "intensity": 2.0,                 // up to 1: share of the occlusion that darkens the
                                      // image; above 1 (to 4): creases deepen further
    "thickness": 0.5                  // meters: how thick surfaces are taken to be
  },
  "lights": [{                        // version 1: at most one light, the sun
    "id": "sun",
    "role": "sun",
    "kind": "directional",
    "direction": [-0.303, -0.505, 0.808], // unit vector from the scene toward the light
    "color": [1.0, 0.863, 0.708],     // linear RGB (#ffefdb)
    "intensity": 3.0,
    "castsShadow": true,
    "shadow": {
      "style": "illustration",        // "plain" | "illustration"
      "softness": 0.012,              // meters: penumbra radius at contact
      "spread": 0.025,                // meters of penumbra per meter of occluder distance
      "maxSoftness": 0.06,            // meters
      "darkness": 0.25,               // 0 keeps ambient light in shadow, 1 is black
      "tint": [0.028, 0.158, 0.745],  // linear RGB (#2f6fe0)
      "tintAlt": [0.050, 0.078, 0.687], // linear RGB (#3f4fd8), hue drift inside the shadow
      "tintStrength": 0.22,           // 0 neutral gray shadow, 1 fully tinted
      "wrap": 0.3,                    // softness of the lit/unlit boundary on a surface
      "noise": {
        "scale": 1.1,                 // meters: feature size, in the light's plane
        "edge": 0.07,                 // meters: displacement of the shadow edge
        "amount": 0.6,                // variation of darkness and hue inside the shadow
        "seed": 7
      }
    }
  }]
}
```

The block above rounds colors and the direction to three decimals for reading; the
exact shipped default is `crates/model/src/lighting.default.json`. Two values differ
from the first draft because the draft misread the viewer: the environment's turn about
+Z is 0 (the viewer's quarter turn only stands the Y-up room upright, which is the
renderer's business), and ambient occlusion has the shipped radius 0.35, intensity 2
and a `thickness`.

Conventions: meters, right-handed Z-up, radians, linear RGB in [0, 1], intensities as
three.js-style scalar multipliers. Every field but `version` has a default, so
`{ "version": 1 }` is the shipped look; a missing `lights` is the default sun and an
empty list is no direct light. Unknown fields, wrong types and out-of-range values are
refused. A range finding names the field by its path (`lights[0].shadow.softness`); a
record that cannot be read at all is refused with the JSON line and column. Source
offsets in an authored file come with the authoring surface that has such a file.

### Scope

| Global to the scene | Per light source |
| --- | --- |
| exposure, tone mapping | kind, direction (later: position, range, cone) |
| environment light | color, intensity |
| ambient (hemisphere) light | whether it casts shadows |
| ambient occlusion | shadow style: softness, spread, darkness, tint, wrap, noise |

Renderer implementation choices are not settings and are not in the record: shadow
map resolution and type, bias, kernel tap counts, AO sample counts, antialiasing.
A renderer chooses them to meet the record's physical parameters.

## Ownership and layering

Resolution order, later entries override earlier ones field by field:

1. **Shipped default**: the washroom look above.
2. **Named preset** from the catalog (`superspec-catalog`), e.g. `illustration-day`.
3. **Theme**: a WorldSpec theme may supply a preset or a full record for its worlds.
4. **Scene or project**: the authored record stored with a Studio project, an item
   preview or a level.
5. **Viewer session**: Studio query parameters and controls. Never saved or exported.

Ownership follows [AGENTS.md](../../AGENTS.md): portable records in `superspec-types`
and `superspec-model`; presets and lookup in `superspec-catalog`; theme choices under
the theme; no renderer or theme import in generic `materialspec`/`itemspec` code. The
resolved record is delivered in the scene packet. An AML authoring surface (for
example a `<lighting>` element) is deferred until the record has been used from
projects and themes.

## Consumer contract

Each consumer declares which features it implements. A setting a consumer cannot
honor is reported once as a diagnostic naming the field and the fallback, never
silently dropped and never an error that blocks the scene.

| Consumer | Must implement | May approximate | Notes |
| --- | --- | --- | --- |
| Studio viewer (material, item, level) | all of version 1 | AO quality on software renderers | reference implementation |
| three.js game runtime | lights, exposure, tone mapping, environment, plain shadows | shadow style, AO (declared) | uses the same shadow-style module as Studio |
| Exporter (GLB) | lights via `KHR_lights_punctual` | — | style, AO, environment and ambient go in `extras.superSpec.lighting` |
| Compiler (baked fields, surveys) | light definitions | — | never reads style or AO |

"The same picture" means the same module and parameters where a feature is
implemented, not bit-identical frames across GPUs.

## Materials reading lights

Planned material inputs (`shadow`, `occlusion`, `sunExposure`) refer to lights by
role or id (`shadow("sun")`). They read only light definitions (kind, direction,
castsShadow) and scene geometry. Consequences:

- A bake key includes exactly the light fields a material field reads. Shadow style,
  AO, exposure, tone mapping and intensity never enter a bake key.
- Baked shadow or exposure is an appearance input (moss, damp, fading). It must not
  darken base color in a way the runtime shadow would repeat.
- A swatch has no scene: it uses the default sun and a stand-in occluder.
- Placement surveys read the same light definitions.

## Static scenes

For a static level the styled shadow may be baked into a per-receiver shadow mask
(same softness, noise and seed as the record), so the runtime needs no shadow-map
pass. The record stays the source of truth; the mask is a derived artifact keyed by
the light definition, the style and the occluders. Dynamic objects keep the runtime
style.

## Versioning and determinism

`version` is required. Additive fields with defaults do not bump it; a change in
meaning does. Noise is a pure function of position in the light's plane and `seed`.
Validation limits: `version` 1; exposure and light, ambient and environment
intensities in [0, 100]; environment rotation in [-2π, 2π]; colors in [0, 1] per
channel; direction of unit length within 0.001 (consumers normalize it); softness,
spread and maxSoftness in [0, 2]; darkness, tintStrength, wrap, noise edge and noise
amount in [0, 1]; noise scale in [0.01, 100] m; seed an unsigned 32-bit integer; AO
radius and thickness in [0.01, 10] m, AO intensity in [0, 4]; ids and the environment
source are 1 to 64 of `A-Z a-z 0-9 . _ -`; at most one light. The ranges are at least
as wide as the viewer's query-parameter overrides were, so every existing URL still
states a valid record.

## Adoption steps

1. **Done, pending review.** Settle this document, including the open questions (see
   below).
2. **Done.** The record and validation are in `superspec-types`, JSON reading and
   `Manifest::lighting` (optional; absent means the shipped default, and the packet
   bytes of a scene without one are unchanged) in `superspec-model`. Not done: no
   producer fills `Manifest::lighting` yet and the viewer does not read it from a
   packet; the record reaches the viewer through the project (step 3).
3. **Done.** The Studio viewer builds its lights, shadow style, AO, tone mapping and
   environment from the record (`SceneLighting`); query parameters are session
   overrides of it; the hard-coded sun and light values are gone from
   `browser/renderer/scene.ts`. A Studio project may carry the record in a `lighting`
   field: it is validated when the project opens, applied when the project is
   accepted and kept by save. Not done: `tests/studio/washroom/_view.mjs` is an
   unreferenced standalone probe with its own, different lights; it is left as it is
   rather than changing its picture. No material program read a hard-coded sun, so
   there was nothing to remove there.
4. Declare `lighting` in the project types (`AuthoringProject`, the engine's
   `Project`, which ignores the field today) and give Studio controls for it; fill
   `Manifest::lighting` and read it in the viewer; add catalog presets and theme
   selection.
5. Export (`KHR_lights_punctual` plus `extras`) and the game runtime module.
6. Material inputs and baked masks, with the occluder service.

Each step keeps native/Wasm parity and the washroom pictures unchanged unless the
step says otherwise.

## Assumed pending review (2026-11-16)

The open questions were not answered before adoption. Each was settled with the
simplest default that keeps the picture and leaves the other answer open. Changing
one later is a version bump only where noted.

- **Intensity units: renderer scalars.** Intensities stay three.js-style multipliers.
  Photometric units need a calibrated exposure that nothing here has yet, and
  converting the shipped look would be a guess. Adopting lux and nits changes the
  meaning of a field, so it would be version 2.
- **Exposure and tone mapping: scene settings.** They stay in the record as drafted.
  There is no camera or output profile to hold them, and a scene is where the look is
  judged. The viewer's exposure control is a session override that replaces the
  record's value. Moving them to an output profile later is a version bump.
- **One styled light.** Version 1 admits at most one light; it has `role: "sun"` and
  `kind: "directional"`. The prototype styles one shadow map. `lights` stays a list
  with ids and roles so more lights are an additive change.
- **Shadow style: per light.** The style is the `shadow` of a light, as drafted. With
  one light this is the same as one style per scene, and it already fits the standing
  rule (global or per light, never per material), so no later move is needed.
- **First authoring surface: the Studio project.** A project's optional `lighting`
  field holds the record. Themes and AML follow once it has been used (steps 4 and
  later); the washroom carries none and so has the shipped look.
- **Theme locks: none.** A theme cannot forbid overrides in version 1. Viewer
  overrides are never saved or exported, so they cannot alter a shipped look; whether
  a project may override a theme is decided with theme selection (step 4).
- **AO and tone mapping on painted materials: both apply.** They act on the finished
  image, after shading, so they cover every pixel whatever its shading model; exempting
  painted surfaces would need a per-material mask, which the scoping rule forbids.
  `shading: painted` does not exist yet, so this is recorded, not exercised.
- **Limits.** The validation limits above replace the draft's proposals (for example
  AO radius up to 10 m, not 5; spread up to 2, not 1): they are the union of the
  draft's ranges and what the viewer's query parameters already accepted.
- **Diagnostics.** Findings carry field paths, not source offsets, until an authored
  source with offsets exists. A consumer that cannot honor a setting says so once on
  the console (`ambientOcclusion` without float render targets, an unknown
  `environment.source`) and continues with the stated fallback.

## Verification of steps 1 to 3

Washroom (`tests/material-washroom-browser.mjs`, 256 px, real GPU through ANGLE
D3D11), viewport screenshots before and after the adoption, built from the same
commit and the same Wasm with only the lighting change between them:

- `?ao=off`, `?ao=off&shadowstyle=plain` and `?ao=off&shadows=off`: 0 of 763,092
  pixels differ.
- Default look (AO on): the GTAO denoise draws a random noise texture at start-up, so
  two runs of one build already differ (about 12% of pixels, largest channel
  difference 39 to 50, mean 0.27). Before against after is the same (12%, 50, 0.27).
  With that noise texture replaced by a fixed one in both builds, 0 pixels differ.
- A project record with `style: "plain"` and AO disabled draws exactly the picture of
  `?ao=off&shadowstyle=plain`, and `castsShadow: false` exactly that of `?shadows=off`:
  the record and the page overrides reach the same state.
- A record with another sun direction and a blue tint changes the picture as stated
  (37% of pixels), and the engine builds the project with the extra field.

The environment's `rotation`, `toneMapping` other than `aces`, `environment.source:
"none"` and an empty `lights` list are implemented but were not rendered and compared.
