# Shared value contracts

The existing source/provenance and PBR records move unchanged into a small
`superspec-types` crate. Core reexports the same Rust types at their current paths.
The new crate depends only on Serde; it owns no XML reader, catalog, geometry
kernel, generator, renderer or filesystem adapter.

This boundary prevents two unwanted dependencies during model extraction: a
model/export consumer should not need the AML parser, and the AML parser should
not acquire the model's geometry/navigation dependencies merely to share a vector,
material or source location. The model and parser are already distinct consumers.

The moved records are `Node`, `Value`, `V3`, source diagnostics and `Material`,
including their existing typed-value accessors and validation helpers. `Node`
retains the normalized source vocabulary used in current delivery metadata; this
move does not introduce a new source format. Raw source admission remains in core.
Keep field order, Serde attributes, defaults, UTF-16 locations and `Material::read`
unchanged. Existing caller paths remain exact reexports, with no conversion or
duplicate data storage.

Verification uses existing source/material checks and frozen complete scene and
export bytes. A compiler-free model/export consumer tests the new architectural
boundary; getter-by-getter tests would only mirror the moved implementation.
