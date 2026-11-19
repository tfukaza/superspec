# Material language version 1

Modules start with `material 1;`. Relative imports are resolved solely in the
host-supplied module map; there is no filesystem or network access. Imports share
a checked namespace. Duplicate declarations, module cycles and recursive helper
functions are errors. A program is reusable across parameter and sampling changes.

Public declarations use `param name: scalar|vec2|vec3|bool = constant;`, optionally
followed by `range(min, max)` before the semicolon. Ranges constrain each numeric
component. `fn helper(p: vec2) -> scalar = expression;` defines a composable field
helper. `let name = expression;` defines an inspectable field. Only the entry
module declares `output base_color = expression;` and optional `roughness`,
`metalness`, `height`, `emissive`, `opacity`, `receiver_normal` and `normal_offset` outputs. The last is
a world-space shading normal the height relief is rotated to before encoding; it defaults
to the receiver's `normal`, which leaves the normal map unchanged. `normal_offset` is a world-space
vector added to the shaded normal after the relief, then renormalized; it defaults to zero. Unknown bindings and types
fail compilation with module and UTF-16 source coordinates.

`uv` and its separate optional `uv_footprint` are dimensionless. `surface`,
`object` and `world` coordinates, height and the sampling `footprint` are in meters.
`bounds_min` and `bounds_max` give the sampled receiver's world-space bounding box.
The host must supply `uvFootprint` to filter UV-space fields; native bakes and scene
receiver sampling derive it from their UV coverage. Domain U/V vectors describe a physical bake
rectangle; geometry is never displaced by a material bake. Colors are linear RGB
in expressions. Bakes encode base color/emissive to sRGB, +Y tangent normals from
physical height derivatives, and linear properties with R=1, G=roughness, B=metalness.
HDR emissive samples are stored in a normalized image plus `emissiveStrength`;
multiply decoded linear RGB by that factor to recover the authored intensity.
RGBA rows begin at the rectangle's upper edge, consistent with existing images.

Seeded `noise`, `fbm` and `cellular` return [0,1]. Coordinate arithmetic propagates
a footprint estimate; details approaching or below a sample footprint blend
toward the field mean. `seed("name")` gives independent stable named seeds.
`scale`, `offset`, `rotate` (radians), `repeat` and ordinary vector arithmetic
compose coordinates. `mix`, `clamp`, `smoothstep`, comparisons, masks and numeric
helpers compose fields. `sample_color("image", uv)` decodes sRGB before periodic
bilinear filtering; `sample_data` keeps channels linear. Images are caller-supplied
complete RGBA8 data. `runoff(width, length)` evaluates finite supplied world-space
edges projected along gravity on the receiver; both parameters are meters.

Compilation and evaluation have explicit source, nesting, declaration, operation,
image and raster limits. Invalid inputs, division by zero and non-finite values
produce errors. No previous program or successful bake is mutated on failure.
Host adapters own last-valid publication, cancellation/revision checks and caches.
An `Evaluator` validates immutable inputs and applies one work allowance across
many samples. Individual samples also retain their operation and nesting limits.
`Program::retained_bytes()` conservatively charges owned sources, syntax trees,
source marks and metadata for cache admission. The authoring session retains at
most 16 programs within 32 MiB, including its separate source lookup copies;
larger successful programs can still execute without entering that cache.

| Helpers | Units and behavior |
| --- | --- |
| `noise(p, seed)`, `cellular(p, seed)`, `fbm(p, seed, octaves)` | Dimensionless field coordinates; output [0,1]. Divide meter coordinates by feature size in meters. FBM permits 1–12 octaves. |
| `scale`, `offset`, `warp`, `repeat` | Coordinate vectors and matching scalar/vector operands; repeat periods must be positive. |
| `rotate(p, angle)` | Two-dimensional coordinates; angle in radians. |
| `mix`, `clamp`, `smoothstep`, `step` | Compatible scalar/vector values; smoothstep requires strictly increasing bounds. |
| `abs`, `floor`, `fract`, `sin`, `cos`, `sqrt`, `min`, `max`, `length`, `normalize`, `dot` | Typed numeric helpers; trig uses radians, normalize rejects zero vectors. |
| `sample_color`, `sample_data`, `sample_alpha` | Literal supplied image ID and periodic UV coordinates; color decodes sRGB, data/alpha stay linear. |
| `runoff(width, length)` | Positive meters and caller-supplied finite world-space edges; receiver normal and gravity determine downhill direction. Empty edges or no downhill component produce zero. |
| `external("name", fallback)` | A scalar or vector the host supplies for each texel of a receiver; the fallback where it supplies none. See "Planes". |

