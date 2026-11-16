# TreeSpec geometry prototype

Procedural foliage geometry for roadmap stages 6 and 7
([tree-item-roadmap.md](../../../../docs/plans/tree-item-roadmap.md), phases 4 and 5):
grass patches, shrubs and trees. This module produces geometry and the per-vertex and
per-instance data that painted materials and wind will read. It has no materials, no
shaders and no renderer. Like `materialspec` and `itemspec` it is generic: it never
imports `worldspec`, a theme or physical assembly (`npm run check:architecture`).

Definitions are JSON, shaped like item definitions (`"format": "treespec"`,
`"version": 1`, `"kind": "grass" | "tree"`). There is no authoring language yet.
Fixtures are in `fixtures/`: `meadow.grass.json`, `shrub.tree.json`,
`broadleaf.tree.json` and `conifer.tree.json`.

| Call | Result |
| --- | --- |
| `grass::parse(source, text)` | `GrassDefinition`, or a `TreeDiagnostic` |
| `GrassDefinition::clump_meshes()` | the clump meshes of every level, without a site |
| `grass::generate(&definition, &site, &cancelled)` | `GrassPatch` |
| `tree::parse(source, text)` | `TreeDefinition` |
| `TreeDefinition::skeleton(&cancelled)` | the branch skeleton alone |
| `tree::generate(&definition, &cancelled)` | `GeneratedTree` |

Coordinates are meters, right-handed, Z up. A plant's origin is where it meets the
ground. Angles in definitions are degrees.

## Output records

