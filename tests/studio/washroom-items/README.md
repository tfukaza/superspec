# Washroom built from items

The washroom of `../washroom` composed from ItemSpec definitions instead of a hand-built
GLB. `../washroom` and its GLB build stay as they are, as the comparison fixture.

| File | Purpose |
| --- | --- |
| `items.mjs` | Seven definitions (slab, tall wall, low wall, duct, pipe, bench, drain) and where each stands. |
| `build.mjs` | Writes `generated/washroom-items/washroom-items.studio.json`, the definitions and the decal images. |
| `../../item-washroom-browser.mjs` | Opens the project in the built Material Studio and runs the item MVP's acceptance checks. |
| `../../item-scene-parity.mjs` | Compares a native and a Wasm build of an item scene byte for byte. |

```bash
node tests/studio/washroom-items/build.mjs 512 128
npm run test:items:browser
```

The materials are the GLB washroom's parameter sets, bound to item slots, plus `glass`.
With `weathering` (the default) the walls, duct and pipe use copies of the shipped
concrete and steel recipes to which the build script adds layers that follow named
features (`base`, `sill`, `joint`); `WEATHERING=0` uses the shipped recipes as they are.
The bolts of the brackets and the vent are the GLB washroom's instanced batch without
its two flange rings: the pipe item makes its own flange bolts.

## Differences from the GLB washroom

- The tall wall has a framed, glazed window with a sill; the low wall an arched recess.
- A notice on the tile, a stencilled number under the window and a label on the pipe.
- The pipe bends with a 0.1 m radius (0.2 m in the GLB), its flanges carry six modeled
  bolts each (eight instanced ones in the GLB) and its couplings stand 18 mm proud.
- The duct's wall brackets are the route's `bracket` fittings; the bench's are three
  boxes each. The drain's rim is a flat ring, not a torus.
- 115 receivers (one per part and face group) instead of 12, each with its own atlas.
