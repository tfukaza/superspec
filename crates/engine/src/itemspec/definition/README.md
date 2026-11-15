# ItemSpec version 1

This module owns persistent `.item.json` definitions, procedural local geometry,
and static GLB scene ingestion. It uses caller-supplied bytes and never reads files
or the network. Native and Wasm callers use the same implementation.

`parse(source_name, text)` reads the definition. `prepare` resolves controls,
material requests and asset identities. `PreparedItem::build` creates immutable
local geometry; `PreparedItem::reuse` accepts an existing artifact with the same
geometry key and renews material bindings, attachments, collision and provenance.
Geometry identity includes effective shape inputs, the item transform, and exact
GLB dependency bytes. It excludes finish controls, placement identity and bindings.
Material requests use MaterialSpec module entries and parameter values; a value
`{"parameter":"roughness"}` binds an item control. Geometry cannot read finish controls.

```json
{
  "format": "itemspec",
  "version": 1,
  "id": "pipe",
  "parameters": {
    "length": {"type":"number", "role":"geometry", "default":2, "min":0.01, "max":100, "unit":"m"},
    "roughness": {"type":"number", "role":"finish", "default":0.45, "min":0, "max":1}
  },
  "generator": {"kind":"pipe", "length":{"parameter":"length"}, "radius":0.1, "wallThickness":0.01, "segments":16, "capped":true},
  "slots": {"surface":{"entry":"materials/metal.ms", "parameters":{"roughness":{"parameter":"roughness"}}, "seed":0}},
  "collision": "mesh"
}
```

Controls support number, integer, boolean, color and choice. Color controls accept
six-digit sRGB hex or linear RGB arrays; hex values become linear RGB when bound
to a material parameter. Pipe and duct length
is local Z; duct width is X and height is Y. Wall thickness zero gives a solid
section. Capping is optional. Generated start/end attachments face away from the
item. Authored attachments have unique names, positions (numeric values or geometry
control references) and direction vectors. The optional transform is four affine
columns: three basis vectors and translation, in right-handed Z-up meters.

Collision is `"none"`, `"bounds"`, `"mesh"`, or a box/sphere object with `kind`,
`center` and `size`/`radius`. Explicit shapes use definition coordinates and the
item transform. Bounds, local node transforms and attachments reflect that transform.

Use `{"kind":"glb","asset":"models/item.glb"}` with optional scene index for
imports. Start with empty slots to retain imported PBR materials. Returned slot
names are `material/0`, `material/1`, etc.; missing primitive material uses
`material/default`. A named assignment replaces that slot only. Local primitives
are shared across hierarchy nodes. The importer converts glTF Y-up positions,
normals and node bases to Z-up, retaining UV values, color alpha, tangent handedness,
additional UV sets, exact image bytes and PBR factors. Flattening applies inverse
transpose normals and corrects winding under reflection. Missing normals produce
flat corners and discard authored tangents, as required by the [glTF specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html).
`delivery_tangents` retains authored normal-map bases or derives them from original
UVs before the delivery adapter flips texture coordinates.

The bounded static importer supports triangles, strips, fans, interleaved and sparse
accessors, embedded or supplied relative resources, PNG/JPEG images and core PBR
textures. Animation, skins, morphs, compression and unrepresented vertex/node
extensions produce explicit errors. Material extension and sampler state remain
on the imported records; `ensure_scene_compatible` checks the narrower current
scene renderer contract (UV0, identity texture transform, repeat/linear mipmapped
sampling, supported PBR channels). An unsupported renderer feature must fail at
that boundary rather than disappear from the imported artifact.

Limits include 1 MiB definition text, 64 controls, 128 slots, 256 attachments,
32 MiB main GLB, 8 MiB GLB JSON, 64 MiB aggregate source/image bytes, 16 MiB per
encoded image, 96 Mi pixels, 4096 nodes/primitives, 500000 local vertices and
250000 source/placed triangles and 1000000 placed vertices. Images are validated before publication. Cancellation
and resolver failures return errors; callers publish completed artifacts only.
`retained_bytes` charges all retained attribute capacities, hierarchy and raw PBR
metadata, images, bindings and provenance. Shared allocations are charged fully
per owner so cache admission remains conservative on native and Wasm targets.

Diagnostics carry source identity and JSON pointers. `parse` locates its errors;
call `locate_diagnostic` on later definition errors with the matching source text
to obtain UTF-16 offsets and one-based line/column. GLB errors name the GLB asset
and JSON pointer instead of falsely pointing into the item definition.
