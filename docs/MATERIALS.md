# Shared material library

Procedural dressing uses shipped materials by default. Scenes no longer need to
copy a study's material declarations or images to obtain the current concrete,
ceramic, wood, painted steel, or stainless finishes. Ordinary compilation loads
local intrinsic PBR assets; it does not run image generation or bake lighting.

The native material tool writes regenerated maps to a fresh directory. Run these
commands from the SuperSpec repository root with a new destination for each
recipe. Authored source art remains in the sibling LevelSpec checkout.

```powershell
cargo run --release -p superspec-cli --bin superspec-materials -- kitchen-metal generated/kitchen-metal
cargo run --release -p superspec-cli --bin superspec-materials -- brushed-metal generated/brushed-metal
cargo run --release -p superspec-cli --bin superspec-materials -- concrete ../levelspec/projects/kowloon-concrete-study/textures/concrete-plain.png ../levelspec/projects/kowloon-concrete-study/textures/concrete-fine-sand.png generated/concrete
cargo run --release -p superspec-cli --bin superspec-materials -- vent-metal ../levelspec/projects/dressing-demo/concepts/duct-material-study/metal-base-ai.png generated/vent-metal
cargo run --release -p superspec-cli --bin superspec-materials -- restaurant-tiles assets/materials/wall-ceramic-weathered.png generated/restaurant-tiles
cargo run --release -p superspec-cli --bin superspec-materials -- crate-wear ../levelspec generated/crate-wear
cargo run --release -p superspec-cli --bin superspec-materials -- shop-cartons ../levelspec generated/shop-cartons
cargo run --release -p superspec-cli --bin superspec-materials -- shop-timber ../levelspec/projects/kowloon-liquor-props/materials/shelf-weathering/wood-long-source.png generated/shop-timber
```

Crate and carton outputs include updated mesh tints and atlas coordinates.
Publish the three maps and both JSON catalogs together.

The same native tool packs baked prop meshes and color, normal, and roughness
maps. The input directory is the raw Blender output with `catalog.json` and
source image channels. Each command writes a fresh directory. The following block
uses the built `superspec-materials` executable on `PATH`; alternatively invoke
`target/release/superspec-materials` (`.exe` on Windows) directly.

```powershell
superspec-materials pack-liquor ../levelspec/generated/liquor-props/bake generated/packed-liquor
superspec-materials pack-restaurant-food ../levelspec/generated/restaurant-props/food-bake generated/packed-food
superspec-materials pack-balcony ../levelspec/generated/kowloon-balcony-props/bake ../levelspec/projects/kowloon-balcony-props/concepts/plain-cotton.png generated/packed-balcony
superspec-materials bag-clusters crates/engine/src/worldspec/themes/kowloon/shop/assets/grocer-props.json generated/bag-clusters.json
superspec-materials clean-stock ../levelspec assets/materials generated/clean-stock
superspec-materials stock-appearances ../levelspec assets/materials generated/stock-appearances
superspec-materials refine-stock ../levelspec ../levelspec/generated/liquor-props/bake/catalog.json generated/refined-stock
superspec-materials kowloon-kit-textures generated/kowloon-kit
superspec-materials kowloon-prop-textures generated/kowloon-props
superspec-materials site-textures generated/site-textures
```

LevelSpec's Blender adapters select the sibling SuperSpec release executable or
the explicit `SUPERSPEC_MATERIALS` environment variable. `SUPERSPEC_ROOT` selects
another SuperSpec source checkout for offline regeneration. They read updated
catalogs between stages; a source checkout and the LevelSpec art projects are
required for these authoring workflows. They use Blender to import and
decimate the approved meshes and render the snack label. The native tool
generates the crate lattice, bottle load, egg basket, apple pile, and snack
packet mesh and prepares their textures. Run `superspec-materials --help` for
the individual recipe inputs.

```xml
<dressing seed="street" theme="kowloon-street@1">
  <building host="tenement" stories="4"/>
  <floor host="courtyard" ground="patched"/>
</dressing>
```

An omitted recipe material selects `builtin:plaster` for walls/buildings and
`builtin:concrete` for floors. Native callers may supply `ArchitectureInput` with
omitted recipe materials too. Concrete trim, foundations, shop slabs and roof
paving always have their own structural-concrete role. A wall's coating is not
that role's fallback.

