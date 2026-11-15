# MaterialSpec language reference

The first part describes the implemented `.ms` v1 language. The second records
the proposed TypeScript SDK and the helper review for the concrete, plaster, and
steel study ports. Each helper family links to its implementation.

## Current language (.ms v1)

### Modules and declarations

```text
material 1;
import "./common.ms";

param chip_size: scalar = 0.08 range(0.005, 0.5);
fn chip_noise(p: vec2) -> scalar = fbm(p / chip_size, seed("chips"), 4);
let chips = coverage(chip_noise(surface), 0.52, 0.08);
output base_color = mix(vec3(0.43, 0.40, 0.35), vec3(0.24, 0.22, 0.19), chips);
output roughness = mix(0.68, 0.96, chips);
output height = -0.002 * chips;
```

- Every module starts with `material 1;`. Declarations end in semicolons.
  Comments use `//` or `/* … */`.
- Imports resolve relative to the containing module in the host-supplied source
  map. Paths use forward slashes and stay inside the module root. Imports share
  one namespace. Imports stay within the `.ms` language; TypeScript recipes have
  separate lexical module bindings. Duplicate bindings, missing modules, import cycles, and
  recursive field or helper dependencies are errors.
- Parameters have a name, a type (`scalar`, `vec2`, `vec3`, or `bool`), and a
  constant default. An optional inclusive numeric range constrains each
  component of the default and any supplied override. Authors give physical
  meaning to values, such as meters for `chip_size`.
- A helper has typed arguments and a typed result defined by one expression. A
  `let` declaration defines an inspectable field, with optional type annotation.
  Fields can be referenced by multiple outputs. Every declared field is
  evaluated for inspection, even when no output uses it.
- Only the entry module declares outputs. Diagnostics identify the module and
  source location with UTF-16 offsets. Source size, syntax depth, declarations,
  evaluation work, image inputs, and raster size are bounded. Invalid types,
  missing required images, invalid arithmetic, and out-of-range channels produce
  errors. Studio retains its last accepted preview when a revision fails.

[Parser](../crates/engine/src/materialspec/program/syntax.rs) ·
[Checker](../crates/engine/src/materialspec/program/check.rs) ·
[Evaluator](../crates/engine/src/materialspec/program/eval.rs)

### Operators and types

Numeric operators work component by component. Two vectors must have the same
dimension. A scalar broadcasts across the other operand's vector components.
Precedence from highest to lowest is component access and calls, unary
`+`/`-`/`!`, `*`/`/`/`%`, binary `+`/`-`, scalar comparisons
`<`/`<=`/`>`/`>=`, equality `==`/`!=`, `&&`, then `||`. Binary operators
at the same precedence associate left to right. Parentheses group expressions.
Division and remainder by zero fail evaluation.

Equality compares like types and compares every component of a vector.
`!`, `&&`, and `||` take booleans, and the binary logical operators short-circuit.
Use one to three component letters from `xyz` or their aliases `rgb` within the
input vector's dimension. For example, `world.xz` returns a `vec2` and
`color.r` returns a scalar. Seed names and image IDs are nonempty literal strings
of at most 1,024 bytes. Strings do not serve as inspectable values or outputs.

### Sample inputs and outputs

| Input | Type | Meaning |
| --- | --- | --- |
| `uv` | `vec2` | Dimensionless UV coordinates. A unit interval spans one image repeat. |
| `surface` | `vec2` | Mapped surface coordinates in meters. |
| `object` | `vec3` | Object-space position in meters. |
| `world` | `vec3` | World-space position in meters, right-handed and Z up. |
| `normal` | `vec3` | Receiver normal in world space. |
| `gravity` | `vec3` | World-space gravity direction. The default points down along Z. |
| `footprint` | `scalar` | Sample diameter in meters. |
| `uv_footprint` | `scalar` | Dimensionless UV footprint. Defaults to zero when the host omits it. Bakes derive it from UV coverage. |
| `bounds_min`, `bounds_max` | `vec3` | World-space bounding box of the receiver being sampled, in meters. A swatch bake supplies its rectangle's corners. |

Coordinate operations propagate a footprint estimate. The noise functions fade
under-resolved detail toward 0.5, per octave for `fbm`. UV-space noise uses the
UV footprint, while fields derived from physical coordinates start with the
meter footprint. `sample_color`, `sample_data` and `sample_alpha` use periodic bilinear
filtering at the supplied UVs and do not choose mip levels. `varied_color` chooses a
linear-light mip level from its coordinate's footprint estimate.

