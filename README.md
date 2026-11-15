# SuperSpec

SuperSpec owns the AML engine, procedural generation, material assets, export
adapters and shared browser preview. Native Rust and browser Wasm use the same
compiler. The sibling [LevelSpec](https://github.com/tfukaza/levelspec) repository
owns authored maps, studies, demos and end-to-end sample acceptance.

SuperSpec uses short crate directories with namespaced `superspec-*` packages.
Its native commands are `superspec`, `superspec-studio` and `superspec-materials`;
LevelSpec consumes these through its pinned harness launcher. The implementation
was extracted from LevelSpec's Rust PoC at
`fb5df8cbf1a3c8b5a8a75a66bf976678cf2106ff`.
Static AML extension registration and source checks are available.
See the [consolidation decisions](docs/plans/superspec-consolidation.md) and
[reorganization status](docs/plans/reorganization-status.md)
for implemented ownership boundaries. MaterialSpec and ItemSpec now have dedicated
editors, versioned authoring languages, saved projects and native export. See the
[authoring guide](docs/AUTHORING.md) and [acceptance inventory](docs/plans/authoring-implementation.md).
Deferred legacy parity remains separate work.
See the [product plan](docs/plans/spec-modularization.md) and
[extraction decisions](docs/plans/extraction-decisions.md).

## Build and check

Rust 1.100 is pinned. Node 24 is used for the browser tooling.

```sh
cargo test --workspace --locked
cargo build -p superspec-cli --release --locked
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
cargo build -p superspec-wasm --target wasm32-unknown-unknown --release --locked
wasm-bindgen --target web --out-dir generated/wasm target/wasm32-unknown-unknown/release/superspec_wasm.wasm
npm ci
npm run check
npm run check:architecture
npm test
npm run build:viewer
```

`tools/rust.ps1` also provides native and Wasm build tasks. `RUST_CLI` selects an
alternate built CLI for transport/browser tests. `tools/package-rust.ps1` creates
a portable release with `superspec`, `superspec-studio`, `superspec-materials`,
`materials/`, and `reviewer/` together.
The CLI development fallback resolves assets inside this repository, never inside
LevelSpec. Export and Blender adapters retain their optional external prerequisites.

Run `npm run dev` to open the editor. **Open AML and companions** accepts a
standalone file or an AML file and its companions selected together;
**Open AML folder** loads its relative companions. A folder with several AML
entries needs a unique `scene.aml`. The last valid scene stays visible on errors.
LevelSpec's `demos/courtyard/scene.aml` is a complete example with companions.

The **Material** and **Item** links open `/material.html` and `/item.html`.
Both editors save reusable source projects and export through the shared Rust
engine. `superspec-studio check|scene|glb` consumes those projects natively
(and `inspect|edit|sdk|catalog` expose TypeScript recipe tooling to agents);
WorldSpec can place their saved items and bind their saved materials.

## Ownership and distribution

- `crates/types`: portable source, diagnostic, material, color and seeded-random utilities.
- `crates/model`: shared scene, delivery metadata, visual admission and packets.
- `crates/aml`: typed AML admission, extension registry and declaration adapters.
- `crates/profiles`: shipped source-profile composition and Kowloon AML decoding.
- `crates/geometry`: portable geometry and image kernels.
- `crates/engine`: physical compilation and session orchestration, with
  `materialspec` for material operations, `itemspec` for local mesh operations and
  imports, and `worldspec` for build coordination, layouts and Kowloon content.
  Its `assets` and `runtime` modules own source admission, artifact storage and
  producer-supplied job execution. The generic session retains producer state
  supplied by adapters; `worldspec::shipped` selects the default source profile
  and theme. The world coordinator consumes a typed theme adapter, with Kowloon
  receiving-surface adaptation isolated in `architecture::kowloon`.
- `crates/export`: target preparation and serialization of the shared
  model; its normal dependency graph contains neither the compiler nor AML parser.
- `crates/cli`, `crates/wasm`: native/browser host boundaries.
- `crates/catalog`: shipped material definitions and image identities;
  the AML crate retains their declaration adapter.
- `assets/materials`: catalog image bytes, supplied by native/browser hosts.
- `browser`: shared model transport, renderer, worker runtime and navigation.
- `editor`: AML, material and item editors, plus native live-preview entry points.
- `tools/browser.mjs`: shared browser-automation launcher for engine checks and
  LevelSpec capture/planning scripts; consumers do not import test internals.
- `tools/architecture`: Rust module and Cargo dependency checks, run locally and in CI.
- `schema`, `tests`: specification and focused native/Wasm/browser contracts.

`superspec.manifest.json` identifies the source distribution by content. Run
`node tools/source-manifest.mjs write` after reviewed source changes, and update
LevelSpec's `superspec.lock.json` to adopt them. `verify` rejects drift. This
permits a pinned local distribution before a release or Git revision is published.
Generated outputs are not source identity; regenerate them from the pinned source.

Hosts can compose a source profile with
`superspec_profiles::default_registry().with_extensions(...)` and use
`superspec_engine::compile_with_registry(...)`. The ordinary entry points retain
the Kowloon profile. Core `parse` and `serialize` use only generic AML vocabulary;
`superspec_profiles::parse` and `serialize` select the shipped profile explicitly.
See [AML registration](docs/plans/aml-registry.md) for source
preparation guarantees and the distinction from physical and navigation checks.

New Rust consumers can use `superspec-model` and `superspec-types` directly.
Material operations use `materialspec`, item imports use `itemspec::imports`, and
layout producers use `worldspec::layouts`. Kowloon-specific APIs live under
`worldspec::themes::kowloon`; the former root `dressing`, `imports` and `generation`
forwarding modules have been removed. Generic material and item operations have no
dependency on Kowloon. Compilation callers use the root `superspec_engine::compile*`
functions; the former `architecture::compile*` paths are retired.
See the [theme runtime boundary](docs/plans/theme-runtime-boundary.md),
[runtime ownership decisions](docs/plans/runtime-modules.md),
[shared model](docs/plans/shared-model.md) and [item geometry](docs/plans/item-geometry.md).