| Material reference | Intended mapping / use |
| --- | --- |
| `builtin:concrete` | Repeating concrete albedo and tangent normals, 8 m period |
| `builtin:plaster` | Repeating plaster coating; facade weathering adds exposure and runoff |
| `builtin:ceramic` | Individual wall/floor ceramic fragments; damage and bedding remain separate |
| `builtin:tile-bed` | Mineral bedding beneath removed/chipped ceramic |
| `builtin:asphalt` | Photographic aggregate with tangent-normal relief |
| `builtin:timber` | Generic repeat-safe wood, including ground planks |
| `builtin:shop-timber` | Member atlas: shelves, door boards, end grain and continuous grain direction |
| `builtin:metal`, `builtin:sheet` | Steel substrates for the shared paint/rust/grime surface sampler |
| `builtin:brushed-metal` | Repeating stainless albedo, normals and packed roughness/metalness |
| `builtin:tarp` | Shared weathered canvas, 1.5 m period, fiber normals and roughness; static Blender cloth forms |
| `builtin:roofing` | Roof membrane; distinct from structural concrete |
| `builtin:restaurant-tiles` | Kitchen tile field with its own physical mapping |
| `builtin:aircon`, `builtin:lettering`, `builtin:restaurant-surfaces` | Dedicated panel/glyph/material atlases; use their existing geometry adapters |

Shop food, stock, posters, cloth and signboard inputs are also catalog roles.
An atlas is never a general replacement for a repeating surface map. UVs use
physical meters and each material's repeat; explicit panel/glyph UVs pass through.
Painted metal uses the existing layered corrosion sampler, including intrinsic
normal relief and gravity-aware dirt. Stainless remains a separate clean-metal
finish. The default component density is 64 texels/m, with finer minimums for larger
metal faces and continuous sheets. Texture density controls quality; it no longer enables/disables the
production component finish.

Facade paper notices use the shared 16-design poster atlas and its padded,
reflection-aware mapping; placeholder colored cards are no longer emitted.
Window and door soot is an opening-dependent intrinsic tint. It is separate from
the shared repeating facade recipe, so moving an opening does not generate a new
concrete or plaster texture. Existing receiving panels are refined locally where
needed for soft tint transitions. Tiled facades compose the same opening deposits
into their finite finish. `grime="0"` disables these added deposits.

The default appearance budgets are 4/8/16 Mi texels at low/medium/high detail,
with 16,384 receiver charts (32,768 at high). Memory is bounded at 64 MiB (128 MiB at high detail); large
streets may set explicit compile budgets, as the demo does.

## Authored overrides

A recipe's explicit `material` remains authoritative. For component roles,
`finishes="custom"` selects a declared `custom-concrete`, `custom-metal`,
`custom-shop-timber`, etc. Missing roles inherit the library independently. Each
override retains its complete color, repeat, scalar factors and PBR channels;
channels are not silently filled from a different material. Declare a normal AML
ID for custom finishes. `builtin:*` names are reserved for canonical catalog
definitions; unchanged definitions survive normalized-source round trips.

## Ownership and image loading

`crates/catalog` owns role definitions and reserved image identities;
the core material-catalog adapter registers them in AML. `assets/materials/` owns
shipped image bytes and provenance from the approved studies. The native authoring
CLI publishes concrete, stainless, shop-timber and restaurant-surface maps there.
Change a role definition or its
assets once to update every consumer using that default. Existing study files
with explicit old declarations remain authored overrides; they do not define the
production library. LevelSpec's `demos/courtyard/scene.aml` demonstrates the defaults.

`superspec-material:*` image references resolve through the Rust catalog.
The native host and Wasm editor provide the corresponding local assets.
A missing referenced image produces an error.
Export resolves only materials used by visible triangles. GLB embeds the used
maps; glTF packages them by content hash, sharing duplicate bytes.

The native compiler and exporter consume the host's resolved image bytes.
Parallel workers receive frozen image snapshots, including library inputs.
Asset bytes participate in source hashes and the demo build fingerprint.

## Why the integration was missed

Earlier kits registered plain colors and looked for optional per-project
`<prefix>-<role>` declarations. Missing concrete inherited the wall substrate;
weathering depended on optional texture density. Wall, building and furnished
shop shutters had separate finish paths. Standalone studies and the demo carried
independent material lists, so approving a study did not update production.
The catalog, default component settings, shared shutter geometry/finish and
complete host image-loading boundaries replace those integration points.

Checks protect bindings, override ownership, image resolution, source immutability,
round trips, cancellation and export packaging. They do not fix texture pixels,
rust distribution, colors or prop placement as acceptance requirements.
