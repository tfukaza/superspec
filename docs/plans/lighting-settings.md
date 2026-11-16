# Lighting and rendering settings: draft specification

**Status: draft proposal for review. Nothing here is implemented or decided except
the scoping rule in the first section.** It answers the requirement in
[performance-and-shadows.md](performance-and-shadows.md): one specification for
global and per-light settings, shared by MaterialSpec, ItemSpec, WorldSpec, the Studio
renderer and the three.js game runtime. Field names and defaults are proposals; the
defaults are today's Studio viewer values (`browser/renderer/scene.ts`) and the
approved shadow look, so adopting the record changes no picture.

## Decided

- Shadow styling and ambient occlusion are rendering settings: global to a scene or
  attached to a light source. They are never part of a material and are not
  `LiveOperation`s. A material may only read a light's shadow or exposure as an input.
- Until this spec is accepted, such settings live only in the Studio viewer (query
  parameters and one scene-level settings object), not in AML, material programs or
  export.

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
    "source": "studio",               // catalog id (proposed name for the viewer's built-in
                                      // reflection environment); "none" disables it
    "intensity": 0.35,
    "rotation": 1.5707963267948966    // radians about +Z
  },
  "ambient": {                        // hemisphere light
    "sky": [1.0, 1.0, 1.0],           // linear RGB
    "ground": [0.184, 0.184, 0.138],  // linear RGB (#777768)
    "intensity": 0.65
  },
  "ambientOcclusion": {
    "enabled": true,
    "radius": 0.4,                    // meters; placeholder until the prototype settles
    "intensity": 1.0                  // 0 disables the effect, 1 is the reference strength
  },
  "lights": [{
    "id": "sun",
    "role": "sun",                    // at most one light has this role
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

Conventions: meters, right-handed Z-up, radians, linear RGB in [0, 1], intensities as
three.js-style scalar multipliers until a photometric unit is chosen (open question).
Every field has a default, so `{ "version": 1 }` is the shipped look. Unknown fields
and out-of-range values are validation errors with source-linked diagnostics.

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
Validation limits (proposed): softness and maxSoftness in [0, 2] m, spread in [0, 1],
darkness, tintStrength, wrap and noise amount in [0, 1], noise scale in [0.05, 20] m,
AO radius in [0.01, 5] m, intensities in [0, 100], at most 8 lights.

## Adoption steps

1. Review and settle this document, including the open questions.
2. Add the records and validation to `superspec-types`/`superspec-model`, with the
   default equal to today's viewer values; deliver it in the scene packet.
3. Make the Studio viewer build its lights, shadow style and AO from the record;
   query parameters become session overrides of it. Delete the hard-coded values in
   `browser/renderer/scene.ts` and `tests/studio/washroom/_view.mjs`.
4. Store the record in Studio projects; add catalog presets and theme selection.
5. Export (`KHR_lights_punctual` plus `extras`) and the game runtime module.
6. Material inputs and baked masks, with the occluder service.

Each step keeps native/Wasm parity and the washroom pictures unchanged unless the
step says otherwise.

## Open questions

- Intensity units: keep renderer scalars, or adopt lux for directional lights and
  nits for the environment so values carry across engines?
- Should `exposure` and `toneMapping` be scene settings as drafted, or belong to a
  camera or output profile shared across scenes?
- One `role: "sun"` light, or allow several styled shadow-casting lights in version 1
  (the prototype supports one)?
- Is a per-light shadow style needed now, or one style per scene with per-light
  override later?
- Where should authors edit this first: Studio project settings, theme files or AML?
- Should a theme be allowed to forbid viewer or project overrides for a shipped look?
