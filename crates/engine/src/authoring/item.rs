use super::{Artifact, Diagnostic, Mapping, Output, Receiver, Request, Session};
use crate::{
    itemspec::definition::{self, Collision, CollisionIntent, CollisionShape, ItemDiagnostic},
    runtime::dependencies::{Node, Plan},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_geometry::{
    Matrix, Solid,
    math::{cross, sub, unit},
};
use superspec_model::instances::{IDENTITY, ImageResource, Instance, LocalMesh, Model};
use superspec_types::lod::{GeometryLodGroup, GeometryLodLevel};
use superspec_types::{diagnostic::SourceLocation, material::Material};

fn render_level(model: &Model, distance: f64) -> GeometryLodLevel {
    GeometryLodLevel {
        distance,
        meshes: model
            .instances
            .iter()
            .filter(|instance| instance.visible)
            .map(|instance| instance.id.clone())
            .collect(),
        batches: model
            .batches
            .iter()
            .filter(|batch| batch.visible)
            .map(|batch| batch.id.clone())
            .collect(),
        screen_coverage: None,
    }
}

/// Local render alternatives own their mesh identities; shared material/image IDs
/// retain their meaning and may merge only when their values agree exactly.
fn append_render_level(
    base: &mut Model,
    mut alternate: Model,
    prefix: &str,
    distance: f64,
) -> Result<GeometryLodLevel, String> {
    for (id, material) in &alternate.materials {
        if base
            .materials
            .get(id)
            .is_some_and(|original| original != material)
        {
            return Err(format!("Render alternative changes shared material {id}."));
        }
    }
    for (id, image) in &alternate.images {
        if base.images.get(id).is_some_and(|original| {
            original.mime != image.mime
                || original.bytes != image.bytes
                || original.pixels != image.pixels
        }) {
            return Err(format!("Render alternative changes shared image {id}."));
        }
    }
    let renamed = |id: &str| format!("{prefix}{id}");
    alternate.meshes = alternate
        .meshes
        .into_iter()
        .map(|(id, mesh)| (renamed(&id), mesh))
        .collect();
    alternate.mesh_artifacts = alternate
        .mesh_artifacts
        .into_iter()
        .map(|(id, key)| (renamed(&id), key))
        .collect();
    for instance in &mut alternate.instances {
        instance.id = renamed(&instance.id);
        instance.mesh = renamed(&instance.mesh);
        instance.collidable = false;
    }
    for batch in &mut alternate.batches {
        batch.id = renamed(&batch.id);
        batch.mesh = renamed(&batch.mesh);
        batch.template = batch.template.as_ref().map(|id| renamed(id));
        batch.collision = superspec_model::batches::Collision::None;
    }
    alternate.decals = alternate
        .decals
        .into_iter()
        .map(|id| renamed(&id))
        .collect();
    let existing_instances: BTreeSet<_> = base
        .instances
        .iter()
        .map(|instance| instance.id.as_str())
        .collect();
    let existing_batches: BTreeSet<_> =
        base.batches.iter().map(|batch| batch.id.as_str()).collect();
    if alternate
        .meshes
        .keys()
        .any(|id| base.meshes.contains_key(id))
        || alternate
            .instances
            .iter()
            .any(|instance| existing_instances.contains(instance.id.as_str()))
        || alternate
            .batches
            .iter()
            .any(|batch| existing_batches.contains(batch.id.as_str()))
    {
        return Err(format!(
            "Render alternative identity prefix {prefix} conflicts with existing geometry."
        ));
    }
    let level = render_level(&alternate, distance);
    base.meshes.extend(alternate.meshes);
    base.mesh_artifacts.extend(alternate.mesh_artifacts);
    base.instances.extend(alternate.instances);
    base.batches.extend(alternate.batches);
    base.materials.extend(alternate.materials);
    base.images.extend(alternate.images);
    base.decals.extend(alternate.decals);
    Ok(level)
}

impl From<ItemDiagnostic> for Diagnostic {
    fn from(value: ItemDiagnostic) -> Self {
        Self {
            code: value.code,
            source_name: value.source,
            line: value.line.unwrap_or(1),
            column: value.column.unwrap_or(1),
            offset: value.offset.unwrap_or(0),
            end: value.end,
            message: format!("{} {}", value.pointer, value.message),
        }
    }
}
fn matrix(value: Matrix) -> [f64; 16] {
    let m = value.0;
    [
        m[0][0], m[0][1], m[0][2], 0., m[1][0], m[1][1], m[1][2], 0., m[2][0], m[2][1], m[2][2],
        0., m[3][0], m[3][1], m[3][2], 1.,
    ]
}
fn location() -> SourceLocation {
    SourceLocation {
        line: 1,
        column: 1,
        offset: 0,
        end: None,
    }
}

fn catalog_entry(material: &crate::materialspec::program::MaterialRequest) -> Option<&str> {
    material
        .entry
        .starts_with("builtin:")
        .then_some(material.entry.as_str())
}

impl Session {
    /// Bind the catalog's material and all its texture companions to the item model.
    fn bind_catalog_slot(
        &self,
        catalog: &str,
        material_id: &str,
        model: &mut Model,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
    ) -> Result<(), String> {
        if model.materials.contains_key(material_id) {
            return Ok(());
        }
        let mut material = superspec_catalog::material(catalog)
            .ok_or_else(|| format!("Unknown shipped material {catalog}."))?;
        for texture in material.companions() {
            if model.images.contains_key(texture) {
                continue;
            }
            let bytes = assets(texture).map_err(|error| format!("{texture}: {error}"))?;
            let decoded = self.validate_image(&bytes)?;
            model.images.insert(
                texture.to_owned(),
                ImageResource {
                    mime: decoded.mime,
                    bytes,
                    pixels: decoded.width * decoded.height,
                },
            );
        }
        material.id = material_id.to_owned();
        model.materials.insert(material.id.clone(), material);
        Ok(())
    }

    /// Publish embedded textures under content IDs in both single-item and project scenes.
    fn import_resources(
        &self,
        generated: &definition::GeneratedItem,
        model: &mut Model,
        select: impl Fn(&str, &definition::ImportedMaterial) -> bool,
    ) -> Result<BTreeMap<String, String>, String> {
        let mut image_ids = BTreeMap::new();
        let mut material_ids = BTreeMap::new();
        for image in &generated.geometry.images {
            let decoded = self.validate_image(&image.bytes)?;
            let id = format!("studio/import/image/{:x}", Sha256::digest(&image.bytes));
            image_ids.insert(image.id.clone(), id.clone());
            model.images.insert(
                id,
                ImageResource {
                    mime: decoded.mime,
                    bytes: image.bytes.clone(),
                    pixels: decoded.width * decoded.height,
                },
            );
        }
        for (id, material) in &generated.geometry.imported_materials {
            if !select(id, material)
                || !generated
                    .geometry
                    .nodes
                    .iter()
                    .flat_map(|node| &node.meshes)
                    .any(|index| generated.geometry.meshes[*index].material_slot == *id)
            {
                continue;
            }
            material.ensure_scene_compatible()?;
            let mut material = material.material();
            for image in [
                &mut material.texture,
                &mut material.normal_texture,
                &mut material.metallic_roughness_texture,
                &mut material.height_texture,
                &mut material.emissive_texture,
                &mut material.occlusion_texture,
            ] {
                if let Some(name) = image {
                    *name = image_ids
                        .get(name)
                        .ok_or_else(|| {
                            format!("Imported material references unknown image {name}.")
                        })?
                        .clone();
                }
            }
            material.id.clear();
            material.id = format!(
                "studio/import/material/{:x}",
                Sha256::digest(serde_json::to_vec(&material).unwrap())
            );
            material_ids.insert(id.clone(), material.id.clone());
            model.materials.insert(material.id.clone(), material);
        }
        Ok(material_ids)
    }
    pub(super) fn item(
        &mut self,
        request: &Request,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Output, Vec<Diagnostic>> {
        let source = if request.kind == "import-glb" {
            json!({"format":"itemspec","version":1,"id":"imported-model","generator":{"kind":"glb","asset":request.entry},"collision":"none"}).to_string()
        } else {
            request.source.clone()
        };
        let item_failure = |mut error: ItemDiagnostic| {
            if error.source == request.entry {
                definition::locate_diagnostic(&source, &mut error);
            }
            vec![Diagnostic::from(error)]
        };
        let definition = definition::parse(&request.entry, &source).map_err(&item_failure)?;
        let mut environment = definition::Environment::new(assets)
            .with_natives(crate::worldspec::shipped::item_generators());
        let prepared = definition::prepare_in(
            &definition,
            &request.parameters,
            &mut environment,
            cancelled,
        )
        .map_err(&item_failure)?;
        let fail = |message: String| vec![Diagnostic::message(&request.entry, message)];
        let plan = Plan::new(
            vec![Node {
                id: "item-geometry".into(),
                implementation: "itemspec-geometry-v1".into(),
                inputs: prepared.geometry_key.as_bytes().into(),
                dependencies: vec![],
                source: None,
            }],
            vec!["item-geometry".into()],
        )
        .map_err(|e| fail(e.to_string()))?;
        let mut item_error = None;
        let result = self
            .runtime
            .run(
                self.build_epoch(),
                &plan,
                cancelled,
                |_, _, stopped| match prepared.build(stopped) {
                    Ok(item) => {
                        let bytes = item.retained_bytes();
                        Ok((Artifact::Item(Arc::new(item)), bytes))
                    }
                    Err(error) => {
                        let message = error.to_string();
                        item_error = item_failure(error).pop();
                        Err(message)
                    }
                },
            )
            .map_err(|e| {
                item_error
                    .clone()
                    .map(|e| vec![e])
                    .unwrap_or_else(|| fail(e))
            })?;
        let Artifact::Item(cached) = result["item-geometry"].as_ref() else {
            unreachable!()
        };
        let generated = prepared
            .reuse(cached)
            .and_then(|item| item.with_material_overrides(&request.slot_overrides))
            .map_err(&item_failure)?;
        let placement = Matrix(
            request
                .placement_transform
                .unwrap_or_else(definition::identity),
        )
        .checked()
        .and_then(|matrix| matrix.inverse().map(|_| matrix))
        .map_err(&fail)?;
        let mut model = Model {
            id: format!("studio/item/{}", generated.id),
            name: generated.id.clone(),
            ..Default::default()
        };
        let mut material_ids = self
            .import_resources(&generated, &mut model, |id, _| {
                !generated.materials.contains_key(id)
            })
            .map_err(&fail)?;
        let imported = matches!(definition.generator, definition::Generator::Glb { .. });
        let selected_meshes: std::collections::BTreeSet<_> = generated
            .geometry
            .nodes
            .iter()
            .flat_map(|node| node.meshes.iter().copied())
            .collect();
        for (index, mesh) in generated.geometry.meshes.iter().enumerate() {
            if !selected_meshes.contains(&index) {
                continue;
            }
            if mesh.colors.iter().any(|rgba| rgba[3] != 1.) {
                return Err(fail(
                    "Per-vertex alpha is not supported by the scene delivery adapter.".into(),
                ));
            }
            let uvs = if mesh.uvs.is_empty() {
                vec![[0., 0.]; mesh.positions.len()]
            } else if imported {
                mesh.uvs.iter().map(|uv| [uv[0], 1. - uv[1]]).collect()
            } else {
                mesh.uvs.clone()
            };
            let tangents = if imported
                && (!mesh.tangents.is_empty()
                    || model
                        .materials
                        .get(
                            material_ids
                                .get(&mesh.material_slot)
                                .unwrap_or(&mesh.material_slot),
                        )
                        .is_some_and(|material| material.normal_texture.is_some()))
            {
                definition::delivery_tangents(mesh).map_err(&fail)?
            } else {
                mesh.tangents.clone()
            };
            let local = LocalMesh {
                positions: mesh.positions.clone(),
                normals: mesh.normals.clone(),
                uvs,
                colors: if mesh.colors.is_empty() {
                    vec![[1.; 3]; mesh.positions.len()]
                } else {
                    mesh.colors.iter().map(|c| [c[0], c[1], c[2]]).collect()
                },
                indices: mesh.indices.clone(),
                tangents,
            };
            model.mesh_artifacts.insert(
                mesh.id.clone(),
                format!(
                    "itemspec-delivery-v1/{}/{}/tangents-{}",
                    generated.geometry_key,
                    mesh.id,
                    !local.tangents.is_empty()
                ),
            );
            model.meshes.insert(mesh.id.clone(), Arc::new(local));
            if let Some(catalog) = generated
                .materials
                .get(&mesh.material_slot)
                .and_then(catalog_entry)
            {
                self.bind_catalog_slot(catalog, catalog, &mut model, assets)
                    .map_err(&fail)?;
                material_ids.insert(mesh.material_slot.clone(), catalog.to_owned());
            } else if !material_ids.contains_key(&mesh.material_slot) {
                material_ids.insert(mesh.material_slot.clone(), mesh.material_slot.clone());
                model.materials.insert(
                    mesh.material_slot.clone(),
                    Material {
                        id: mesh.material_slot.clone(),
                        ..Default::default()
                    },
                );
            }
        }
        let transforms = generated.geometry.world_transforms().map_err(&fail)?;
        let mut receiver_transforms = BTreeMap::new();
        let mut mesh_slots = BTreeMap::new();
        // Keep the first part/anchor in definition order, as in ItemSpec lookup.
        let parts_by_node: BTreeMap<_, _> = generated
            .geometry
            .parts
            .iter()
            .rev()
            .map(|p| (p.node, p))
            .collect();
        let anchors_by_part: BTreeMap<_, _> = generated
            .geometry
            .anchors
            .iter()
            .rev()
            .map(|a| (a.part.as_str(), a))
            .collect();
        let mut anchors = request.anchors.clone();
        for (index, (node, transform)) in
            generated.geometry.nodes.iter().zip(&transforms).enumerate()
        {
            let anchor = parts_by_node
                .get(&index)
                .and_then(|part| anchors_by_part.get(part.id.as_str()))
                .map(|anchor| super::Anchor {
                    origin: placement.point(anchor.origin),
                    u: placement.direction(anchor.u),
                    v: placement.direction(anchor.v),
                });
            for &index in &node.meshes {
                let mesh = &generated.geometry.meshes[index];
                let id = format!("{}/{}", node.id, mesh.id);
                let transform = matrix(placement.then(*transform));
                if let Some(anchor) = anchor {
                    anchors.entry(id.clone()).or_insert(anchor);
                }
                receiver_transforms.insert(id.clone(), transform);
                mesh_slots.insert(id.clone(), mesh.material_slot.clone());
                model.instances.push(Instance {
                    id,
                    object_id: generated.id.clone(),
                    mesh: mesh.id.clone(),
                    material: Some(material_ids[&mesh.material_slot].clone()),
                    transform,
                    source: location(),
                    visible: true,
                    collidable: matches!(
                        generated.collision,
                        Collision::Intent(CollisionIntent::Mesh)
                    ),
                });
            }
        }
        collision(&mut model, &generated, placement).map_err(&fail)?;
        // Shape drafts keep imported and catalog finishes. Procedural receiver maps
        // are evaluated by the full build, which reuses this exact geometry node.
        if request.kind != "item-geometry" && !generated.materials.is_empty() {
            let scene = model.scene(revision as u32).map_err(&fail)?;
            for (slot, material) in &generated.materials {
                if catalog_entry(material).is_some() {
                    continue;
                }
                let receivers: Vec<_> = scene
                    .manifest
                    .meshes
                    .iter()
                    .filter(|mesh| mesh.visible && mesh_slots.get(&mesh.id) == Some(slot))
                    .map(|mesh| Receiver {
                        id: mesh.id.clone(),
                        object_id: mesh.object_id.clone(),
                        positions: scene.positions
                            [mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3]
                            .iter()
                            .map(|v| *v as f64)
                            .collect(),
                        normals: scene.normals
                            [mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3]
                            .iter()
                            .map(|v| *v as f64)
                            .collect(),
                        uvs: scene.uvs
                            [mesh.vertex_start * 2..(mesh.vertex_start + mesh.vertex_count) * 2]
                            .iter()
                            .map(|v| *v as f64)
                            .collect(),
                        indices: scene.indices
                            [mesh.index_start..mesh.index_start + mesh.index_count]
                            .to_vec(),
                        object_to_world: receiver_transforms.get(&mesh.id).copied(),
                        contacts: Vec::new(),
                    })
                    .collect();
                let source = request.modules.get(&material.entry).ok_or_else(|| {
                    fail(format!(
                        "Material module {} was not supplied.",
                        material.entry
                    ))
                })?;
                let material_request = Request {
                    kind: "material".into(),
                    entry: material.entry.clone(),
                    source: source.clone(),
                    parameters: material.parameters.clone(),
                    seed: material.seed,
                    receivers,
                    // Object space unless the host asks for the parts' pattern anchors.
                    mapping: if request.mapping == Mapping::Anchored {
                        Mapping::Anchored
                    } else {
                        Mapping::Object
                    },
                    anchors: anchors.clone(),
                    ..request.clone()
                };
                let baked = self.material(&material_request, revision, assets, cancelled)?;
                model.meshes.extend(baked.model.meshes);
                model.materials.extend(baked.model.materials);
                model.images.extend(baked.model.images);
                for replacement in baked.model.instances {
                    let existing = model
                        .instances
                        .iter_mut()
                        .find(|instance| instance.id == replacement.id)
                        .ok_or_else(|| {
                            fail("Baked item receiver has no matching placement.".into())
                        })?;
                    existing.mesh = replacement.mesh;
                    existing.material = replacement.material;
                    existing.transform = IDENTITY;
                }
            }
        }
        let parameters: Vec<_> = definition
            .parameters
            .iter()
            .map(|(name, parameter)| {
                let mut value = serde_json::to_value(parameter).unwrap();
                value["name"] = json!(name);
                value
            })
            .collect();
        let collision_name = match &generated.collision {
            Collision::Intent(CollisionIntent::None) => "none",
            Collision::Intent(CollisionIntent::Mesh) => "mesh",
            Collision::Intent(CollisionIntent::Bounds) => "bounds",
            Collision::Shape(CollisionShape::Box { .. }) => "box",
            Collision::Shape(CollisionShape::Sphere { .. }) => "sphere",
        };
        let attachments = generated
            .attachments
            .iter()
            .map(|attachment| {
                Ok(definition::Attachment {
                    name: attachment.name.clone(),
                    position: placement.point(attachment.position),
                    direction: unit(placement.direction(attachment.direction))?,
                })
            })
            .collect::<Result<Vec<_>, String>>()
            .map_err(&fail)?;
        if !definition.lods.is_empty() {
            let mut levels = vec![render_level(&model, 0.)];
            for (index, level) in definition.lods.iter().enumerate() {
                let (alternative, parameters) =
                    definition.render_alternative(level, &request.parameters);
                let mut alternate_request = request.clone();
                alternate_request.source =
                    serde_json::to_string(&alternative).map_err(|error| fail(error.to_string()))?;
                alternate_request.parameters = parameters;
                let alternate = self.item(&alternate_request, revision, assets, cancelled)?;
                let prefix = format!("lod{}/", index + 1);
                let transforms: BTreeMap<String, [f64; 16]> =
                    serde_json::from_value(alternate.metadata["receiverTransforms"].clone())
                        .map_err(|error| fail(error.to_string()))?;
                receiver_transforms.extend(
                    transforms
                        .into_iter()
                        .map(|(id, t)| (format!("{prefix}{id}"), t)),
                );
                levels.push(
                    append_render_level(&mut model, alternate.model, &prefix, level.distance)
                        .map_err(&fail)?,
                );
            }
            model.geometry_lods.push(GeometryLodGroup {
                id: format!("{}/lod", generated.id),
                levels,
            });
        }
        Ok(Output {
            model,
            metadata: json!({"parameters":parameters,"attachments":attachments,"collision":collision_name,"materials":generated.materials,"meshSlots":mesh_slots,"receiverTransforms":receiver_transforms,"geometryKey":generated.geometry_key,"provenance":generated.provenance,"dependencies":generated.provenance.assets.keys().collect::<Vec<_>>()}),
        })
    }
}

/// What a session keeps of an item a scene project placed, by model identity.
pub(crate) struct PlacedItemState {
    item: Arc<definition::GeneratedItem>,
    /// Counts the times any part was generated.
    generation: u64,
    /// The parts generated the last time the geometry changed.
    rebuilt: Vec<String>,
}

/// Features of wider reach one receiver is given at most, from the parts of its item other
/// than its own and, counted apart, from the other items of its scene: the nearest are kept
/// and the number left out is reported (`reachDropped` in the item's metadata).
pub const MAX_REACHED_FEATURES: usize = 256;

/// A feature that reaches beyond its part (`itemspec::definition::Reach`), in world meters,
/// with the box around it: a receiver is given it when its own box comes within `within`.
#[derive(Clone)]
pub(super) struct Reaching {
    feature: crate::materialspec::program::NamedFeature,
    /// The part of its item it belongs to, whose receivers have it already.
    part: String,
    within: f64,
    bounds: [[f64; 3]; 2],
}

impl Reaching {
    fn new(feature: &definition::Feature, world: &[f64; 16]) -> Self {
        let named = super::item_features(std::slice::from_ref(feature), world).remove(0);
        Self {
            bounds: bounds_of(named.points.iter().copied()),
            feature: named,
            part: feature.part.clone(),
            within: feature.within,
        }
    }
    /// How far the box of a receiver is from the feature's, when within its reach.
    fn gap(&self, receiver: &[[f64; 3]; 2]) -> Option<f64> {
        let gap = (0..3)
            .map(|axis| {
                (self.bounds[0][axis] - receiver[1][axis])
                    .max(receiver[0][axis] - self.bounds[1][axis])
                    .max(0.)
                    .powi(2)
            })
            .sum::<f64>()
            .sqrt();
        (gap <= self.within).then_some(gap)
    }
}

fn bounds_of(points: impl Iterator<Item = [f64; 3]>) -> [[f64; 3]; 2] {
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for point in points {
        for axis in 0..3 {
            bounds[0][axis] = bounds[0][axis].min(point[axis]);
            bounds[1][axis] = bounds[1][axis].max(point[axis]);
        }
    }
    bounds
}

/// The features among `reaching` that a receiver with these bounds is given, nearest first and
/// at most [`MAX_REACHED_FEATURES`] (ties keep their order), and how many were left out.
/// `own` is the receiver's part, whose features it has already; `None` takes every one.
pub(super) fn reached(
    reaching: &[Reaching],
    own: Option<&str>,
    receiver: &[[f64; 3]; 2],
) -> (Vec<crate::materialspec::program::NamedFeature>, usize) {
    let mut near: Vec<(f64, &Reaching)> = reaching
        .iter()
        .filter(|feature| own != Some(feature.part.as_str()))
        .filter_map(|feature| Some((feature.gap(receiver)?, feature)))
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let dropped = near.len().saturating_sub(MAX_REACHED_FEATURES);
    near.truncate(MAX_REACHED_FEATURES);
    (
        near.into_iter()
            .map(|(_, reaching)| reaching.feature.clone())
            .collect(),
        dropped,
    )
}

/// One item of a scene project, ready to compose: its meshes as placements of local
/// resources, and what its owner knows about each of them.
pub(super) struct PlacedItem {
    /// Placements under the item's own identities; the composer prefixes them.
    pub model: Model,
    /// The item slot of each visible mesh, by placement identity.
    pub slots: BTreeMap<String, String>,
    /// Object-to-world transform of each mesh.
    pub transforms: BTreeMap<String, [f64; 16]>,
    /// The named features of each mesh's part, in world meters.
    pub features: BTreeMap<String, Vec<crate::materialspec::program::NamedFeature>>,
    /// The pattern anchor of each mesh's part, in world meters.
    pub anchors: BTreeMap<String, definition::PatternAnchor>,
    /// The item's weathering layers per item slot, lowest first.
    pub layers: BTreeMap<String, Vec<definition::LayerRequest>>,
    /// The texels per meter the item asks for per item slot (its definition's `densities`,
    /// merged through its sub-items): a default under whatever the project states.
    pub densities: BTreeMap<String, f64>,
    /// World bounds of each mesh, low corner then high.
    pub bounds: BTreeMap<String, [[f64; 3]; 2]>,
    /// The item's features that reach the receivers of other items of the scene.
    pub scene_features: Vec<Reaching>,
    /// Per entry of the metadata's `placements`, in order: the prototype slot and the
    /// placement identity of each mesh it covers, for the composer to state what each is bound to.
    pub placements: Vec<Vec<(String, String)>>,
    /// For inspection and overlays: controls, derived values, parts with their keys,
    /// features, anchors, attachments, decals and handles, in world meters; and the
    /// prototypes the item places with their placements, in the item's own coordinates.
    pub metadata: serde_json::Value,
}

impl Session {
    /// Generates an item model of a scene project (`kind: "item"`) under its placement.
    /// Parts whose geometry key is unchanged since the session last placed this model keep
    /// their meshes. `prefix` is put before the mesh identities the metadata names.
    pub(super) fn place_item(
        &mut self,
        project: &super::Project,
        model: &super::ModelSource,
        prefix: &str,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PlacedItem, Vec<Diagnostic>> {
        let entry = model
            .entry
            .clone()
            .unwrap_or_else(|| format!("{}.item.json", model.id));
        let fail = |message: String| vec![Diagnostic::message(&entry, message)];
        let source = match (&model.source, project.modules.get(&entry)) {
            (Some(source), _) => source,
            (None, Some(source)) => source,
            (None, None) => {
                return Err(fail(format!(
                    "Model {} names no item definition among the project's modules.",
                    model.id
                )));
            }
        };
        let item_failure = |mut error: ItemDiagnostic| {
            if error.source == entry {
                definition::locate_diagnostic(source, &mut error);
            }
            vec![Diagnostic::from(error)]
        };
        let parsed = definition::parse(&entry, source).map_err(&item_failure)?;
        if matches!(parsed.generator, definition::Generator::Glb { .. }) {
            return Err(fail(
                "A scene places imported models with kind glb, not through an item.".into(),
            ));
        }
        // Sub-item entries name the project's modules first, then its assets.
        let modules = |path: &str| -> Result<Arc<[u8]>, String> {
            match project.modules.get(path) {
                Some(text) => Ok(Arc::from(text.as_bytes())),
                None => assets(path),
            }
        };
        let mut environment = definition::Environment::new(&modules)
            .with_natives(crate::worldspec::shipped::item_generators());
        let prepared =
            definition::prepare_in(&parsed, &model.parameters, &mut environment, cancelled)
                .map_err(&item_failure)?;
        let previous = self
            .placed_items
            .borrow()
            .get(&model.id)
            .map(|state| (state.item.clone(), state.generation, state.rebuilt.clone()));
        let (generated, generation, rebuilt) = match previous {
            Some((item, generation, rebuilt)) if item.geometry_key == prepared.geometry_key => (
                prepared.reuse(&item).map_err(&item_failure)?,
                generation,
                rebuilt,
            ),
            Some((item, generation, _)) => {
                let (item, built) = prepared.rebuild(&item, cancelled).map_err(&item_failure)?;
                (item, generation + 1, built)
            }
            None => {
                let item = prepared.build(cancelled).map_err(&item_failure)?;
                let built = if prepared.part_keys.is_empty() {
                    vec!["item".to_owned()]
                } else {
                    prepared
                        .part_keys
                        .iter()
                        .map(|(id, _)| id.clone())
                        .collect()
                };
                (item, 1, built)
            }
        };
        {
            let mut kept = self.placed_items.borrow_mut();
            if kept.len() >= 64 && !kept.contains_key(&model.id) {
                kept.clear();
            }
            kept.insert(
                model.id.clone(),
                PlacedItemState {
                    item: Arc::new(generated.clone()),
                    generation,
                    rebuilt: rebuilt.clone(),
                },
            );
        }
        let placement = Matrix(model.transform.unwrap_or_else(definition::identity))
            .checked()
            .and_then(|matrix| matrix.inverse().map(|_| matrix))
            .map_err(&fail)?;
        let world = matrix(placement);
        let geometry = &generated.geometry;
        let mut placed = PlacedItem {
            model: Model {
                id: format!("studio/item/{}", generated.id),
                name: generated.id.clone(),
                ..Default::default()
            },
            slots: BTreeMap::new(),
            transforms: BTreeMap::new(),
            features: BTreeMap::new(),
            anchors: BTreeMap::new(),
            layers: generated.layers.clone(),
            densities: generated.densities.clone(),
            bounds: BTreeMap::new(),
            scene_features: Vec::new(),
            placements: Vec::new(),
            metadata: serde_json::Value::Null,
        };
        let textured_slots = self
            .import_resources(&generated, &mut placed.model, |slot, material| {
                !matches!(slot, "lettering" | "board") && !material.textures.is_empty()
            })
            .map_err(&fail)?;
        // Features that reach beyond their part: every receiver of the item near enough is
        // given them, and those of scene reach are handed on for the other items' receivers.
        let reaching: Vec<Reaching> = geometry
            .features
            .iter()
            .filter(|feature| !feature.reach.is_part())
            .map(|feature| Reaching::new(feature, &world))
            .collect();
        placed.scene_features = geometry
            .features
            .iter()
            .filter(|feature| feature.reach == definition::Reach::Scene)
            .map(|feature| Reaching::new(feature, &world))
            .collect();
        let mut reach_dropped = 0;
        let anchor = |anchor: &definition::PatternAnchor| -> Result<_, String> {
            let inverse = placement.inverse()?;
            Ok(definition::PatternAnchor {
                part: anchor.part.clone(),
                origin: placement.point(anchor.origin),
                u: placement.direction(anchor.u),
                v: placement.direction(anchor.v),
                normal: unit([
                    superspec_geometry::math::dot(inverse.0[0], anchor.normal),
                    superspec_geometry::math::dot(inverse.0[1], anchor.normal),
                    superspec_geometry::math::dot(inverse.0[2], anchor.normal),
                ])?,
            })
        };
        let parts_by_node: BTreeMap<_, _> =
            geometry.parts.iter().rev().map(|p| (p.node, p)).collect();
        let anchors_by_part: BTreeMap<_, _> = geometry
            .anchors
            .iter()
            .rev()
            .map(|a| (a.part.as_str(), a))
            .collect();
        let mut features_by_part = BTreeMap::<_, Vec<_>>::new();
        for (feature, placed_feature) in geometry
            .features
            .iter()
            .zip(super::item_features(&geometry.features, &world))
        {
            features_by_part
                .entry(feature.part.as_str())
                .or_default()
                .push(placed_feature);
        }
        let transforms = geometry.world_transforms().map_err(&fail)?;
        let mut parts = Vec::new();
        for (index, (node, transform)) in geometry.nodes.iter().zip(&transforms).enumerate() {
            let (part, key) = parts_by_node
                .get(&index)
                .map(|p| (p.id.as_str(), p.geometry_key.as_str()))
                .unwrap_or(("item", generated.geometry_key.as_str()));
            let features = features_by_part
                .get(part)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let part_anchor = anchors_by_part
                .get(part)
                .copied()
                .map(&anchor)
                .transpose()
                .map_err(&fail)?;
            let mut meshes = Vec::new();
            for &mesh_index in &node.meshes {
                let mesh = &geometry.meshes[mesh_index];
                let count = mesh.positions.len();
                // The resource is named by its part's key, so an unchanged part shares its
                // meshes with every later build and placement.
                let resource = format!("{key}/{}", mesh.id);
                placed.model.mesh_artifacts.insert(
                    resource.clone(),
                    format!("itemspec-part-v1/{key}/{}", mesh.id),
                );
                placed.model.meshes.insert(
                    resource.clone(),
                    Arc::new(LocalMesh {
                        positions: mesh.positions.clone(),
                        normals: mesh.normals.clone(),
                        uvs: if mesh.uvs.is_empty() {
                            vec![[0., 0.]; count]
                        } else {
                            mesh.uvs.clone()
                        },
                        // A fixture's tints (a sign's board and ink) come through for the slots a
                        // project leaves plain.
                        colors: if mesh.colors.len() == count {
                            mesh.colors.iter().map(|c| [c[0], c[1], c[2]]).collect()
                        } else {
                            vec![[1.; 3]; count]
                        },
                        indices: mesh.indices.clone(),
                        tangents: mesh.tangents.clone(),
                    }),
                );
                let catalog = generated
                    .materials
                    .get(&mesh.material_slot)
                    .and_then(catalog_entry)
                    .filter(|_| !model.slots.contains_key(&mesh.material_slot));
                let material = catalog
                    .map(str::to_owned)
                    .or_else(|| textured_slots.get(&mesh.material_slot).cloned())
                    .unwrap_or_else(|| format!("studio/item/slot/{}", mesh.material_slot));
                // A sign's slots are the shipped catalog materials (the glyph atlas and the
                // board). Keep their existing local aliases.
                let catalog = catalog.or(match mesh.material_slot.as_str() {
                    "lettering" => Some("builtin:lettering"),
                    "board" => Some("builtin:signboard"),
                    _ => None,
                });
                if let Some(catalog) = catalog {
                    self.bind_catalog_slot(catalog, &material, &mut placed.model, assets)
                        .map_err(&fail)?;
                }
                placed
                    .model
                    .materials
                    .entry(material.clone())
                    .or_insert_with(|| {
                        let mut value = geometry
                            .imported_materials
                            .get(&mesh.material_slot)
                            .map(|imported| imported.material())
                            .unwrap_or_default();
                        value.id = material.clone();
                        value
                    });
                let id = format!("{}/{}", node.id, mesh.id);
                let transform = matrix(placement.then(*transform));
                placed.model.instances.push(Instance {
                    id: id.clone(),
                    object_id: generated.id.clone(),
                    mesh: resource,
                    material: Some(material),
                    transform,
                    source: location(),
                    visible: true,
                    collidable: matches!(
                        generated.collision,
                        Collision::Intent(CollisionIntent::Mesh)
                    ),
                });
                placed.transforms.insert(id.clone(), transform);
                placed.slots.insert(id.clone(), mesh.material_slot.clone());
                let bounds = bounds_of(mesh.positions.iter().map(|p| {
                    std::array::from_fn(|row| {
                        transform[row] * p[0]
                            + transform[4 + row] * p[1]
                            + transform[8 + row] * p[2]
                            + transform[12 + row]
                    })
                }));
                let mut features = features.to_vec();
                if !reaching.is_empty() {
                    let (near, dropped) = reached(&reaching, Some(part), &bounds);
                    features.extend(near);
                    reach_dropped += dropped;
                }
                placed.bounds.insert(id.clone(), bounds);
                if !features.is_empty() {
                    placed.features.insert(id.clone(), features);
                }
                if let Some(anchor) = &part_anchor {
                    placed.anchors.insert(id.clone(), anchor.clone());
                }
                meshes.push(format!("{prefix}{id}"));
            }
            parts.push(json!({
                "id": part, "key": key, "meshes": meshes,
                "rebuilt": rebuilt.iter().any(|built| built == part),
                "anchor": part_anchor,
            }));
        }
        collision(&mut placed.model, &generated, placement).map_err(&fail)?;
        // Decals: one image resource per image, however many placements show it; each
        // placement is a render-only mesh of its own.
        let mut decals = Vec::new();
        for decal in &generated.decals {
            let bytes = assets(&decal.image).map_err(|error| {
                fail(format!("Decal {} image {}: {error}", decal.id, decal.image))
            })?;
            let decoded = self.validate_image(&bytes).map_err(&fail)?;
            let digest = Sha256::digest(&bytes);
            let image_id = format!("studio/decal/image/{digest:x}");
            let material_id = format!("studio/decal/material/{digest:x}");
            placed
                .model
                .images
                .entry(image_id.clone())
                .or_insert(ImageResource {
                    mime: decoded.mime,
                    bytes: bytes.clone(),
                    pixels: decoded.width * decoded.height,
                });
            placed
                .model
                .materials
                .entry(material_id.clone())
                .or_insert_with(|| Material {
                    id: material_id.clone(),
                    color: "#ffffff".into(),
                    roughness: 0.9,
                    texture: Some(image_id),
                    opacity: Some(1.),
                    alpha_mode: Some(superspec_types::material::AlphaMode::Blend),
                    double_sided: Some(false),
                    ..Default::default()
                });
            let id = format!("decal/{}", decal.id);
            placed.model.meshes.insert(
                id.clone(),
                Arc::new(LocalMesh {
                    positions: decal.positions.clone(),
                    normals: decal.normals.clone(),
                    uvs: decal.uvs.clone(),
                    colors: vec![[1.; 3]; decal.positions.len()],
                    indices: decal.indices.clone(),
                    tangents: vec![],
                }),
            );
            placed.model.instances.push(Instance {
                id: id.clone(),
                object_id: generated.id.clone(),
                mesh: id.clone(),
                material: Some(material_id),
                transform: world,
                source: location(),
                visible: true,
                collidable: false,
            });
            placed.model.decals.insert(id.clone());
            decals.push(json!({
                "id": decal.id, "part": decal.part, "face": decal.face, "image": decal.image,
                "layer": decal.layer, "size": decal.size, "offset": decal.offset,
                "mesh": format!("{prefix}{id}"),
                "corners": decal.corners.map(|corner| placement.point(corner)),
            }));
        }
        let parameters: Vec<_> = parsed
            .parameters
            .iter()
            .map(|(name, parameter)| {
                let mut value = serde_json::to_value(parameter).unwrap();
                value["name"] = json!(name);
                value["value"] = prepared.parameters[name].clone();
                value
            })
            .collect();
        let mut features = Vec::new();
        for feature in &geometry.features {
            let mut feature = feature.clone();
            for point in &mut feature.points {
                *point = placement.point(*point);
            }
            feature.normal = unit(placement.direction(feature.normal)).map_err(&fail)?;
            features.push(feature);
        }
        let mut attachments = Vec::new();
        for attachment in &generated.attachments {
            attachments.push(definition::Attachment {
                name: attachment.name.clone(),
                position: placement.point(attachment.position),
                direction: unit(placement.direction(attachment.direction)).map_err(&fail)?,
            });
        }
        let mut handles = Vec::new();
        for handle in &generated.handles {
            let mut handle = handle.clone();
            // A handle's axis keeps its length under the placement: a scaled item moves a
            // dimension further for the same change of its control.
            let axis = placement.direction(handle.axis);
            handle.scale /= superspec_geometry::math::length(axis).max(1e-12);
            handle.axis = unit(axis).map_err(&fail)?;
            handle.position = placement.point(handle.position);
            handles.push(handle);
        }
        // The prototypes the item places (`docs/plans/parts-as-placements.md`, 8.2): each shared
        // shape by its content key, and each placement in the item's own coordinates, whatever
        // the model's placement, naming the meshes it covers as the scene names them.
        let prototypes: serde_json::Map<_, _> = geometry
            .prototypes
            .iter()
            .map(|(key, prototype)| {
                let slots: Vec<&str> = prototype
                    .meshes
                    .iter()
                    .map(|mesh| mesh.material_slot.as_str())
                    .collect();
                let triangles: usize = prototype
                    .meshes
                    .iter()
                    .map(|mesh| mesh.indices.len() / 3)
                    .sum();
                let mut value = json!({
                    "kind": prototype.kind, "name": prototype.name,
                    "identity": prototype.identity, "content": prototype.content,
                    "mirrored": prototype.mirrored, "roles": prototype.roles,
                    "mount": prototype.mount, "slots": slots, "triangles": triangles,
                });
                if let Some(group) = crate::worldspec::shipped::prototype_lod_group(prototype) {
                    value["lodGroup"] = json!(group);
                }
                (key.clone(), value)
            })
            .collect();
        let mut placements = Vec::new();
        for placement in geometry.placed().map_err(&fail)? {
            let (Some(node), Some(prototype)) = (
                geometry.nodes.get(placement.node),
                geometry.prototypes.get(&placement.prototype),
            ) else {
                return Err(fail(format!(
                    "Placement {} names an unknown node or prototype.",
                    placement.path
                )));
            };
            let mut meshes = Vec::with_capacity(placement.meshes.len());
            // The prototype's surviving meshes, in its mesh order, are the covered ones.
            let mut covered = Vec::with_capacity(placement.meshes.len());
            let surviving = prototype
                .meshes
                .iter()
                .filter(|mesh| placement.slots.contains_key(&mesh.material_slot));
            for (source, &index) in surviving.zip(&placement.meshes) {
                let mesh = geometry
                    .meshes
                    .get(index)
                    .ok_or_else(|| fail(format!("Placement {} names no mesh.", placement.path)))?;
                let id = format!("{}/{}", node.id, mesh.id);
                meshes.push(format!("{prefix}{id}"));
                covered.push((source.material_slot.clone(), id));
            }
            placements.push(json!({
                "prototype": placement.prototype, "path": placement.path,
                "transform": matrix(Matrix(placement.transform)),
                "down": placement.down().map_err(&fail)?,
                "inputs": placement.inputs, "slots": placement.slots,
                "binding": {}, "assignments": {}, "meshes": meshes,
            }));
            placed.placements.push(covered);
        }
        // A prototype drawn as it is (a `prototype` part) keeps its roles on its meshes.
        let roles: BTreeMap<String, &[u8]> = geometry
            .nodes
            .iter()
            .flat_map(|node| {
                node.meshes.iter().filter_map(|&index| {
                    let mesh = geometry.meshes.get(index)?;
                    (!mesh.roles.is_empty()).then(|| {
                        (
                            format!("{prefix}{}/{}", node.id, mesh.id),
                            mesh.roles.as_slice(),
                        )
                    })
                })
            })
            .collect();
        placed.metadata = json!({
            "prototypes": prototypes, "placements": placements,
            "entry": entry, "definition": generated.id, "geometryKey": generated.geometry_key,
            "generation": generation, "rebuilt": rebuilt,
            "parameters": parameters, "derived": prepared.derived,
            "parts": parts, "features": features, "attachments": attachments,
            "decals": decals, "handles": handles,
            "reachDropped": reach_dropped,
            "meshSlots": placed.slots.iter().map(|(id, slot)| (format!("{prefix}{id}"), slot.clone())).collect::<BTreeMap<_, _>>(),
        });
        if !roles.is_empty() {
            placed.metadata["meshRoles"] = json!(roles);
        }
        if !parsed.lods.is_empty() {
            let mut levels = vec![render_level(&placed.model, 0.)];
            for (index, level) in parsed.lods.iter().enumerate() {
                let (alternative, parameters) = parsed.render_alternative(level, &model.parameters);
                let relative = format!("lod{}/", index + 1);
                let mut alternate_source = model.clone();
                alternate_source.id = format!("{}/lod{}", model.id, index + 1);
                alternate_source.source = Some(
                    serde_json::to_string(&alternative).map_err(|error| fail(error.to_string()))?,
                );
                alternate_source.parameters = parameters;
                let alternate = self.place_item(
                    project,
                    &alternate_source,
                    &format!("{prefix}{relative}"),
                    assets,
                    cancelled,
                )?;
                levels.push(
                    append_render_level(
                        &mut placed.model,
                        alternate.model,
                        &relative,
                        level.distance,
                    )
                    .map_err(&fail)?,
                );
                placed.slots.extend(
                    alternate
                        .slots
                        .into_iter()
                        .map(|(id, slot)| (format!("{relative}{id}"), slot)),
                );
                placed.transforms.extend(
                    alternate
                        .transforms
                        .into_iter()
                        .map(|(id, transform)| (format!("{relative}{id}"), transform)),
                );
                placed.features.extend(
                    alternate
                        .features
                        .into_iter()
                        .map(|(id, features)| (format!("{relative}{id}"), features)),
                );
                placed.anchors.extend(
                    alternate
                        .anchors
                        .into_iter()
                        .map(|(id, anchor)| (format!("{relative}{id}"), anchor)),
                );
                placed.bounds.extend(
                    alternate
                        .bounds
                        .into_iter()
                        .map(|(id, bounds)| (format!("{relative}{id}"), bounds)),
                );
            }
            placed.model.geometry_lods.push(GeometryLodGroup {
                id: format!("{}/lod", generated.id),
                levels,
            });
        }
        Ok(placed)
    }
}

fn collision(
    model: &mut Model,
    item: &definition::GeneratedItem,
    placement: Matrix,
) -> Result<(), String> {
    let (solid, transform) = match &item.collision {
        Collision::Intent(CollisionIntent::None | CollisionIntent::Mesh) => return Ok(()),
        Collision::Intent(CollisionIntent::Bounds) => {
            let b = item.geometry.bounds;
            let mut m = Matrix::default();
            m.0[3] = b[0];
            (Solid::cube(sub(b[1], b[0]))?, m)
        }
        Collision::Shape(CollisionShape::Box { center, size }) => {
            let mut m = Matrix::default();
            m.0[3] = std::array::from_fn(|i| center[i] - size[i] / 2.);
            (Solid::cube(*size)?, Matrix(item.transform).then(m))
        }
        Collision::Shape(CollisionShape::Sphere { center, radius }) => {
            let mut m = Matrix::default();
            m.0[3] = *center;
            (Solid::sphere(*radius, 24)?, Matrix(item.transform).then(m))
        }
    };
    let mesh = solid.mesh();
    let stride = mesh.num_prop as usize;
    let mut local = LocalMesh {
        positions: vec![],
        normals: vec![],
        uvs: vec![],
        colors: vec![],
        indices: vec![],
        tangents: vec![],
    };
    for indices in mesh.tri_verts.chunks_exact(3) {
        let p: [[_; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|axis| {
                mesh.vert_properties[indices[i] as usize * stride + axis] as f64
            })
        });
        let normal = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])))?;
        for point in p {
            local.positions.push(point);
            local.normals.push(normal);
            local.uvs.push([0., 0.]);
            local.colors.push([1.; 3]);
            local.indices.push(local.indices.len() as u32);
        }
    }
    let id = format!("studio/collision/{}", item.id);
    model.mesh_artifacts.insert(
        id.clone(),
        format!(
            "itemspec-collision-v1/{}/{:x}",
            item.geometry_key,
            Sha256::digest(
                serde_json::to_vec(&(&item.collision, &item.transform))
                    .map_err(|e| e.to_string())?
            )
        ),
    );
    model.meshes.insert(id.clone(), Arc::new(local));
    model.instances.push(Instance {
        id: id.clone(),
        object_id: item.id.clone(),
        mesh: id,
        material: None,
        transform: matrix(placement.then(transform)),
        source: location(),
        visible: false,
        collidable: true,
    });
    Ok(())
}
