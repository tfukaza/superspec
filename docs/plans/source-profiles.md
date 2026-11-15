# AML source profile ownership

The generic AML crate owns XML admission, the core vocabulary, scoped extension
registration, source locations and source checks. It does not choose a visual
theme. Its `architecture::parse` and `format::serialize` use the empty registry;
callers that need extensions pass an explicit registry.

`superspec-profiles` owns the shipped source profile. Its `kowloon` module contains
the existing decoder, tags and source check without changes to their semantics.
The crate composes `default_registry()` and provides profile-aware `parse` and
`serialize` convenience functions. It depends on AML core, while the compiler and
native host depend on profiles. Core does not depend on the compiler or profiles.

The compiler's ordinary entry points, source asset discovery and sessions select
this profile explicitly. `compile_with_registry` continues to accept caller-owned
profiles. Native source workflows select the same profile; the Wasm host consumes
the compiler's shared entry points. Existing Kowloon sources, omitted theme
defaults, diagnostics, geometry and serialized scene packets remain unchanged.

Core tests retain generic-only parsing, custom extension/check admission, source
errors and profile isolation. Kowloon defaults, theme errors and profile-aware
round trips move beside the decoder. Compiler tests compare default and explicit
profiles through full scene packets and session/asset discovery entry points.
Source inspection remains vocabulary-independent and does not imply theme
admission. This is an ownership change, not a new dynamic plugin mechanism.
