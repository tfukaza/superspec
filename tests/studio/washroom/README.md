# Washroom benchmark scene

A small scene for measuring and tuning MaterialSpec: a concrete corner with a tiled wall, a
galvanized duct with a vent, one large rusted pipe, a wooden bench and a floor drain. It is
modeled after a piece of concept art and is meant to be recreated with the shipped example
recipes, so that work on those recipes and on the bake pipeline shows up here. Detailed
fixtures (a sink, valves, taps) are left out on purpose: they are a job for ItemSpec.

| File | Purpose |
| --- | --- |
| `geometry.mjs` | Mesh helpers (boxes, bars, tubes, ducts, tori) and a GLB writer. |
| `model.mjs` | The scene: 11 meshes, one per material receiver, in meters with Z up, and the bolts: one instanced batch of a twelfth mesh, the bolt template. |
| `build.mjs` | Writes `generated/washroom/washroom.glb` and `washroom.studio.json`, a version 2 project. |
| `materials/` | The one recipe the examples lack: bench wood. |
| `../../material-washroom-browser.mjs` | Opens the project in the built Material Studio and times it. |

## Materials

Seven of the eight materials are parameter sets of shipped recipes. Wear, broken edges and
standing water are left to the materials rather than modeled:

- `kowloon-concrete` on the walls and, with `ground`, the slab.
- `kowloon-rusted-steel` for the galvanized duct, the rusted pipe, the flat steel brackets and
  drain, and the dark openings: four parameter sets of one recipe.
- `tiled-courtyard` for the wall and floor tile.

## Painted slab

`buildProject(resolution, density, { painted: true })`, `PAINTED=1` for `build.mjs` or for the browser
benchmark, gives the slab `materials/painted-ground.material.ts` instead of concrete: a painted ground
(roadmap stage 5) whose shade boundary follows the sun of the lighting record, with the room's cast
shadows folded in. The browser benchmark then moves the sun and checks that the boundary moves with no
rebuild and no bake (it adds `gpuprobe` and `ao=off` to the page's query).

## Instanced batch

Every bolt of the scene (the pipe's two flanges, the bracket anchors, the vent's screws: 32) is an instance of one
mesh, `bolt`, listed in the project's `batches` (see `superspec_model::batches::of_project`). The template is an
ordinary mesh with the flat steel material, standing 5 m below the slab where it touches and shades nothing; the
project bakes its material once, there, and the delivered scene holds the batch in its place: one mesh, one material,
32 placements with a tint and an `instance` value each. `buildProject(resolution, density, { batches: false })`, or
`BATCHES=0` for the browser benchmark, builds the scene without them, as it was before.

On an RTX 5060 Ti at 256 px (2026-11-16, three runs each): the batch adds one draw call per pass (27 to 29 with
ambient occlusion, 11 to 12 without), 2,112 triangles per pass and nothing measurable to the frame's GPU time with
ambient occlusion (fastest frames 0.86 to 0.88 ms either way; run medians 0.87 to 2.0 ms without the batch and 0.87 to
1.33 ms with it); without ambient occlusion the median went from 0.34 to 0.42 ms in one run each. `?batchstress=10000` (tests only) draws the batch
with 10,000 instances on a lattice through the room: 2.2 ms of GPU time a frame with ambient occlusion (1.3 million
triangles in two passes), 1.5 ms without, still 29 and 12 draw calls.

## Running it

```bash
node tests/studio/washroom/build.mjs 512 128
```

writes the project at 512 px with 128 texels per meter (0 for no density). Build it natively with
`superspec-studio glb generated/washroom/washroom.studio.json > washroom.glb`, or open
`washroom.studio.json` in Material Studio with **Open project**.

```bash
npm run test:washroom:browser
```

runs the browser benchmark against `generated/rust-editor` (build the Wasm module and
`npm run build:viewer` first). `RESOLUTION` sets the texture size and `SCREENSHOT` saves the
loaded viewport. This is the repository's default scene for scene-level tests, timings and
screenshots; extend it instead of creating another one. Headless Chromium renders on
SwiftShader unless `BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist"` selects the GPU
(the test prints the WebGL renderer in its frame-time line). On an RTX 5060 Ti at 512 px:
first preview and loaded about 4.4 s, 33 ms per frame.

## Measurements

One development machine (12 threads), release builds, headless Chromium for the browser rows.
"First preview" is the 64 px draft; "loaded" is the full-size build ready in the viewport.
Treat these as a record of what changed, not as a specification.

| Step | Native 512 px | Browser first preview | Browser loaded |
| --- | --- | --- | --- |
| Start | failed: feature queries exceeded the sample budget | n/a | n/a |
| Feature queries search a grid | 19.6 s | never (bakes evicted each other) | never |
| Material variants share decoded images | 19.0 s | 13.1 s | 13.9 s |
| Texel density of 128 per meter | 11.0 s | 8.8 s | 9.5 s |
| Repeated samples copied, small faces sampled at density | 9.7 s | 7.9 s | 8.6 s |
| One build reports every bake; opening shows the draft first | 9.7 s | 2.5 s | 8.7 s |

Native builds use one thread at the preview profile; the browser spreads bakes over up to six
helper workers. The table was measured with 17 meshes, before the modeled pebbles and puddle were
removed. Native peak memory at 512 px fell from 220 MB to 119 MB. The two concrete
materials account for about three quarters of the remaining bake time.
