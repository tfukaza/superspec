# MaterialSpec SDK and Material Editor

MaterialSpec is SuperSpec's code-first SDK for procedural materials. The first product milestone after the Rust port and SuperSpec handoff is a Material Editor where an author can open a scene or model, apply a recipe to a surface, and see code and parameter changes live. The editor and scene builds use the same material compilation and evaluation contracts. [SuperSpec modularization plan](spec-modularization.md)

## Material source and compiled contracts

See the [MaterialSpec language reference and helper review](../material-language.md)
for current `.ms` semantics, the proposed TypeScript subset and operation
contracts, and the reference recipe checks.

Authors write versioned material modules with imports, typed public parameters, defaults, seed inputs and asset references. SDK helpers let them define and compose fields, build surface layers and produce PBR channels. Themes and projects can import common concrete, grime or metal recipes, supply presets, and author their own materials through the same interface. Saving a material preserves its source, dependencies and presets. Compiled programs and baked maps are derived artifacts.

Have the compiler expose the material's parameters, dependencies, output channels and required inputs to the host. The renderer uses that contract to evaluate supported live operations. A baker uses it to evaluate the material over a texture or surface mapping. Preserve source locations for compilation and dependency errors. The TypeScript SDK milestone uses Rust to parse the supported subset into an in-memory program shared by native and Wasm. Precompiled modules and generated kernels are future optimization work.

## Composable SDK helpers

A field evaluates a value at a sample location. Its inputs identify the coordinate space and any required seed or sampling footprint. Fields can yield scalar values, vectors or colors. Helpers can take another field as input so an author can scale, warp, combine or reuse a result without repeating the sampling logic. User-authored helpers follow the same conventions as SDK helpers.

| Helper family | Scope |
| --- | --- |
| Fields and masks | Seeded noise, soft thresholds, remap, and explicit mask combinations. |
| Coordinates and sampling | UV, surface, object, and world coordinates. Share varied sample coordinates and weights across related maps. Compose triplanar projection and contrast correction separately. |
| Receiver queries | Feature distances and frames, plus gravity-relative coordinates from finite source edges. Studio exposes missing feature data in inspection. |
| Channels and normals | Ordered linear-space layers, explicit additive or replacement height, height-derived relief, and receiver-normal adjustments. |
| Live lighting | Typed renderer operations for grazing falloff, environment filtering, luminance banding, and diffuse modulation. Bind recipe-declared inputs through the Three.js adapter. |
| First expansion | Rectangular and staggered grids, per-cell variation, filtered joint coverage, and bounded neighborhoods for damage crossing cell boundaries. |
| Follow-ups | Directional fields, imported normal-map composition, authored masks, and alpha cutout. Use reference recipes to establish which public operations are needed. |

Document each helper's parameter units, output range and sampling behavior. Derive named seeds for independent effects so tuning one effect can leave unrelated patterns stable. Define the material in surface, object or world coordinates. Use object space for detail that follows a prop and world space for continuity or gravity-dependent effects. A live renderer can supply a pixel footprint for filtering. A baker supplies a texel footprint. Helpers requiring scene depth, color or other view inputs declare those dependencies explicitly.

