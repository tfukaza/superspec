# WorldSpec city layout MVP

The previous avenue study combines a corner building, street surfaces, planting
and furniture in a studio preview project. Its avenue record and placement code
are the dimensional reference. WorldSpec owns the arrangement of these objects.
ItemSpec owns a building and its attached parts, a segment of paving, a tree or
a lamp in local coordinates.

1. Admit a city section in the shipped AML profile and schema. Give every placed
   object a source ID, world position and heading. Use a count and a world-space
   displacement for straight rows. Bound expansion during source admission.
2. Compile the architectural base through the existing physical pipeline. Resolve
   each role to a theme-owned ItemSpec definition and place the resulting local
   resources in source order. Carry the AML location onto each delivered mesh.
   Both the native CLI and the browser worker consume the same Rust implementation.
3. Port the corner building, carriageway, pedestrian avenue, far sidewalk, median
   trees and lamp rows. The avenue and sidewalk reuse the existing stone generators.
   The tree and shrub placements use the street study’s illustrated TreeSpec foliage batches.
   The [compiler fixture](../../crates/engine/src/worldspec/fixtures/corner-avenue.aml)
   exercises the reference positions and dimensions.
4. Verify rotated strip bounds, row spacing and ancestry, source errors and recovery,
   nested building assets, physical independence, and native/Wasm delivery.

The MVP handles explicit straight segments and repeated placements. The AML
preview uses the shop-house material recipes for city receivers, the avenue
median's planted groundcover, and the study's lighting and shadow settings.
Mesh contours are off in the city preview.

Add `?edgeblur=on` to the preview URL to soften strong color transitions
within one device pixel. `edgethreshold` sets the minimum display-color
difference on a scale from 0 to 1. The default is `0.12`.

Path-following and intersections require further placement and physical contracts.
