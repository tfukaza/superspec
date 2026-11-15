# basisu_c_sys (vendored)

- Source: crates.io `basisu_c_sys` 0.7.1, <https://github.com/beicause/bevy_basisu_loader>,
  licensed MIT OR Apache-2.0. It bundles Binomial's Basis Universal (`vendor/basis_universal`,
  Apache-2.0, see its `LICENSE`) and a prebuilt Emscripten encoder (`wasm/basisu_encoder.*`,
  built by that project's CI from the same sources).
- Used by `superspec-geometry` to encode and transcode KTX2 (`KHR_texture_basisu`) textures,
  natively through the compiled C++ and on `wasm32-unknown-unknown` through the embedded encoder.

## Changes from upstream

- `bindings/` holds the files the upstream build script generates with `bindgen` and `syn`
  (produced once with libclang 18 from the headers in `vendor/`). `build.rs` copies them into
  `OUT_DIR` instead, so building needs a C++17 compiler on native targets and nothing extra for
  wasm32. Regenerate them whenever the vendored headers change.
- Removed the optional high-level `extra` API (it depends on `wgpu-types`), the
  `__gen_make_wasm` tool, the transcoder-only wasm and the build dependencies `bindgen`, `syn`,
  `prettyplease` and `clap`.
- Added `src/safe.rs` (declared in `src/lib.rs`): a safe encode/transcode API, since the
  SuperSpec workspace forbids `unsafe` code. Otherwise `src/`, `vendor/` and `wasm/` are
  unmodified apart from removing the `extra` module declaration from `src/lib.rs`.
- `build.rs` defines `NDEBUG` for the C/C++ sources (as upstream release builds do), so
  malformed KTX2 input is rejected by the codec's checks rather than aborting on an assert.

## Known upstream issue

`bt_ktx2_create_transcode_state` value-initializes `ktx2_transcoder_state`, leaving its
decompressed-level index at 0 instead of -1, so transcoding level 0 of a Zstandard-supercompressed
file with such a state reads an empty buffer. `safe::transcode_rgba32` passes state 0 (the
transcoder's own, correctly cleared state) instead.
