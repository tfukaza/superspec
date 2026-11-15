# Shared scene model and exporter ownership

This step moves the existing delivered scene contract into `superspec-model`.
The compiler builds that model; the exporter consumes it directly. It preserves
the current flattened meshes, source metadata, packet layout and export behavior.
Reusable mesh instances and immutable derived export views were separate work
from that extraction; the subsequent [authoring milestone](authoring-implementation.md)
adds `instances::Model` and owned scene/export adapters for new material/item inputs.

## Decisions before extraction

| Decision | Coupling removed | Preserved contract |
| --- | --- | --- |
| Put portable source/material records in the serde-only `superspec-types` crate; core reexports them. | A shared model need not import XML admission, the built-in registry or catalog content to carry source/material data. | Existing Node, Value, Material and diagnostic helpers and public core paths remain available. |
| Move Scene, Manifest and their concrete nested delivery records to `superspec-model`, depending on types and geometry. | Exporter no longer imports compiler implementation to receive geometry and metadata. | Rust types stay concrete; field order, Serde attributes, units, axes and binary packets remain unchanged. |
| Keep legacy architecture, validation, review, sign and ground metadata as explicitly named typed records. | Delivery data no longer requires its producing algorithms. | No opaque JSON payload or generic metadata proxy replaces existing typed fields. |
| Move neutral transactional visual admission with Scene; procedural drawings and baked materials implement small input contracts. | Inherent Scene methods can remain on the owned model without importing theme generators, rasterizers or compiler caches. | Existing append calls retain validation, rollback, source ancestry and resource accounting. |
| Reexport delivery records from their old compiler modules. | Consumers can adopt model ownership without a broad simultaneous import migration. | Compiler Scene is the same model type, not a second scene representation or copied wrapper. |
| Move the frozen sRGB conversion/table to shared types color utilities. | The exporter no longer reaches into the theme palette for color conversion. | Exact transfer values and palette entry points remain unchanged. |
| Keep context construction, validation, review binding, material baking and generation in the compiler. | Shared records do not pull algorithms into a supposed data library. | Native/Wasm behavior and ordering remain the existing implementation. |
| Exporter imports model/types directly; compiler is allowed only as an integration-test dependency. | Exporting a caller-created model no longer requires compiling the engine. | Existing GLB/OBJ/SVG/Quake/runtime serializers and lightmap preparation remain in place. |

The model still uses the portable geometry crate for retained solid triangles and
navigation data. No XML parser, compiler, theme implementation, filesystem, renderer
or cache implementation belongs in its dependency graph. Theme-specific report
records are retained as legacy delivery vocabulary, not generalized into new
generation interfaces.

## Verification

Before moving implementation, add an exporter test that constructs a complete
triangle scene directly from model/types and verifies GLB and runtime output. Keep
the existing compiler-driven export cases, visual admission and rollback tests,
native/Wasm parity and scene-update contracts. Compare frozen complete packets
including typed metadata across the cutover. Check the normal exporter dependency
tree excludes the compiler and the model's dependency tree excludes the AML parser.

At this extraction, lightmap preparation still mutated its supplied scene after
successful generation. The later [consolidation](superspec-consolidation.md)
replaces that API with immutable derived views and retains world instances until
delivery; the extraction checks below describe the earlier boundary.

## Physical and theme assembly ownership

The physical compiler keeps AML host admission, receiving-face selection, base-wall
substitution, canonical collision and walking inputs, source diagnostics, and scene
publication order. These operations read architectural resolution and physical
surfaces directly; moving them would require exposing unrelated compiler state.

Existing independent orchestration moves to its actual theme owner: completed host
drawings and finished-geometry span assembly live under the Kowloon building module;
procedural image admission and baking live in Kowloon visual baking. Their explicit
inputs and existing algorithms remain unchanged. The architectural visual adapter
still finishes the drawing and publishes admitted artifacts into the scene. This
does not add a facade around Compiler or change the public scene contract.

Existing visual admission, source-dependency, cancellation, building, span and
scene-contract tests cover these moves. The standalone model/export fixture also
calls the model-owned update encoder directly, proving that ordered scene delivery
does not require a compiler dependency.