| Output | Type | Accepted values | Default |
| --- | --- | --- | --- |
| `base_color` | `vec3` | Linear RGB, components in `[0, 1]` | Required |
| `roughness` | `scalar` | `[0, 1]` | 1 |
| `metalness` | `scalar` | `[0, 1]` | 0 |
| `height` | `scalar` | Signed meters, magnitude at most `1e6` | 0 |
| `emissive` | `vec3` | Linear RGB, components in `[0, 1e6]` | Black |
| `opacity` | `scalar` | `[0, 1]` | 1 |
| `receiver_normal` | `vec3` | Finite nonzero world-space direction facing the receiver's side; normalized | The receiver's `normal` |
| `normal_offset` | `vec3` | Finite world-space vector, components at most `1e3` in magnitude | Zero |

Bakes encode base color and emissive RGB as sRGB and keep opacity linear.
An HDR emissive map stores normalized RGB with `emissiveStrength` to recover the
linear intensity. The linear properties map stores R=1, G=roughness, and
B=metalness. Physical height derivatives produce a tangent-space +Y normal map
without displacing geometry. A declared `receiver_normal` rotates that relief by the
smallest rotation from the receiver's own normal to it before encoding; omitting it, or
setting it to `normal`, leaves the map byte-identical. A declared `normal_offset` is then added
to the shaded normal in world space and the sum renormalized, as a shader bends a normal after
normal mapping (both Kowloon studies round edges this way); zero leaves the map byte-identical,
and a result facing away from the receiver is a bake error. Encoded image rows start at the top.

[Sample contract](../crates/engine/src/materialspec/program/mod.rs) ·
[Baker](../crates/engine/src/materialspec/program/bake.rs)

### Built-ins

All 49 built-ins are listed below: the 31 of language version 1, plus the receiver
operations `crease_distance`, `crease_normal`, `crease_tangent`, `crease_adjacent_normal` and
`adjust_normal`, `cross`, the mask primitives
`remap`, `soft_threshold`, `distance_band`, `soft_union`, `mask_intersect` and
`mask_subtract`, and `hash`, `value_noise`, `pow`, `exp`, `triplanar_weights` and
`varied_color` (new reserved names). `pow`, `exp` and the new operations' trigonometry give
identical bits on every target, so native and Wasm bakes agree. In the signatures, **S** is a scalar,
**V** is a `vec2` or `vec3`, and **N** is either. Repeated **V** or **N**
means the same type within a row. “Broadcast” allows a scalar to pair with a
vector and returns that vector type. These are documentation conventions.

