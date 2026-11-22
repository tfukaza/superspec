# SuperSpec

SuperSpec reads AML XML and builds scenes. It owns physical lowering, procedural
generation, material assets, export adapters and shared browser preview. Native
Rust and browser Wasm use the same AML reader and scene compiler. The sibling
[LevelSpec](https://github.com/tfukaza/levelspec) repository owns AML authoring,
authored maps, studies, demos and end-to-end sample acceptance. AML XML is the
handoff whether the file comes from LevelSpec, an editor, or a person.

The runtime handoff follows **LevelSpec → SuperSpec → game**. The Stoa authoring
project records its layout and semantic material roles locally. The Kowloon theme
resolves those roles and owns the appearance recipes. Its publisher in
`tools/kowloon-export/` consumes the supplied AML and companion assets and writes
the scene, textures, collision data and optional runtime records.

The standard glTF materials and meshes provide the portable appearance.
`worldSpecEnvironment`, `worldSpecLighting` and `worldSpecRender` are optional scene extras. Version 1
of the render record lists named effects. The Three.js package in
`adapters/three/` binds supported effects and material records to the loaded
scene. Klali installs the packaged adapter, supplies its renderer and camera,
and owns gameplay navigation, match logic and game capture tools.
The publisher can capture reflection probes directly from an exported GLB.
WorldSpec carries geometry and material alternatives from their source owners to
the viewer. See the [LOD authoring and export contract](docs/LOD.md) for item,
tree and material declarations, the core glTF fallback and runtime selection.

SuperSpec uses short crate directories with namespaced `superspec-*` packages.
Its native commands are `superspec`, `superspec-studio`, `superspec-materials` and `superspec-gi`;
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

## Native authoring

With Node 24 and the Rust toolchain installed, run `npm ci` and `npm run dev:native`.
Open the loopback address printed by `superspec-studio`. Material and Item open their
respective editors, World opens the AML editor, and the landing page accepts saved
projects, compiled packets and standalone `.tree.json` sources. For subsequent
runs, reuse the built assets with `superspec-studio serve --ui generated/native-viewer`.
Package this configuration with `tools/package-rust.ps1 -NativeOnly`.
To inspect a published map with external buffers and textures, pass `--artifacts PATH_TO_MAPS`
and open `/native.html?gltf=/artifacts/stoacity.glb` at the printed address.
Use **Graphics** or **Graphics options** in the published viewer and the WorldSpec,
MaterialSpec, ItemSpec, and TreeSpec previews. [Graphics settings and profiles](docs/graphics-settings.md)

### Build on one host

Use Windows PowerShell for a Windows checkout and Windows builds, including the
Kowloon GPU bake. Keep AML, source assets, caches and generated outputs on
Windows storage. Use Windows Node, Python and Rust, and use native
`superspec.exe`, `superspec-studio.exe` and `superspec-gi.exe` from the same
build. The GPU bake can then use ANGLE on Windows. If Cargo uses a custom target
directory, pass the compiler's absolute path to `compile.py` and set
`STUDIO_CLI` and `SUPERSPEC_GI` to the absolute paths of the two helpers.

For a Linux build in WSL, put the checkout, inputs and outputs inside the
distro's filesystem and use Linux versions of the tools throughout.
[Microsoft's WSL storage guidance](https://learn.microsoft.com/en-us/windows/wsl/filesystems)
describes the cost of accessing files across the filesystem boundary.
Point `--artifacts` at the staged pack directory to review a WorldSpec map before
copying it into Klali. The preview uses the shared game lighting and finishing
passes. **Painted lighting** compares the painted sun-shadow and fixture
projector effects from the current view. Set `paintedshadows=on` or
`paintedspots=on` in the URL to inspect either effect on its own.
The viewer build requires no compiler Wasm. The Basis transcoder decodes KTX2 textures.
For offline GPU bakes, install the native Node bindings with `npm ci --prefix tools/headless`,
build the shared shaders with `npm run build:headless`, and build `superspec-studio` in release mode.
LevelSpec's Stoa building and arena exporters use these tools directly. Set `STUDIO_CLI`
when the executable is outside `target/release`. See the [pipeline and comparison notes](docs/plans/native-pipelines.md) for timings and visual checks.

With the native viewer and release Studio built, `npm run bench:world-edits`
replays building moves, footprint changes and a rename. It reports the first
visible response, drawn frame rate during each drag and time to the settled scene.
Use `npm run bench:world-edits -- --millimeter` to measure a 1 mm resize and see
how many stock material texels were reused or newly drawn. Add
`--trace-maps generated/stoa-weathering.json` for a separate diagnostic run
that reads the maps before and after the edit and lists the changes for each surface.

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
a portable release with `superspec`, `superspec-studio`, `superspec-materials`, `superspec-gi`,
`materials/`, `native-viewer/`, and `reviewer/` together.
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
The AML editor's [city vocabulary](docs/AML.md#city-layouts) places buildings, road and sidewalk segments, trees and lamps.

## Built-in demos

Choose **Open Kowloon bench demo** in the Level editor to load the
[Kowloon Bench Demo AML](editor/demos/kowloon-bench.aml). It uses the shipped
Kowloon casements and services alongside a tiled concrete shell and timber bench.
The scene needs no companion files. A future theme can ship its own AML with
the same layout and its own materials and fixtures.

Run `node tools/kowloon-export/publish-bench-demo.mjs` to publish the demo with
bench-scale lighting, the Kowloon duct finish, baked diffuse bounce and local reflections.
The publisher uses `superspec` and `superspec-gi` on PATH, or the paths in
`RUST_CLI` and `SUPERSPEC_GI`. For the duct bake, use the headless build described
above and set `STUDIO_CLI` if Studio is outside the release target directory.
Full publication also needs Playwright Chromium. Serve `generated/demos` with Studio's `--artifacts` option
and open `/native.html?gltf=/artifacts/kowloon-bench.glb&camera=5.4,6.2,2.45&target=1.35,1,1.1`.
Append `&gi=off` or `&reflectionprobes=off` to compare either effect in the same view.
Use `--diffuse-only` to publish `kowloon-bench-diffuse.glb` without launching a
browser for the reflection capture.

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
