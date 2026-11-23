# Native exports

`superspec build` validates AML and publishes GLB, glTF, collision geometry,
runtime JSON, source-linked findings and a game manifest. Domain data uses
right-handed Z-up metres. glTF and the game manifest use Y-up metres.

```sh
superspec build projects/my-level/level.aml --out generated/my-level --json
superspec build projects/my-level/level.aml --out generated/portable --portable --json
superspec build projects/my-level/level.aml --out generated/fbx --blender /path/to/blender --json
superspec game-export projects/my-level/level.aml --out generated/game --json
```

Default artifacts include `scene.glb`, `scene.gltf`, `scene.bin`, `collision.glb`,
`scene.lsp`, `scene.json`, `scene.assets`, `manifest.json` and `game.json`.
`scene.json` references the sibling encoded image blob. `game.json` identifies
visual and collision files and supplies transformed markers and player dimensions.

For a live build directory, open `game.json` once and resolve its `visual`,
`collision` and `artifacts` paths relative to that file. The CLI publishes each
complete generation under `superspec-generations/<sha256>/`, then atomically
replaces `game.json`. A reader can finish loading a generation across the next
build. The CLI keeps older generations and reports all current generation files
and flat aliases in its JSON artifact list. The flat filenames remain available
for tools that consume completed builds. Opening independent flat aliases during
a build can span two generations.

UV1 generation runs by default. `--no-uv` skips unwrapping. The portable unwrapper
preserves geometry ownership and existing material UVs, duplicates seam vertices
and packs one shared lightmap page. Oversized or unsupported input fails explicitly.

## Portable glTF

Pass `--portable` to `superspec glb` or `superspec build` when the importer
expects ordinary glTF mesh nodes and PNG or JPEG textures. Repeated placements
reference shared mesh data through ordinary nodes. An importer can apply its
own batching and LOD policy. The output uses standard PBR material channels,
with SuperSpec's additional lighting and render records available as optional data.

## Texture formats

GLB exports store textures as KTX2 by default: Basis Universal payloads with a full mip chain,
declared by the required `KHR_texture_basisu` extension. Color maps (base color, emissive) use
ETC1S in sRGB; data maps (normals, metallic-roughness, occlusion and MaterialSpec live channels)
use UASTC with Zstandard supercompression. KTX2 is several times smaller on disk and stays
compressed in GPU memory; the 8 x 3.2 m Kowloon study scene at 2048 px is 16.3 MB with KTX2 and
30.0 MB with PNG.

- `superspec glb` and `superspec build` take `--textures ktx2|png`; `build --blender` defaults to
  PNG, because Blender cannot import KTX2. `superspec-studio glb` takes the same option.
- Material Studio and the AML editor have a **KTX2 textures** checkbox beside **Download GLB**,
  checked by default. The browser loads the encoder (a separate 2 MB module) on the first KTX2
  export or GLB import only.
- Browser Wasm has no threads, so browser KTX2 exports run in three steps: the export worker
  bakes and stages the scene (`stage_project` / `stage_glb`), up to four short-lived workers
  (`browser/runtime/texture-encoder.ts`) encode the listed textures with `encodeTexture`, and
  `finish_project` / `finish_glb` write the GLB. The status line counts encoded textures.
  Native exports encode on up to 8 threads inside `glb`.
- Encoding is deterministic, and native and browser exports are byte-identical
  (`tests/material-ktx2-browser.mjs` with `NATIVE_STUDIO`); KTX2 adds encoding time (about 36 s
  for the 24 maps of the study scene natively, on 8 threads).
- Loaders need a KTX2 transcoder. Use `createRenderGltfLoader(renderer, { transcoderPath: "basis/" })`
  from `@superspec/three`. Serve Three's `basis_transcoder.js` and `.wasm` from that directory.
  The SuperSpec Vite build copies both files there.
  LevelSpec viewers use their own `src/viewer/gltf-loader.mjs`, which imports the transcoder as
  Vite asset URLs; other viewers built on a plain `GLTFLoader` need one of these before they can
  load default exports, or export with `--textures png`.
- Imported KTX2 GLBs keep their compressed textures: Studio uploads them as compressed GPU
  textures, a KTX2 re-export passes them through and a PNG export transcodes them.