| Family | Signature | Behavior |
| --- | --- | --- |
| Vector | `vec2(S) / vec2(S, S) / vec2(vec2) → vec2` | Splat, construct, or pass through. |
| Vector | `vec3(S) / vec3(S, S, S) / vec3(vec2, S) / vec3(vec3) → vec3` | Splat, construct, append a component, or pass through. |
| Math | `abs(N) / floor(N) / fract(N) → N` | Componentwise absolute value, round down, or fractional part `x - floor(x)`. |
| Math | `sin(N) / cos(N) → N` | Componentwise, in radians. |
| Math | `sqrt(N) → N` | Componentwise square root. Negative inputs fail. |
| Math | `min(a, b) / max(a, b) → N` | Componentwise with scalar broadcast. |
| Vector | `length(V) → S` | Euclidean length. |
| Vector | `normalize(V) → V` | Unit direction. Length below `1e-12` fails. |
| Vector | `dot(V, V) → S` | Dot product of equal-dimension vectors. |
| Mix | `mix(a: N, b: N, t: S or N) → N` | Linear interpolation. Values of `t` outside `[0, 1]` extrapolate. |
| Mask | `clamp(x: N, lo: S or N, hi: S or N) → N` | Clip to inclusive bounds. A reversed bound fails. |
| Mask | `smoothstep(lo: S or N, hi: S or N, x: N) → N` | Cubic transition for strictly increasing bounds. |
| Mask | `step(edge, x) → N` | Zero below the edge, one at or above it. Scalar broadcast applies. |
| Mask | `remap("clamp" or "extrapolate", x: N, in_lo: S, in_hi: S, out_lo: S, out_hi: S) → N` | Map a strictly increasing input range to an output range (which may descend). `clamp` keeps results inside the output range; `extrapolate` continues the line. |
| Mask | `soft_threshold(x: N, threshold: S, softness: S) → N` | Cubic rise from zero to one across `threshold ± softness / 2`, in the value's units. Softness is positive; the value's own variation across the sample widens the transition. |
| Mask | `distance_band(distance: S, inner: S, outer: S) → S` | Cubic fall from one at `inner` to zero at `outer` meters, `0 <= inner < outer`. Half the distance's footprint softens each side. |
| Mask | `soft_union(a: S, b: S) / mask_intersect(a: S, b: S) / mask_subtract(a: S, b: S) → S` | Inputs clamped to `[0, 1]`; `1 - (1 - a)(1 - b)`, `a * b` and `a * (1 - b)`. |
| Selection | `select(test: bool, yes: T, no: T) → T` | Matching boolean or numeric branches. Evaluates only the chosen branch. |
| Coordinates | `scale(V, S or V) → V` | Componentwise multiplication. |
| Coordinates | `offset(V, S or V) / warp(V, S or V) → V` | Both add a displacement. The graph labels can distinguish a constant offset from a varying warp. |
| Coordinates | `rotate(vec2, radians: S) → vec2` | Rotate around the coordinate origin. |
| Coordinates | `repeat(V, period: S or V) → V` | Wrap each component into `[0, period)`. Periods must be positive. |
| Seed | `seed("name") → S` | Derive a stable named seed from the request seed. |
| Noise | `noise(V, seed: S) / cellular(V, seed: S) → S` | Gradient noise or nearest-cell distance field in `[0, 1]`. Coordinates are in units of feature size. Two-dimensional noise depends only on `x` and `y`, so `noise(vec2(1.5), s)` equals `noise(vec2(1.5, 1.5), s)`. |
| Noise | `hash(N, seed: S) → S` | Stable value in `[0, 1)` for the lattice cell containing the coordinate (its floor). |
| Noise | `value_noise(V, seed: S) → S` | Smooth lattice value noise in `[0, 1]`, one cell per unit. Detail at or below the footprint fades toward 0.5. |
| Math | `pow(N, exponent: S) → N` / `exp(N) → N` | Componentwise power of nonnegative bases (a negative base fails) and natural exponential (arguments above 200 fail). |
| Coordinates | `triplanar_weights(normal: vec3, sharpness: S) → vec3` | `abs(normal)` raised to a positive sharpness, normalized; weights for the x, y and z projections. |
| Image | `varied_color("image", vec2, seed: S, rotation: S, correction: S) → vec3` | Rotated, offset copies of a color image blended over a triangular lattice (one cell per UV unit) with cubed barycentric weights. Copies turn by up to `rotation / 2` radians either way; `correction` in `[0, 1]` restores contrast around the image mean. Mip-filtered by the coordinate footprint; sRGB is decoded first. |
| Noise | `fbm(V, seed: S, octaves: S) → S` | Filtered gradient noise summed over 1–12 integer octaves. Each octave doubles frequency and halves weight. Returns `[0, 1]`. |
| Image | `sample_color("image", vec2) → vec3` | Periodic bilinear sample with sRGB decoded before interpolation. |
| Image | `sample_data("image", vec2) → vec3` | Same UV sampling with linear byte values. |
| Image | `sample_alpha("image", vec2) → S` | Same UV sampling of linear alpha. |
| Mapping | `runoff(width: S, length: S) → S` | Mask in `[0, 1]` from finite world-space source edges. Positive width and downhill length are in meters. |
| Receiver | `crease_distance("selector", min_angle: S, max_distance: S) → S` | Meters from `world` to the nearest selected receiver edge, at most the positive `max_distance`. Selector `convex`, `concave`, `boundary`, `exposed`, `any`, `seam` (UVs differ across the edge) or `contact` (the edge lies on another surface of a composed scene); creases count when at least `min_angle` degrees sharp. Receivers without a selected edge report `max_distance`. |
| Receiver | `crease_normal("selector", min_angle: S, max_distance: S) → vec3` | World-space unit normal halfway between the faces of the nearest selected crease within `max_distance`, ignoring boundaries and creases facing away from the sample's `normal`. Returns `normal` when there is none. |
| Receiver | `crease_tangent("selector", min_angle: S, max_distance: S) → vec3` | World-space unit direction along the nearest selected edge within `max_distance` (any selector, boundaries included). Its sign follows the stored edge order. `vec3(0)` when there is none. |
| Receiver | `crease_adjacent_normal("selector", min_angle: S, max_distance: S) → vec3` | Normal of the face across the crease `crease_normal` chooses: `normal` reflected across its middle normal. Returns `normal` when there is none. |
| Vector | `cross(a: vec3, b: vec3) → vec3` | Right-handed cross product. |
| Receiver | `adjust_normal(normal: vec3, target: vec3, weight: S) → vec3` | Turn `normal` toward `target` along the great circle; `weight` is clamped to `[0, 1]`. Opposite directions are unchanged; zero vectors fail. |

