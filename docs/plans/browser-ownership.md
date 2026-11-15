# Shared browser ownership

The existing browser code is shared infrastructure, not part of the AML editor.
Move it from the undifferentiated `review/rust` directory into these owners:

- `browser/model`: packet decoding, delta application and typed validation records.
- `browser/renderer`: Three.js scenes, textures, view state and camera interaction.
- `browser/runtime`: compiler worker lifecycle, Wasm initialization and asset loading.
- `browser/navigation`: the existing movement/collision implementation and worker.

The editor and live preview import these owners directly. Renderer and model code
do not import editor state. Domain and transport behavior, worker messages, source
loading, cancellation and GPU resource lifetimes remain unchanged. No render or
generation optimization is part of this move.

Update maintained imports, worker URLs and analysis entry points together. Preserve
the generated worker names and packaged preview routes. Typechecking, viewer build,
packet/update tests and browser acceptance verify the change; no duplicate renderer
or compatibility implementation is retained.
