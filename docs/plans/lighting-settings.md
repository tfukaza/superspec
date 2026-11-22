# Lighting and rendering settings: specification, version 1

**Status: adoption steps 1 to 3, the Studio controls of step 4 and step 5 (export and
the game runtime module) are implemented (2026-11-16). The open questions were settled
with the defaults under
[Assumed pending review](#assumed-pending-review-2026-11-16); those are the parts to
review. Not started: catalog presets, theme selection, the viewer reading the record
from a packet (rest of step 4) and step 6.** It answers the requirement in
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
implementations cannot drift apart. The Studio's controls are
`editor/studio/lighting.ts` (the Lighting panel). Delivery is
`crates/export/src/lighting.rs` (GLB) and `browser/renderer/game-lighting.ts` (the
game runtime module); the one renderer default that is not the record's,
ambient occlusion on software renderers, is `browser/renderer/software-occlusion.ts`.

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
  (query parameters), not in AML or material programs. Export delivers the record
  beside the scene (step 5), never inside a material.

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
    "thickness": 0.5,                 // meters: how thick surfaces are taken to be
    "method": "gtao",                 // optional: "gtao" (default) | "horizon", see below
    "tint": 0                         // optional, 0 to 1: share of the sun shadow's hue it takes
  },
  "painted": {                        // optional; how the image's finishing treats shading: painted
    "toneMapping": false,             // exposure and tone curve skip painted materials
    "ambientOcclusion": true          // the AO pass covers them (false is reported as not honored)
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
        "edge": 0.004,                // meters: displacement of the shadow edge
        "amount": 0.015,              // variation of darkness and hue inside the shadow
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

`ambientOcclusion.method` and `ambientOcclusion.tint` are written only when they differ
from their defaults (`gtao`, 0), so records and exports that never set them keep their
bytes. `gtao` is the viewer's denoised ground-truth occlusion with its short-range contact
term, darkening the scene before tone mapping. `horizon` searches eight jittered directions
of the depth alone with a quadratic falloff over `radius` plus `thickness`, and darkens the
tone-mapped image by up to 0.8 (`intensity` scales it), so contact shading under small props
survives on bright sunlit ground. It is the shader in `browser/renderer/horizon-occlusion.ts`,
which a game renderer can draw from the depth it already keeps: Klali's map renderer carries
a byte-for-byte copy, and the Kowloon city record (`city-lighting.json`) selects it so a level
looks the same in the game and the WorldSpec viewer. `tint` moves the darkening toward the
sun shadow's tone (its `tint` at its `tintStrength`, brightest channel 1) as it deepens.

Conventions: meters, right-handed Z-up, radians, linear RGB in [0, 1], intensities as
three.js-style scalar multipliers. Every field but `version` has a default, so
`{ "version": 1 }` is the shipped look; a missing `lights` is the default sun and an
empty list is no direct light. Unknown fields, wrong types and out-of-range values are
refused. A range finding names the field by its path (`lights[0].shadow.softness`); a
record that cannot be read at all is refused with the JSON line and column. Source
offsets in an authored file come with the authoring surface that has such a file.

Two meanings that were settled when the paths were first drawn (2026-11-16):
`environment.rotation` turns the environment about the world's +Z after it is stood
upright (positive turns it counter-clockwise seen from above), so light from above
stays above at every angle; and `toneMapping: "none"` is no curve, with the exposure
still multiplying (the image is `exposure × color`, clipped).

### Scope

| Global to the scene | Per light source |
| --- | --- |
| exposure, tone mapping | kind, direction (later: position, range, cone) |
| environment light | color, intensity |
| ambient (hemisphere) light | whether it casts shadows |
| ambient occlusion | shadow style: softness, spread, darkness, tint, wrap, noise |
| contour lines (experimental) | form light in shadow and its fill (experimental) |

Renderer implementation choices are not settings and are not in the record: shadow
map resolution and type, bias, kernel tap counts, AO sample counts, antialiasing.
A renderer chooses them to meet the record's physical parameters.

## Ownership and layering

Resolution order, later entries override earlier ones field by field:

1. **Shipped default**: the washroom look above.
2. **Named preset** from the catalog (`superspec-catalog`), e.g. `illustration-day`.
3. **Theme**: a WorldSpec theme may supply a preset or a full record for its worlds.
4. **Scene or project**: the authored record stored with a Studio project, an item
   preview or a level. The Studio's Lighting panel edits this record.
5. **Viewer session**: Studio query parameters. Never saved or exported.

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
| Studio viewer (material, item, level) | all of version 1 | AO on software renderers (off where the record is silent, see below) | reference implementation |
| three.js game runtime (`game-lighting.ts`) | all of version 1 | the same as the Studio | holds the Studio's `SceneLighting`; no rendering code of its own |
| Exporter (GLB) | lights via `KHR_lights_punctual` | — | the whole record goes in `extras.superspec.lighting` |
| Compiler (baked fields, surveys) | light definitions | — | never reads style or AO |

"The same picture" means the same module and parameters where a feature is
implemented, not bit-identical frames across GPUs.

### Delivery in a GLB

A visual GLB exported with a record (`superspec_export::Options::lighting`, or the
scene's `Manifest::lighting`) carries it twice. For any glTF consumer, each light is a
`KHR_lights_punctual` light (the extension is listed in `extensionsUsed`, not
required): a directional light with the record's linear color and its intensity as
written, on a node under the scene's root node. The root node turns the domain's Z-up
frame to glTF's Y-up, so the light node's rotation is in domain coordinates: it turns
the node's +Z onto the record's `direction`, and the light shines along the node's -Z,
away from where the sun stands. For the SuperSpec runtime, the whole record is under
`extras.superspec.lighting`, complete (every default filled in). A scene without a
record exports exactly as before, a record with no lights writes no extension, and a
collision export carries neither.

A project's record reaches the exporter outside the engine: the engine's `Project`
has no `lighting` field and ignores it (no build or bake depends on lighting), and
the host that delivers the project (`crates/wasm`, `superspec-studio glb`) reads the
field with `superspec_model::lighting::of_project` and hands it to the export. An
invalid record refuses the export.

### The game runtime module

`browser/renderer/game-lighting.ts` is for a three.js game that loaded a SuperSpec
GLB with three's own loader: `gltfLighting(gltf)` reads the record from the extras and
`new GameLighting(renderer, scene, camera, record)` sets up the lights, the shadow
look, ambient occlusion, exposure and tone mapping on the game's scene; `adopt(root)`
gives loaded meshes the shadow look and removes the file's own punctual lights (the
module lights the scene from the whole record; both would light it twice);
`render()` draws a frame. It holds the Studio viewer's `SceneLighting` and adds only
what such a host needs. The world may be Y-up (a loaded glTF scene, the default) or
the record's Z-up; in a Y-up world the record's directions, the hemisphere, the
environment's turn and the shadow map's roll (which places the style's noise) are
turned with the world. Renderer settings outside the record (antialiasing, output
color space, pixel ratio) stay the host's. It lives beside the renderer modules it is
made of, with `ktx2.ts`'s `createGltfLoader`, not under `browser/runtime`, which holds
the compiler's workers.

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
source are 1 to 64 of `A-Z a-z 0-9 . _ -`; at most one light; `shadow.form` and `shadow.formFill` in [0, 1]; contour `strength` in [0, 1], `width` in [0.5, 8] px, `depth` in [0.001, 10] m, `normal` in [0.01, π] rad, `fade` in [0.001, 10] m per pixel. The ranges are at least
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
4. **Partly done.** `lighting` is declared in `AuthoringProject` (TypeScript) and the
   Studio has a Lighting panel for it: sun on or off, azimuth and elevation, color,
   intensity; whether the sun casts shadows, their style, softness, darkness, tint,
   tint strength, noise amount and scale; ambient intensity and colors; the
   environment, its intensity and turn; ambient occlusion on or off, radius and
   intensity; exposure; tone mapping; reset to defaults. An edit is admitted by the
   record's own validation (`parseLighting`; a refused value is said by field and
   changes nothing), lights the viewport at once through `SceneLighting` with no
   material rebuild and no engine request, and is stored in the project's `lighting`
   field, which Save project and Download GLB carry. The shipped look is stored as no
   field at all. Not done: the engine's `Project` does not declare the field (it has
   no `deny_unknown_fields`, so it ignores it; the hosts read it beside the engine,
   see [Delivery in a GLB](#delivery-in-a-glb)); the viewer does not read
   `Manifest::lighting` from a packet; catalog presets and theme selection; spread,
   maximum softness, wrap, the second tint, the noise edge and seed and the AO
   thickness have no control (they keep the record's values).
5. **Done.** Export fills `Manifest::lighting` for the scene it delivers and writes
   `KHR_lights_punctual` plus `extras.superspec.lighting`
   ([Delivery in a GLB](#delivery-in-a-glb)), the same bytes from native and Wasm;
   the game runtime module is `browser/renderer/game-lighting.ts`
   ([The game runtime module](#the-game-runtime-module)).
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
  judged. The Studio's exposure control edits the record's value (it was a session
  override before the Lighting panel existed). Moving them to an output profile later
  is a version bump.
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
- **AO and tone mapping on painted materials: a scene setting, `painted`** (revised in
  stage 5, 2026-11-16, unreviewed). The record has an optional
  `painted: { toneMapping, ambientOcclusion }`, global to the scene and never per material.
  Defaults: `toneMapping: false` (exposure and the tone curve skip painted materials, so the
  paint reaches the screen as authored, as plan step 1C.1 asks) and `ambientOcclusion: true`
  (the GTAO pass acts on the finished image; leaving painted pixels out would need a mask, so
  the viewer reports `ambientOcclusion: false` as not honored). It is written only when it
  differs from the default, so records and exports that never set it keep their bytes; the
  viewer applies it to every painted material (`setPaintedFinishing` in `shade.ts`). This
  replaces the earlier assumption that both apply.
- **Programs read the sun from this record** (stage 5): `SceneLighting.set` keeps the shared
  uniform `msSunDirection` (`programSun` in `shade.ts`) at the sun's direction, zero without a
  sun, so a sun edit redraws painted materials without a rebuild. The cast shadow a painted
  material folds into its shade band is the sun's shadow in this record's style
  (`CAST_SHADOW` in `shadow-style.ts`).
- **Limits.** The validation limits above replace the draft's proposals (for example
  AO radius up to 10 m, not 5; spread up to 2, not 1): they are the union of the
  draft's ranges and what the viewer's query parameters already accepted.
- **Diagnostics.** Findings carry field paths, not source offsets, until an authored
  source with offsets exists. A consumer that cannot honor a setting says so once on
  the console (`ambientOcclusion` without float render targets, an unknown
  `environment.source`) and continues with the stated fallback.
- **Ambient occlusion on software renderers: off unless asked for.** Assumed pending
  the user's decision. When the WebGL renderer is a software rasterizer (SwiftShader,
  llvmpipe, softpipe, WARP; recognized by its `WEBGL_debug_renderer_info` name),
  ambient occlusion is not drawn unless the record as stated says
  `ambientOcclusion.enabled` or the page says `ao=on`; it is said once on the console
  and in the Lighting panel, whose box then shows off until the author ticks it. The
  record's default stays on, and a real GPU is unaffected. So that the rule can tell a
  choice from a default, a project stores `ambientOcclusion.enabled` only once the
  author chose it in the panel (or turned it off); an exported GLB always states it.
  The whole policy is `browser/renderer/software-occlusion.ts` and one call in
  `SceneLighting.set`; deleting both removes it.
- **Punctual light intensity: the record's scalar, as written.** `KHR_lights_punctual`
  defines a directional light's intensity in lux; the record's intensities are
  renderer scalars (first item), so the export writes the number unchanged, which is
  also how three.js reads it. A photometric consumer sees a dim sun until the units
  question is settled.
- **Extras key: `extras.superspec.lighting`.** The exporter's existing extras object is
  `superspec` (lower case); the record goes there, not under a new `superSpec` key as
  first drafted.
- **A project without a customized look stores no record.** Reset to defaults removes
  the `lighting` field, so projects that never touched lighting stay byte for byte as
  they were and follow a later change of the shipped look.
- **Sun direction is edited as azimuth and elevation**, degrees: azimuth from +X
  toward +Y about +Z, elevation above the ground plane. The record keeps the unit
  vector; the angles are shown to two decimals and written back only when one of the
  two is edited.
- **`toneMapping: "none"` keeps the exposure**, and **`environment.rotation` is a turn
  about the world's vertical**: see [The record](#the-record). Both were found wrong
  or unstated when the paths were first rendered.

## Verification of steps 4 and 5 (2026-11-16)

Washroom, real GPU (ANGLE D3D11, NVIDIA RTX 5060 Ti) unless said; pictures are in
`generated/verification/lighting-*.png`.

- **The default picture is unchanged.** `tests/material-washroom-browser.mjs` at
  256 px, `?ao=off` and `?ao=off&shadowstyle=plain`, the build before these steps
  against the build after: 0 of 763,092 pixels differ, with the Wasm of either.
- **The panel** (`tests/lighting-washroom-browser.mjs`, 128 px,
  `?ao=off&preview=bake`): twelve edits and a tone mapping choice give the record the
  test works out itself, with the input revision and the status unchanged (nothing is
  rebuilt); softness 5 is refused as `lights[0].shadow.softness must be between 0 and
  2` and changes nothing; shadows off and on, the plain style and the sun off and on
  again return to the same picture (0 pixels); the saved project carries the record
  and, opened in a new page, draws the saved picture (0 pixels) with the panel showing
  the values; reset draws the shipped picture (0 pixels) and the project then has no
  `lighting` field.
- **Export.** The GLB of that project lists `KHR_lights_punctual`, holds one
  directional light with the record's color and intensity, and the complete record
  under `extras.superspec.lighting`. `superspec-studio glb` (native, Linux) writes the
  same 1,126,528 bytes as the browser's Wasm export of the same saved project.
  Three's own loader reads the file's light as shining along the recorded direction
  reversed, in Y-up, within 1e-6.
- **Round trip.** A three.js page (`tests/browser/rust-game-lighting.fixture.ts`)
  loads that GLB with three's `GLTFLoader` into a Y-up scene and lights it from the
  extras through `GameLighting`; the Studio's camera is given to it. Against the
  Studio's canvas (738 × 1034, `?ao=off&preview=bake`): 77,922 of 763,092 pixels
  differ (10.2%), 2,589 by more than 8 levels (0.34%), largest channel difference 53,
  mean 0.19. Every one of the larger differences that is not on an edge lies on a
  mesh whose material has live operations (tiles, duct, pipe, brackets, drain,
  openings), which the Studio runs in its shader and a plain glTF loader does not:
  that is material delivery, not lighting. On the other meshes (walls, slab, bench:
  210,983 pixels, with the sun, the styled shadows every mesh casts on them and the
  tone mapping) 202,252 pixels are identical, 7,886 differ by one level, 89 by more
  than 8 (largest 46), those on antialiased edges; the mean difference is 0.03. With the
  Studio's default GPU-baked preview maps instead of the baked ones (`?ao=off` alone)
  the two differ more, in texture: 31.5% of pixels, 1.6% by more than 8, mean 0.71.
- **Paths drawn for the first time** (256 px, `?ao=off`, records through `LIGHTING`):
  - `environment.rotation`: wrong before. The viewer turned the lying environment
    about its own axis, which tilts the room instead of turning it. Lit by the
    environment alone, the slab's upper face read 223.7 at 0°, and 207.8 at 90° before
    the fix against 223.4 at 90° and 223.2 at 180° after it; 360° is the 0° picture
    (0 pixels), and 0° is unchanged (0 pixels).
  - `environment.source: "none"`: failed before. Materials with a live reflection
    operation (the steel) did not compile without an environment (`live.ts` called
    three's radiance lookup, which exists only with one); the scene now draws, darker
    and without reflections.
  - `toneMapping: "neutral"` and `"none"` draw (the sunlit slab reads 192 with `aces`,
    173 with `neutral`, 181 with `none`); with `none` the exposure had no effect
    before and now has (181 at exposure 1, 132 at 0.5).
  - `lights: []` draws the scene by ambient and environment light alone, with no
    shadow (slab 143 against 192). The washroom test's own check assumed a sun; it now
    admits an empty list.
- **Software renderer.** On SwiftShader the washroom opens with ambient occlusion not
  drawn (`data-rendering` states `ambientOcclusionDefault`), the panel's box off and
  the reason under the controls; ticking the box draws it, and `?ao=on` draws it from
  the start. On the GPU the same page draws it by default.
- Checks: `tsc --noEmit`; `node --test tests/browser/rust-lighting.test.ts` (11
  tests); `cargo test -p superspec-types -p superspec-model -p superspec-export`;
  `check:architecture`. Rust was built from a snapshot of `HEAD` plus the lighting
  files, since the engine had another agent's uncommitted work.

Not verified: `tests/rust-studio-browser.mjs`, which presses keys on the exposure
slider, cannot run on this machine (it needs the native Windows CLI); the slider keeps
its id, range and step. KTX2 exports were not round-tripped (the record is written by
the same code after the textures). The game module's Z-up mode and its
`set`/`release` were not drawn. The item Studio has the same panel; it was driven by
hand-written steps only (an elevation and a darkness edit light its viewport with no
rebuild), not by a kept test.

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

## Small casters (2026-11-18, pending review)

A scene of fixtures on a wall (cages, brackets, pipes, air conditioners) reads as illustrated when small things shade
lightly and large forms clearly. Version 1 has no caster-size or per-model shadow weight; within the record the same
effect comes from three global or per-light settings, as `tests/item-facade-browser.mjs` sets them: the sun's shadow
`softness` 0.04 with `spread` 0.16 and `maxSoftness` 0.5 (the penumbra grows with the distance to the caster, so a
narrow caster's umbra fades within a hand's breadth; `spread` 0.3 with `maxSoftness` 0.9, tried first, showed the
shadow map's texels as blocky speckle over the whole lit wall), `darkness` 0.1, and ambient occlusion with a moderate `radius` (0.55 m),
`intensity` below 1 (0.8) and a `thickness` of 0.3 m. (Radius 1.1 m with thickness 0.12 m, tried first, broke up into blocky noise over open wall.) A per-light caster
size threshold or a per-model shadow class would be an additive field of `lights[].shadow` or of the scene, never of a
material.

## Illustration experiments (2026-11-19, pending review)

The shipped lighting record leaves form light and contour lines off. The facade capture
script supports `FORM=<0 to 1>` and `FORM_FILL=<0 to 1>`. Contours are an opt-in
comparison with `CONTOURS=1`. Captures use screen-space edge blur by default.
Set `QUERY="edgeblur=off"` to disable it or set `edgethreshold` to tune the sensitivity.
WorldSpec city previews also brighten the sunlit side of geometric edges within one device
pixel. `edgeboost=on|off` overrides this per view, including in ItemSpec and Material Studio.
The effect uses geometric normals and depth for edge ownership and compares each receiver
with the sun's shadow depth. Its default resets when another AML scene is opened.
Form light belongs to the light record and contours belong to the scene record. Like
`painted`, each is written only when it differs from its default.

```jsonc
{
  "version": 1,
  "contours": {                       // global; absent is off
    "enabled": true,
    "strength": 1.0,                  // how much of the line's color replaces the image on a line
    "width": 1.25,                    // CSS pixels on a silhouette; a crease is 0.6 of it
    "darken": 0.6,                    // the line looks this bright against its own surface
    // "color": [r, g, b]             // optional tint, linear RGB; absent is none
    "depth": 0.04,                    // meters: the step in depth that draws a silhouette
    "normal": 0.7,                    // radians: the turn that draws a full crease (it starts at half)
    "fade": 0.08                      // meters a CSS pixel covers where lines are gone (they fade from a quarter)
  },
  "lights": [{ "shadow": {
    "form": 0.35,                     // per light; absent is 0
    "formFill": 0.0                   // absent is 0
  } }]
}
```

Page overrides: `shadowform`, `shadowformfill`; `contours=on|off`, `contourstrength`, `contourwidth`, `contourdarken`,
`contourcolor` (sRGB hex tint), `contourdepth`, `contournormal`, `contourfade`.

### Form light in shadow (`lights[].shadow.form`, `formFill`)

A small prop standing wholly in shadow is lit by ambient light alone and loses its form. With `form` above 0 a surface
that something else shades keeps that share of the sun's diffuse shading, so the side turned to the sun stays brighter
than the side turned away. It is implemented in the shadow style (`shadow-style.ts`), where the sun's shadow is applied,
which every lit material of the viewer goes through, baked or per pixel:

- It is weighted by how much the cast shadow hides (`1 - cast`), so nothing changes in sunlight, and it is added before
  the shadow's shade multiplies the color, so it is darkened and tinted (cool, desaturated) like the rest of the shadow.
- The light comes from the sun's compass direction, level with the ground, not from the sun itself. The first version
  used the sun's own direction; the ground, which faces the sun more than any wall, was then lifted most, and the cast
  shadows on the paving visibly faded at 0.35. Level light gives a level surface nothing:
  the ground, a roof and an awning's top keep their shadows in full, and the sides of what stands on them are told
  apart. A wall that faces the sun's side and lies in a cast shadow is still lifted a little (the trees' dapple on the
  side face is fainter at 0.35); that is the size-blind part of the trick. A sun nearly overhead has no compass
  direction and gives no form light.
- `formFill` is the same shading from the opposite compass direction, on what is in shadow or turned from the sun, so
  the far sides are graded as well. It is diffuse only, like `form`.
- Painted materials shade themselves (their own band) and painted foliage has its own lighting: both are left alone.
- The form light is compiled into the shadow style only while `form` or `formFill` is above 0 (turning it on or off
  recompiles the patched programs once, like changing the style). A uniform alone was not enough: with the code present
  and switched off by a zero, one washroom pixel differed by one level.

### Contour lines (`contours`)

A drawn line along silhouettes and creases, in the darkened color of the surface it outlines. It is a pass over the composited frame
(`browser/renderer/contours.ts`, added to the frame `ambient-occlusion.ts` composes) and draws nothing of the scene: it
reads the depth, the face normals and the foliage mask the ambient occlusion already draws. With ambient occlusion off
the frame is still composed, with the occlusion's passes left out and that depth drawn for the lines alone.

- **Silhouettes** come from depth. Across a flat surface the reciprocal of the view distance is linear on screen
  however steeply the surface is seen, so its second difference is zero on a wall at a grazing angle and positive on
  the nearer side of a step. The line therefore lies on the nearer surface only, one `width` wide. The step is `depth`
  meters, and never less than the depth buffer resolves at that distance nor than a line width and a half as the pixel
  covers it.
- **Creases** come from the face normals: the angle to the neighboring pixel's normal, full at `normal` and starting at
  half of it, owned by the nearer pixel of the pair so the line is on one side. A crease is drawn at 0.7 of the width:
  the outline of a form weighs a little more than the edges inside it (0.6 since 2026-11-19).
- **The surface's own color, darker** (revised 2026-11-19: a fixed dark brown, #3a2a20 multiplied at 0.7, read as
  too cartoony). On a line the image becomes the color of the surface the line belongs to (linear, before tone
  mapping), made `darken` as bright to the eye (the factor goes through the display's gamma, 2.2, since the image is
  linear: 0.6 is about a third of the linear color; a linear 0.6 was tried and the lines all but vanished), 15% more saturated so it does not read as dirt, times the optional `color` tint: a cream wall
  gets a darker cream line, a green frame a darker green one, a red sign a darker red one. That color is read from the
  same taps as the line, weighted by how much line each holds, so the soft edge spilling onto the far surface still
  takes the nearer surface's color. The old look is `{ "darken": 1, "strength": 0.7, "color": [0.042, 0.023, 0.014] }`.
- **A fine line.** The default is 1.25 CSS pixels (creases 0.75, at least one device pixel), scaled by the device pixel
  ratio, so a picture at `SCALE=2` has lines of the same apparent weight (2 pixels at first; thinner since
  2026-11-19). The depth has one sample a pixel, so the line is read through a small tent filter (0.35 pixel) and has a
  soft edge without losing most of a one-pixel line's strength.
- **Too small or too dense to draw.** A pixel whose surface is farther on both sides within three line widths (a cage
  bar, a cable, a lattice, a glazing bar) is part of something too thin to hold a line on each side and gets none; and
  where more than about a third of the surrounding block (eight line widths across) is line, the lines are left out,
  since they would only blacken (the block is at least 12 CSS pixels, so a thinner line does not shrink it onto a single
  tile). With the thinner line the minimum feature, three line widths, is about 4 pixels instead of 6, so thinner
  frames and sills carry a line. Lines also fade with the size of a pixel in the scene (`fade`), so far detail stays
  clean. Texture never draws a line: only geometry is in the depth and normals.
- **Glass** is in the depth as a surface: a pane gets a line where it meets its frame and none across it, and nothing
  seen through it is outlined.
- **Foliage: none.** Foliage is not in that depth (it would darken the building through the tree), so it gets no line,
  and lines of what is behind a leaf are masked out. A line along the edge of the leaf mask was tried on the avenue and
  changed nothing that could be seen: the viewer draws the painted crowns over the frame with passes of their own. So
  trunks, crowns, shrubs and grass have no contour.
- The line is drawn in linear color before the output pass, so it is tone mapped with the image, and painted
  materials, which that frame already brings back to their authored color, take it like any other.

### Verification

Building scene, real GPU (ANGLE D3D11, NVIDIA RTX 5060 Ti), pictures in `generated/verification/illus/`:
`compare-<view>.png` is a sheet of four (top: both off, form 0.35; bottom: contours, both) for `storey`, `shops`,
`street`, `side-face` and `base`, from one bake; `crop-*.png` are parts of them at full size;
`generated/verification/illus-2x/` holds `shops` and `storey` at `SCALE=2`.

- Form: on the side face the stacked cartons by the shutter, in the trees' shadow, show a lit right side and a darker
  front where they were one flat tone; the paving's shadows are unchanged.
- Contours: a fine brown line on the building's edges, sills, courses, signs, crates, cartons, bollards and kerbs; none
  on the lattice grilles, the cage bars, the hanging cables or the wall's texture; with ambient occlusion off the lines
  are the same.
- The washroom (`tests/material-washroom-browser.mjs`, 256 px) with both off, against a build of the same tree without
  these changes: 0 of 763,092 pixels differ with `ao=off` and with `ao=on`.
- Cost: at 1420 by 1868 device pixels the frame took 8.8 ms without and 9.2 ms with contours (median of 9 runs of 20
  frames, `shops`; the frame is bound by the CPU's draw calls, so this is the difference in frame time, not the passes'
  GPU time). The form light made no measurable difference. With ambient occlusion off, contours draw the scene's depth
  once more (the occlusion otherwise has drawn it).
- Checks: `tsc --noEmit`, `node --test tests/browser/rust-lighting.test.ts` (13 tests),
  `cargo test -p superspec-types -p superspec-model lighting`.

Revision (2026-11-19), surface-colored lines: `generated/verification/illus-surface/compare-<view>.png` for `reference`,
`storey`, `shops` and `street` (form 0.35 in all four panels; top: no contours, the first brown line at 2 px; bottom:
the brown at the new 1.25 px, the surface-colored line at 1.25 px), and `zoom-<view>.png`, the same four enlarged. A
blue crate gets a darker blue edge, a carton a darker tan one, the orange cart a darker orange one; the window grilles,
already dark, keep a near-black line. Darken 0.5 and 0.7 were rendered beside 0.6 (`<view>-surface-d50.png`,
`-d70.png`): 0.7 is faint at 1x, 0.5 starts to read as an outline again.

Limits: the form light cannot tell a prop from a wall (see above); a joint or bevel whose step is near `depth` draws a
broken line (the paving's joints); a kerb's bevels draw two or three parallel lines close up; the Studio's Lighting
panel has no controls for either setting (they keep the record's values); the game runtime draws both through the same
`SceneLighting`, which was not pictured.