The image operations use lower-left UVs and caller-supplied RGBA8 images.
`runoff` projects gravity onto the receiver plane, measures downhill and
lateral distance from each finite edge, and takes the maximum contribution.
It widens the lateral falloff by half the sample footprint. Empty edges or
gravity with no downhill component produce zero.

### Review of existing helpers

| Family | Disposition |
| --- | --- |
| Numeric, vector, interpolation and selection | Keep the primitives. Share their semantics between frontends. Preserve lazy selection and derive mask conveniences from these operations. [Checker and signatures](../crates/engine/src/materialspec/program/check.rs) |
| Coordinates | Keep the descriptive graph labels over the shared arithmetic. Preserve wrap and footprint semantics. [Implementation](../crates/engine/src/materialspec/program/eval.rs) |
| Seed and noise | Keep gradient, cellular, and fractal operations distinct. The expression evaluator shares a lattice hash with the intrinsic recipes, but their value noise and periodic lattice use different evaluation and period rules. Preserve each pattern during the port. [Evaluator](../crates/engine/src/materialspec/program/eval.rs) · [Intrinsic noise](../crates/engine/src/materialspec/noise.rs) · [Offline periodic lattice](../crates/engine/src/materialspec/authoring/sampling.rs) |
| Image sampling | Keep the color, data, and alpha ports. The evaluator and legacy surface rasterizer both wrap UVs, use half-texel bilinear coordinates, and decode sRGB before filtering. Their wrapping expressions and validation boundaries differ. Check negative and boundary UVs, extreme admitted coordinates, decode order, alpha, and output rounding before extracting a shared kernel. [Evaluator](../crates/engine/src/materialspec/program/eval.rs) · [Surface rasterizer](../crates/engine/src/materialspec/surface.rs) |
| Runoff | Keep the author-facing helper over shared gravity projection, finite-edge coordinates, and falloff. Surface missing edges or zero downhill direction in Studio when authors inspect the effect. [Evaluator](../crates/engine/src/materialspec/program/eval.rs) |
| Example helpers | `coverage(field, threshold, softness)` is a reusable smooth threshold with positive softness. `tinted(base, detail)` uses `0.85 + 0.3 * detail`. Keep that particular contrast in the recipe, expressed directly or as a range remap. [Example module](../crates/engine/src/materialspec/program/examples/common.ms) |

## Proposed TypeScript SDK

> **Status.** A first slice is implemented: see [TypeScript material recipes](material-typescript.md)
> for the supported subset, groups, structured edits, command line and Studio. The sections below
> remain the design reference. Implemented differently from the tables: coordinate mapping is
> chosen per bake (`uv`, `object` or `world`) and read as `surface` rather than through
> `mapCoordinates`; `variedColor` and `triplanarWeights` cover varied and triplanar sampling;
> `rectangularGrid`, `periodicStrips` and `directionalCoordinates` are groups; `hash` is the
> cell random and `distanceBand` the radial profile. Not implemented: `neighborCells` (groups
> evaluate bounded neighbors themselves), `blendNormals`, `authoredMask` and `alphaCutout`.

Use valid `.material.ts` files with versioned SDK imports, supplied relative
imports, `const` declarations, pure functions, and a `defineMaterial` entry.
Rust parses and checks the documented subset into an in-memory program for both
native and Wasm. Authored JavaScript never executes. The session can cache the
checked program and reuse outputs for matching effective bake inputs. Keep the
`.ms` loader for existing projects.