## Cutouts, authored masks and ramps

- `alpha_cutout(coverage, cutoff)` (`alphaCutout`) is valid only as the whole opacity channel
  (directly, or as the field the channel names), with a constant cutoff in [0, 1]; anywhere else, or
  with another cutoff, compiling fails where it is written. The opacity is the coverage clamped to
  [0, 1], `Metadata::alpha_cutoff` records the cutoff and a bake delivers the material with alpha
  mode MASK and that cutoff. `doubleSided: true` on `defineMaterial` or `defineDecal` sets
  `Metadata::double_sided` and the delivered material's `doubleSided`.
- `authored_mask(name, coords)` (`authoredMask`) is the red channel of a linear image over the unit
  square of its coordinates (V up), bilinear between texel centers with the edge texels continuing
  to the square's edge, and zero outside the square: it never repeats. A missing image fails naming
  it.
- `ramp(mode, t, p0, c0, p1, c1, ...)` takes two to seven stops (a position and a color each, in
  ascending order) after the mode and `t`, which is clamped to the stops. `linear` interpolates
  neighboring stops and `constant` holds the stop at or below `t`, both on linear-light colors;
  `display-linear` and `display-constant` take display-referred (sRGB-encoded, [0, 1]) stops,
  interpolate them as such and decode the result. The GLSL has one helper per stop count.

## Render-only inputs

`time` (seconds of the renderer's clock), `instance` and `instance_w` (the drawn instance's vec4
value, as its first three components and its fourth), `instance_attribute(name, like)` and
`vertex_attribute(name, like)` (named per-instance and per-vertex attributes such as `root`,
`random`, `heightRatio`, typed like the second argument) exist only where a material is drawn.
The shader reads them from what the renderer binds: it declares `uniform float msTime`
(`ms_time`) and reads the varyings `msInstanceValue` (`ms_instance`), `msInstance_<name>` and
`msVertex_<name>`, which the host declares and sets; `Shader::render_inputs` lists them, and
`Shader::reads` names them `time`, `instance` and `vertex` among an output's sample inputs. They
enter no bake key: a bake (or `evaluate`) of a value that reads one fails with `MS_INPUT` where it is
read, and values that read none bake as before. Live operations whose arguments read them are not
yet routed to the renderer: they fail like any baked value.

Stage 5 adds (all render-only; `Shader::render_inputs` lists each with its `kind`):

- `sun_direction` (`sunDirection`): the unit vector toward the sun of the scene's lighting record
  (uniform `msSunDirection`). A bake that needs it is the source-linked `MS_INPUT` above.
- `instance_root`, `instance_tint`, `instance_random` (`instanceRoot`, ...): where the drawn
  instance's origin stands, its tint (white without one) and a number in [0, 1) per instance (its
  `random` attribute, else a hash of its index; 0 outside a batch). `placed_world` is the drawn point
  through the instance's placement; `world` stays the batch's own space, where its one bake is made.
  The host derives these varyings (`kind: "derived"`).
- `sample_field(name, xy)` (`sampleField`): a top-down scene field at a world XY position, `ground.color`
  (linear RGB) or `ground.normal` (world unit normal); any other name is refused where it is written.
  The shader reads `msField<k>` and `msFieldRect<k>` (`kind: "field"`, `name`), bilinear between texel
  centers with the edge texels continuing (`SceneField::sample` on the CPU). A renderer without the
  field, or a bake, fails where it is read, naming the field.
- A named attribute's `RenderInput::fallback` is the GLSL of the value its call is typed by, which a
  host reads where the drawn mesh has no such attribute.

`Inputs::render` (`RenderBindings`) binds all of them for a CPU evaluation that stands for a drawn
pixel: a reference for the shader, never a bake of the authoring session, which binds nothing.
`RenderBindings::default()` is the default binding: clock 0, the record's default sun, no instance
(origin, white, random 0, value zero), no cast shadow, each named attribute at its typed value and no
field. With it a program whose instance terms vanish at those values bakes the bytes it baked without
them (`with_default_bindings_a_program_that_reads_instance_inputs_bakes_as_before`). The row
evaluator declines programs that read render-only inputs, `shade_band` or `sample_field`; their
samples are evaluated one at a time.

