# Implementation assets

`materials/` contains the shipped image catalog used by native and browser
compilation. Catalog image identities use the `superspec-material:` scheme;
material definitions and image bytes retain their LevelSpec extraction baseline.
The provenance file records the original LevelSpec art
project paths; those source projects remain in the LevelSpec repository.

The extraction moved these assets together with their consumers:

| Original LevelSpec path | SuperSpec owner |
| --- | --- |
| `src/materials/library/` | `assets/materials/` |
| `src/dressing/kowloon/assets/` | `crates/engine/src/worldspec/themes/kowloon/assets/` |
| `src/dressing/kowloon/shop/assets/` | `crates/engine/src/worldspec/themes/kowloon/shop/assets/` |
| `tools/lighting/` | `crates/cli/assets/lighting/` |

The native CLI and offline material tool resolve shipped images from a
`materials/` directory beside their executable, falling back to this checkout's
`assets/materials/` during development. Browser builds bundle the same files.
Kowloon mesh catalogs and Blender lighting adapters are embedded in the native
compiler. Offline recipes still accept the LevelSpec project root for authored
source art. During regeneration, they read the latest catalogs from the SuperSpec
source checkout, so each stage can consume the preceding stage's output. Set
`SUPERSPEC_ROOT` to select that checkout explicitly.