Recipes define a PBR base, ordered masked layers, and direct channel expressions.
Clamp layer coverage to `[0, 1]`, pass omitted channels through, and blend
ordinary channels in linear space. Height layers explicitly add a delta or
blend toward replacement height before normal derivation. Carry receiver crease
normals as explicit inputs. The recipe owns stable parameter IDs, defaults,
ranges, and units. Graph metadata associates fields and ports with source spans
and keeps layout in the project.

The versioned operation catalog distinguishes three kinds of entries:

- **Primitives** provide shared math, coordinates, sampling, masks, and channel
  operations. Generate SDK declarations, graph ports, and the compact AI
  reference from this catalog.
- **Groups** compose primitives around an authoring task. Examples include
  chipped edges, ground staining, oxide coverage, and polished rims. Groups
  retain the names and controls authors tune.
- **Renderer operations** declare the view, lighting, material, or environment
  data they require. The live operation graph travels with the portable PBR
  material in GLB as described below.

### Static typing and validation

Material recipes must type-check with ordinary TypeScript in strict mode using
the versioned SDK declarations. Generate those declarations and Studio's port
types from the same operation catalog. Give each operation explicit input and
output types, including field value, coordinate space, and evaluation stage
where they affect compatibility. Expose named ports through typed properties so
editors can autocomplete supported operations and connections.

The SDK must reject unknown operations, misspelled ports, missing required
inputs, invalid parameter defaults by type, and incompatible connections during
type checking. For example, a scalar field can feed roughness. Sending a color
field to that port requires an explicit conversion. A world-space direction
needs a coordinate transform before use in an operation expecting object space.
Use typed helpers for conversions and for transitions between evaluation stages.
Keep the same checks for helper groups and direct output declarations. Avoid
catch-all string lookups or `any` in the public connection API.

Rust checks the supported TypeScript subset independently and validates incoming
graphs before use. It reports source-linked errors for constraints TypeScript
cannot prove, such as numeric ranges, dependency cycles, missing assets, missing
receiver data, and unavailable renderer capabilities. A passing TypeScript
check verifies the declared types. Successful build and rendering also depend
on these input and capability checks.

### Operation catalog

The tables describe proposed typed ports. Names illustrate the contracts and can
be refined during implementation. Each entry in the versioned catalog must also
carry a short example. Sample fields evaluate during a bake and may be available
during rendering if the backend implements the same sampling contract. Receiver
queries consume geometry or authored features through declared inputs. Renderer
operations consume lighting and view inputs at their declared stage.

Coordinates are finite and carry a declared space. UVs are dimensionless.
Surface coordinates describe a physical parameterization in meters. Object-space
coordinates follow the receiver and world-space coordinates support continuity
across receivers. Conversion from UV to a physical space requires a supplied
mapping. Normals and tangents specify their frame. Widths, extents, and dimensions
are positive unless a port explicitly admits zero. Required field inputs must
typecheck. Missing assets and required mappings generate diagnostics. A declared
optional feature can contribute an empty field, with its absence shown in Studio.

#### First port