## Painted shading, shade bands and delivered programs

- `defineMaterial({ shading: "painted", paint })` (`Metadata::shading`, `Metadata::paint`, the field
  `painted.color`) declares live inputs: the material is drawn by its program, and `paint` (linear
  RGB; the base color without it) reaches the screen without the light loop (`Shader::painted`). The
  bake still bakes its PBR channels, which must not read render-only inputs: they are the fallback.
- `shade_band(normal, sun, threshold, softness, range)` (`shadeBand`) is
  `smoothstep(-w, w, dot(normal, sun) - threshold)` with `w = max(range, 1e-6, softness * fwidth)`:
  `softness` is screen pixels of antialiasing, which a sample (and so the CPU) does not have. Where the
  material is drawn the sun's cast shadow, in the scene's shadow style, is folded in by the minimum
  (`ms_cast_shadow`, which the host sets from `shadow-style.ts`'s `msCastShadow`); a bake has none. The
  `edgeBreak` group gives a world-anchored offset (fractal noise over the position's XY) to add to the
  threshold.
- A painted material's delivered `Material::program` (`authoring::material::live_program`) carries the
  GLSL, the painted expression, the mapping, uniform values from the request's parameters and seed,
  `requires` (sample and render inputs, `shadow`, `field:<name>`) and the render inputs to bind. It sits
  beside the baked maps, enters no map's key, and is exported in `extras.materialSpec.program` with
  `shading`. A program that reads supplied images, receiver features, edges or planes is not delivered
  (its maps are). `superspec_export::live_report` names what an importer without the payload skips.
- `tests/material_live.rs` covers these; `tests/live-program-browser.mjs` evaluates the generated GLSL
  on the CPU's sample grid and checks that the viewer binds each input.
- Foliage (roadmap stage 6, `browser/renderer/foliage.ts`) evaluates a painted program in the vertex
  stage, once per vertex at a blade's root: the same `source`, with screen derivatives defined as zero
  (so a band's `softness` in pixels does not apply) and `shade_band` wrapped so the host keeps the band
  it reached. The program itself has no vertex form and no foliage input: the host sets `world`,
  `placedWorld` and `normal` to the root's.

## Recipe kinds: decals and weathering layers

A TypeScript recipe's default export says what it is (`Metadata::kind`).

- `defineDecal({ size, baseColor, opacity, roughness, metalness, height, emissive, blend })` is a
  bounded appearance sized in meters: it is baked on its own rectangle (`uv` spans it, `surface` is
  meters from its lower left corner) without receivers, and `metadata.decal` says where the
  rectangle lies in the texture. Its channels are its own values; `Metadata::blend` says how
  whoever places it combines each with the surface beneath, by the decal's opacity (`mix`, `add`,
  `multiply`, `max`, `min`).
- `defineLayer({ coverage, baseColor, roughness, metalness, height, opacity, normalOffset, blend })`
  is a weathering layer over a whole surface. It reads the surface beneath through `below.<channel>`
  (the external input `base.<channel>`), and each channel it gives is
  `mix(beneath, combined, clamp(coverage, 0, 1))` under the channel's blend rule; the channels it
  does not give are not outputs and pass through. A layer reads abstract inputs only (named
  features, masks, `runoff`, the occluder planes), never a receiver's bounds; `Metadata::reads`
  lists the channels beneath it reads.

## Layered slots

`Request::layers` (and `MaterialAssignment::layers` of a project) compose weathering layers over
the request's material, the lowest first; each layer has its own parameters, seed and `follow`, which
maps the layer's feature names to the receiver's (`{ "follow": "sill" }`). The composition is a
chain of planes, not one linked program:

- The base (`stack.base`) and each layer (`stack.1`, ...) are materials planes are baked from
  (`PlaneSource::Material`). A layer's `base.<channel>` input reads the plane of the material below
  it that last changed the channel; a channel a layer does not change passes by reference. The map
  bake is a composite program that reads each channel of the result as the plane of the material
  that last changed it (and the base's live-operation fields, so the base's live operations are
  delivered as the base bakes them). It bakes the bytes the stack written as one program bakes, the
  relief included (`a_stack_of_layers_bakes_the_bytes_of_the_same_appearance_written_as_one_program`).
- A plane node of the stack is keyed by its material's program and images, what its values read of
  that material's parameters and seed, what it follows, the layout, and the planes beneath it it
  reads; never by the composite. So the base is baked once under any stack: editing, adding or
  removing a layer bakes only that layer and the ones above it that read it. One pass bakes every
  plane of one material over a receiver (each reads all the planes beneath that any of them reads).
- The stack's output planes are in the exact form (`Plane::exact_external`: full precision, no
  filtering estimate, 8 bytes a component) and are baked on the profile's threads. They are not
  nodes the build graph runs (`StackPlanes`): a map is keyed by the keys of the planes it reads,
  and the planes are made when the map's rows are evaluated in this session, from the planes
  beneath, or found where an earlier build kept them (`LAYER_PLANES`, apart from the graph's
  store). A preview build keeps the planes it made; a production build keeps none, and lets each
  receiver's planes go once its maps are baked (and each plane once nothing more reads it).
- A layered bake is distributed like any large bake (`Session::distribute_bakes`): a plane's value
  at a texel reads the planes beneath only at that texel (and its leg points), so a session baking
  a share makes the whole stack over its rows and the rows beside them (whose heights its relief
  reads), into planes that hold just that window of the atlas (`Plane::exact_window`,
  `Plane::new_window`), and returns rows of the maps. The session that reported the bake makes no
  plane. A helper keeps the planes of its rows under keys of its share (index and count), so its
  share of an edit bakes only the materials the edit can change. The bytes are a local bake's
  (`a_distributed_stack_bakes_the_bytes_of_a_local_one_and_its_session_makes_no_plane`,
  `tests/stage2-layers-parity.mjs`).
- A composite that cannot be compiled (a stack whose channels and the base's live inputs exceed
  the 64 inputs a composite reads) is reported on the slot's base module, and
  `Session::check_layered` (which `shade_project` runs for a GPU host) refuses a slot as its bake
  would: the stack, the composite and each layer's parameters.
- Outputs of a material of the stack that are intrinsic (the anchored base's are) are also kept by
  lattice point (`INTRINSIC_OUTPUTS`), keyed by no layout, feature or plane: another receiver, a
  resized one and another stack over the same base read them instead of evaluating them.
- Planes carry no filtering estimate: a layer that feathers a value of the surface beneath by its
  estimate (`soft_threshold` of `below.height`) sees none.
- For a GPU host, `shade_project` adds `metadata.layers` to a layered assignment: each layer's
  shader plan, in which `below.<channel>` is a CPU field whose texture is that channel beneath,
  with the layer's seeds, `follow` and which material each input reads, and `metadata.channels`.
  Each mesh of the slot also gets `layerFeatures`: per layer, its receiver features as that layer's
  plan queries them (derived features if it reads any, and the named features it reads, renamed by
  its `follow`, with kind 5 plus the name's place in the plan's `namedFeatures`).
  `Session::bake_project_fields` bakes a layer's own CPU fields (not its `below.*` ones) for a name
  `stack.<k>/<field>`, with the layer's module, parameters, seed and followed features.
- The Studio's scene view composes a layered slot on the GPU (`GpuBaker.bakeStack`, browser
  `renderer/bake.ts`): a pass-graph node per plane (each material's channels, unencoded, read by
  texel), each layer's plan reading the plane nodes beneath it in place of its `below.*` fields,
  the maps composed from the planes of the materials that last changed each channel, and the
  relief from the height and offset planes. Nodes are keyed as the CPU's planes are, so an edit to a
  layer draws only it and the planes above that read what it changed. The base's intrinsic outputs
  are drawn into shared lattice tiles (512 cells square, keyed by the lattice step and the values
  they read, no receiver) when every chart of a receiver lies on its anchor's lattice, and a
  per-receiver mapping copies each texel's cell, so a resized receiver and another receiver over
  the same stretch of the pattern draw no base. It is a preview: planes are half floats (heights
  single), have no leg points, and the per-pixel look of a layered slot shows its base alone.

The Kowloon concrete is split this way beside the shipped recipe: `kowloon-concrete-anchored`
(intrinsic, on the pattern anchor), `kowloon-top-grime` (follows `top`), `kowloon-ground-stain` and
`kowloon-moss` (follow `base`; moss also reads `occlusion` and `sunExposure`),
`kowloon-concrete-edges` (the receiver's creases) and `kowloon-feature-grime` (any feature the slot
names). `kowloon-joint-rust` is the joint rust of the item washroom's steel as a layer over the
shipped `kowloon-rusted-steel`.

## Named features

A geometry's owner names polylines in world meters (`NamedFeature`: a sill, a joint, a base),
and a program measures to them with `feature_distance("name", maxDistance)` and
`feature_tangent("name", maxDistance)`. A material so never assumes where a sill is.

- `NamedFeatures` holds a receiver's segments by name, each name with its own grid
  (`FeatureSet`), so a query of one name never visits another's. A receiver holds at most
  65,536 named segments under at most 256 names, and a program queries at most 32 names; more
  is an error, never a silent cut. A receiver with no feature of a name answers as one whose
  features are all out of reach.
- Queries are charged like crease queries (one step, one more per 16 segments visited, eight
  operations per segment toward the row), and `Dependencies::named_features` and
  `Metadata::named_features` list the names read. A value that reads one is not intrinsic.
- `Program::evaluator_with_feature_sets` shares a receiver's derived features and named
  features, with their grids, between evaluators. A receiver bake derives and indexes both once
  with the layout (`Atlas::features`, `Atlas::named`): named features are part of the layout's
  key, so a moved sill bakes the receiver again and an unchanged one keeps everything.
- A receiver's derived features are at most `MAX_FEATURES` (4,096): creases and open edges
  first, in the order of their welded vertices, then UV seams, then contacts, and what does not
  fit is dropped from the end. A bake that reads a receiver which lost features says so
  (`featuresDropped` on its entry of `metadata.atlases`).
- `Evaluator::replenish_row` sizes a row's allowance by its samples (2,200,000 operations
  each, between 64,000,000 and 4,000,000,000), so a long row of a program with many queries is
  no longer failed by its length.
