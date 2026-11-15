# World build and physical lowering

This records the initial build-stage extraction. The later
[theme runtime boundary](theme-runtime-boundary.md) supplies the theme through an
adapter and moves concrete planning state out of the generic coordinator.

WorldSpec owns the existing build sequence: source/material preparation, physical
lowering, theme planning, structural review, visual publication and asset delivery.
Move that actual sequence from architecture into `worldspec::build` while keeping
the existing compile/session entry points compatible.

The physical compiler remains private. An internal `PhysicalBuild` exposes bounded
stages over typed physical output and painted render surfaces. Receiver adapters
continue to derive inputs from resolved AML and canonical geometry; they do not
expose compiler maps or mutable resolution state. Stage records retain their
existing concrete prepared data, with no JSON conversion or geometry copies.

The selected theme owns the floor finish plans, selected building hosts, render edge
capture and sign survey state. Pass that state explicitly through receiver adaptation.
Pure geometry publication receives only an optional vertex capture, independent
of the theme. Physical compiler state no longer retains theme planning state.

Preserve phase and error order exactly. In particular, collision-only builds still
perform the existing theme planning needed for checks and metadata, painted ground
receivers remain available, base-wall substitution does not change canonical
collision, and physical context/review precede visual publication. The successful
source preparation boundary still does not certify resolved references or geometry.

Verification uses the frozen complete scene/export corpus, building integration,
ground/parallel assembly and source-linked diagnostic tests, followed by native and
Wasm/consumer acceptance. Independent review checks state ownership and ordering.
