# Persistent world composition and derived export preparation

WorldSpec retains its canonical AML scene, reusable local mesh resources and
independent item placements until a delivery consumer requests buffers. Ordered
material overrides are retained as derived visual layers. Creating a receiver
view for material sampling does not replace these canonical inputs.

The shared model owns this composition contract. Producers declare artifact
identities separately from local resource names. Repeated identities share an
immutable `Arc`; conflicting attributes under one identity are rejected. ItemSpec
uses its existing geometry key plus the delivery variant. There is no geometric
similarity search or content-wide deduplication. Placement identity,
transform, visibility, collision and source ownership remain separate. Bakes
whose effective receiver coordinates differ remain distinct resources; this
change does not remove spatial inputs or alter raster behavior.

Admission happens before a layer becomes part of the accepted model. A compact
ledger applies the existing delivery limits after each append or replacement:
6,000,000 vertices, 18,000,000 indices, 256 MiB of encoded images and 192 Mi pixels.
Every placement counts toward delivery geometry; image identities count once in
the effective output, with conflicting bytes rejected. Replacements subtract
the old receiver's geometry and discard unused images from the effective ledger.
These checks do not construct expanded geometry buffers.

Persistent composition additionally has a 1 GiB retained-resource allowance.
It includes the physical base, local buffers, encoded images, placement and
resource metadata, and superseded visual layers. Shared `Arc` payloads count once
within a model; this charge is an admission policy, not process memory telemetry.
Hosts can lower either allowance through `composition::Limits`. Failed admission
publishes neither a layer nor its resource identity, and a corrected edit can
retry against the unchanged model. This additional bound is intentional: final
delivery limits alone cannot bound resources kept by immutable prior layers.

The existing scene packet and export ordering are compatibility contracts.
Expansion applies the existing append/replacement rules in authoring order, so
canonical collision, navigation, source records and validation invalidation keep
their current meaning. The scene-returning entry points remain delivery adapters;
new model-returning entry points expose persistent composition to independent
consumers.

Exporter prepares an owned view from an immutable scene or world model. Lightmap
UV generation, seam duplication and target serialization operate on that view.
Failed or cancelled preparation leaves both the canonical model and independent
exports unchanged. Maintained hosts use the immutable preparation API; the
mutating implementation helper is private to Exporter.

Verification covers shared local-resource identity across actual world placements,
delivery equality against the former eager composition, separate collision and
source metadata, independent exports, seam attribute preservation, cancellation,
failure and recovery. This is an ownership change, without performance claims.

Before the product namespace rename, a saved pre-change native executable and
the persistent-model implementation produced identical complete WorldSpec
packets and GLBs from the same saved project. Local evidence is recorded in
`generated/verification/immutable-world-parity.json`. The GLB is 76,040 bytes with
SHA-256 `6288a0516a118fe74427126b1d54f1e073ea1c49a9c2090b272436d0cccf79b5`.
Focused tests exercise resource identity conflicts, actual repeated procedural
and imported placements, distinct finish controls, source/collision delivery,
independent lightmap preparation, tangent seam remapping and cancellation.
After the namespace migration and aggregate-admission fixes, the five model
admission tests, two immutable export tests and five authored-world tests pass.
Their raw logs are `generated/verification/world-admission-tests.log`,
`generated/verification/immutable-export-renamed-tests.log` and
`generated/verification/immutable-world-renamed-tests.log`.