- In GLSL a name is a selector of its own: `Shader::named_features[k]` is selector `8 + k`,
  which reads the segments a host uploads to `ms_features` with kind `5 + k`.
- `authoring::item_features` turns an item's features (joint, end, bracket-contact,
  opening-edge, sill, base, top) into named features under the item's placement;
  `Request::features` and `Project::features` bind them to receivers.

## Weathering zones

A named feature may also be the edge of a zone its owner states (`NamedFeature::zone`, see
`zones.rs` and the ItemSpec README): where runoff, rust or soot hangs below a sill, a roofline,
a drip. `zone_coord("name")` (TS `zoneCoord`) is `vec3(u, v, id)` of the zone the sample lies in:
`u` meters along the edge (continuing from zone to zone as the owner numbers them), `v` meters
below it along downhill (0 at the edge, never above it), `id` the zone's identity in [0, 1);
`vec3(0, -1, 0)` outside every zone. `zone_extent("name")` (`zoneExtent`) is
`vec3(inside, limit, scale)`: 1 inside, where the zone ends in this column (its length or the next
edge below) and the owner's scale; `vec3(0)` outside. Where zones of the name overlap, the one whose
edge is nearest above wins (the least `v`). `zone_span("name")` (`zoneSpan`) is
`vec3(start, end, inside)`: the `u` where that zone's edge starts and ends, so a material can weight
streaks toward an edge's ends; `vec3(0)` outside. A layer follows zones as it follows features
(`follow: {"zone": "runoff-top"}` or a list).