| Operation and typed ports | Units, range, and filtering | Stage and required inputs |
| --- | --- | --- |
| `mapCoordinates(position, mapping) → Coordinates2` | Accepts UV, surface, object, or world coordinates with an explicit source and target space. Carries the transformed footprint or gradients. | Sample field. Requires the selected receiver coordinates and any transform needed for that mapping. |
| `variedSamples(coordinates, seed, profile) → WeightedSamples` and `sampleImage(image, samples, channelKind) → Channels` | Produces dimensionless UVs, rotations in radians, and nonnegative weights summing to one. Rotate coordinates and gradients together. Images decode according to the channel kind before filtering. | Sample fields. Require mapped coordinates and a seed profile. The sampler also requires an image. Reuse the same sample positions, rotations, and weights for correlated color, roughness, height, or mask maps. |
| `triplanarCoordinates(position, normal, repeat, sharpness) → WeightedProjections` | Three dimensionless UV projections from a physical repeat in meters. Matching-space normal and positive sharpness determine normalized weights. Project the footprint or gradients along with the coordinates. | Sample field. Requires position and normal. Apply a sampler to each projection and blend the results. |
| `correctSampleContrast(color, mean, gain) → LinearColor` | Linear RGB input and mean, finite nonnegative gain, with an explicit output clamp when required. No spatial filtering of its own. | Sample field. The recipe supplies the measured mean and correction gain. |
| `queryFeature(receiver, selector) → Optional<FeatureFrame>` | Returns distance in meters and, when available, a tangent or adjoining normals in the declared frame. A crease selector uses a dihedral threshold and convex or concave selection. Ignore coplanar triangulation edges. | Receiver query. Select a crease, rim, seam, or contact line from supported geometry or supplied features. A missing feature returns an empty result. |
| `downhillCoordinates(position, normal, gravity, sourceEdges) → EdgeCoordinates` | Projects gravity onto the receiver surface. Returns downhill and lateral distances in meters to finite source edges. Downstream falloff can use the local footprint. | Receiver field. Requires all inputs in world space. No source edge or downhill direction gives no contribution and an inspection finding. |
| `remap(value, inputRange, outputRange, mode) → Scalar`, `softThreshold(value, threshold, softness) → Mask`, and `distanceBand(distance, inner, outer) → Mask` | Require a strictly increasing input range for remap and an explicit clamp or extrapolate mode. Threshold softness is positive and uses the value's units. A distance band falls from one to zero between ordered nonnegative bounds in meters. | Sample fields. Use explicit feathering and pass a footprint when a profile needs spatial filtering. A band over an absent optional feature gives zero coverage. |
| `softUnion(a, b) → Mask` and other explicit mask combinations | Clamp inputs to `[0, 1]`. Soft union computes `1 - (1 - a) * (1 - b)`, intersection uses `a * b`, and subtraction uses `a * (1 - b)`. No extra filtering. | Sample fields. Require only their input masks. |
| `heightNormal(height, surfaceFrame) → TangentNormal` and `adjustReceiverNormal(normal, featureFrame, weight) → Normal` | Height is signed meters and derives a tangent-space +Y normal from physical derivatives. The receiver adjustment uses a unit normal, available feature directions, and a weight in `[0, 1]`. Filter the weight through its source profile. | Height derivatives use bake texel spacing or a supported live derivative path. Receiver adjustment requires feature directions in a common frame and has zero influence when an optional direction is absent. |

Evaluate ordered height layers first. A delta adds `mask * delta` and a
replacement blends the current height toward the layer value. Derive relief from
the composed height. The recipe then declares where the receiver adjustment
applies. An adjustment to the base normal changes the frame in which the relief
is applied. A post-relief adjustment takes the already shaded normal. Preserve
the concrete study's post-relief round-over and the steel study's rim adjustment
after normal mapping. A view-dependent adjustment runs in the renderer.
Geometry and collision retain their authored dimensions.

#### First expansion and follow-ups

| Operation and typed ports | Units, range, and filtering | Stage and required inputs |
| --- | --- | --- |
| `rectangularGrid(position, size, origin, rowOffset) → CellSample` | Physical 2D coordinates and positive cell dimensions in meters. A row offset is a fraction in `[0, 1)` applied to alternate rows. Returns signed integer row and column, local coordinates, and distances to sides and corners in meters. IDs use floor at cell boundaries, including negative coordinates. | Sample field. Point query independent of footprint. Align or stagger cells without changing the material model. |
| `periodicStripCoverage(position, period, gap, footprint) → Mask` | All values are meters. Require `0 ≤ gap ≤ period` and a nonnegative footprint. Integrate solid coverage over the footprint and use point coverage at zero. Evaluate each axis in its grid coordinates and retain the reference row-offset behavior. | Sample field. Requires the physical sampling footprint, including at coarse bake resolutions. |
| `cellRandom(id, seed) → Scalar`, `neighborCells(id, radius) → CellIds`, and `radialProfile(distance, inner, outer) → Mask` | Random value in `[0, 1)` independent of footprint. Neighbor radius is a bounded nonnegative integer in cells. Profile takes ordered radii in one physical space and returns a feathered mask. | Sample fields. Require a stable cell ID and seeds for variation. The authored group evaluates bounded neighbors and applies footprint-aware contours for damage across cells. The port may use the same helpers for spot fields before the tile proof. |
| `directionalCoordinates(position, direction, scale) → Coordinates2` | Explicit axis and positive feature scales in a tagged coordinate space. Carry the transformed footprint or gradients into noise or image sampling. | Sample field. Requires a UV axis or receiver tangent for detail that follows a surface. Useful for grain, weave, and brushing. |
| `blendNormals(base, detail, weight) → TangentNormal` | Follow-up. Use normalized +Y tangent-space normals in a common frame and a weight in `[0, 1]`. Decode normal texture samples as data, then compose with procedural relief and renormalize. | Sample field. Requires a valid tangent frame and the declared normal-map convention. Evaluate after deriving height relief and use the adjusted frame at shading time. |
| `authoredMask(source, mapping) → Mask` and `alphaCutout(coverage, threshold) → AlphaOutput` | Follow-up. Mask and cutoff are unitless in `[0, 1]`. Filter the mask using its image or mesh-attribute sampling contract. Cutout declares the output alpha mode and cutoff. | Mask is a sample field and requires a typed image or mesh attribute. Cutout is output metadata consumed by the renderer and exporter. Check exported alpha mode and cutoff. |