The codec is the vendored `third_party/basisu_c_sys` (see its `PROVENANCE.md`): native builds need
a C++17 compiler; no other tools are required.

## Authored levels of detail

Geometry alternatives use `superspec_types::lod::GeometryLodGroup` in the shared
model and delivered `geometryLods` manifest record. Each level references actual
mesh placements or instance batches and carries its starting distance in metres.
The first level starts at zero. Missing, hidden, duplicated or overlapping
references fail validation; these records do not invent geometry or imply culling.

GLB export writes optional `MSFT_lod` node and material chains. Only near nodes
are reachable through the ordinary scene graph, and ordinary mesh primitives
retain their near material binding. Lower geometry nodes and material texture
companions stay in the file for supporting consumers. Node extras carry
`worldSpecLod: {version: 1, distances: [0, ...]}`; authored coverage thresholds
use `MSFT_screencoverage`. Additional render levels never add collision, even if
an input accidentally marks them collidable. Native and Wasm call the same model
and exporter code.

The Kowloon packer preserves declared native GLB graphs through its mesh merging
and material finishing stages. Each placement receives its own node/LOD graph
while sharing exact, content-addressed geometry and image bytes. This preservation
path accepts static, self-contained SuperSpec GLBs with PNG/JPEG textures, node
instancing, emissive strength and authored LODs. It rejects unsupported extensions,
animation, skins, morph targets and compressed-only images explicitly; use
`--textures png` for these city inputs. Authored prototypes keep their supplied
materials instead of receiving the packer's optional finish substitutions.
GI reads only the default scene, and automatic city index streams leave complete
authored chains alone. The illustrated street-tree pack keeps its existing bark
level 1 and painted canopy near, then uses authored bark level 2 at its recorded
distance; the portable painted canopy currently has one level.

## Size limits

Shared allowances (`superspec_types::limits`, mirrored in `browser/model/limits.ts`): 512 MiB per
model or companion file, 64 MiB per encoded image, 1 GiB of companions per project, 1 GiB of
images per scene and 2 GiB per exported GLB. KTX2 images are validated structurally rather than
decoded. JSON projects embed companions as base64 in one string, which JavaScript limits to about
512 M characters, so Studio saves projects with more than 64 MiB of companions as `.studio.zip`
archives (see `docs/AUTHORING.md`).

## Text formats

`superspec obj SOURCE.aml --out DIRECTORY --json` writes `scene.obj`, `scene.mtl`
and referenced diffuse textures. Positions remain Z-up metres. Unsupported PBR
channels produce an explicit error so materials can be delivered through glTF.

`superspec plan SOURCE.aml --out PLAN.svg --json` writes an architectural SVG
with floor loops, holes, water and wall caps. Both encoders also build for Wasm.
## Brush delivery

```sh
superspec build projects/my-level/level.aml --out generated/quake --quake-palette palette.lmp --qbsp /path/to/qbsp --vis /path/to/vis --light /path/to/light --json
```

The palette contains exactly 768 RGB bytes. The exporter retains closed source
solids, splits convex pieces and face-material regions, writes MAP/WAD inputs and
runs the configured Quake tools. The resulting file uses BSP2 and 32 Quake units
per metre. Tool logs and warnings accompany the output. Open surfaces and material
channels unsupported by the brush format produce explicit errors.

The executable flags also accept `QBSP_PATH`, `VIS_PATH` and `LIGHT_PATH` defaults.
FBX uses an external Blender installation. Native compilation and portable exports
run without Node or Blender.

## Lighting

```sh
superspec bake projects/my-level/level.aml --out generated/lighting --blender /path/to/blender --samples 64 --resolution 1024 --json
```

Rust prepares the validated scene, UV charts, material images and input hashes.
The external Blender adapter calibrates Cycles and bakes linear EXR transport.
Outputs include a `.blend` project, raw passes, padded lightmaps, mip data, a
lighting manifest and a matching GLB. `--resolution` accepts powers of two from
128 through 4096. `BLENDER_PATH` can select the executable.

Build publication preserves earlier managed artifacts when compilation or an
external tool fails. Finite JSON commands write one result to stdout and progress
to stderr. Review the reported validation and tool warnings for each delivery.