- No search for edges per texel beyond the receiver's own few zone segments. A GPU bake evaluates
  them in its shader (`ms_zone_coord`, `ms_zone_extent`, `ms_zone_span` in `glsl.rs`, `ZONES`):
  `Shader::zones` names the zones in the order of their kinds, and `shade_project` gives each
  receiver its zones packed for each shader that reads them (`zones`, `layerZones`: `pack`, seven
  RGBA texels a segment, then the zones' stops and holes), only the segments whose zones can reach
  the receiver's box. A host binds the rows to `ms_zones` and their number to `ms_zone_count`. The
  GPU computes in single precision (a texel within 0.1 mm above an edge counts as on it); otherwise
  it agrees with the CPU (`item_zones::the_packed_zones_a_gpu_reads_give_what_the_cpu_finds`,
  `tests/item-zone-gpu-browser.mjs`). Before, the zone values were CPU fields of every layer of
  every receiver: 670 fields of 176 receivers, 14.8 s natively and most of a 19 to 24 s bake's
  field time in the Studio; the reference building now bakes with no CPU field at all.
- A grime layer then samples its tile at `vec2(u / period + shift(id), v / length)`, fading with
  `v / limit`: streaks start at their edge, stop at the next edge below and never climb.
- Zones are part of a receiver's named features, so they are part of its layout's key; a value
  that reads one is not intrinsic. Native and Wasm bake them with the same code, byte for byte
  (`tests/item-zone-parity.mjs`).

## Intrinsic values

A value is intrinsic (`Dependencies::intrinsic`) when it reads nothing of a sample but its
`surface` position and footprint, and no features, edges or planes. Two samples with the same
bits of both then have the same value on any receiver, so it can be kept by position of the
material's own domain instead of by a receiver's texels.

- A serial receiver bake keeps each costly intrinsic field in an intrinsic raster
  (`authoring/material.rs`, `IntrinsicRaster`), keyed by the program, the decoded images, the
  field and the parameters and seed it reads, and by no layout. Inside, values are found by
  lattice point: the footprint's bits (the lattice step) and whole numbers `i`, `j` with
  `surface = ((i, j) + 0.5) * footprint` exactly, in tiles of 64 by 64.
- A receiver's mapping reads the raster wherever one of its samples is exactly on its
  footprint's lattice, as it reads a kept plane, and computes the rest; what it computes at
  lattice points goes to the raster when the build finishes. A sample beside the lattice is
  never looked up or kept, so sharing cannot change a baked value: the maps are the bytes a
  bake that shares nothing makes. Another receiver over the same stretch of the domain, the
  same receiver resized, and a second receiver of the same build all read what was computed.
- Samples are on the lattice when a chart is axis-aligned in the surface domain, starts at a
  multiple of the texel size and has edge lengths whose arithmetic lands exactly (a power of
  two of texels is the sure case); `ReceiverLayout::surface` says so for each chart. Other
  charts share nothing. Sharing them needs sample positions snapped to the lattice, or
  resampling for charts that are turned or scaled, and either changes baked pixels.
- An anchored mapping (`authoring::Mapping::Anchored`, opt-in) puts receivers on the lattice by
  construction. The surface domain is measured from a pattern anchor the receiver's owner
  supplies (`Request::anchors`; an item part's `PatternAnchor`), so a resize does not move the
  pattern. With a texel density, a chart in a plane of the anchor's frame is a whole flat
  face (the triangles of that plane joined by shared edges) in its bounding rectangle along
  the anchor's axes, one texel for each lattice cell, starting wherever on the lattice its
  corner falls; each texel samples the triangle it lies in, or the nearest one. Texels past the receiver's edge sample the pattern where it continues, and a sample's
  surface position and footprint are the lattice point's and the lattice step's exactly. The
  item fixture `washroom-wall` resized through its width keeps the bytes of every lattice
  point both sizes cover, and reads the raster from its third size on. Charts in other
  planes, and receivers that cannot reach the density, are laid out as before in anchored
  coordinates. A face costs its bounding rectangle, so an L-shaped face pays for its notch.
- For a GPU host, `Shader::reads` flags intrinsic outputs, and the entry points
  `ms_evaluate_intrinsic` and `ms_evaluate_remainder` evaluate the two halves.
  `ReceiverLayout::surface` gives each chart's map from atlas texels to the surface domain.

## Planes

A plane (`planes.rs`, `Plane`) holds one value per texel of a receiver's atlas and is bound to
what its values stand for. It is the one way per-texel data from outside a sample enters an
evaluation, and both evaluators read it at the texel a sample is given with.

- **External inputs.** `external("name", fallback)` reads the plane bound to `name` at the
  sample's texel, or the fallback where no plane is bound or the texel holds no value (NaN in a
  supplied plane). A name has one type in a program (`Metadata::externals`), at most 16 names are
  read, and a plane of another component count fails the sample with `MS_INPUT`. The read costs
  its own step and its arguments' whether or not a plane is bound, so a program's cost bound
  never depends on the host. An external plane is an input like an image: `Dependencies::externals`
  lists the names each output and field reads, and a host keys what it bakes by the content of
  the planes bound to them. A value that reads one is not intrinsic. Planes supplied by a host
  hold single-precision numbers (`Plane::external`, `encode_external`, `decode_external`); a
  field plane another bake filled becomes an input at full precision with `Plane::into_external`.
  The names `shadow`, `occlusion` and `sunExposure` are meant for the occluder service; nothing
  supplies them yet.
- **Kept fields.** A plane bound to a field holds what an earlier evaluation computed for it. A
  reference to the field costs the reference's one step where the plane holds the texel, and the
  field's body is neither evaluated nor charged, in both evaluators. Because that spares steps,
  field planes serve only programs whose cost is bounded whatever their feature queries visit
  (`Evaluator::rows_evaluate_together`); for other programs they are ignored, so a limit never
  depends on what was kept.
- **Texels.** `Evaluator::evaluate_row_with_planes` and `evaluate_outputs_at` take the texel of
  each sample. A sample without one reads no plane. A bake gives a texel that repeats the sample
  of the texel before it in its row that texel's result; with external planes it never repeats
  the row above (which would depend on where a block of rows starts), and the height one step
  along a chart leg that is no texel's center is read at the texel the step starts from.
- **Planes from another program.** `Evaluator::fill_row` leaves the selected fields of a row in
  planes that are complete (a row that cannot be evaluated together fills them one sample at a
  time), so one program's fields become another's external inputs with their exact values and
  filtering estimates. A receiver bake uses it for `PlaneSource::Material`: a request lists
  other materials of its modules (`Request::materials`), and a plane node bakes one of their
  fields or outputs over the receiver, keyed by what that value reads of that material's
  parameters, seed and images. A layer over planes of a base bakes the bytes the two written
  as one program bake, the relief normal included: a relief needs heights one step along each
  chart leg, and where that point is no texel's center the plane bake evaluates the base there
  too and keeps the values as leg points (`Plane::leg`, `Plane::set_leg`). A plane a host
  supplies has none, so a height read from it is flat between texels. Such bakes of a request's
  other materials are not distributed (a session baking a share would bake the planes again; a
  layered slot's are, see "Layered slots"), and the materials planes are baked from read no
  planes themselves.
- **What a shader's outputs read.** `Shader::reads` lists, for each output and each live
  argument, the parameter uniforms, seed, images, edges, features, sample inputs, CPU fields
  and external planes it reads, and whether it is intrinsic.
- **GPU preview.** `Program::shader_plan` treats a field that reads an external input like any
  CPU field: it is baked by `Session::bake_fields` (which binds the request's planes) and read
  from a texture, and `CpuField::planes` names the inputs it follows. An `external` used
  directly in an output or helper cannot be partitioned, like `runoff`; the plain shader
  (`Program::shader`) evaluates the fallback.

## Evaluator performance

A bake evaluates a few thousand nodes per texel, so per-node cost decides bake time. The
evaluator keeps these properties; a change that breaks one shows up as a slower
`bench_fingerprint` (`cargo test --release -p superspec-engine --test material_kowloon
bench_fingerprint -- --ignored --nocapture`), which also prints material identities that a pure
optimization must leave unchanged.

- Compare instruction counts (`perf stat -e instructions:u` on the bench binary) before trusting
  a timing difference of a few percent. Where the linker places the row evaluator beside the
  kernels it calls moved bake times by 2 to 3 percent with no change to the work (measured by
  restoring one build's symbol order), and on WSL a buffer that is freshly mapped instead of
  recycled adds its page faults (about 2 ms per megabyte) to whichever step allocates it.
- Names, operators and catalog operations resolve to indices and enums once (`compiled.rs`);
  samples never compare strings, including feature selectors.
- Values are plain copies, and each node writes its value into a slot its caller provides.
  Returning the 48-byte value instead builds it in a temporary and copies it out, and reading
  that copy back right after narrower writes stalls the processor on every node.
- Literals, helper arguments and fields already computed for the sample return before the
  general evaluator, with the same step accounting.
- An `Evaluator` reuses its field table and resolved images (with their mip pyramids) across
  samples, and `evaluate_selected` returns chosen fields by index, so a sample allocates nothing.
- The noise lattice hash avoids the floating-point remainder, which is a software routine in Wasm.
- Receiver feature queries search a uniform grid over the evaluator's features (`features.rs`),
  visiting only the cells within the query's distance. Results equal a scan of every feature in
  order, including which of two equally near features wins, and a query is charged for the
  features it visited.
- Decoded images are shared (`Inputs.images` holds `Arc<Image>`), and a host may give several
  inputs one `MipCache`: the authoring session does, so every parameter set of a material
  samples one copy of the pixels and one set of mip levels.

- `Evaluator::evaluate_row` evaluates the samples of a row together (`lanes.rs`): each node of the
  tree computes its value for up to 64 samples before its parent runs, with the same scalar kernels,
  so the dispatch, step accounting and value copying of the per-sample walk are paid once per row.
  `select`, `&&`, `||` and fields stay lazy per sample, and nodes that read no sample input
  (literals, parameters, seeds and arithmetic on them) are computed once per row. It never reports
  an error: a sample that would fail, an invalid input, or a program whose compile-time cost bound
  (`compiled::Cost`: nodes, extra steps, nesting, with both `select` branches counted) could reach a
  limit makes the call decline, and the samples are evaluated one at a time, which fails with the one
  error that order always produced. `tests/material_lanes.rs` compares both paths bit for bit,
  including on generated programs, and `tests/material_bake_profile.rs` prints the material
  identities a bake of the example programs must keep.
- `Program::dependency_table` lists the parameters, seeds, images and edges each output and field
  reads (`deps.rs`), and `Evaluator::evaluate_outputs` evaluates only some outputs.

Receiver bakes (`authoring/material.rs`) add these on top:

- A receiver's maps are baked, keyed and retained per group (base color and opacity, relief normal,
  roughness and metalness, emission, live arguments). A group's key holds the layout, the program,
  the decoded images and the effective values of only the parameters, seed and edges its outputs
  read, so an edit that cannot change an output keeps the map baked from it, and a build evaluates
  in one pass just the outputs of the maps it has to bake (the relief is derived only for the normal
  map). A distributed bake (`PendingBake.groups`) asks other sessions for the rows of those maps only.
- The maps of a production bake are encoded at once, on native threads.
- Serial (preview) bakes keep the per-texel values of costly fields (`FieldPlane`, at least 24 steps
  per field) in a bounded partition of their own (48 MiB, at most 24 MiB added per bake). A plane is
  keyed by the layout, program, decoded images and the effective values of only the parameters, seed
  and edges the field reads, so a dirty map reads the planes of the fields its edit cannot change and
  computes the rest; the lane evaluator seeds its field table from them. Planes are kept only by a
  build that finishes, so a cancelled or failed one publishes nothing.
- The heights one step along each leg that no adjacent texel has, and a preview of one field, are
  evaluated with the row evaluator too.
- A request binds planes to a receiver's external inputs by asset (`Request::planes`). Each bound
  plane is a node (`plane/<receiver>/<name>`) keyed by its bytes and the layout it covers, and a
  map's node depends on the planes of only the inputs its outputs read, so a new plane bakes just
  those maps. Threaded bakes, shares baked by other sessions (`PendingBake::planes` names the
  assets they need) and field bakes read the same planes and give the same texels.

- A normal needs the height one texel along each chart leg. Where that point is an adjacent
  texel's center, the bake reuses that texel's height instead of evaluating the height graph
  again; rows are evaluated in blocks, and a block evaluates the heights of the rows just
  outside it at the same centers, so output never depends on how rows are grouped.
- Texels that sample the same point as a neighbor copy its result, and a face smaller than the
  minimum chart is sampled at the receiver's density rather than once per padded texel.
- Hosts without threads share large bakes between sessions: `Session::distribute_bakes` makes a
  build stop at its uncached large bakes (a project reports all of them in one build), other
  sessions evaluate `bake_share`s (runs of consecutive rows), and the build is repeated once
  with those rows. The browser does this with helper
  workers (`browser/runtime/bake-pool.ts`); the result is byte-identical to a local bake.

The examples exercise common helpers with concrete and painted metal. Tests must
cover typed imports and metadata, source errors/recovery, native/Wasm determinism,
physical scale/filtering, finite image/edge inputs and PBR channel conventions.
