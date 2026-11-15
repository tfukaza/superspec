# Native producers

The Rust compiler produces editable AML and validates candidates through the same
collision and sampled-navigation path used by the CLI.

```sh
superspec generate --seed run-42 --depth 4 --min-routes 2 --out generated/run-42.aml --json
superspec trace radar.png --cells 46 --bands 4 --out generated/radar.aml --evidence generated/radar.json --json
superspec game-export generated/run-42.aml --out generated/game --json
```

Generation supports `three_lane`, `ring`, `stacked`, `compound`, `bridges`,
`sky_islands` and `fightyard`. Select a family with `--archetype` and a palette
with `--theme`. `--attempts` bounds candidate retries. Seeded layout and dressing
use the shared Rust random stream. Pickups and enemy posts use sampled navigation
reachable from the generated spawn. Each published candidate is validated again
after placement.

Image tracing decodes PNG or JPEG in Rust. It estimates the background, segments
color bands and merges raster cells into AML floors with exterior walls. Missing
cells remain void. Tracing supplies a flat geometric proposal. Review elevations,
architectural intent and gameplay circulation separately. `--raster` accepts a
saved `levelspec-raster/1` observation file for repeatable proposal generation.
`--evidence` saves those observations alongside the AML source.

The default trace settings use 46 cells across the longest dimension and four
color bands. Available options include `--grid`, `--span`, `--fill`, `--bg-tol`,
`--alpha-cut`, `--smooth`, `--min-band` and `--no-crop`. Generated source and raster
output paths must be distinct. Publication stages all files before replacing
existing outputs.

Rust callers can use `generation::propose`, `generation::populate`,
`tracing::trace_encoded`, `tracing::Raster::aml` and `Session` from
`superspec-engine`. Callers provide companion bytes and cancellation callbacks.
Keep source files and independent references alongside projects, with generated
artifacts under `generated/`.

The former TypeScript package exports and Node producer commands are retired.
Navigation remains sampled, and a valid generated map still needs architectural
and gameplay review appropriate to its intended use.
