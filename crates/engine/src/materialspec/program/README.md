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