`FoliageMesh` is positions, geometric normals, texture coordinates, indices, a material
slot (`blade`, `bark`, `leaf`) and named per-vertex attributes: `scalars` and `vectors`,
one value per vertex. `InstanceBatch` is a mesh index, one affine transform per instance
(three basis columns and a translation) and named per-instance `scalars` and `vectors`.
`batches.rs` converts both to the portable `superspec_model::batches::InstanceBatch`: the
transform becomes a placement, and the named attributes keep their names per vertex and per
instance. A grass batch also carries `tile`, the index of each instance's tile in
`GrassPatch::tiles` (a tile's instances are consecutive). `batches::grass_levels` gives every
level of a patch, each batch with a foliage look (`superspec_types::batch::Foliage`: the
shader family, the level's `distance` and `keep`, the patch's `far`, the recipe and the
family parameters), which is how a renderer draws grass with its fixed shader families
(roadmap stage 6; `browser/renderer/foliage.ts`).

| Name | On | Meaning | Painted-material input |
| --- | --- | --- | --- |
| `uv` | blade, leaf vertex | across (0 to 1) and root to tip (0 to 1) | cutout mask, tip highlight |
| `uv` | bark vertex | meters around the branch at its base girth, meters along it | bark pattern |
| `heightRatio` | every vertex | 0 at the root of a blade, leaf card or branch, 1 at its tip | root-to-tip ramp (1D.2 `heightRatio`) |
| `bendWeight` | every vertex, leaf instance | wind weight from 0 to 1 | vertex stage of the wind family |
| `shadeNormal` | blade and bark vertex, leaf instance | points out of the clump or canopy | `shadeBand` normal |
| `pivot` | blade vertex | the blade's root in clump space (per-vertex root offset) | bend and rejection per blade |
| `pivot` | bark vertex, leaf instance | the base of the branch | hierarchical wind |
| `random` | every vertex, every instance | stable value in [0, 1) per blade, branch, card or instance | `instance.random` and per-blade jitter |
| `element` | vertex | blade index in a clump, branch ID, or card index | grouping |
| `level` | bark vertex, leaf instance | branch level, 0 for a trunk | wind family parameters |
| `root` | instance | world position of the instance origin | `instance.root`, `sampleField(name, root.xy)` |
| `groundNormal` | grass instance | ground normal under the root | the ground's shade at the root |
| `threshold` | instance | stable thinning rank in [0, 1) | density and level-of-detail rejection |
| `variation` | instance | colour variation value in [0, 1], coherent over about 1.7 m for grass and per limb for leaves | `instance.tint` through a ramp |
| `cluster` | leaf instance | the limb (level 1 branch or trunk) the cluster belongs to | per-clump shading |

A leaf's `bendWeight` on the instance is the branch's weight where the cluster hangs;
the card mesh's own `bendWeight` is flutter within the cluster.

## Grass

A patch is a small set of clump meshes plus instances. Each of `clump.variants` clumps
holds `clump.blades` blades rooted in a disc. A blade is a ribbon along a circular arc:
it leaves the ground at `lean` from vertical and turns through `curvature` more by its
tip, narrows by `taper` and ends in a point. With `n` segments it has `2n + 1` vertices
and `2n - 1` triangles; every level samples the same arc.

Placement uses a jittered grid fixed in world space with one candidate per cell of
`1 / sqrt(density)` meters. A candidate is kept when its stable value is below the local
density: the mask times the `clumping` drift. The surface is found by looking straight
down, so overhangs and caves are not supported, and density is per square meter of plan,
not of slope. Ground steeper than `maxSlope` grows nothing. Because candidates are fixed,
changing the mask or the ground leaves every surviving clump exactly as it was.

The mask is `uniform`, a `polygon` in plan, or a `field` of values from 0 to 1 read with
bilinear interpolation.

Instances are grouped into square tiles of `tile` meters, one batch per clump variant,
and sorted by `threshold`. A level of detail lists a mesh per variant, `keep` and `widen`
(`1 / sqrt(keep)`, applied to blade width in that level's meshes). To draw a level, draw
the first `GrassTile::drawn(batch, keep)` instances of each batch with that level's mesh.
The fixture steps 12, 3 and 1 segments at 100%, 50% and 20%.

## Shrubs and trees

The skeleton is a list of `Branch` records: a centre line with a radius at each point, a
parent and the fraction of the parent's length where it attaches. Trunks are level 0;
`levels[i]` describes the branches on level `i`. A branch starts on its parent's centre
line with `radius` times the parent's radius there, leaves at `angle`, turns `gravity`
degrees toward the ground over its length (negative values turn it up, as growth toward
light does) and wobbles. Branches end where they leave the `envelope` (an ellipsoid or
an upright cone); one that starts outside it, or keeps less than a fifth of its length,
does not grow. `trunk.count` above one gives several stems, for shrubs.

Bark is one tube per branch, closed by a flat end. Tubes are not welded: a child tube
starts inside its parent and the two overlap. Trunks and branches are separate meshes.

Leaves are one `InstanceBatch` over a cluster mesh of `leaves.cards` cards that grow
along +Y from the origin, with +Z as their face. `shape` is `quad` (for a cutout
texture) or `leaf` (a pointed outline that reads without one). The instance
`shadeNormal` blends the canopy's outward direction (70%) with the direction out of the
cluster's own limb (30%). `canopy` reports the leaf volume and one ball per limb; no
hull mesh or shadow proxy is built from them yet.

A level of detail has its own trunk, branch and leaf-cluster meshes (`radialSegments`,
`ringStep`, `maxLevel`, `leafCards`) and draws the first `leaf_instances` leaf clusters,
widened by `1 / sqrt(leafKeep)`.

`features` holds one `base` ring per stem where it meets the ground, in the shape of an
ItemSpec feature, for weathering layers.

## Keys, determinism and limits

Each mesh has a `geometry_key`, and results list `part_keys`: for grass
`clump-<variant>/lod<level>` and `placement`; for a plant `skeleton`, `trunk/lod<n>`,
`branches/lod<n>`, `leaves/lod<n>` and `leaf-placement`. A key hashes only what its part
reads, so a mask change touches `placement` alone and a leaf size change touches
`leaf-placement` alone.

The same definition gives the same bytes on native and wasm32 targets. The generators
use integer hashing, IEEE basic operations, `sqrt` and the shared deterministic
`sin`/`cos` from `superspec_geometry::math`; they call no platform math library. The
tests pin a digest of each fixture, and the same digests were reproduced by a
wasm32-unknown-unknown build run in Node.

`limits.maxVertices` (default 400000) and `limits.maxInstances` (default 200000) are
checked with `TREE_BUDGET` errors that name the count. Hard caps: 2000000 vertices,
1000000 instances, 8000000 grass candidates, 500000 ground triangles, 20000 branches,
1 MiB of definition text. Generation polls `cancelled` every 4096 grass candidates and
every branch, and returns `TREE_CANCELLED`.

Diagnostics carry a code, the source name and a JSON pointer (`TREE_JSON`, `TREE_FORMAT`,
`TREE_VALUE`, `TREE_LIMIT`, `TREE_BUDGET`, `TREE_GROUND`, `TREE_MASK`, `TREE_CANCELLED`).

## Looking at the output

```bash
cargo test --release -p superspec-engine --test treespec_verification -- --ignored --nocapture
node tests/treespec-browser.mjs
```

The first writes GLBs under `generated/verification/treespec/` and prints timings. The
second opens each in the built Material Studio and saves pictures beside them, then adds
the grass strip and the shrub to the washroom scene. The GLBs carry vertex colours
derived from `heightRatio`, `variation`, `random` and light on `shadeNormal`; that is a
stand-in for painted materials, not their design.

Painted grass (roadmap stage 6) is the washroom's lawn, drawn by the grass shader families
with a painted recipe:

```bash
cargo test --release -p superspec-engine --test treespec_batches -- --ignored write_washroom_lawn
BROWSER_ARGS="--use-angle=d3d11 --ignore-gpu-blocklist" node tests/grass-washroom-browser.mjs
```

The first writes `washroom-lawn.json` (the lawn's ground and the meadow grown on it, every
level with its foliage look); `tests/studio/washroom/build.mjs` adds both to the washroom
when asked for grass. The second measures and pictures both profiles under
`generated/verification/grass/`.
