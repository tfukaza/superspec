# Architecture boundaries

Run `npm run check:architecture` or
`cargo run --locked -p superspec-architecture-check`. The native CI job runs this
check and its parser fixtures. The tool is development tooling, not a shipped
engine component.

`src/rules.rs` records the architectural policy:

- Portable types, geometry, catalog and model crates have a narrow normal Cargo
  dependency graph. Exporter consumes those layers without the engine or AML.
  Profiles explicitly depend on AML; generic AML does not select a profile.
- Engine material, item, asset and runtime kernels cannot import source parsing,
  theme recipes or physical scene assembly. Neutral data reexports resolve to
  their actual owners.
- Session owns neutral resources and artifacts. `worldspec::shipped` supplies the
  shipped profile and theme. Generic physical lowering does not select that
  composition; `architecture::kowloon` is its explicit receiving-surface adapter.
- World build order uses a supplied theme interface and the typed
  `architecture::build` boundary. Generic visibility has no concrete theme edge.

Cargo metadata supplies actual package identities, including renamed dependencies,
optional normal dependencies and target-specific dependencies. Dev dependencies
are deliberately excluded. Rust `syn` parsing checks production library modules
under `crates`; integration and acceptance harnesses may exercise several layers.

The resolver follows nested imports, aliases, type aliases, reexports, glob
imports, `crate`/`self`/`super`, inline modules and literal external `#[path]`
modules. A relocated file keeps its physical owner. Macro token paths are checked;
comments and string contents are not code dependencies. Test-only modules are
excluded. Feature-gated production modules are checked regardless of host target.

This is a source ownership check, not Rust compiler name resolution or a call
graph. It does not infer dependencies behind dynamic dispatch or method bodies
in an unrestricted facade. New facades still need code review. Macro-generated
modules/imports, item-generating `include!`, computed include paths, inline path
attributes and conditional module path attributes produce actionable errors
instead of silently skipping ownership. Literal expression includes are parsed.
If a new language form is needed, extend the resolver and its fixtures together.
