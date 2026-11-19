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
   The tree prop uses the street tree’s backing canopy for static mesh delivery.
   The [compiler fixture](../../crates/engine/src/worldspec/fixtures/corner-avenue.aml)
   exercises the reference positions and dimensions.
4. Verify rotated strip bounds, row spacing and ancestry, source errors and recovery,
   nested building assets, physical independence, and native/Wasm delivery.

The MVP handles explicit straight segments and repeated placements. The studio
study remains the reference for baked facade and paving finishes, illustrated
foliage, shops, clutter, grass and shrubs. Later slices can bind those materials
and foliage batches to the AML layout, then add path-following and intersection
rules once their placement and physical contracts are specified.
