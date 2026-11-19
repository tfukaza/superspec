# TypeScript material recipes

`.material.ts` files are the TypeScript front end for MaterialSpec. They are ordinary
strict TypeScript that type-checks against a generated SDK, but they are never
executed: Rust parses a documented subset and lowers it to the same program the `.ms`
language produces, so checking, evaluation and baking are shared by native and Wasm.
Existing `.ms` projects keep working unchanged. The design background is in the
[MaterialSpec SDK plan](plans/material-spec.md) and the
[language reference](material-language.md#proposed-typescript-sdk).

## What exists

| Area | State |
| --- | --- |
| Operation catalog | 49 operations: the 31 `.ms` v1 built-ins; the receiver operations `creaseDistance`, `creaseNormal`, `creaseTangent`, `creaseAdjacentNormal` and `adjustNormal`; `cross`; the mask primitives `remap`, `softThreshold`, `distanceBand`, `softUnion`, `maskIntersect` and `maskSubtract`; and `hash`, `valueNoise`, `pow`, `exp`, `triplanarWeights` and `variedColor`. Plus 15 operator functions, 9 groups, 6 live renderer operations and 10 sample inputs. One typed table drives the checker, the generated declarations and the AI reference. |
| Receiver features | Creases (convex or concave, with their angle and middle normal) and open boundary edges derived from each receiver's geometry, queried with `creaseDistance`, `creaseNormal`, `creaseTangent` and `creaseAdjacentNormal`; the receiver's world bounds as `boundsMin` and `boundsMax`. |
| Receiver normals | A `receiverNormal` channel that turns the frame the height relief is applied in, a `normalOffset` channel that bends the shaded normal after the relief, `adjustNormal`, and the `roundedEdges` group. |
| Study ports | The Kowloon concrete and rusted-steel studies as recipes (see [Kowloon ports](#kowloon-ports)). |
| Recipe subset | Imports, `const`, pure helpers, `param.*`, `layer`, group calls and `export default defineMaterial(...)`. |
| Structured edits | `inspect` and `edit` operations that produce minimal text patches, shared by the browser and the command line. |
| Studio | Material Studio opens `.material.ts`, offers a Recipe panel and TypeScript starters, keeps its last valid preview on error, and composes several imported models with a different material per mesh (see [Scene projects](#scene-projects)). |
| Command line | `superspec-studio check\|scene\|glb\|inspect\|edit\|sdk\|catalog`, including version 2 scene projects. |
| Type checking | Rust diagnostics at edit time; strict TypeScript on demand: **Check types** in Material Studio, `check --types` and `npm run check:sdk`. |

Coordinates are mapped per bake (`mapping`: `uv`, `object` or `world`) and read as `surface`;
there is no separate `mapCoordinates` operation. Not implemented from the design reference:
`neighborCells`, `blendNormals`, `authoredMask` and `alphaCutout`.

### Coordinate spaces

The SDK declarations type vectors by coordinate space: `Vec2<S>` and `Vec3<S>` with `S` one of
`"none"`, `"uv"`, `"surface"`, `"object"`, `"world"` or `"direction"`. `uv` and `surface` carry
their own spaces, `object` is object space, `world`, `boundsMin` and `boundsMax` are world space,
and `normal` and `gravity` are directions. Literals, constructors, parameters, colors and group
outputs have `"none"`.

- Every vector argument of one operation must agree: `sub(world, object)` and `add(surface, uv)`
  are type errors that name both spaces (`SpaceMismatch<"world", "object">`).
- `"none"` and `"direction"` combine with any space and take the other operand's space, so
  `add(world, mul(normal, 0.01))` stays in world space.
- A vector result keeps the combined space; image samples, varied colors and triplanar weights
  have `"none"`. Swizzles keep the space: `world.xy` is `Vec2<"world">`.
- A plain `Vec3` annotation accepts every space, but its value has no particular space and
  cannot meet a spaced vector. Helpers that keep a space are generic:
  `const twice = <S extends Space>(p: Vec3<S>): Vec3<S> => mul(p, 2);`.

Spaces exist only for TypeScript. The compiler accepts the annotations, the `<S extends Space>`
parameter lists and `type` imports and ignores them, so they never change output. Evaluation
stages need no separate types: live operations return `LiveEffect`, which only `live` accepts,
and recipes have no view-dependent inputs to misuse.

## Source format

```ts
import { defineMaterial, layer, param, tileAndGrout, vec2, vec3 } from "@superspec/material/v1";
import { coverage } from "./common.material";

const tileSize = param.vec2({ default: vec2(0.3, 0.15), range: [0.05, 1], unit: "m" });
const tiles = tileAndGrout({ size: tileSize, rowOffset: 0.5 });

export default defineMaterial({
  base: { baseColor: vec3(0.78, 0.76, 0.7), roughness: 0.18 },
  layers: [layer({ id: "grout", mask: tiles.joint, baseColor: vec3(0.32, 0.31, 0.29), height: { add: -0.003 } })],
});
```

- **Imports.** The SDK is `@superspec/material/v1`; only a named import list is allowed
  (`import type` too). Relative imports (`./common.material`) name sibling modules the
  host supplies, with `.ts` implied. Each imported field, parameter or helper must be
  explicitly exported by its defining module. Helpers retain access to that module's
  private bindings; callers can use only their own declarations and named imports.
  Import aliases and imported layer/group objects are outside the supported subset;
  declare those objects in the consuming module. Imports stay within their source
  language: `.ms` and `.material.ts` modules cannot import each other. Only the entry
  module has a default export.
- **Declarations.** `const` values are inspectable fields. Helpers are
  `const f = (x: ScalarInput): Scalar => ...` or `function f(x: Vec3): Vec3 { return ...; }`
  with explicit types `Scalar`, `ScalarInput`, `Vec2`, `Vec3`, `Bool`, `BoolInput`. Use the
  `*Input` aliases for parameters so helpers accept literals.
- **No infix operators.** A branded value type cannot be added with `+`, so recipes call
  `add`, `sub`, `mul`, `div`, `rem`, `neg`, `lt`, `le`, `gt`, `ge`, `eq`, `ne`, `and`,
  `or`, `not`. They lower to the same expression nodes as the `.ms` operators.
- **Parameters.** `param.scalar|vec2|vec3|bool({ default, range?, unit?, label? })`. The
  constant name is the stable host parameter ID, even for a private parameter used by
  an imported helper. Parameter IDs must be unique across the supplied dependency
  graph; TypeScript `export` controls recipe bindings, not visibility of host controls.
  A range applies to every component. Imported fields and helpers have module-qualified
  internal identities, so separate modules may reuse their local names.
- **Names.** `snake_case` catalog names are `camelCase` in recipes (`sample_color` is
  `sampleColor`, `uv_footprint` is `uvFootprint`). Using an SDK name without importing it,
  or declaring a name that collides with one, is an error.
- **Outside the subset** (reported as `MT_SUBSET`): `let`/`var`, classes, template
  literals, ternaries, loops, `as` aliases, operators, and anything that would execute.
  Numeric literals may be negative (`-0.002`); other negation uses `neg()`.

### Recipe semantics

`base` gives each channel its starting value. `layers` apply in order: for each channel a
layer names, the value blends in linear space, `mix(previous, value, coverage)`, with
`coverage = clamp(mask, 0, 1)`. A `height` layer either adds, `{ add: delta }` becomes
`previous + coverage * delta`, or blends toward a replacement, `{ replace: height }`.
Channels a layer does not name pass through, and channels nobody sets use the `.ms`
defaults. `receiverNormal` is the exception to linear blending: a layer turns the normal
toward its value along the great circle, `adjustNormal(previous, value, coverage)`, and the
channel starts at the receiver's own `normal`. `channels` replace a channel's final value. Every layer exposes inspectable
fields `layers.<id>.coverage` and `layers.<id>.<channel>`; the base exposes `base.<channel>`.

### Groups

A group is a named, parameterized set of fields written in MaterialSpec itself, so it
expands to ordinary fields (`<instance>.<field>`) with independent named seeds per
instance. `const tiles = tileAndGrout({...})` exposes `tiles.face`, `.joint`, `.chip`,
`.substrate`.

| Group | Fields | Notes |
| --- | --- | --- |
| `tileAndGrout` | `face`, `joint`, `chip`, `substrate` | Square or staggered tile, per-cell missing tiles, chips. Joint softness follows the sample footprint. |
| `groundWeathering` | `stain`, `growth`, `contact` | Fades with world Z; growth needs an upward-facing receiver. |
| `runoffStreaks` | `coverage` | Needs finite source edges and gravity from the host. |
| `edgeWear` | `coverage`, `height`, `distance` | Wear along exposed edges (convex creases at least `minAngle` sharp, and open boundaries), with an irregular outline. Zero on receivers without exposed edges. |
| `oxide` | `edge`, `surface`, `spot`, `combined` | Edge oxide from the same query, patchy surface oxide, isolated spots, and their soft union. |
| `roundedEdges` | `distance`, `weight`, `receiverNormal` | Shading round-over of convex creases within `radius` (default 14 mm): the weight falls linearly from one on the crease to zero at the radius, and `receiverNormal` turns that far toward the crease's middle normal. Geometry and collision are unchanged. |
| `rectangularGrid` | `sideDistance`, `cornerDistance`, `random`, `cell`, `local` | Cells of `size` meters from `origin` over `position` (default `surface`); odd rows shift by `rowOffset` of a cell. `cell` is the integer column and row (floored, so negative coordinates work), `local` the position inside the cell in meters, and `random` a stable per-cell value. |
| `periodicStrips` | `joint`, `solid` | Joints `gap` wide every `period` meters along one coordinate, centered on multiples of the period, with coverage integrated over the footprint. |
| `directionalCoordinates` | `along`, `across`, `coordinates` | Coordinates along `direction` projected onto the surface and across it, divided by `scale`; zero where the direction is parallel to the normal. |

### Receiver features

`creaseDistance(selector, minAngle, maxDistance)` returns the distance in meters from the
sample to the nearest selected receiver edge, and `maxDistance` when there is none, so a
receiver without features is an empty result rather than an error. Selectors:

| Selector | Edges |
| --- | --- |
| `"convex"` | Outward creases at least `minAngle` degrees sharp (box corners). |
| `"concave"` | Inward creases at least `minAngle` degrees sharp (a floor meeting a wall). |
| `"boundary"` | Open edges with a single face (the rim of a panel). |
| `"exposed"` | Convex creases and boundaries: what wears and rusts first. |
| `"any"` | Every crease at least `minAngle` sharp, and every boundary. |
| `"seam"` | Edges where the two faces' UVs differ, bent or flat: texture seams. |
| `"contact"` | Crease and rim edges lying within 1 mm of another visible mesh, such as a wall's base on a slab. Project builds supply these from the composed scene: version 2 scene projects and version 1 projects with a target mesh in an AML or GLB base, in Studio, export and `superspec-studio` alike. A bake with no surrounding scene (a swatch) has none. |

When Studio or the CLI bakes a receiver, it derives that receiver's features from its own
world-space triangles. Queries search a grid over those features, so a receiver with thousands
of edges costs a query about as much as one with a dozen. Vertices within a micrometer are welded, so meshes with split per-face
vertices still share edges. Edges whose faces bend by less than 1 degree are triangulation
seams and never features, and non-manifold edges are ignored. At most 4,096 features are kept
per receiver. Direct API callers pass features in `Inputs.features` or per receiver with
`Program::evaluator_with_features`. A misspelled selector is a compile error in Rust and in
strict TypeScript. Groups that query features cap the distance beyond their own falloff
(including the sample footprint), so coarse bakes do not read a capped distance as wear.

### Receiver normals

`creaseNormal(selector, minAngle, maxDistance)` returns the world-space unit normal at the
middle of the nearest selected crease, halfway between its two faces: the normal a rounded
edge has at its center. Boundaries have one face and are never chosen, and creases whose
middle normal faces away from the sample (the far side of a thin part) are ignored. Without
a crease it returns the receiver's `normal`. `adjustNormal(normal, target, weight)` turns a
normal toward a target along the great circle, with the weight clamped to [0, 1].
`creaseAdjacentNormal` returns the normal of the face across that crease (the receiver normal
reflected across the middle normal), and `creaseTangent(selector, minAngle, maxDistance)` the unit
direction along the nearest selected edge of any kind, or `vec3(0)` without one. The tangent's
sign follows the stored edge order: stable, but not oriented, so use it for alignment rather than
for a side.

The `normalOffset` channel (`normal_offset` in `.ms`) is a world-space vector added to the shaded
normal after the height relief, then renormalized. A sum that would point into the surface, as
interpolated normals near a crease can produce, stops just above the surface's horizon, and an
offset that exactly cancels the normal changes nothing. A crease's adjacent face normal is
`sub(mul(m, mul(2, dot(m, normal))), normal)` for its middle normal `m`, so a round-over is
`normalOffset: mul(adjacent, weight)`. Leaving the channel out, or zero, changes nothing.

```ts
const edges = roundedEdges({ radius: 0.014 });
export default defineMaterial({
  base: { baseColor: vec3(0.5, 0.5, 0.5) },
  channels: { receiverNormal: edges.receiverNormal },
});
```

The `receiverNormal` channel (`receiver_normal` in `.ms`) is the world-space shading normal
the baked height relief is applied around. The bake rotates the relief by the smallest
rotation from the receiver's own normal (the interpolated vertex normal a renderer decodes
the normal map around) to this one, then encodes it in the usual tangent space. Leaving the
channel out, or setting it to `normal`, produces byte-identical maps. A receiver normal facing
away from the surface is a bake error rather than an inverted surface. Only the normal map
changes: geometry, collision and the other channels are untouched.

### Live renderer operations

`live: [modulateDiffuse({ factor, mask }), grazingFalloff({ from, to, mask }), scaleSpecular({ factor, mask }), lightFloor({ red, green, blue, mask, from, to }), filterEnvironment({ roughness, spread }), bandLuminance({ spacing, softness, mask, brightSoftness, liftRed, liftGreen, liftBlue })]`.
Each operation's arguments are fields. The sample evaluator never runs them; the renderer does.

**At bake time**, every argument field is evaluated per texel and stored in the baked
material's `live` record:
- An argument that never varies becomes a constant.
- Varying arguments are quantized four to a linear RGBA texture on the material's UVs, decoded
  as `byte / 255 * scale + offset`.

**In GLB**, the record is written to `materials[].extras.materialSpec`:

```json
{ "version": 1, "live": [{ "id": "modulateDiffuse", "stage": "lighting-composition",
  "requires": ["diffuse-contribution"],
  "arguments": { "factor": { "texture": 3, "channel": 0, "scale": 0.04, "offset": 0.98 }, "mask": { "value": 1 } } }] }
```

Texture arguments name glTF textures. Renderers without support ignore the extras and show
the baked PBR material.

**In Material Studio**, the Three.js viewer executes all six operations in the material's
shader:

| Operation | Effect |
| --- | --- |
| `modulateDiffuse` | Scales direct and indirect diffuse light by `mix(1, factor, mask)`; specular is untouched. |
| `grazingFalloff` | Ramps from zero at view cosine `from` to `mask` at `to`. On surfaces facing the sun and visible in its shadow map, this raises specular F90 toward 1.08. |
| `scaleSpecular` | Scales blended specular color by the masked change from 1 to `factor`. Positive changes use 12% of the authored gain, weighted by sun facing and shadow visibility. Reductions apply at full strength. The resulting color is capped at 1. |
| `lightFloor` | Adds linear radiance `(red, green, blue)` times `mask`; when `from` and `to` differ the mask also ramps with the view-facing cosine, as in the study's silhouette floor. An operation may appear more than once. |
| `filterEnvironment` | Reflects nine environment taps spread `spread` radians around the reflection at `roughness`, keeping the mean of the quadrant with the lowest luminance variance (a directional Kuwahara filter). A spread of zero is the ordinary reflection. |
| `bandLuminance` | Steps reflected luminance into bands `spacing` apart in square-root luminance, with transitions `softness` wide (as a fraction of a band), blended in by `mask`. Optional `brightSoftness` takes over for bands whose middle is between 0.60 and 0.95, and `liftRed`/`liftGreen`/`liftBlue` brighten bands below 0.42 (fading out by 0.87): the study's painted values. |
## Kowloon ports

The LevelSpec concrete and steel studies are ported as recipes in
`crates/engine/src/materialspec/program/examples/`:
- `kowloon-noise.material.ts` has the shared fields.
- `kowloon-concrete.material.ts` is the concrete study.
- `kowloon-rusted-steel.material.ts` is the steel and rust study.

Material Studio offers each as a starter, plus **Kowloon yard**, which puts both on two models
with four materials. Parameters keep the studies' art defaults and labels.

**Concrete** follows the study's shader stage by stage:
- Two source images are sampled with varied lattice copies, limited to ±0.16 rad of rotation,
  with contrast correction around each image's mean. The images are the study's own, copied
  beside the recipe with their provenance.
- Then come the A and B darkness fields, the eroded blend field and the pigment drift.
- Repair cutouts sit in 2 m × 1.6 m panels.
- Construction joints run every 2 m, and the face rims and cutout outlines get chipped
  shoulders.
- Form-tie holes sit on a 1 m grid with 4 cm of face clearance.
- Chips follow the study's slider defaults: 24 cm scale, 22 % amount, intact roughness 0.30,
  chipped roughness 0.92 and 3.5 mm relief.
- The base of walls gets ground stain and moss.
- The study's top grime mask is evaluated per sample from its own formula.
- Convex creases sharper than 35° get the 14 mm crease treatment: broken grooves, chipped
  shoulders and a shading round-over.
- An optional plaster coat uses the catalog plaster image.
- `ground` switches to the study's slab relief.

The study's per-face attributes come from the receiver:
- **Face coordinates:** face-centered meters on each face's dominant plane, from `world` and
  the receiver bounds.
- **Face size and top distance:** from the bounds.
- **Ground contact:** height above the bounds' base.

This is exact for axis-aligned walls and slabs like the study's.

**Rusted steel** has these parts:
- **Substrate:** the study's bare steel by default. Galvanized and painted values are noted in
  the recipe.
- **Oxide patterns:** independent edge, surface and raindrop oxide, each with a strength.
  Raindrops sit on the study's tilted 38-per-meter lattice.
- **Oxide color:** triplanar oxide from the catalog rust image, with fully rotated lattice
  copies and no contrast correction, toned between the study's two browns. Oxide roughness is
  0.92 and its metalness is 0.
- **Polished rims:** an 8 mm band that becomes glossy and rounds, narrower on thin parts.
- **Live operations:** diffuse-only grain, the silhouette lift and banded gloss are live renderer
  operations. Material Studio renders them and GLB carries them.

Where a port differs from its study, by design:
- **Noise fields.** The noise comes from the engine's lattice hash with a cubic fade rather than
  the GLSL `sin` hash with a quintic fade. Octaves, gains, scales and thresholds are the
  study's, so fields have the same structure but not the same pattern.
- **Edge distances.** They come from the receiver's creases and open rims instead of the study's
  analytic box, cylinder and tube dimensions. Edge bands are capped by the receiver's smallest
  half extent, like the study's boxes.
- **Antialiasing.** Screen-space derivatives become the propagated footprint estimate (see
  `softThreshold`). Features much smaller than a texel, such as the millimeter raindrops, fade
  toward their average coverage.
- **Round-overs.** Both studies bend the shaded normal after the relief with `normalOffset`, adding
  the adjacent face's normal (the receiver normal reflected across the crease's middle normal)
  weighted by crease distance. The concrete study sums up to four nearby creases; the port uses
  the nearest.
- **Lighting.** The study's grain, silhouette lift, rim specular boost, rim and silhouette light
  floors, gloss filtering, bands and the painted shadow lift are live operations. Comparison
  against frozen LevelSpec views belongs to the LevelSpec harness.

Baked detail is limited by the atlas. Each receiver gets one texture of the requested size,
packed with rectangular charts at one texel density:

- Each triangle starts at its corner closest to a right angle, so its legs are the chart axes.
- The two coplanar triangles of a quad (sharing the hypotenuse, matched by vertex position and
  UV) share one chart, the second turned a half turn, so no texels are wasted beyond a diagonal.
- Charts are sized by their sides in meters and rotated a quarter turn to lie wider than tall;
  the largest density whose shelf packing fits the square is used. Every chart keeps a 2-texel
  gutter of clamped samples, and texels outside every chart are transparent (normals flat).
- A chart is at least 8 texels on a side. A face smaller than that at the receiver's density is
  padded to 8 texels but sampled at the density, so its texels repeat those few samples: a mesh
  of thousands of tiny faces (gravel, a tessellated pipe) costs one evaluation per face instead
  of 64, and shows no finer detail than its larger faces do.
- Texels that sample the same point as their left or upper neighbor, such as the doubled gutter
  texels, copy that neighbor's result instead of evaluating it again. This never changes a map.

**Texel density.** By default every receiver fills the requested `resolution`, so a hand wheel
gets as many texels as a wall. A project or request may set `texelDensity` (texels per meter,
1 to 100000). A receiver that reaches that density in a smaller square then takes the smallest
such square, the resolution halved some number of times down to 16, at exactly that density. A
receiver too large for it keeps the resolution at the density that fits, as before. Each
material's metadata lists its receivers' `atlases` (`width`, `height`, `texelsPerMeter`).

An 8 m x 3.2 m x 22 cm wall gets about 61 texels per meter at 512 px and about 250 (4 mm
texels) at 2048. Normals are quantized to 8 bits with a half-step, per-texel white-noise offset:
plain rounding leaves gentle relief in wide plateaus of identical normals whose edges a glossy
highlight shows as contour stripes. `material_atlas` checks the pairing, the uniform density,
the dithering and that every chart orientation decodes to the analytic relief normal.

Measured natively , the concrete recipe costs
about 46 µs for a full evaluation and 16 µs for a height-only one, per sample.

## Scene projects

A version 2 Studio project can compose several imported models and give each mesh its own
material. Rust builds it through the same `build_project` for Studio preview (Wasm
`preview_project`), GLB export and `superspec-studio scene|glb`.

```json
{
  "schemaVersion": 2,
  "models": [
    { "id": "yard", "kind": "aml", "source": "<architecture …>" },
    { "id": "crate", "kind": "glb", "name": "crate.glb" }
  ],
  "materials": [
    { "id": "concrete", "entry": "kowloon-concrete.material.ts", "parameters": {}, "seed": 7, "meshes": ["yard/wall"] },
    { "id": "steel", "entry": "kowloon-rusted-steel.material.ts", "parameters": { "edgeRust": 0.5 }, "seed": 7, "meshes": ["crate/root/body"] }
  ]
}
```

The rest of the project (modules, assets, resolution, mapping and so on) is unchanged. Scenes
with parts of very different sizes should set `texelDensity` (see
[texel density](#kowloon-ports)), so small parts get small textures.

- **Models** are GLB companions or AML scene text. Each model's mesh and owner identities are
  placed under `<id>/`, so two models, or two copies of one model, never collide. Model ids
  are 1 to 64 letters, digits, `.`, `_` or `-`, and a project has at most 16 models. Image
  and material identities are shared: two AML models may reuse an id only for identical
  content.
- **Materials** all read the project's shared module map, so several entries, or the same
  entry with different parameters, are edited side by side. They apply in order to disjoint
  sets of visible meshes, at most 64 materials per project. A material with no meshes yet is
  still compiled, so its diagnostics and parameters are reported. A mesh named twice, or a
  mesh that doesn't exist, is an error.
- **Reuse.** The scene is rebuilt whole on every edit. Unchanged materials, models and decoded
  images come from the session's content-keyed caches, so editing one material re-bakes only
  that material. Materials that sample the same image share one decoded copy and one set of mip
  levels, however many parameter sets use it.

In Material Studio:
- **Add models** accepts several GLB and AML files, plus image companions, at once.
- The **Scene** section lists the models and the materials. Choose a material to edit it: the
  source editor, Recipe panel and parameter controls follow the active material.
- **New material** starts one from the current module.
- **Assign to selected mesh** moves the mesh selected in the viewport or the mesh list to the
  active material. **Unassign selected mesh** returns it to its imported material.
- **Save project**, **Open project** and **Download GLB** carry the whole scene.

`tests/material-scene-browser.mjs` drives this workflow end to end.

## Diagnostics

Diagnostics carry the module, line, column and UTF-16 offsets, as `.ms` does, and the
compiler reports every independent problem in one pass (parse errors stop at the first).
Codes: `MS_*` shared with `.ms`, plus `MT_SUBSET`, `MT_PROPERTY`, `MT_ARGUMENT`, `MT_BASE`,
`MT_CAPABILITY` and `MT_EDIT`. Strict TypeScript findings keep their `TS####` codes.

## Structured edits

`inspect` returns parameters, fields (with the span of every use), helpers, groups, base,
layers, channels and live operations, with UTF-16 spans and source text. `edit` applies one
operation and returns minimal patches plus the new source, or diagnostics and nothing else.
The patched recipe is compiled before anything is returned, so an incompatible edit can
never reach the file. Comments and unrelated expressions are untouched.

| `op` | Fields | Effect |
| --- | --- | --- |
| `setLiteral` | `offset`, `value` | Replace the numeric literal starting at `offset`. |
| `reconnect` | `start`, `end`, `expression` | Replace exactly one expression, such as one use of a shared field. |
| `setParameterDefault` | `name`, `value` | Number, boolean or `[x, y(, z)]` for vectors. |
| `setParameterRange` | `name`, `range` | `[min, max]`, or `null` to remove. |
| `addLayer` | `source`, `index?` | Insert `layer({...})`; creates `layers` when missing. |
| `removeLayer` | `id` | Also removes the layer's constant when nothing else uses it. |
| `moveLayer` | `id`, `to` | Reorders; separators and comments stay in place. |

## Command line (for agents and scripts)

All commands print JSON with `schemaVersion: 1` and `success`; failures print the same shape
on stderr with exit code 1 (2 for argument errors).

```bash
superspec-studio sdk --out generated/material-sdk      # index.d.ts, ports.json, ai-reference.md, tsconfig.json
superspec-studio catalog                               # the operation catalog
superspec-studio inspect concrete.material.ts          # structure and spans
superspec-studio edit concrete.material.ts --op '{"op":"setParameterDefault","name":"chipSize","value":0.12}'
superspec-studio edit concrete.material.ts --op @op.json --write   # saves only if it still compiles
superspec-studio check concrete.material.ts --types    # Rust diagnostics + strict tsc
superspec-studio scene concrete.material.ts --resolution 64 --parameters preset.json > concrete.lsp
superspec-studio glb concrete.material.ts --profile production --resolution 2048 > concrete.glb
```

A workflow that works well for an agent: run `sdk` once, read `ai-reference.md`, write the
recipe, run `check --types` until it is clean, then `inspect`/`edit` for tuning and `scene`
or `glb` to produce output. `--types` needs `tsc` (`SUPERSPEC_TSC`, `node_modules/.bin`, or
`PATH`) and writes the SDK to `--sdk` (default `generated/material-sdk`).

## Material Studio

Choose a TypeScript starter or add a `.material.ts` file. Edits compile on a short debounce
and the last valid preview stays visible while a revision fails. The **Recipe** panel edits
parameters (default and range), layers (reorder, remove, add, change masks and values),
group arguments and each use of a shared field; every control issues the operations above,
so the text changes minimally. A searchable operation list inserts examples at the caret,
and the SDK declarations and AI reference can be downloaded.

**Check types** runs strict TypeScript 5.9 over every `.material.ts` module in a worker of its own,
against the same generated SDK declarations the Download button offers. Findings keep their
`TS####` codes and select their source range. The preview never waits for it, and Rust stays
the authority on whether a recipe builds. The compiler loads on first use, and its program is
reused between checks.

Parameter controls pass overrides to the compiled program without recompiling the source;
only text edits recompile. Overrides for parameters a new source no longer declares are
dropped automatically, without reporting a failure for them.

A control changed while the panel is still refreshing after its previous edit is applied once
the refresh lands, to the same layer channel, group argument or field use, so quick successive
edits are not lost. If the source text changed in between (typing, reopening, switching
modules), the old control does nothing and the panel asks you to wait for it to refresh.

**Preview speed.** Baking dominates edit latency and scales with texture size. For
textures of 128 px or more, an edit first publishes a 64 px draft, then refines to the
requested size. The draft changes only resolution and scheduling (same source, seed and
parameters); it is never ready, saved or exportable, and a newer edit cancels the
refinement, so dragging a control stays cheap. The viewport swaps textures only when a
build completes, and `tests/material-ts-browser.mjs` asserts that no frame shows an empty
scene during rapid edits. Measured in headless Chromium with software rendering on a
development machine (256 px texture, including the 180 ms debounce, a seed edit so every map
rebakes): before the CPU-side speedups (tiled courtyard) the draft appeared after about 0.45-0.55 s
and the full-size build after about 3-4 s; with them (chipped concrete, Kowloon concrete and
rusted steel starters) the draft appears after about 0.20, 0.31 and 0.22 s and the full-size build
after about 0.28, 0.65 and 0.40 s. Treat these as orders of magnitude, not benchmarks. A build rebakes only the maps an edit can reach: the color, normal,
properties, emissive and live maps are each keyed by the parameters, seed and source edges of the outputs
they are baked from (`program/deps.rs`), so changing a roughness parameter keeps the color and normal maps.
Samples are also evaluated 64 at a time (`program/lanes.rs`) with bit-identical results. Preview bakes also keep
the per-texel values of costly fields (a separate 48 MiB partition, keyed by the parameters, seed and
edges the field reads), so a map that an edit does reach recomputes only the fields that edit can change.

For a swatch, Studio shows a **GPU preview** (`browser/renderer/bake.ts`) before the draft
when the browser supports float render targets: the program's GLSL
([`Program::shader`](../crates/engine/src/materialspec/program/glsl.rs)) is drawn over the
swatch's texel grid. It is approximate and labeled `gpu-preview` (`data-gpu-preview`): never a
ready revision, saved or exportable, and replaced by the CPU draft and then the full build. A
parameter edit only updates uniforms and rebakes. Programs that use `runoff` preview as a hybrid:
the CPU bakes the runoff field once per layout and the shader reads it as a texture (see the
[authoring guide](AUTHORING.md)), and the caption says which channels rest on CPU data. Programs the
plan cannot place, and every non-swatch receiver, skip it and show
the CPU draft alone (`data-gpu-preview-fallback` names the reason); `?gpupreview=off` disables it.

**Bake profiles.** Requests and projects carry `profile`: `"preview"` (the default) or
`"production"`. Both evaluate the same program with the same seeds and write the same pixels
for a given resolution; production compresses its PNG maps harder (DEFLATE level 3 with adaptive
filters), so its files are smaller and differ in bytes, and the bake cache keeps the two apart. Preview admits textures up to 512 px per receiver and bakes on the
calling thread. Production admits up to 2048 px, the 4 MiB-texel atlas allowance, and splits a
receiver's texel rows across up to eight native threads, each with its own evaluator. Rows are
written back in their original order and the earliest failing row is the one reported, so
threaded output and errors match a serial bake. Wasm always bakes on one thread; in the browser
the profile only raises the size limit. Cancellation is checked between batches of rows. In the browser, large bakes are evaluated by
helper workers: one build reports every such bake of the project, the helpers evaluate them, and
the build runs once more with their rows. Opening a project shows the 64 px draft first, like an
edit. The
CLI takes `--profile preview|production`; a bundle may set `"profile"` itself.
`material_kowloon` checks that threaded production maps decode to the preview's pixels, and its ignored
`bench_profiles` measured a 384 px concrete wall at 9.0 s serial and 1.8 s on 8 threads
(release build, 12-thread development machine).

## Verification

- `cargo test -p superspec-engine --test material_ts --lib catalog` covers the catalog,
  the front end, groups, layers, diagnostics and edits.
- `cargo test -p superspec-engine --test material_features --test material_masks --test material_primitives`
  covers receiver features and normals, mask primitives, noise, varied sampling and receiver bounds.
- `cargo test --release -p superspec-engine --test material_kowloon --test material_project` bakes the
  Kowloon ports on real receivers and builds multi-model, multi-material projects.
- `cargo test -p superspec-cli --test recipe_tools` covers the command line.
- `npm run check:sdk` type-checks the example recipes and `tests/sdk-types/negative.material.ts`,
  where every line must fail strict TypeScript.
- `npm run test:recipe:browser` drives the built studio through the Wasm workers.
- `npm run test:scene:browser` drives scene projects: the Kowloon yard, material switching and
  edits, mesh assignment, adding models, save, reopen and export.
- `npm run test:washroom:browser` opens the [washroom benchmark scene](../tests/studio/washroom/README.md)
  (11 meshes, 8 materials, all but one from the shipped recipes) in the built Studio and reports its load time.
- `npm run test:material:browser` compares native and Chromium/Wasm bytes for the `.ms`,
  TypeScript and Kowloon fixtures.