Generate SDK declarations, graph ports, and the AI reference from the versioned
catalog. Verify ordinary strict TypeScript checking for recipe declarations and
connections. Add positive examples and failing cases for a misspelled operation
or port, a missing required input, a channel type mismatch, and an incompatible
coordinate space or evaluation stage. Check Rust diagnostics separately for
asset, receiver, and renderer constraints. See the
[static typing contract](../material-language.md#static-typing-and-validation).

Use editable groups such as Edge Wear, Runoff, Ground Weathering, Oxide,
and Tile and Grout to expose commonly tuned controls and inspectable masks.
Edge Detection queries receiver features. The recipes and presets supply the
material scale, tuning values, and assets.

### Reference recipes

Compose a concrete base from aggregate-scale variation and fine surface detail. Use coarse noise to locate chipped patches, warp the coordinates to break up their outlines, and add fine noise near the boundary. Reuse the resulting chip mask to vary color, roughness and height together. Keep the height and chip dimensions in physical units. Add edge runoff as a separate layer using a shared grime image and the receiver's downhill mapping. The same grime image can serve multiple slopes.

Port the existing concrete and plaster study with its texture variation, layers,
and receiver data. Port the steel study with its oxide masks and live lighting
effects. Inspect chip coverage, runoff, oxide masks, height-derived relief, and
crease shading independently. Check that clean-metal grain modulates only the
diffuse contribution.

Use procedural ceramic as the first expansion proof. One Tile and Grout group
must express clean square, staggered rectangular, and existing worn tile.
Inspect face, joint, chip, and exposed substrate coverage. Compare close and
coarse bakes against the ceramic reference where applicable. Keep shading
relief in the material and verify geometry and collision with the
[ceramic reference tests](../../crates/engine/tests/tile.rs). The
restaurant's low-wall staining is a separate layer. Brick, planks, brushed
metal, and cutout foliage are follow-up callers for these same contracts.

## Opening and inspecting scenes

| Entry point | Behavior |
| --- | --- |
| **Open AML** | Build through SuperSpec's ordinary AML and asset-loading path. Preserve authored placements and bindings. Accept the same standalone files and companion folders as the Level Viewer, including scenes from LevelSpec's `demos/` library. |
| **Open 3D Model** | Import a model into the shared 3D representation with its hierarchy, transforms, meshes, UVs and material slots. Start with GLB as the first supported format and add import adapters through the same boundary. Resolve model companions when a supported format needs them. |

Both paths use the same viewport, selection and material assignment workflow. Pick a surface or a material slot, then apply a preview override to that slot or to all slots sharing the selected binding. Display the current assignment and allow the author to restore it. Preview overrides belong to the editing session. Saving a recipe writes its source and presets as reusable content. The original AML or model file remains intact during preview and recipe saves.

Provide live mapping controls alongside recipe parameters. Use existing UVs when suitable, offer object-space projection for models with missing or unsuitable UVs, and use world-space mapping for effects that need it. Preserve model dimensions through import and expose physical scale so the same recipe produces a consistent feature size on an AML wall and an imported test panel.

## Editing and hot reload

Provide source editing with location-linked diagnostics, controls from public parameter declarations, and a live lit preview. Allow channel and intermediate-field inspection on the selected surface. Use the shared asset system, generation runtime and preview renderer. Retain the current model, selection and camera during material edits. On compilation failure, keep the last valid material visible and show the error at the source location. Publish the latest successful revision when its inputs still match the editor state.

Keep the TypeScript recipe as the editable source for the graph inspector.
Associate typed ports and shared fields with source spans and keep layout in
project metadata. Prove a literal edit and a compatible reconnection with
targeted source changes that preserve comments and unrelated expressions.

Track module source and imports, referenced textures, parameter values and any receiver inputs through the shared dependency graph. A source edit recompiles affected modules. Parameter edits reuse compatible compiled code and update bindings or bakes according to their declared dependencies. Identical effective bake requests share cached results. Camera movement changes filtering during rendering. It can reuse compiled material code, textures and receiver mappings. Exporter requests texture bakes or reports unsupported view-dependent effects through its target capability contract.

## GLB export and live rendering

Write the portable material to standard glTF channels in GLB and put the
versioned MaterialSpec operation graph and typed bindings in per-material
`extras.materialSpec`. ItemSpec and WorldSpec resolve this data through the
shared MaterialSpec model. Generic operation IDs carry across recipes.
Individual materials choose connections, presets, and asset bindings.

Ship a Three.js adapter in this milestone. Babylon.js and other engine
adapters are future work. A standard glTF importer uses the baked appearance.
SuperSpec validates the payload, supplies supported live inputs, and reports
when it uses the fallback. See the
[runtime binding and GLB contract](../material-language.md#runtime-bindings-and-glb-delivery).
See the [native material conversion appendix](#appendix-native-material-converters)
for future import tools that save materials in the destination application's format.

## First milestone and acceptance

1. Establish the source compilation contract and a small SDK that can express the concrete, plaster, and steel studies. Verify seeded repeatability, physical scale, and native and Wasm bake agreement.
2. Open a GLB test panel and a courtyard AML scene. Apply the same recipe to a selected slot in both. Exercise the same AML through the Level Viewer and check that it resolves the same authored assets and bindings.
3. Change chip scale in code and adjust a declared parameter. Inspect the chip mask and final material at close range and at a distance. Check that the physical feature size matches across both test surfaces and that filtering responds to the view.
4. Try a source error and a missing dependency, then correct both. Verify that the last valid preview remains available during failure, the corrected revision publishes, and model geometry, selection and camera survive the material edits. Check literal edits and compatible reconnections through the graph inspector.
5. Save and reopen the material source and use it in a WorldSpec theme. Confirm that repeated bindings reuse compatible compiled and baked artifacts. Apply one grime image across differently angled planar receivers and verify that slope edits update the mapping while reusing the image.
6. Compare the concrete, plaster, and steel recipe outputs against frozen LevelSpec views under matched settings. Inspect intermediate masks, roughness, and normals. Exercise the steel lighting controls while orbiting. Validate the GLB and load it with a standard importer to check the baked fallback. Reopen it through SuperSpec and check ItemSpec and WorldSpec bindings and live rendering through Three.js. Verify that changing a live input needs no rebake, an unsupported payload retains the fallback with a diagnostic, and the export report names effects that standard importers skip. Record cold compile time, edit-to-preview time, and jobs reused after an appearance-only edit.

After the first port, verify that the tile group preserves thin joints at coarse
resolution, stable cell variation, isolated missing tiles, and damage across
cells. Compare the reference fields and baked color, normals, and roughness.
Use the same native and browser workflows for this expansion.

## Appendix: Native material converters

An editor plugin could import a GLB with `materials[i].extras.materialSpec` and
save the supported behavior as native assets. The saved material should render,
remain editable through the destination's tools, and respond to supported live
inputs after the MaterialSpec converter is removed. Reimporting a changed GLB
or regenerating the native graph requires the converter again. This is future
work beyond the Three.js milestone.

### Conversion path

1. Import the standard glTF material and use its PBR channels as the fallback.
   Validate the MaterialSpec payload version, operation IDs, typed connections,
   and referenced assets. Resolve the payload for each source material slot.
2. Use a destination-specific capability table to map generic operations to native
   nodes, shader functions, or parameters on a native material template. Check
   the required shading stages, units, and coordinate spaces. Map supported view
   inputs to the destination's active render context. Expose gameplay inputs as
   native parameters or resource slots and report which bindings the project
   still needs to supply.
3. Save the native material and its dependencies in the project or document and
   assign it to the imported slots. Keep shaders, parent materials, node groups,
   and textures available independently of the converter. Record provenance and
   affected material slots so a later conversion can identify its output.
4. Retain the PBR fallback if a required operation, asset, or binding cannot be
   represented. Report the specific gap. Reflection filters that need a
   particular stage of the lighting pipeline may require a separate renderer
   integration. Keep that dependency explicit if such a target is supported.

| Destination | Possible integration | Saved result |
| --- | --- | --- |
| Unreal Engine | Custom Interchange import pipeline or editor conversion action. [Interchange framework](https://dev.epicgames.com/documentation/en-us/unreal-engine/importing-assets-using-interchange-in-unreal-engine) | Material graph or Material Instance with a project-owned parent. Map supported inputs to native material expressions or parameters. [Material instances](https://dev.epicgames.com/documentation/en-us/unreal-engine/creating-and-using-material-instances-in-unreal-engine) |
| Unity | UnityGLTF material hook to capture the payload, then an editor conversion action to persist independent assets. [UnityGLTF import hooks](https://github.com/KhronosGroup/UnityGLTF/blob/main/Runtime/Scripts/Plugins/Core/GltfImportPlugin.cs) | Standalone `.mat` with compatible shader and textures in the project. Choose the target render pipeline explicitly. Save standalone output outside the source import callback. [Native asset creation](https://docs.unity3d.com/ja/current/ScriptReference/AssetDatabase.CreateAsset.html) |
| Blender | glTF material import hook in a Python add-on. [Blender import hooks](https://github.com/KhronosGroup/glTF-Blender-IO/tree/main/example-addons/example_gltf_importer_extension) | Editable native nodes and groups saved in the `.blend`, with referenced or packed images. [Packed resources](https://docs.blender.org/manual/en/3.6/files/blend/packed_data.html) |

Other tools can adopt the same versioned payload and supply their own mappings.
A runtime loader is a separate option for applications that open new MaterialSpec
GLBs after deployment. Its responsibilities include interpreting the payload and
binding live inputs in the running application.

### First proof

Start with one destination and a small GLB that uses a generic grazing-angle
mask with a typed strength parameter. Compare the default import with the
converted material. Disable the converter, reopen the project or document, and
verify that the saved material can be edited and still responds to the active
camera. For a game engine, run a packaged build that uses the converted asset.
Include an unsupported operation to verify fallback and diagnostics. Exercise
the steel reflection effects separately once the destination exposes the
required shading stage.