#### Renderer operations for the first port

| Operation and typed ports | Units, range, and filtering | Stage and required inputs |
| --- | --- | --- |
| `grazingFalloff(normal, viewDirection, bounds) → Mask` | Common-frame unit directions, ordered facing bounds in `[0, 1]`, and a smooth mask in `[0, 1]`. No texture sampling. | After shading normal setup. Requires view direction and the declared normal. |
| `filterEnvironment(reflectionDirection, roughness, profile) → LinearRadiance` | Unit direction, roughness in `[0, 1]`, and a bounded angular filter profile with offsets in radians. Use the renderer's environment sampling path. | Environment reflection stage. Requires a reflection direction and environment texture. |
| `bandLuminance(radiance, profile) → LinearRadiance` | Nonnegative linear radiance with a profile declaring band spacing, transition widths, and optional lift. Soft transitions are part of the profile. | Reflection response stage. Requires filtered or unfiltered reflected radiance as declared. |
| `modulateDiffuse(diffuse, factor, mask) → LinearRadiance` | Nonnegative linear diffuse input and multiplier, mask in `[0, 1]`. A field driving the multiplier uses its own filtering contract. | Lighting composition. Requires the diffuse contribution as a distinct input. Specular lighting does not consume this result. |

Report a missing or unsupported renderer input in Studio. Directional surface
detail can bake. Directional reflections need a renderer capability contract.
Additional curve types, shape families, and shading models can follow once a
reference recipe exercises them.

### Runtime bindings and GLB delivery

Recipes declare named, typed inputs with the stage that consumes them and,
where applicable, units, coordinate space, and defaults. A binding can supply
a value such as a scalar or vector, or a resource such as a texture. The host
connects recipe ports to values from Studio, gameplay, the active render pass,
or another producer. An operation that needs a renderer service declares that
capability explicitly. Updating an input used only during rendering updates
its binding without invalidating the bake or recompiling the shader.

- **Standard material.** Export every representable channel as standard glTF
  material properties and textures in the GLB. Bake portable fields where
  needed. A regular importer renders this PBR fallback.
- **MaterialSpec data.** Store the additional live behavior under
  `materials[i].extras.materialSpec`. Use one versioned, renderer-neutral
  schema for the operation graph, typed input declarations, preset values, and
  asset bindings. Use versioned operation IDs from the catalog and resolve
  static asset references against the GLB. For example, a grazing-angle
  falloff takes a normal, view direction, and profile. The pipe recipe
  chooses which data to connect and which profile to use. Exported metadata
  carries data and graph connections. The receiving runtime supplies the
  operation implementations and host bindings.
- **SuperSpec import.** ItemSpec and WorldSpec use the shared MaterialSpec
  contract to resolve the payload for their material slots and scene bindings.
  Keep the portable material usable when the payload is absent, invalid, or
  unsupported. Report invalid or unsupported data and missing required
  bindings. The export report identifies live behavior that a standard
  importer will skip, including behavior preserved in `extras`.
- **Renderer adapter.** Implement Three.js first for Studio and shared scene
  preview. It maps operation IDs to supported shading stages and binds values
  and resources from the host. A future Babylon.js or other engine adapter
  can read the same payload and declare its own capabilities.

