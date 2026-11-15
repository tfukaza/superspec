# Native exports

`superspec build` validates AML and publishes GLB, glTF, collision geometry,
runtime JSON, source-linked findings and a game manifest. Domain data uses
right-handed Z-up metres. glTF and the game manifest use Y-up metres.

```sh
superspec build projects/my-level/level.aml --out generated/my-level --json
superspec build projects/my-level/level.aml --out generated/fbx --blender /path/to/blender --json
superspec game-export projects/my-level/level.aml --out generated/game --json
```

Default artifacts include `scene.glb`, `scene.gltf`, `scene.bin`, `collision.glb`,
`scene.lsp`, `scene.json`, `scene.assets`, `manifest.json` and `game.json`.
`scene.json` references the sibling encoded image blob. `game.json` identifies
visual and collision files and supplies transformed markers and player dimensions.

UV1 generation runs by default. `--no-uv` skips unwrapping. The portable unwrapper
preserves geometry ownership and existing material UVs, duplicates seam vertices
and packs one shared lightmap page. Oversized or unsupported input fails explicitly.

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
- Encoding is deterministic, and native and browser exports are byte-identical
  (`tests/material-ktx2-browser.mjs` with `NATIVE_STUDIO`); KTX2 adds encoding time (about 36 s
  for the 24 maps of the study scene natively, on 8 threads).
- Loaders need a KTX2 transcoder. With Three.js use `createGltfLoader(renderer)` from
  `browser/renderer/ktx2.ts`, which serves Three's `basis_transcoder.js` and `.wasm` from `basis/`
  beside the page (`setTranscoderPath` changes it); the SuperSpec Vite build copies them there.
  Viewers built on a plain `GLTFLoader` (for example LevelSpec's study viewers) need this before
  they can load default exports, or export with `--textures png`.
- Imported KTX2 GLBs keep their compressed textures: Studio uploads them as compressed GPU
  textures, a KTX2 re-export passes them through and a PNG export transcodes them.

The codec is the vendored `third_party/basisu_c_sys` (see its `PROVENANCE.md`): native builds need
a C++17 compiler; no other tools are required.

## Size limits

Shared allowances (`superspec_types::limits`, mirrored in `browser/model/limits.ts`): 512 MiB per
model or companion file, 64 MiB per encoded image, 1 GiB of companions per project, 1 GiB of
images per scene and 2 GiB per exported GLB. KTX2 images are validated structurally rather than
decoded. In the browser, a saved project embeds its companions as base64 in one JSON string, which
JavaScript limits to about 512 M characters: projects with more than about 380 MB of companions
cannot be saved or reopened in Studio yet, although their GLBs open directly.

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
