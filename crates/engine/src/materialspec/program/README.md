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

## Evaluator performance

A bake evaluates a few thousand nodes per texel, so per-node cost decides bake time. The
evaluator keeps these properties; a change that breaks one shows up as a slower
`bench_fingerprint` (`cargo test --release -p superspec-engine --test material_kowloon
bench_fingerprint -- --ignored --nocapture`), which also prints material identities that a pure
optimization must leave unchanged.

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

Receiver bakes (`authoring/material.rs`) add two things on top:

- A normal needs the height one texel along each chart leg. Where that point is an adjacent
  texel's center, the bake reuses that texel's height instead of evaluating the height graph
  again; rows are evaluated in blocks, and a block evaluates the heights of the rows just
  outside it at the same centers, so output never depends on how rows are grouped.
- Hosts without threads share large bakes between sessions: `Session::distribute_bakes` makes a
  build stop at an uncached large bake, other sessions evaluate `bake_share`s (runs of
  consecutive rows), and the build is repeated with those rows. The browser does this with helper
  workers (`browser/runtime/bake-pool.ts`); the result is byte-identical to a local bake.

The examples exercise common helpers with concrete and painted metal. Tests must
cover typed imports and metadata, source errors/recovery, native/Wasm determinism,
physical scale/filtering, finite image/edge inputs and PBR channel conventions.
