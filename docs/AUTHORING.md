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

The version 1 backend produces portable textures for preview and export; the CPU
baker is the authority for both. It does not expose scene depth/color buffers, so
view-dependent shader operations are outside its capability; unknown operations are
compilation errors, not silent approximations. See the
[language and helper reference](../crates/engine/src/materialspec/program/README.md)
for units, ranges, input contracts and limits.

Studio also has two GPU previews, which are approximations and never authoritative:

- **Per-pixel shading.** `Program::shader` (`program/glsl.rs`) compiles a checked
  program to GLSL ES 3.00 and `browser/renderer/shade.ts` evaluates it per fragment
  for scene projects, with no bake. Values are 32-bit floats, filtering follows
  screen derivatives, both branches of a `select` are evaluated, `runoff` reads no
  edges, and receiver features are supplied only for scene meshes.
- **GPU preview bake.** `browser/renderer/bake.ts` draws the same shader into render
  targets over a swatch's texel grid (base color, packed properties, emissive,
  height, then a normal pass), so an edit to a swatch shows a texture a few tens of
  milliseconds after it is made, before the CPU draft. Its output is labeled
  `gpu-preview`: it is never a ready revision, is never saved or exported, and is
  replaced by the CPU draft and then the full CPU bake. It is skipped, and the CPU
  path runs alone, when WebGL2 or float render targets are missing, after a lost
  context or a shader compile failure, and for programs the plan cannot place (below)
  and receivers other than the swatch. A swatch supplies
  its four rim edges as receiver features, so the crease queries work on it. The
  normal pass applies the `receiver_normal` and `normal_offset` outputs with the
  CPU's `orient_relief` and `bend_shaded` rules; a `receiver_normal` that faces away
  from the receiver is a bake error on the CPU, so the preview gives way to it.
  Live renderer operations follow the edit: the program's live arguments (fields the
  shader already computes) are evaluated on the GPU into half-float textures, four to
  a texel, and the viewer's lighting effects read those instead of the last CPU
  build's baked arguments. This makes the Kowloon concrete and rusted steel studies
  preview.
  Targets of 256 px or more show a
  quarter-size bake first and are drawn in bands (131072 texels each) so no draw
  approaches a driver watchdog; a newer edit cancels the refinement. The viewport
  shows "GPU preview" next to the title while these textures are displayed. GPU and
  CPU pixels are not compared or expected to match.
- **Hybrid lane (CPU fields on the GPU).** `Program::shader_plan` places a program's
  CPU-only operations (`runoff`, which reads the supplied source edges) on the CPU and
  the rest on the GPU, as a compile-time decision rather than a failure. Data flows
  from the CPU to the GPU only: a field that uses `runoff` is evaluated by the CPU with
  everything upstream of it (so that math is absent from the shader unless something
  else needs it), and the shader reads its value from a float texture, one texel per
  bake texel, fetched with exact nearest alignment. Everything downstream stays in
  the shader. The plan lists each CPU field with the parameters, seed, images and
  edges it reads, so the host knows when to bake it again: `Session::bake_fields`
  (Wasm `bake_fields`) evaluates it over the same atlas a full bake uses. A GPU-side
  edit reuses the cached field; an edit to what the field reads (the source edges,
  gravity, a parameter it reads) keeps showing the previous field until the new one
  arrives. The shader plan and the field bake run on a worker of their own, in
  parallel with the CPU build: the CPU build never cancels them and they never
  terminate the CPU build's worker (a newer edit abandons a stale request, whose
  reply is ignored). The caption and `data-gpu-preview` name the channels that rest
  on CPU data (`cpuChannels`) and those that are GPU only. A CPU-only operation
  inside a helper function the shader needs, or written directly in an output rather
  than in a named field, is not supported in this prototype: the preview falls back
  to the CPU build with that reason. Likewise a CPU field consumed at coordinates
  other than the plain bake texel (for example through a warped coordinate) is not
  supported and is refused, not approximated; `runoff` only reads the sample's world
  position, so today's CPU fields are always at the plain domain coordinates.
- **Cold programs, texture units and real GPUs.** A GPU program's first draw is where
  the driver finishes compiling it, which for the Kowloon concrete takes about 0.7-0.9 s
  on an idle RTX 5060 Ti (D3D11) and several seconds while the CPU build runs beside
  it. That cost scales with the number of inlined noise calls; none of the loops,
  derivatives or `select`s tried dominated it. The GPU value is in edits, so the CPU
  draft and full build always show first, a late first preview is dropped, and the
  program keeps compiling in the background: a bake that gave way still compiles and
  draws its program once at a tiny size, so the next parameter or seed edit shows its
  preview within a few frames with no compile left (the concrete: a parameter edit
  previewed 78 ms after the edit on the RTX once warm). The baking program and the
  viewer material (four maps, the environment, the sun's shadow map and the packed live
  argument textures) must each fit `MAX_TEXTURE_IMAGE_UNITS` (16 on that GPU, not 32
  as under SwiftShader); a preview that would not fit falls back with that reason, and
  `data-gpu-preview` reports the units used. Headless Playwright defaults to
  SwiftShader, a software renderer, so timings there are order-of-magnitude only. To run
  the browser suites on a GPU set
  `BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist"` (PowerShell:
  `$env:BROWSER_ARGS=...`); `tests/material-gpu-preview-browser.mjs` prints the WebGL
  renderer string, texture-unit and draw-buffer limits at its start.
- **Shadows.** The Studio viewer casts and receives shadows from its sun by default
  (a 2048 px PCF soft shadow map fitted to the loaded models, drawn when the models
  change rather than every frame). `?shadows=off` in the page URL turns them off for
  debugging. `?shadowstyle=illustration` tries a stylized look (a prototype, not the
  default): shadows darker than plain and clearly blue, including faces turned from the
  sun; an edge gradient that stays tight where the caster touches the receiver and widens
  with the distance between them; and a large-scale noise that makes the edge wobble and
  varies darkness and hue inside the shadow. Overrides: `shadowdark` (extra darkness,
  0-1), `shadowtint` and `shadowtint2` (the hue and the hue the noise drifts to, hex
  colors without `#`), `shadowtintstrength` (saturation, 0-1), `shadowblur` (penumbra
  radius at contact, meters), `shadowspread` (radius gained per meter to the caster),
  `shadowblurmax` (largest radius, meters), `shadowwrap` (how far past the terminator a
  face starts to shade, as a cosine), `shadowedge` (edge displacement, meters),
  `shadownoise` (interior variation, 0-1), `shadownoisescale` (noise feature size,
  meters) and `shadowseed`.
- **Relief in the shader preview.** A scene project's materials are computed per pixel,
  and their relief normal comes from the height at the pixel and at the surface points
  one pixel across and one pixel down (two extra evaluations of only the fields the
  height reaches), not from screen derivatives of the height, which a GPU shares within
  each 2x2 pixel block and so step in blocks. The extra height code about doubles the
  first compile of a scene's programs on a real GPU; `?relief=quad` keeps the cheaper
  derivative relief.
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

Version 2 scene bundles with several models and materials build the same way. A bundle's
`texelDensity` (texels per meter) sizes each receiver's texture by its surface instead of giving
every receiver the full resolution; `tests/studio/washroom` is a scene that uses it. Scene and GLB
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