[glTF extras](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#extras)

### Editable groups

**Edge Detection** is a receiver query. **Edge Wear**, **Runoff**, **Ground
Weathering**, **Oxide**, and **Tile and Grout** are editable groups over shared
operations. Edge Wear exposes coverage and height. Runoff exposes coverage and
downhill coordinates. Ground Weathering exposes stain and growth masks. Oxide
exposes edge, surface, and spot coverage plus the combined mask, which can drive
both channel blends and the attenuation of live effects. Tile and Grout exposes
face, joint, chip, and exposed substrate coverage. Studio puts missing-input
findings on the affected query or group and lets authors inspect fields and
reuse them at several ports. Recipes and their presets own physical scales,
colors, chip probabilities, and other appearance settings.

### Study audit and parity checks

| Existing study logic | Share underneath | Preserve in the study |
| --- | --- | --- |
| Concrete/plaster varied sampling, oxide texture sampling, and the offline concrete baker | Triangular lattice weights and transformed sampling. Compare actual neighbor ordering on both sides of the triangle boundary in all three paths before consolidating. Triplanar projection can compose the sampler. | Concrete and plaster keep limited rotation, source means, and contrast correction. Oxide keeps full rotation, its hash and offsets, triplanar weights, and oxide tint. |
| Concrete chipped seams and creases, metal edge rust and polished rims | Feature-distance fields and a distance-to-mask profile. Concrete already shares its chipped shoulder between seams and exposed edges. | Keep concrete's 14 mm crease treatment, chip controls, and separate height and normal adjustments. Keep pipe and cage rim geometry, width limits, and attenuation under rust. |
| Top grime, ground stain, moss, and rust coverage | Directional coordinates, range remaps, and soft-mask composition. | Keep cap and ground-contact inputs, placement and falloff, and independent masks for inspection. Retain separate edge, surface, and raindrop rust controls. |
| Concrete form ties and raindrop rust | Distance from a cell center and softened radial profiles. | Form ties keep their grid, face margins, and recess. Rust keeps the tilted lattice, 3D jitter, probability, and grain. |
| Steel reflection and lighting controls | Separate environment filtering, luminance banding, view-facing falloff, and diffuse modulation. | Keep banded gloss, painted values, silhouette highlight, and polished rims independently controllable. Keep the rust fades and apply clean-metal grain only to diffuse lighting. |

[Concrete shader](../crates/wasm/src/study_shaders/concrete_sampling.glsl) ·
[Offline concrete baker](../crates/engine/src/materialspec/authoring/concrete.rs) ·
[Steel sampling and spot fields](../crates/wasm/src/study_shaders/metal_fragment.glsl) ·
[Oxide composition](../crates/wasm/src/study_shaders/metal_oxide.glsl) ·
[Reflection filter](../crates/wasm/src/study_shaders/metal_reflection.glsl) ·
[Concrete normal composition](../crates/wasm/src/study_shaders/concrete_normal.glsl) ·
[Steel normal composition](../crates/wasm/src/study_shaders/metal_normal.glsl) ·
[Diffuse modulation](../crates/wasm/src/study_shaders/metal_output.glsl)

### Procedural tile proof

Use the same Tile and Grout group for clean square tile, staggered rectangular
tile, and the existing worn ceramic. Derive joint and chip coverage from the
grid and keep exposed bedding identifiable where tiles are absent or damaged.
Preserve stable per-cell variation, isolated missing tiles, and irregular damage
clusters. Keep thin joints visible at coarse bake resolutions and derive bevel
relief through the height channel. Apply the restaurant's low-wall staining as a
separate weathering layer. Follow with brick and planks to exercise grid and joint
reuse, brushed metal for directional fields, and cutout foliage for masks and
export.

[Kowloon ceramic field](../crates/engine/src/worldspec/themes/kowloon/tile.rs) ·
[Restaurant tile bake](../crates/engine/src/worldspec/themes/kowloon/authoring/intrinsic.rs) ·
[Ceramic reference tests](../crates/engine/tests/tile.rs)

### Checks for the port

- Validate shared operations against their original callers before replacing
  them. Cover negative and seam UVs and both triangle orientations. Check that
  coplanar triangulation edges receive no crease wear, and check flat, sloped,
  and missing-edge receivers for runoff.
- Derive the SDK declarations, graph ports, and AI reference from one catalog.
  In Studio, reconnect one use of a shared mask and check that only the intended
  source expression changes. An invalid edit must retain the previous preview.
- Compare against frozen LevelSpec views and intermediate masks, roughness, and
  normals before claiming visual parity. Record deliberate appearance changes
  separately for review.
- Compare tile face, joint, chip, and exposed substrate fields and baked color,
  normals, and roughness at close and coarse resolutions. Anchor the worn
  ceramic case to the existing field and raster fixtures. Check that a material
  change preserves receiver geometry and collision.
