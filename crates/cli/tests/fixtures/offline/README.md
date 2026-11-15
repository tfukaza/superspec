# Offline authoring fixtures

The small cube meshes and asymmetric source images are synthetic inputs, independent
of the generator implementation. `expected.json` records SHA-256 hashes from the
pre-extraction `levelspec-materials` executable for `brushed-metal`, `bag-clusters`
and `pack-restaurant-food`, before moving authoring code out of the CLI. The local
source and executable evidence is described in `docs/plans/offline-authoring.md`.

These fixtures verify material pixels, channel names, catalog geometry, optional
source maps and publication contracts. They are correctness evidence, not timing
or quality comparisons.
