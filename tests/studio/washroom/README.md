# Washroom benchmark scene

A small scene for measuring and tuning MaterialSpec: a concrete corner with a tiled wall, an
enamel sink, a galvanized duct, rusted pipes with a valve, a wooden bench and a floor drain. It is modeled after a piece of concept art and is meant to be recreated with the shipped
example recipes, so that work on those recipes and on the bake pipeline shows up here.

| File | Purpose |
| --- | --- |
| `geometry.mjs` | Mesh helpers (boxes, bars, tubes, ducts, tori) and a GLB writer. |
| `model.mjs` | The scene: 15 meshes, one per material receiver, in meters with Z up. |
| `build.mjs` | Writes `generated/washroom/washroom.glb` and `washroom.studio.json`, a version 2 project. |
| `materials/` | The one recipe the examples lack: bench wood. |
| `../../material-washroom-browser.mjs` | Opens the project in the built Material Studio and times it. |

## Materials

Eleven of the twelve materials are parameter sets of shipped recipes. Wear, broken edges and
standing water are left to the materials rather than modeled:

- `kowloon-concrete` on the walls and, with `ground`, the slab.
- `kowloon-rusted-steel` for the galvanized duct, the rusted pipes, the flat steel brackets and
  drain, the brass valve, the red handwheel, the chrome tap and trap, the enamel sink and the
  dark openings: nine parameter sets of one recipe.
- `tiled-courtyard` for the wall and floor tile.

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
loaded viewport.

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
