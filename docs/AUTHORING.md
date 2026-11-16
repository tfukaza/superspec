# Material and item authoring

Run `npm run dev` after building the Wasm module as described in the repository
README. Open `/material.html` for MaterialSpec, `/item.html` for ItemSpec, or `/`
for the AML editor. The packaged native preview server serves the material and
item pages at the same paths, the AML editor at `/index.html`, and its watched
AML preview at `/`.

## Material workflow

Choose **Chipped concrete** or **Painted metal**, edit the source, and adjust the
controls generated from its public parameters. Compilation runs after edits;
**Compile** also starts it explicitly. The module selector lets you edit imported
helpers. **Add files** and **Add folder** load modules and companion images with
their relative names. **Use as entry** selects the material to build.

Use the swatch, open a GLB, or open an AML file and its companions. The AML folder
action follows the same relative-path rules as the Level editor. Select a visible
mesh in the viewport or inspector. **Apply material** assigns the recipe to that
mesh or every mesh sharing its original material binding. **Restore original**
removes the override. Selecting another mesh inspects it; application is explicit.

To edit several materials together, use **Add models** to import any number of GLB and AML
files at once. The **Scene** section lists the models and their materials. Choose a material
to edit its source and parameters. **Assign to selected mesh** gives the selected mesh to that
material. Every material previews at the same time, and the saved project and GLB carry them
all. The **Kowloon yard** starter shows the ported concrete and rusted-steel studies this way.
See [scene projects](material-typescript.md#scene-projects).

Choose authored UVs, object projection or world projection. Dimensions are meters.
Object projection follows an imported item's transform; world coordinates remain
fixed across placements. The channel selector shows PBR channels and named `let`
fields. Runoff inputs contain finite world-space edge segments and a gravity
vector. **Use swatch top edge** supplies an example. Horizontal receivers have no
downhill runoff; this version does not simulate pooling or flow across curved
surfaces. Original geometry and collision are never displaced by material height.
The exposure control changes preview lighting without compiling or changing saved
material inputs. The material starter uses a 256-pixel atlas so its eight-centimeter
chips remain visible on the one-meter swatch.

Source errors identify the module and source range. A failed or superseded edit
keeps the last valid preview, selection and camera. Correcting the inputs resumes
publication. **Save project** writes source modules, parameter presets, original
scene input and companion bytes into a versioned `.studio.json` bundle. When the
companions exceed 64 MiB it writes a `.studio.zip` archive instead: `project.json`
plus one uncompressed entry per companion under `assets/`, with each `assets` value
in `project.json` naming its entry rather than holding base64. Open project and
`superspec-studio` read both forms, including archives re-zipped with deflate.
**Save source** writes only the selected module. **Download GLB** exports the accepted
project through the same Rust build used by the native command. It does not
export a failing draft or silently apply a newly inspected surface.

## Material language

```text
material 1;
param feature_size: scalar = 0.08 range(0.005, 0.5);
param tint: vec3 = vec3(0.43, 0.40, 0.35) range(0, 1);
fn coverage(value: scalar) -> scalar = smoothstep(0.4, 0.6, value);
let patches = coverage(fbm(surface / feature_size, seed("patches"), 4));
output base_color = mix(tint, tint * 0.6, patches);
output roughness = mix(0.65, 0.95, patches);
output height = -patches * 0.002;
```

Modules use relative `import "./common.ms";` declarations. Public types are
`scalar`, `vec2`, `vec3` and `bool`. Functions compose expressions and fields;
recursive calls, ambiguous declarations, missing imports and import cycles are
diagnosed. Only the entry module declares outputs. Image names and named seeds
are string literals. Source modules are supplied explicitly by the host, without
language-level filesystem or network access.

`uv` is dimensionless; `surface`, `object`, `world`, height and sampling footprint
use meters. Colors in expressions are linear RGB. Bakes encode base color and
emissive maps in sRGB, normal maps in tangent space with positive Y, and packed
properties as R=1, G=roughness, B=metalness. HDR emission retains its intensity in
an emissive-strength factor. `sample_color` decodes sRGB; `sample_data` and
`sample_alpha` keep data channels linear. A physical texel footprint filters
procedural fields during baking. The preview's texture mipmaps respond to camera
distance without regenerating source programs or maps.

The version 1 backend produces portable textures for preview and export. It does
not compile arbitrary MaterialSpec expressions to GPU shaders or expose scene
depth/color buffers. View-dependent shader operations are therefore outside this
backend's capability; unknown operations are compilation errors, not silent
approximations. See the [language and helper reference](../crates/engine/src/materialspec/program/README.md)
for units, ranges, input contracts and limits.

## TypeScript recipes

Materials can also be written as `.material.ts` files: strict TypeScript against a
generated SDK, parsed by the same Rust compiler and never executed. Studio offers
TypeScript starters, a Recipe panel for structured edits and an on-demand strict type check; `superspec-studio` adds
`inspect`, `edit`, `sdk`, `catalog` and `check --types` so agents can read, tune and
verify recipes without a browser. See [TypeScript material recipes](material-typescript.md).

## Item workflow

Start with a pipe or duct, or **Open GLB** to author an imported static model.
Edit the versioned JSON definition and use its generated parameter controls.
Geometry controls change shape; finish controls resolve ordinary MaterialSpec
parameters while retaining compatible local geometry. The viewport shows bounds,
attachment points and optional collision inspection. Item coordinates are meters,
right-handed Z-up; pipe and duct length runs along local Z.

Use **Import material project** to bring in a saved material, including its imports,
presets and images. Select a mesh/material slot and **Assign material**. The source
records the assignment. A parameter reference such as `{"parameter":"paintColor"}`
can connect a public item control to a compatible material parameter. Hex color
controls use sRGB and are converted to linear RGB before material evaluation.
Save/reopen and GLB export use the same project actions as the Material editor.

Static GLB ingestion preserves hierarchy, transforms, local shared meshes, UVs,
material slots, supplied images and supported PBR factors. It converts glTF's Y-up
coordinates to Z-up. Supplied tangent bases are retained through preview and
export when normals are present. Missing normals become flat corners and supplied
tangents are ignored, as required by glTF. The current delivery adapter supports
UV0, repeat wrapping, linear magnification and trilinear minification. Unsupported material transforms,
samplers, per-vertex alpha, animation, skinning, morphs and compression fail
explicitly. The imported records retain additional UVs and material metadata for
future adapters. See the [ItemSpec definition reference](../crates/engine/src/itemspec/definition/README.md)
for the schema, collision shapes and import boundaries.

## Native builds and WorldSpec

`cargo build -p superspec-cli --release --locked` also builds `superspec-studio`.
These commands accept a saved bundle, a raw `.ms` or `.material.ts` recipe,
or a raw `.item.json`:

```sh
superspec-studio check pipe.studio.json
superspec-studio scene pipe.studio.json > pipe.lsp
superspec-studio glb pipe.studio.json > pipe.glb
superspec-studio check concrete.ms --resolution 64 --seed 12 --parameters preset.json
superspec-studio glb pipe.studio.json --profile production --resolution 2048 > pipe.glb
```

`--profile production` admits textures up to 2048 px per receiver (preview stops at 512)
and bakes rows on native threads; its maps decode to the same pixels as a preview bake at the same
resolution, compressed harder. `glb` stores textures as KTX2 by default (see
[Native exports](EXPORTS.md#texture-formats)); `--textures png` writes PNG.

Version 2 scene bundles with several models and materials build the same way. Scene and GLB
commands write binary stdout; use a shell or process API that preserves bytes. Check results and diagnostics are JSON. Raw-source mode admits
bounded sibling modules and relative companions inside the source folder. Bundles
use their embedded companions and the explicit shipped catalog, so moving a bundle
does not depend on its former working directory. Catalog images require the
packaged `materials/` directory beside the executable.

A world project combines ordinary AML with saved item definitions and material
modules. For example, this Node script converts a saved pipe project into a small
world bundle:

```js
import { readFileSync, writeFileSync } from "node:fs";
const item = JSON.parse(readFileSync("pipe.studio.json", "utf8"));
const world = {
  ...item,
  kind: "world",
  entry: "scene.aml",
  modules: {
    ...item.modules,
    "scene.aml": '<architecture version="1" id="example"><box id="floor" width="8" depth="8" height="0.2"/></architecture>',
  },
  base: { kind: "swatch" },
  target: { scope: "mesh", enabled: false },
  world: {
    projectPresets: { [item.entry]: item.parameters },
    items: [
      { id: "pipe-a", entry: item.entry, parameters: { length: 2 } },
      { id: "pipe-b", entry: item.entry, parameters: { length: 3 },
        transform: [[1, 0, 0], [0, 1, 0], [0, 0, 1], [2, 0, 0]] },
    ],
    materials: [],
  },
};
writeFileSync("world.studio.json", JSON.stringify(world, null, 2));
```

Build it with `superspec-studio scene world.studio.json` or `glb`. Explicit item
parameters override project presets, then theme presets, then definition defaults.
`world.projectPresets` and `world.themePresets` map item entry names to parameter
objects. An item's saved finishes remain authoritative unless a placement has an
explicit `slots` override. `world.materials` accepts assignments with `objects`,
`entry`, optional `parameters`, `seed` and `mapping`. Placed item object IDs are
`item/<placement-id>`. The browser editors reopen material/item bundles; world
composition is currently a native/API workflow.

The `superspec_engine` APIs include `materialspec::program::{compile, Program}`,
`itemspec::definition::{parse, prepare}`, `Session::build_project` and
`worldspec::authored::build_model`. One session owns AML and authored artifact
lifetimes. Hosts provide bytes and cancellation; the engine owns deterministic
generation. `superspec_model::instances::Model` separates immutable local mesh
resources from placements. `composition::WorldModel` retains those resources and
ordered visual assignments; `ProjectOutput::scene` creates a fresh delivery view.
`superspec_export::world_glb` expands at export. Its `prepare_lightmap` and
`prepare_lightmap_sized` APIs derive independent scenes from immutable input.

Appending items invalidates the previous whole-scene validation report. A material
replacement retains that report when canonical physical triangles are unchanged.
Hosts requiring navigation or route checks call the compiler's validation API on
the assembled scene; a successful visual build does not claim navigation proof.

## Revision and resource contracts

The dependency runtime keys source/import, image, program, mapping, geometry and
bake jobs by their effective content and implementation identity. Equal jobs share
immutable results. A changed image retains compatible receiver mapping; a changed
finish retains compatible local geometry. Failed jobs are not cached as successes,
and canceled or obsolete consumers cannot publish. Dependency-graph producers
execute serially; concurrent consumers can join the same result. Existing native
theme batches retain their worker execution behind the shared artifact store.

Input sizes, nesting, evaluator work, image pixels, geometry and retained cache
bytes have independent admission bounds. The authoring cache is bounded at
128 MiB, with a separate 32 MiB/16-entry program lookup cache. The same session
retains theme rasters and bindings in independent 256 MiB and 384 MiB partitions.
Clearing it releases all these retained artifacts without changing live outputs.
An artifact larger
than a cache budget can be returned without being retained. Live outputs and
temporary build buffers have their own lifetimes and admission bounds; a cache
budget is not a total process-memory guarantee. No performance measurements or
tuning were undertaken for this milestone.

Retained world composition checks each addition or replacement against the
existing delivery limits: 6 million vertices, 18 million indices, 256 MiB of
encoded images and 192 Mi pixels. Its separate 1 GiB retained-resource allowance
also includes superseded layers. Shared resources count once toward retention;
placed geometry counts toward delivery for each placement. Hosts can lower these
limits with `WorldModel::with_limits`. Rejected edits leave the prior world intact.
